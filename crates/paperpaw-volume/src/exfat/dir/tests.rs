//! The exFAT driver against cuts: every operation is recorded as the
//! blocks it writes, and after every one of them the volume must mount,
//! hold each file whole (as before the operation or as after it), name no
//! cluster twice or one marked free, and take further writes.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::io::{self, SeekFrom as StdSeek};
use std::rc::Rc;
use std::vec::Vec;

use super::*;
use crate::exfat::format::format_exfat;
use crate::io::StdIoWrapper;

type Log = Rc<RefCell<Vec<(u64, Vec<u8>)>>>;

/// A disk in memory that remembers every write and counts the reads.
struct Rec {
    data: Rc<RefCell<Vec<u8>>>,
    pos: u64,
    log: Log,
    reads: Rc<Cell<u64>>,
}

impl io::Read for Rec {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reads.set(self.reads.get() + 1);
        let data = self.data.borrow();
        let at = (self.pos as usize).min(data.len());
        let n = buf.len().min(data.len() - at);
        buf[..n].copy_from_slice(&data[at..at + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl io::Write for Rec {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut data = self.data.borrow_mut();
        let at = (self.pos as usize).min(data.len());
        let n = buf.len().min(data.len() - at);
        data[at..at + n].copy_from_slice(&buf[..n]);
        self.log.borrow_mut().push((self.pos, buf[..n].to_vec()));
        self.pos += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl io::Seek for Rec {
    fn seek(&mut self, to: StdSeek) -> io::Result<u64> {
        let len = self.data.borrow().len() as i64;
        self.pos = match to {
            StdSeek::Start(n) => n,
            StdSeek::End(n) => (len + n) as u64,
            StdSeek::Current(n) => (self.pos as i64 + n) as u64,
        };
        Ok(self.pos)
    }
}

type Disk = StdIoWrapper<Rec>;
type Fs = ExFat<Disk>;

struct Card {
    data: Rc<RefCell<Vec<u8>>>,
    log: Log,
    reads: Rc<Cell<u64>>,
}

impl Card {
    fn new(bytes: usize, cluster: u32) -> Self {
        let card = Self::of(vec![0; bytes]);
        format_exfat(&mut card.disk(), bytes as u64, cluster).unwrap();
        card.log.borrow_mut().clear();
        card
    }

    fn of(image: Vec<u8>) -> Self {
        Self {
            data: Rc::new(RefCell::new(image)),
            log: Rc::default(),
            reads: Rc::default(),
        }
    }

    fn disk(&self) -> Disk {
        StdIoWrapper::new(Rec {
            data: self.data.clone(),
            pos: 0,
            log: self.log.clone(),
            reads: self.reads.clone(),
        })
    }

    fn mount(&self) -> Fs {
        ExFat::mount(self.disk()).unwrap()
    }

    fn image(&self) -> Vec<u8> {
        self.data.borrow().clone()
    }

    /// The card after each block `body` writes, from the card as it is.
    fn cuts(&self, body: impl FnOnce(&Fs)) -> Vec<Vec<u8>> {
        let before = self.image();
        self.log.borrow_mut().clear();
        {
            let fs = self.mount();
            body(&fs);
            fs.flush().unwrap();
        }
        let mut image = before;
        let mut states = vec![image.clone()];
        for (at, bytes) in self.log.borrow().iter() {
            let mut done = 0;
            while done < bytes.len() {
                let start = *at as usize + done;
                let n = (512 - start % 512).min(bytes.len() - done);
                image[start..start + n].copy_from_slice(&bytes[done..done + n]);
                done += n;
                states.push(image.clone());
            }
        }
        states
    }
}

fn data(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

fn write(dir: &XDir<'_, Disk>, name: &str, bytes: &[u8]) {
    let mut file = dir.create_file(name).unwrap();
    file.seek(crate::io::SeekFrom::End(0)).unwrap();
    let mut done = 0;
    while done < bytes.len() {
        done += file.write(&bytes[done..]).unwrap();
    }
    file.flush().unwrap();
}

fn read(entry: &XEntry<'_, Disk>) -> Vec<u8> {
    let mut file = entry.to_file();
    let mut out = vec![0; entry.len() as usize];
    let mut done = 0;
    while done < out.len() {
        let n = file.read(&mut out[done..]).unwrap();
        assert!(n > 0, "a file reads short");
        done += n;
    }
    out
}

/// Every file of the volume by its path, read whole; folders end in `/`.
/// Fails where a cluster is named by two entries (but two that hold the
/// same file, as a move cut off leaves them) or is marked free.
fn tree(fs: &Fs) -> BTreeMap<String, Vec<u8>> {
    fn walk(
        fs: &Fs,
        dir: &XDir<'_, Disk>,
        path: &str,
        out: &mut BTreeMap<String, Vec<u8>>,
        owners: &mut BTreeMap<u32, u32>,
    ) {
        for entry in dir.iter() {
            let entry = entry.unwrap();
            let name = format!("{path}{}", entry.file_name());
            let mut walked = (0, entry.first);
            for index in 0..entry.clusters() {
                let cluster = fs
                    .cluster_at(entry.chain(), &mut walked, index)
                    .unwrap()
                    .unwrap_or_else(|| panic!("{name}: its clusters end before its size"));
                assert!(
                    fs.is_taken(cluster).unwrap(),
                    "{name}: cluster {cluster} is free"
                );
                let owner = *owners.entry(cluster).or_insert(entry.first);
                assert_eq!(owner, entry.first, "{name}: cluster {cluster} is another's");
            }
            if entry.is_dir() {
                out.insert(format!("{name}/"), Vec::new());
                walk(fs, &entry.to_dir(), &format!("{name}/"), out, owners);
            } else {
                out.insert(name, read(&entry));
            }
        }
    }
    let mut out = BTreeMap::new();
    let mut owners = BTreeMap::new();
    walk(fs, &fs.root_dir(), "", &mut out, &mut owners);
    out
}

/// `fsck.exfat` on `image`, where it is installed: no error but the mark
/// of a card in use.
fn fsck(image: &[u8]) {
    let Some(tool) = ["/usr/sbin/fsck.exfat", "/sbin/fsck.exfat"]
        .into_iter()
        .find(|tool| std::path::Path::new(tool).exists())
    else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("pp-exfat-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{:?}.img", std::thread::current().id()));
    std::fs::write(&path, image).unwrap();
    let out = std::process::Command::new(tool)
        .arg("-n")
        .arg(&path)
        .output()
        .unwrap();
    std::fs::remove_file(&path).ok();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "fsck.exfat: {text}");
}

/// Runs `body` on `card` cut after every block: each state mounts, holds
/// what `sound` accepts (given the files before and after), and takes a
/// new file after.
fn every_cut(
    card: &Card,
    body: impl FnOnce(&Fs),
    sound: impl Fn(&BTreeMap<String, Vec<u8>>, &BTreeMap<String, Vec<u8>>, &BTreeMap<String, Vec<u8>>),
) {
    let before = tree(&card.mount());
    let states = card.cuts(body);
    let after = tree(&card.mount());
    fsck(&card.image());
    assert!(states.len() > 1, "nothing was written");
    for (n, state) in states.into_iter().enumerate() {
        let cut = Card::of(state);
        let now = {
            let fs = cut.mount();
            tree(&fs)
        };
        for (path, bytes) in &before {
            // What the operation does not touch stays as it is.
            if after.get(path) == Some(bytes) {
                assert_eq!(now.get(path), Some(bytes), "cut {n}: {path} changed");
            }
        }
        sound(&before, &after, &now);
        // The volume takes more: the slots and clusters a cut left do not
        // stand in the way, and nothing of them comes back.
        let fs = cut.mount();
        let root = fs.root_dir();
        let more = data(1500, n as u8);
        write(&root, "after the cut, a longer name.bin", &more);
        root.create_dir("made after").unwrap();
        fs.flush().unwrap();
        drop(root);
        let then = tree(&fs);
        assert_eq!(
            then.get("after the cut, a longer name.bin"),
            Some(&more),
            "cut {n}"
        );
        for (path, bytes) in &now {
            assert_eq!(
                then.get(path),
                Some(bytes),
                "cut {n}: {path} changed by a later write"
            );
        }
        assert_eq!(then.len(), now.len() + 2, "cut {n}: an entry came back");
    }
}

/// `path` is as before or as after, whole.
fn old_or_new(
    path: &'static str,
) -> impl Fn(&BTreeMap<String, Vec<u8>>, &BTreeMap<String, Vec<u8>>, &BTreeMap<String, Vec<u8>>) {
    move |before, after, now| {
        let got = now.get(path);
        assert!(
            got == before.get(path) || got == after.get(path),
            "{path} is neither as before nor as after: {:?} bytes",
            got.map(Vec::len)
        );
    }
}

/// A card with a few files and folders, in clusters of `cluster` bytes.
fn filled(cluster: u32) -> Card {
    let card = Card::new(4 << 20, cluster);
    {
        let fs = card.mount();
        let root = fs.root_dir();
        write(&root, "first.txt", &data(700, 1));
        write(
            &root,
            "a book with a rather long name, volume two.epub",
            &data(3000, 2),
        );
        let sub = root.create_dir("books").unwrap();
        write(&sub, "inside.txt", &data(1200, 3));
        write(&root, "last.txt", &data(10, 4));
        fs.flush().unwrap();
    }
    fsck(&card.image());
    card
}

#[test]
fn files_and_folders_read_back() {
    for cluster in [512, 4096, 32768] {
        let card = filled(cluster);
        let fs = card.mount();
        let files = tree(&fs);
        assert_eq!(files["first.txt"], data(700, 1));
        assert_eq!(files["books/inside.txt"], data(1200, 3));
        assert_eq!(files.len(), 5);
        let root = fs.root_dir();
        assert!(root.find("FIRST.TXT").unwrap().is_some());
        assert!(root.find("first.tx").unwrap().is_none());
        assert_eq!(fs.cluster_changes().0, 0);
    }
}

#[test]
fn a_new_file_is_absent_empty_or_whole() {
    for cluster in [512, 4096] {
        let card = filled(cluster);
        let bytes = data(2600, 9);
        every_cut(
            &card,
            |fs| {
                write(
                    &fs.root_dir(),
                    "new file with a name of some length.bin",
                    &bytes,
                )
            },
            |_, after, now| {
                let got = now.get("new file with a name of some length.bin");
                assert!(got.is_none_or(|got| got.is_empty()
                    || Some(got) == after.get("new file with a name of some length.bin")));
            },
        );
    }
}

#[test]
fn an_append_is_old_or_new() {
    for cluster in [512, 4096] {
        let card = filled(cluster);
        // `first.txt` lies before other files: it grows into a chain.
        every_cut(
            &card,
            |fs| write(&fs.root_dir(), "first.txt", &data(1900, 7)),
            old_or_new("first.txt"),
        );
        // `last.txt` has free clusters after it: it stays one run.
        every_cut(
            &card,
            |fs| write(&fs.root_dir(), "last.txt", &data(5000, 8)),
            old_or_new("last.txt"),
        );
    }
}

#[test]
fn a_replace_is_old_or_new() {
    let card = filled(512);
    {
        let fs = card.mount();
        write(&fs.root_dir(), "~ABCDEFG.NEW", &data(900, 5));
        fs.flush().unwrap();
    }
    every_cut(
        &card,
        |fs| fs.root_dir().replace("~ABCDEFG.NEW", "first.txt").unwrap(),
        |before, _, now| {
            let got = &now["first.txt"];
            assert!(got == &before["first.txt"] || got == &before["~ABCDEFG.NEW"]);
            // Called again, it finishes.
            assert!(now.contains_key("~ABCDEFG.NEW") || got == &before["~ABCDEFG.NEW"]);
        },
    );
    let fs = card.mount();
    assert_eq!(tree(&fs)["first.txt"], data(900, 5));
    assert!(fs.root_dir().find("~ABCDEFG.NEW").unwrap().is_none());
}

#[test]
fn a_replace_cut_off_finishes_when_called_again() {
    let card = filled(512);
    {
        let fs = card.mount();
        write(&fs.root_dir(), "~ABCDEFG.NEW", &data(900, 5));
        fs.flush().unwrap();
    }
    let states = card.cuts(|fs| fs.root_dir().replace("~ABCDEFG.NEW", "first.txt").unwrap());
    for state in states {
        let cut = Card::of(state);
        let fs = cut.mount();
        let root = fs.root_dir();
        if root.find("~ABCDEFG.NEW").unwrap().is_some() {
            root.replace("~ABCDEFG.NEW", "first.txt").unwrap();
        }
        fs.flush().unwrap();
        let files = tree(&fs);
        assert_eq!(files["first.txt"], data(900, 5));
        assert_eq!(files.len(), 5);
    }
}

#[test]
fn a_move_leaves_the_file_at_one_place_or_both() {
    let card = filled(512);
    let whole = data(3000, 2);
    every_cut(
        &card,
        |fs| {
            let root = fs.root_dir();
            let to = root.find("books").unwrap().unwrap().to_dir();
            root.rename(
                "a book with a rather long name, volume two.epub",
                &to,
                "moved.epub",
            )
            .unwrap();
        },
        |_, _, now| {
            let old = now.get("a book with a rather long name, volume two.epub");
            let new = now.get("books/moved.epub");
            assert!(
                old.is_some() || new.is_some(),
                "the file is at neither place"
            );
            assert!(old.is_none_or(|b| b == &whole) && new.is_none_or(|b| b == &whole));
        },
    );
}

#[test]
fn a_removal_leaves_the_file_whole_or_gone() {
    let card = filled(512);
    every_cut(
        &card,
        |fs| fs.root_dir().remove("first.txt").unwrap(),
        old_or_new("first.txt"),
    );
    every_cut(
        &card,
        |fs| {
            let root = fs.root_dir();
            let sub = root.find("books").unwrap().unwrap().to_dir();
            sub.remove("inside.txt").unwrap();
            root.remove("books").unwrap();
        },
        |_, _, now| {
            assert!(
                now.get("books/inside.txt")
                    .is_none_or(|b| b == &data(1200, 3))
            );
        },
    );
}

#[test]
fn folders_grow_at_every_cut() {
    // 16 slots to a cluster: the root (a chain) and a folder (a run, then
    // a chain once another file lies behind it) grow more than once.
    let card = filled(512);
    every_cut(
        &card,
        |fs| {
            let root = fs.root_dir();
            let sub = root.find("books").unwrap().unwrap().to_dir();
            for n in 0..14 {
                write(
                    &sub,
                    &format!("a file of the folder, number {n}.txt"),
                    &data(40, n),
                );
                write(
                    &root,
                    &format!("a file of the root, number {n}.txt"),
                    &data(600, n),
                );
            }
        },
        |_, _, now| {
            for (path, bytes) in now {
                let n = path.rsplit(' ').next().and_then(|n| n.strip_suffix(".txt"));
                if let Some(n) = n.and_then(|n| n.parse::<u8>().ok()) {
                    let len = if path.starts_with("books/") { 40 } else { 600 };
                    assert!(bytes.is_empty() || bytes == &data(len, n), "{path}");
                }
            }
        },
    );
    let fs = card.mount();
    assert_eq!(tree(&fs).len(), 5 + 28);
}

#[test]
fn a_folder_whose_size_is_not_whole_clusters_is_damaged_and_left_alone() {
    let card = filled(512);
    for len in [32, 512 + 32] {
        let fs = card.mount();
        let root = fs.root_dir();
        let books = root.find("books").unwrap().unwrap();
        fs.update(&books.loc, |set| {
            set_stream(set, books.first, len, len, books.contiguous);
        })
        .unwrap();
        fs.flush().unwrap();
        let before = card.image();
        let dir = root.find("books").unwrap().unwrap().to_dir();
        assert!(
            matches!(dir.iter().next(), Some(Err(Error::CorruptedFileSystem))),
            "{len}"
        );
        assert!(
            matches!(dir.create_file("more.txt"), Err(Error::CorruptedFileSystem)),
            "{len}"
        );
        fs.flush().unwrap();
        assert!(card.image() == before, "{len}: the card was written");
    }
}

#[test]
fn a_file_cut_short_is_old_or_new() {
    let card = filled(512);
    every_cut(
        &card,
        |fs| {
            let entry = fs
                .root_dir()
                .find("a book with a rather long name, volume two.epub");
            let mut file = entry.unwrap().unwrap().to_file();
            file.seek(crate::io::SeekFrom::Start(700)).unwrap();
            file.truncate().unwrap();
        },
        old_or_new("a book with a rather long name, volume two.epub"),
    );
    let fs = card.mount();
    assert_eq!(
        tree(&fs)["a book with a rather long name, volume two.epub"],
        data(700, 2)
    );
}

/// A card whose root holds file `across.bin` with its file entry in the
/// last slot of a block and its stream entry in the next, as a computer
/// may place it.
fn straddling() -> Card {
    let card = Card::new(4 << 20, 4096);
    {
        let fs = card.mount();
        let root = fs.root_dir();
        // Slots 2 to 14 of the root's first block, then the set at 15.
        for n in 0..3 {
            write(&root, &format!("filler{n}.x"), &[n]);
        }
        write(&root, "a name of four entries", &[9]);
        fs.flush().unwrap();
        let (units, hash) = encode::<io::Error>("across.bin").unwrap();
        let set = build(&units, hash, &Fresh::empty(ATTR_ARCHIVE));
        let at = fs.cluster_offset(fs.root()).unwrap() + 480;
        let mut slot = [0u8; 1];
        fs.read_at(at, &mut slot).unwrap();
        assert_eq!(slot[0], 0, "the slot is the root's end");
        fs.write_at(at, set.used()).unwrap();
        fs.flush().unwrap();
        write(&root, "across.bin", &data(5000, 6));
        fs.flush().unwrap();
        let entry = root.find("across.bin").unwrap().unwrap();
        assert_eq!(entry.loc.at % 512, 480);
    }
    fsck(&card.image());
    card
}

#[test]
fn a_set_across_two_blocks_is_changed_whole() {
    let card = straddling();
    every_cut(
        &card,
        |fs| write(&fs.root_dir(), "across.bin", &data(9000, 6)[5000..]),
        |before, after, now| {
            old_or_new("across.bin")(before, after, now);
        },
    );
    assert_eq!(tree(&card.mount())["across.bin"], data(9000, 6));
    // Cut between its second and its third write, the first read finishes
    // the set for every other reader: its checksum is right on the card.
    let card = straddling();
    let states = card.cuts(|fs| write(&fs.root_dir(), "across.bin", &data(9000, 6)[5000..]));
    let mut finished = 0;
    for state in states {
        let cut = Card::of(state);
        let fs = cut.mount();
        let Some(entry) = fs.root_dir().find("across.bin").unwrap() else {
            panic!("the file is gone");
        };
        drop(entry);
        fs.flush().unwrap();
        let mut set = Set::new(3);
        let at = fs.cluster_offset(fs.root()).unwrap() + 480;
        fs.read_at(at, &mut set.bytes[..96]).unwrap();
        assert_eq!(layout::set_checksum(set.used()), u16_at(&set.bytes, 2));
        finished += usize::from(cut.log.borrow().len() == 1);
    }
    assert_eq!(finished, 1, "one cut leaves the set to finish");
}

#[test]
fn a_set_with_a_wrong_checksum_is_left_alone() {
    let card = filled(512);
    let fs = card.mount();
    let root = fs.root_dir();
    let entry = root.find("first.txt").unwrap().unwrap();
    // Its size changed without its checksum, as nothing here writes it.
    let stream = fs.entry_pos(&entry.loc, 1).unwrap();
    fs.write_at(stream + 24, &100u64.to_le_bytes()).unwrap();
    let before = card.image();
    assert!(root.find("first.txt").unwrap().is_none());
    write(&root, "another.txt", &data(50, 1));
    fs.flush().unwrap();
    let image = card.image();
    let head = fs.entry_pos(&entry.loc, 0).unwrap() as usize;
    assert_eq!(image[head..head + 96], before[head..head + 96]);
    assert!(fs.is_taken(entry.first).unwrap());
}

#[test]
fn a_set_that_runs_past_the_folder_end_is_left_alone() {
    let card = filled(512);
    let fs = card.mount();
    // The root's last cluster, its free slots taken by entries not in use, and in its last slot
    // a file entry that counts two entries more: past the end of the root's chain.
    let mut last = fs.root();
    while let Some(next) = fs.fat_next(last).unwrap() {
        last = next;
    }
    let base = fs.cluster_offset(last).unwrap();
    let slot = base + fs.cluster_size() - ENTRY as u64;
    for at in (base..slot).step_by(ENTRY) {
        let mut kind = [0u8; 1];
        fs.read_at(at, &mut kind).unwrap();
        if kind[0] == 0 {
            fs.write_at(at, &[ENTRY_UNUSED]).unwrap();
        }
    }
    fs.write_at(slot, &[ENTRY_FILE, 2]).unwrap();
    fs.flush().unwrap();
    let before = card.image();
    let files = tree(&fs);
    assert_eq!(files["first.txt"], data(700, 1));
    assert_eq!(files.len(), 5);
    let root = fs.root_dir();
    write(&root, "another.txt", &data(50, 1));
    fs.flush().unwrap();
    let at = slot as usize;
    assert_eq!(card.image()[at..at + ENTRY], before[at..at + ENTRY]);
    assert_eq!(tree(&fs)["another.txt"], data(50, 1));
    // In a folder of one cluster (its size says where it ends), a whole set whose file entry is
    // its last slot and whose other entries are the next cluster's first bytes (another file's).
    let books = root.find("books").unwrap().unwrap();
    assert!(books.contiguous);
    let base = fs.cluster_offset(books.first).unwrap();
    let slot = base + fs.cluster_size() - ENTRY as u64;
    for at in (base..slot).step_by(ENTRY) {
        let mut kind = [0u8; 1];
        fs.read_at(at, &mut kind).unwrap();
        if kind[0] == 0 {
            fs.write_at(at, &[ENTRY_UNUSED]).unwrap();
        }
    }
    let (units, hash) = encode::<io::Error>("hidden.txt").unwrap();
    let set = build(&units, hash, &Fresh::empty(ATTR_ARCHIVE));
    fs.write_at(slot, &set.used()[..ENTRY]).unwrap();
    fs.write_at(slot + ENTRY as u64, &set.used()[ENTRY..])
        .unwrap();
    fs.flush().unwrap();
    let names: Vec<String> = books
        .to_dir()
        .iter()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["inside.txt"]);
}

#[test]
fn the_bitmap_is_looked_for_and_walked_only_as_far_as_the_volume_needs() {
    // A bitmap entry whose length is far past what the volume's clusters need: the clusters
    // that hold their bits are walked, no more.
    let card = filled(4096);
    let root_slots = |fs: &Fs| {
        let base = fs.cluster_offset(fs.root()).unwrap();
        (base..base + fs.cluster_size()).step_by(ENTRY)
    };
    let kind_at = |fs: &Fs, at: u64| {
        let mut kind = [0u8; 1];
        fs.read_at(at, &mut kind).unwrap();
        kind[0]
    };
    {
        let fs = card.mount();
        let at = root_slots(&fs)
            .find(|&at| kind_at(&fs, at) == layout::ENTRY_BITMAP)
            .unwrap();
        fs.write_at(at + 24, &u64::MAX.to_le_bytes()).unwrap();
        fs.flush().unwrap();
    }
    assert_eq!(tree(&card.mount())["first.txt"], data(700, 1));
    // A root with no bitmap entry and no end, whose FAT runs in a loop: read as far as the
    // bound, not through every cluster of the volume.
    {
        let fs = card.mount();
        for at in root_slots(&fs) {
            if matches!(kind_at(&fs, at), 0 | layout::ENTRY_BITMAP) {
                fs.write_at(at, &[ENTRY_UNUSED]).unwrap();
            }
        }
        fs.fat_set(fs.root(), fs.root()).unwrap();
        fs.flush().unwrap();
    }
    card.reads.set(0);
    assert!(matches!(
        ExFat::mount(card.disk()),
        Err(Error::CorruptedFileSystem)
    ));
    assert!(card.reads.get() < 9_000, "{} reads", card.reads.get());
}

#[test]
fn the_card_is_marked_in_use_only_while_it_is() {
    let card = filled(512);
    let flags = |card: &Card| u16_at(&card.image(), 106) & layout::VOLUME_DIRTY;
    assert_eq!(flags(&card), 0);
    let fs = card.mount();
    write(&fs.root_dir(), "x", &[1]);
    assert_ne!(flags(&card), 0);
    fs.flush().unwrap();
    assert_eq!(flags(&card), 0);
    // A card that came in use stays so: whoever left it so is not done.
    card.data.borrow_mut()[106] |= 2;
    let fs = card.mount();
    write(&fs.root_dir(), "y", &[1]);
    fs.flush().unwrap();
    assert_ne!(flags(&card), 0);
}

#[test]
fn a_full_card_says_so_and_keeps_its_files() {
    let card = Card::new(1 << 20, 4096);
    let fs = card.mount();
    let root = fs.root_dir();
    let mut file = root.create_file("big.bin").unwrap();
    let chunk = data(4096, 1);
    let mut written = 0usize;
    let full = loop {
        match file.write(&chunk) {
            Ok(n) => written += n,
            Err(e) => break e,
        }
    };
    assert!(matches!(full, Error::NotEnoughSpace));
    file.flush().unwrap();
    drop(file);
    fs.flush().unwrap();
    assert_eq!(tree(&fs)["big.bin"].len(), written);
    assert_eq!(fs.count_free().unwrap(), 0);
    fsck(&card.image());
    root.remove("big.bin").unwrap();
    fs.flush().unwrap();
    assert_eq!(fs.count_free().unwrap() as usize, written / 4096);
    fsck(&card.image());
}

#[test]
fn names_are_checked_and_compared_without_case() {
    let card = Card::new(2 << 20, 512);
    let fs = card.mount();
    let root = fs.root_dir();
    write(&root, "Größe Ünïcode Ελληνικά.txt", &[1]);
    assert!(root.find("GRÖSSE ÜNÏCODE ΕΛΛΗΝΙΚΆ.TXT").unwrap().is_none());
    assert!(root.find("GRÖßE ÜNÏCODE ΕΛΛΗΝΙΚΆ.TXT").unwrap().is_some());
    let long = "n".repeat(255);
    write(&root, &long, &[2]);
    assert_eq!(root.find(&long).unwrap().unwrap().file_name(), long);
    for bad in ["", ".", "..", "a/b", "a:b", "a\u{1}b", &"n".repeat(256)] {
        assert!(root.create_file(bad).is_err(), "{bad:?}");
    }
    fs.flush().unwrap();
    fsck(&card.image());
}
