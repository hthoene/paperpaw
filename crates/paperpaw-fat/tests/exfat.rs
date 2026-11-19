//! PaperPaw's storage on an exFAT card (#54): the same storage as on FAT,
//! on the driver's second volume type. Every call that changes the card is
//! cut off after each block it writes: the card mounts, each file is whole
//! (as before the call or as after it), the files beside it are untouched,
//! the card takes further writes, and a computer's check (`fsck.exfat`,
//! where installed) finds nothing it must repair at the cost of a file.
#![allow(clippy::unwrap_used, reason = "test assertions")]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use paperpaw_fat::fatfs::{VolumeKind, format_exfat};
use paperpaw_fat::{BLOCK, BlockDisk, Blocks, DiskError, FatStorage, SharedDisk};
use paperpaw_platform::{Storage, StorageError};

/// A block written: where, and what.
type Written = (u32, Vec<u8>);

/// A card in memory that remembers the blocks written to it.
#[derive(Clone)]
struct Card {
    bytes: Rc<RefCell<Vec<u8>>>,
    log: Rc<RefCell<Vec<Written>>>,
}

impl Blocks for Card {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        let at = index as usize * BLOCK;
        block.copy_from_slice(self.bytes.borrow().get(at..at + BLOCK).ok_or(DiskError)?);
        Ok(())
    }

    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        let at = index as usize * BLOCK;
        self.bytes
            .borrow_mut()
            .get_mut(at..at + BLOCK)
            .ok_or(DiskError)?
            .copy_from_slice(block);
        self.log.borrow_mut().push((index, block.to_vec()));
        Ok(())
    }

    fn blocks(&mut self) -> Result<u32, DiskError> {
        Ok((self.bytes.borrow().len() / BLOCK) as u32)
    }
}

type Exfat = FatStorage<SharedDisk<Card>>;

impl Card {
    fn of(image: Vec<u8>) -> Self {
        Self {
            bytes: Rc::new(RefCell::new(image)),
            log: Rc::default(),
        }
    }

    /// A card of `size` bytes formatted as exFAT in clusters of `cluster`.
    fn new(size: usize, cluster: u32) -> Self {
        let card = Self::of(vec![0; size]);
        let mut disk = BlockDisk::new(card.clone()).unwrap();
        format_exfat(&mut disk, size as u64, cluster).unwrap();
        card
    }

    /// Mounted as the device mounts it.
    fn mount(&self) -> Exfat {
        let storage = paperpaw_fat::mount(self.clone()).unwrap();
        assert_eq!(storage.kind(), VolumeKind::ExFat);
        storage
    }

    fn image(&self) -> Vec<u8> {
        self.bytes.borrow().clone()
    }
}

fn data(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(13).wrapping_add(seed))
        .collect()
}

