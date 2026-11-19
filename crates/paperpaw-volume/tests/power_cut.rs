//! Write ordering of the driver, held at every block it writes: a power cut
//! after any write leaves a volume that mounts, and the file a program
//! wrote (progress, settings, highlights) is whole or as before, never
//! short, never lost, never a chain into freed clusters.
//!
//! The driver writes a changed FAT entry before the directory entry that
//! points at it, removes a directory entry before it frees its clusters,
//! and writes a new entry before it removes the one it replaces. Each test
//! records the writes of one operation, then replays every prefix of them on
//! the volume as it was before.

use std::cell::RefCell;
use std::io::{Cursor, Read as _, Seek, SeekFrom, Write as _};
use std::rc::Rc;

use fatfs::{FatType, FileSystem, FormatVolumeOptions, FsOptions, StdIoWrapper, format_volume};

type Log = Rc<RefCell<Vec<(u64, Vec<u8>)>>>;

/// A disk in memory that remembers every write.
struct Recorder {
    inner: Cursor<Vec<u8>>,
    log: Log,
}

impl std::io::Read for Recorder {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.inner.read(buf)
    }
}

impl std::io::Write for Recorder {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let at = self.inner.position();
        let n = self.inner.write(buf)?;
        self.log.borrow_mut().push((at, buf[..n].to_vec()));
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Seek for Recorder {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(to)
    }
}

const CLUSTER: usize = 512;

fn formatted() -> Vec<u8> {
    let mut disk = Cursor::new(vec![0_u8; 40 << 20]);
    let mut wrapped = StdIoWrapper::new(&mut disk);
    format_volume(
        &mut wrapped,
        FormatVolumeOptions::new()
            .fat_type(FatType::Fat32)
            .bytes_per_cluster(CLUSTER as u32),
    )
    .unwrap();
    disk.into_inner()
}

fn data(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(7).wrapping_add(seed))
        .collect()
}

type Fs = FileSystem<StdIoWrapper<Recorder>>;

/// Runs `body` on `image`: the volume before and the writes it made.
fn recorded(image: &[u8], body: impl FnOnce(&Fs)) -> (Vec<u8>, Vec<(u64, Vec<u8>)>) {
    let log: Log = Rc::default();
    let disk = Recorder {
        inner: Cursor::new(image.to_vec()),
        log: log.clone(),
    };
    let fs = FileSystem::new(StdIoWrapper::new(disk), FsOptions::new()).unwrap();
    body(&fs);
    fs.unmount().unwrap();
    let writes = log.borrow().clone();
    (image.to_vec(), writes)
}

/// The volume as it is after `k` writes.
fn after(base: &[u8], writes: &[(u64, Vec<u8>)], k: usize) -> Vec<u8> {
    let mut image = base.to_vec();
    for (at, bytes) in &writes[..k] {
        image[*at as usize..*at as usize + bytes.len()].copy_from_slice(bytes);
    }
    image
}

/// The file after a cut: `None` when it is not there.
fn read_file(image: Vec<u8>, name: &str) -> Option<Vec<u8>> {
    let fs = FileSystem::new(StdIoWrapper::new(Cursor::new(image)), FsOptions::new())
        .expect("a cut volume mounts");
    let root = fs.root_dir();
    let mut file = root.open_file(name).ok()?;
    let mut out = Vec::new();
    file.read_to_end(&mut out)
        .expect("a file reads to its size");
    Some(out)
}

fn put(fs: &Fs, name: &str, bytes: &[u8]) {
    let mut file = fs.root_dir().create_file(name).unwrap();
    file.truncate().unwrap();
    file.write_all(bytes).unwrap();
    file.flush().unwrap();
}

#[test]
fn replace_leaves_the_old_or_the_new_file_at_every_cut() {
    let (old, new) = (data(3 * CLUSTER + 100, 1), data(5 * CLUSTER + 7, 2));
    let (_, written) = recorded(&formatted(), |fs| {
        put(fs, "state.txt", &old);
        put(fs, "state.tmp", &new);
    });
    let base = after(&formatted(), &written, written.len());
    let (base, writes) = recorded(&base, |fs| {
        fs.root_dir().replace("state.tmp", "state.txt").unwrap()
    });
    assert!(writes.len() > 2, "the replace writes more than one block");
    for k in 0..=writes.len() {
        let got = read_file(after(&base, &writes, k), "state.txt")
            .unwrap_or_else(|| panic!("cut after {k} of {} writes lost the file", writes.len()));
        assert!(
            got == old || got == new,
            "cut after {k} of {} writes left {} bytes",
            writes.len(),
            got.len()
        );
    }
}

#[test]
fn removing_a_file_leaves_it_whole_or_gone_at_every_cut() {
    let bytes = data(6 * CLUSTER + 3, 3);
    let blank = formatted();
    let (_, written) = recorded(&blank, |fs| put(fs, "gone.txt", &bytes));
    let base = after(&blank, &written, written.len());
    let (base, writes) = recorded(&base, |fs| fs.root_dir().remove("gone.txt").unwrap());
    for k in 0..=writes.len() {
        if let Some(got) = read_file(after(&base, &writes, k), "gone.txt") {
            assert!(
                got == bytes,
                "cut after {k} of {} writes left a damaged file",
                writes.len()
            );
        }
    }
}

#[test]
fn a_new_file_never_shows_more_than_it_has_at_every_cut() {
    let bytes = data(8 * CLUSTER + 11, 4);
    let blank = formatted();
    let (base, writes) = recorded(&blank, |fs| put(fs, "new.txt", &bytes));
    for k in 0..=writes.len() {
        if let Some(got) = read_file(after(&base, &writes, k), "new.txt") {
            assert!(
                got == bytes[..got.len()],
                "cut after {k} of {} writes shows wrong data",
                writes.len()
            );
        }
    }
}

#[test]
fn growing_a_file_never_shows_more_than_it_has_at_every_cut() {
    let (first, more) = (data(2 * CLUSTER + 5, 5), data(4 * CLUSTER, 6));
    let blank = formatted();
    let (_, written) = recorded(&blank, |fs| put(fs, "log.txt", &first));
    let base = after(&blank, &written, written.len());
    let (base, writes) = recorded(&base, |fs| {
        let mut file = fs.root_dir().open_file("log.txt").unwrap();
        file.seek(SeekFrom::End(0)).unwrap();
        file.write_all(&more).unwrap();
        file.flush().unwrap();
    });
    let whole = [first.clone(), more].concat();
    for k in 0..=writes.len() {
        let got = read_file(after(&base, &writes, k), "log.txt")
            .unwrap_or_else(|| panic!("cut after {k} of {} writes lost the file", writes.len()));
        assert!(got.len() >= first.len(), "cut after {k} shortened the file");
        assert!(
            got == whole[..got.len()],
            "cut after {k} of {} writes shows wrong data",
            writes.len()
        );
    }
}