/// Every file of the storage by its path, read whole.
fn files(storage: &dyn Storage) -> BTreeMap<String, Vec<u8>> {
    fn walk(storage: &dyn Storage, dir: &str, out: &mut BTreeMap<String, Vec<u8>>) {
        let mut found = Vec::new();
        storage
            .list(dir, &mut |entry| {
                found.push((entry.name.to_string(), entry.is_dir))
            })
            .unwrap();
        for (name, is_dir) in found {
            let path = if dir.is_empty() {
                name
            } else {
                format!("{dir}/{name}")
            };
            if is_dir {
                walk(storage, &path, out);
            } else {
                let size = storage.size(&path).unwrap() as usize;
                let mut bytes = vec![0; size];
                let mut read = 0;
                while read < size {
                    let n = storage
                        .read_at(&path, read as u64, &mut bytes[read..])
                        .unwrap();
                    assert!(n > 0, "{path} reads short");
                    read += n;
                }
                out.insert(path, bytes);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(storage, "", &mut out);
    out
}

/// A computer's check of `image`, repairing what it finds, where
/// `fsck.exfat` is installed: the image after it.
fn checked(image: &[u8]) -> Option<Vec<u8>> {
    let tool = ["/usr/sbin/fsck.exfat", "/sbin/fsck.exfat"]
        .into_iter()
        .find(|tool| std::path::Path::new(tool).exists())?;
    let dir = std::env::temp_dir().join(format!("pp-exfat-storage-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{:?}.img", std::thread::current().id()));
    std::fs::write(&path, image).unwrap();
    let out = std::process::Command::new(tool)
        .arg("-y")
        .arg(&path)
        .output()
        .unwrap();
    let after = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).ok();
    if out.status.code() != Some(0) && std::env::var_os("PP_FSCK_SAYS").is_some() {
        eprintln!("fsck.exfat: {}", String::from_utf8_lossy(&out.stdout));
    }
    // 0: nothing found; 1: found and corrected.
    assert!(
        matches!(out.status.code(), Some(0 | 1)),
        "fsck.exfat: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    Some(after)
}

/// Fails where a computer's check changed `card`, naming the first bytes.
#[track_caller]
fn untouched(card: &Card) {
    let Some(repaired) = checked(&card.image()) else {
        return;
    };
    let image = card.image();
    let differ: Vec<usize> = (0..image.len())
        .filter(|&i| image[i] != repaired[i])
        .take(8)
        .collect();
    assert!(
        differ.is_empty(),
        "a computer's check changed the card at {differ:?}"
    );
}

/// A card with a few files, as a reader's card holds them.
fn filled(cluster: u32) -> Card {
    let card = Card::new(8 << 20, cluster);
    let storage = card.mount();
    storage.write("keep.txt", &data(5000, 1)).unwrap();
    storage.write("note.txt", &data(1800, 2)).unwrap();
    storage
        .write("books/a long name of a book, part one.epub", &data(9000, 3))
        .unwrap();
    storage.write("books/more/b.txt", &data(300, 4)).unwrap();
    storage.append("log.txt", &data(100, 5)).unwrap();
    storage.sync().unwrap();
    drop(storage);
    card
}

type Files = BTreeMap<String, Vec<u8>>;

/// Fails where `got` is not `want`, naming the paths that differ.
#[track_caller]
fn same(got: &Files, want: &Files, what: &str) {
    let differ: Vec<String> = got
        .keys()
        .chain(want.keys())
        .filter(|path| got.get(*path) != want.get(*path))
        .map(|path| {
            format!(
                "{path}: {:?} for {:?} bytes",
                got.get(path).map(Vec::len),
                want.get(path).map(Vec::len)
            )
        })
        .collect();
    assert!(differ.is_empty(), "{what}: {differ:?}");
}

/// Runs `call` on a filled card and cuts it off after every block: `sound`
/// judges the files then, given those before and after the whole call.
fn every_cut(cluster: u32, call: impl Fn(&dyn Storage), sound: impl Fn(&Files, &Files, &Files)) {
    let card = filled(cluster);
    let before_image = card.image();
    let before = files(&card.mount());
    card.log.borrow_mut().clear();
    {
        let storage = card.mount();
        call(&storage);
        storage.sync().unwrap();
    }
    let after = files(&card.mount());
    if let Some(repaired) = checked(&card.image()) {
        same(
            &files(&Card::of(repaired).mount()),
            &after,
            "the check changed a file",
        );
    }
    let log = card.log.borrow().clone();
    assert!(!log.is_empty(), "the call wrote nothing");
    let mut image = before_image;
    for cut in 0..=log.len() {
        if cut > 0 {
            let (index, block) = &log[cut - 1];
            let at = *index as usize * BLOCK;
            image[at..at + BLOCK].copy_from_slice(block);
        }
        let state = Card::of(image.clone());
        let now = files(&state.mount());
        for (path, bytes) in &before {
            if after.get(path) == Some(bytes) {
                assert_eq!(now.get(path), Some(bytes), "cut {cut}: {path} changed");
            }
        }
        sound(&before, &after, &now);
        // The card goes on being used, and a computer's check then costs
        // no file.
        let storage = state.mount();
        let settled = files(&storage);
        storage
            .write("after/the cut.txt", &data(2500, cut as u8))
            .unwrap();
        storage.append("log.txt", b"more").unwrap();
        storage.sync().unwrap();
        let mut expected = settled;
        expected.insert("after/the cut.txt".into(), data(2500, cut as u8));
        expected
            .entry("log.txt".into())
            .or_default()
            .extend_from_slice(b"more");
        drop(storage);
        same(
            &files(&state.mount()),
            &expected,
            &format!("cut {cut}: after further writes"),
        );
        if let Some(repaired) = checked(&state.image()) {
            same(
                &files(&Card::of(repaired).mount()),
                &expected,
                &format!("cut {cut}: the check changed a file"),
            );
        }
    }
}

/// `path` is as before or as after, whole.
fn old_or_new(path: &'static str) -> impl Fn(&Files, &Files, &Files) {
    move |before, after, now| {
        let got = now.get(path);
        assert!(
            got == before.get(path) || got == after.get(path),
            "{path}: neither as before nor as after ({:?} bytes)",
            got.map(Vec::len)
        );
    }
}

#[test]
fn the_storage_works_on_exfat() {
    for cluster in [512, 4096, 32768] {
        let card = filled(cluster);
        let storage = card.mount();
        assert_eq!(files(&storage).len(), 5);
        storage.write("note.txt", &data(40_000, 9)).unwrap();
        storage.write_at("note.txt", 512, &[7; 512]).unwrap();
        storage.append("log.txt", &data(70_000, 6)).unwrap();
        storage.rename("keep.txt", "books/more/kept.txt").unwrap();
        storage.rename("books/more", "books/less").unwrap();
        storage.rename("note.txt", "NOTE.txt").unwrap();
        storage.create_dir("empty/inside").unwrap();
        storage
            .remove("books/a long name of a book, part one.epub")
            .unwrap();
        assert_eq!(storage.remove("nothing.txt"), Ok(()));
        assert_eq!(storage.size("nothing.txt"), Err(StorageError::NotFound));
        for n in 0..40 {
            storage
                .write(&format!("many/file number {n}.txt"), &data(n * 37, n as u8))
                .unwrap();
        }
        storage.remove_dir_all("empty").unwrap();
        storage.sync().unwrap();
        let mut free = None;
        for _ in 0..64 {
            free = free.or(storage.free_space());
        }
        let total = storage.total_space().unwrap();
        let free = free.expect("the free space is counted");
        assert!(free < total && free > total / 2, "{free} of {total}");
        let now = files(&storage);
        drop(storage);
        let again = card.mount();
        same(&files(&again), &now, "mounted again");
        let mut note = data(40_000, 9);
        note[512..1024].fill(7);
        assert_eq!(now["NOTE.txt"], note);
        assert_eq!(now["books/less/kept.txt"], data(5000, 1));
        assert_eq!(now["log.txt"].len(), 70_100);
        assert_eq!(now.len(), 4 + 40);
        // As counted from the bitmap anew.
        let mut counted = None;
        for _ in 0..64 {
            counted = counted.or(again.free_space());
        }
        assert_eq!(counted, Some(free));
        untouched(&card);
    }
}

#[test]
fn a_write_is_cut_off_at_every_block() {
    for cluster in [512, 4096] {
        every_cut(
            cluster,
            |s| s.write("new file.txt", &data(3000, 7)).unwrap(),
            old_or_new("new file.txt"),
        );
        every_cut(
            cluster,
            |s| s.write("note.txt", &data(6000, 8)).unwrap(),
            old_or_new("note.txt"),
        );
        every_cut(
            cluster,
            |s| s.write("note.txt", &[]).unwrap(),
            old_or_new("note.txt"),
        );
    }
}

#[test]
fn an_append_is_cut_off_at_every_block() {
    every_cut(
        512,
        |s| {
            s.append("log.txt", &data(700, 1)).unwrap();
            s.append("log.txt", &data(900, 2)).unwrap();
            s.append("fresh.log", &data(2000, 3)).unwrap();
        },
        |before, after, now| {
            // What was there stays, and what follows is what was appended.
            let log = &now["log.txt"];
            assert!(log.starts_with(&before["log.txt"]) && after["log.txt"].starts_with(log));
            assert!(
                now.get("fresh.log")
                    .is_none_or(|b| after["fresh.log"].starts_with(b))
            );
        },
    );
    every_cut(
        4096,
        |s| s.write_at("note.txt", 512, &[9; 512]).unwrap(),
        old_or_new("note.txt"),
    );
}

#[test]
fn a_move_is_cut_off_at_every_block() {
    let moved = |from: &'static str, to: &'static str| {
        move |before: &Files, after: &Files, now: &Files| {
            let (old, new) = (now.get(from), now.get(to));
            assert!(
                old == before.get(from) && new == before.get(to)
                    || old == after.get(from) && new == after.get(to),
                "{from} -> {to}: neither moved nor not"
            );
        }
    };
    every_cut(
        512,
        |s| s.rename("note.txt", "renamed.txt").unwrap(),
        moved("note.txt", "renamed.txt"),
    );
    every_cut(
        512,
        |s| s.rename("note.txt", "books/more/moved.txt").unwrap(),
        moved("note.txt", "books/more/moved.txt"),
    );
    every_cut(
        512,
        |s| s.rename("note.txt", "keep.txt").unwrap(),
        moved("note.txt", "keep.txt"),
    );
    every_cut(
        512,
        |s| s.rename("books", "shelf").unwrap(),
        moved("books/more/b.txt", "shelf/more/b.txt"),
    );
}

#[test]
fn a_removal_is_cut_off_at_every_block() {
    every_cut(
        512,
        |s| s.remove("note.txt").unwrap(),
        old_or_new("note.txt"),
    );
    every_cut(
        512,
        |s| s.remove_dir_all("books").unwrap(),
        |before, _, now| {
            for path in [
                "books/a long name of a book, part one.epub",
                "books/more/b.txt",
            ] {
                assert!(now.get(path).is_none_or(|b| b == &before[path]), "{path}");
            }
        },
    );
    every_cut(512, |s| s.create_dir("a/b/c").unwrap(), |_, _, _| {});
}

/// A seeded sequence of calls against what the files must be, remounted on
/// the way, then checked by a computer.
#[test]
fn many_calls_keep_every_file() {
    for (cluster, seed) in [(512u32, 1u64), (4096, 2), (32768, 3)] {
        let card = Card::new(16 << 20, cluster);
        let mut storage = card.mount();
        let mut model: Files = BTreeMap::new();
        let mut state = seed;
        let mut next = |below: u64| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) % below
        };
        let names = [
            "a.txt",
            "b with a longer name than one entry holds.bin",
            "dir/c.txt",
            "dir/deeper/d.txt",
            "dir/E.TXT",
            "ünï/cödé.txt",
            "f",
            "dir/deeper/g and more.dat",
        ];
        for step in 0..600 {
            let path = names[next(names.len() as u64) as usize];
            let bytes = data(next(9000) as usize, step as u8);
            match next(8) {
                0 | 1 => {
                    storage.write(path, &bytes).unwrap();
                    model.insert(path.into(), bytes);
                }
                2 | 3 => {
                    storage.append(path, &bytes).unwrap();
                    model
                        .entry(path.into())
                        .or_default()
                        .extend_from_slice(&bytes);
                }
                4 => {
                    storage.remove(path).unwrap();
                    model.remove(path);
                }
                5 => {
                    let to = names[next(names.len() as u64) as usize];
                    match storage.rename(path, to) {
                        Ok(()) => {
                            if !path.eq_ignore_ascii_case(to)
                                && let Some(bytes) = model.remove(path)
                            {
                                model.insert(to.into(), bytes);
                            }
                        }
                        Err(StorageError::NotFound) => assert!(!model.contains_key(path)),
                        Err(e) => panic!("rename {path} -> {to}: {e:?}"),
                    }
                }
                6 => {
                    if let Some(old) = model.get_mut(path).filter(|old| !old.is_empty()) {
                        let at = next(old.len() as u64) as usize;
                        let n = bytes.len().min(old.len() - at);
                        storage.write_at(path, at as u64, &bytes[..n]).unwrap();
                        old[at..at + n].copy_from_slice(&bytes[..n]);
                    }
                }
                _ => {
                    storage.sync().unwrap();
                    drop(storage);
                    storage = card.mount();
                }
            }
            if step % 50 == 49 {
                storage.sync().unwrap();
                same(
                    &files(&storage),
                    &model,
                    &format!("step {step}, clusters of {cluster}"),
                );
            }
        }
        storage.sync().unwrap();
        drop(storage);
        same(&files(&card.mount()), &model, "at the end");
        untouched(&card);
    }
}
