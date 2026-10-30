//! The storage on exFAT cards: images made by exfatprogs' `mkfs.exfat`
//! and filled by exfat-fuse (`tests/images/make.sh`), read as they are,
//! written to, mounted again, and checked by hadris-fat's checker (and by
//! `fsck.exfat` where it is installed, as on CI).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{Bytes, exfat, exfat_card, image, names, read};
use paperpaw_fat::{BlockDisk, ExDisk, ExFatStorage, SharedDisk};
use paperpaw_platform::{Storage, StorageError};

fn small() -> Bytes {
    exfat_card()
}

fn filled() -> Bytes {
    image(include_bytes!("images/filled.sparse"))
}

type Card = ExFatStorage<Bytes>;

/// The storage on `bytes`, as the X4 mounts it.
fn mount(bytes: &Bytes) -> Card {
    exfat(bytes)
}

/// What hadris-fat's checker and `fsck.exfat` (where installed) find
/// wrong on `bytes`: nothing.
fn assert_clean(bytes: &Bytes) {
    let (disk, _) = BlockDisk::find(bytes.clone()).unwrap();
    let mut dev = ExDisk::new(SharedDisk::new(disk));
    let mut scratch = vec![0u8; 8192];
    let mut findings = Vec::new();
    hadris_fat::exfat::sync::check(&mut dev, &mut scratch, |f| {
        findings.push(format!("{f:?}"));
    })
    .unwrap();
    assert_eq!(findings, Vec::<String>::new());
    fsck(&bytes.0.borrow());
}

/// `fsck.exfat -n` over the image, where it is installed. With
/// `PAPERPAW_EXFAT_IMAGES` set to a folder, the image is kept there too,
/// to check elsewhere (`fsck.exfat` in a container).
fn fsck(image: &[u8]) {
    // A volume from the first block on (fsck.exfat takes no partitions).
    if &image[3..11] != b"EXFAT   " {
        return;
    }
    if let Some(dir) = std::env::var_os("PAPERPAW_EXFAT_IMAGES") {
        let path = std::path::Path::new(&dir).join(format!("{:016x}.img", fnv(image)));
        std::fs::write(path, image).unwrap();
    }
    let dir = std::env::temp_dir().join(format!("paperpaw-exfat-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{:x}.img", fnv(image)));
    std::fs::write(&path, image).unwrap();
    let run = std::process::Command::new("fsck.exfat")
        .arg("-n")
        .arg(&path)
        .output();
    std::fs::remove_file(&path).ok();
    if let Ok(out) = run {
        assert!(
            out.status.success(),
            "fsck.exfat: {}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// `seq 1 n`, as `make.sh` wrote the files.
fn seq(n: u32) -> Vec<u8> {
    (1..=n)
        .flat_map(|i| format!("{i}\n").into_bytes())
        .collect()
}

const LONG: &str = "books/Ünïcödé Földer/a book with a very long name that goes on and on and on beyond eleven chars.epub";

#[test]
fn a_card_filled_elsewhere_reads_as_it_is() {
    let bytes = filled();
    let card = mount(&bytes);
    assert_eq!(names(&card, ""), [".crosspoint/", "big.bin", "books/"]);
    assert_eq!(names(&card, "books"), ["a.txt", "Ünïcödé Földer/"]);
    assert_eq!(read(&card, "books/a.txt"), b"hello");
    assert_eq!(read(&card, "big.bin"), seq(3000));
    assert_eq!(read(&card, LONG), seq(6000));
    assert_eq!(names(&card, ".crosspoint/cache").len(), 40);
    // Without regard to case, through the up-case table.
    assert_eq!(read(&card, &LONG.to_uppercase()), seq(6000));
    assert_eq!(card.size("BOOKS/A.TXT"), Ok(5));
    // The cluster heap mkfs.exfat left of 8 MiB, and what the files take.
    assert_eq!(card.total_space(), Some(6 << 20));
    let free = card.free_space().unwrap();
    assert!((5 << 20..6 << 20).contains(&free), "{free}");
    drop(card);
    assert_clean(&bytes);
}

#[test]
fn files_written_moved_and_removed_are_so_after_a_remount() {
    let bytes = small();
    let card = mount(&bytes);
    card.write(".paperpaw/plugins/reader/state.ini", b"page = 1")
        .unwrap();
    card.write(".paperpaw/plugins/reader/state.ini", b"page = 2")
        .unwrap();
    card.write("books/b.txt", &seq(5000)).unwrap();
    card.append("logs/a.log", b"one\n").unwrap();
    card.append("logs/a.log", b"two\n").unwrap();
    card.rename("books/b.txt", "books/moved/c.txt").unwrap();
    card.write("gone.txt", b"x").unwrap();
    card.remove("gone.txt").unwrap();
    card.remove("never.txt").unwrap();
    card.create_dir("empty/inner").unwrap();
    card.write("tree/a/b.txt", b"b").unwrap();
    card.remove_dir_all("tree").unwrap();
    card.sync().unwrap();
    drop(card);
    assert_clean(&bytes);
    let card = mount(&bytes);
    assert_eq!(
        read(&card, ".paperpaw/plugins/reader/state.ini"),
        b"page = 2"
    );
    assert_eq!(read(&card, "books/moved/c.txt"), seq(5000));
    assert_eq!(read(&card, "logs/a.log"), b"one\ntwo\n");
    assert_eq!(card.size("books/b.txt"), Err(StorageError::NotFound));
    assert_eq!(card.size("gone.txt"), Err(StorageError::NotFound));
    assert_eq!(
        names(&card, ""),
        [".paperpaw/", "books/", "empty/", "logs/"]
    );
    assert_eq!(names(&card, "empty"), ["inner/"]);
}

#[test]
fn a_card_with_a_partition_table_and_large_clusters_mounts() {
    for bytes in [
        image(include_bytes!("images/mbr.sparse")),
        image(include_bytes!("images/big32k.sparse")),
    ] {
        let card = mount(&bytes);
        card.write("books/a.txt", &seq(20_000)).unwrap();
        card.sync().unwrap();
        drop(card);
        assert_clean(&bytes);
        assert_eq!(read(&mount(&bytes), "books/a.txt"), seq(20_000));
    }
}

#[test]
fn the_storage_own_names_are_not_listed_or_taken() {
    let bytes = small();
    let card = mount(&bytes);
    card.write("a.txt", b"a").unwrap();
    assert_eq!(names(&card, ""), ["a.txt"]);
    assert_eq!(
        card.write("a.txt~pp.new", b"x"),
        Err(StorageError::InvalidPath)
    );
    assert_eq!(card.size("~PPMOVE.BIN"), Err(StorageError::InvalidPath));
}

// Cut off at any point.

/// A block written to a card: its index, its bytes.
type Written = (u32, [u8; 512]);

/// A card as blocks in memory with every block written to it in order.
#[derive(Clone)]
struct Recorded {
    data: Rc<RefCell<Vec<u8>>>,
    writes: Rc<RefCell<Vec<Written>>>,
}

impl paperpaw_fat::Blocks for Recorded {
    fn read_block(
        &mut self,
        index: u32,
        block: &mut [u8; 512],
    ) -> Result<(), paperpaw_fat::DiskError> {
        let at = index as usize * 512;
        block.copy_from_slice(&self.data.borrow()[at..at + 512]);
        Ok(())
    }

    fn write_block(
        &mut self,
        index: u32,
        block: &[u8; 512],
    ) -> Result<(), paperpaw_fat::DiskError> {
        let at = index as usize * 512;
        self.data.borrow_mut()[at..at + 512].copy_from_slice(block);
        self.writes.borrow_mut().push((index, *block));
        Ok(())
    }

    fn blocks(&mut self) -> Result<u32, paperpaw_fat::DiskError> {
        Ok((self.data.borrow().len() / 512) as u32)
    }
}

/// The card `image` after `op` on it, cut off after each block it wrote
/// (and before the first).
fn cut_images(image: Vec<u8>, op: impl FnOnce(&ExFatStorage<Recorded>)) -> Vec<Vec<u8>> {
    let card = Recorded {
        data: Rc::new(RefCell::new(image.clone())),
        writes: Rc::default(),
    };
    let (disk, _) = BlockDisk::find(card.clone()).unwrap();
    let storage = ExFatStorage::mount(SharedDisk::new(disk)).unwrap();
    op(&storage);
    drop(storage);
    let mut now = image;
    let mut cuts = vec![now.clone()];
    for (index, block) in card.writes.borrow().iter() {
        let at = *index as usize * 512;
        now[at..at + 512].copy_from_slice(block);
        cuts.push(now.clone());
    }
    cuts
}

/// A card prepared by `setup`, synced.
fn prepared(setup: impl FnOnce(&Card)) -> Vec<u8> {
    let bytes = small();
    let card = mount(&bytes);
    setup(&card);
    card.sync().unwrap();
    drop(card);
    bytes.0.borrow().clone()
}

/// The bytes of a card image, to mount.
fn card_of(image: &[u8]) -> Bytes {
    Bytes(Rc::new(RefCell::new(image.to_vec())))
}

/// All of the file at `path`, as its size says.
fn whole(s: &dyn Storage, path: &str) -> Result<Vec<u8>, StorageError> {
    let mut data = vec![0u8; s.size(path)? as usize];
    let mut at = 0;
    while at < data.len() {
        match s.read_at(path, at as u64, &mut data[at..])? {
            0 => return Err(StorageError::Io),
            n => at += n,
        }
    }
    Ok(data)
}

/// What hadris-fat's checker finds on `image`, but clusters a cut left
/// taken by no file, or linked past a file's or folder's length before
/// the new length was written (lost, not wrong: nothing reads them), and
/// the dirty
/// flag (set before the first write after a mount, it stays where a cut
/// left it, as the specification asks: a checker clears it, having
/// looked at the whole card).
fn findings(image: &[u8]) -> Vec<String> {
    let (disk, _) = BlockDisk::find(card_of(image)).unwrap();
    let mut dev = ExDisk::new(SharedDisk::new(disk));
    let mut scratch = vec![0u8; 8192];
    let mut found = Vec::new();
    hadris_fat::exfat::sync::check(&mut dev, &mut scratch, |f| found.push(format!("{f:?}")))
        .unwrap();
    found.retain(|f| {
        let lost = f.contains("no allocation reaches") || f.contains("longer than its data length");
        !lost && !f.contains("marked dirty")
    });
    found
}

const A: &str = "books/A rather long name of a book that takes several entries.epub";

fn old_and_new() -> (Vec<u8>, Vec<u8>) {
    let old: Vec<u8> = (0..9000u32).map(|i| (i % 251) as u8).collect();
    let new: Vec<u8> = (0..14_000u32).map(|i| (i % 241) as u8 ^ 0x5a).collect();
    (old, new)
}

/// Entries before the file written in its folder: each of 0 to 15 puts
/// its entry set at another place in a block (16 entries), so that every
/// way it can lie across two blocks is cut off.
const SHIFTS: std::ops::Range<usize> = 0..16;

/// Files beside the one written: `shift` entries before it in its folder
/// (see [`SHIFTS`]), and so many there that its entries take several
/// blocks, with gaps between.
fn beside(s: &Card, shift: usize) {
    // Entry sets of three entries, then one of four or five.
    for n in 0..shift / 3 {
        s.write(&format!("books/s{n:02}"), b"s").unwrap();
    }
    match shift % 3 {
        1 => s.write("books/t with a name of two.txt", b"t").unwrap(),
        2 => s
            .write(
                "books/u with a name that takes three entries here.txt",
                b"u",
            )
            .unwrap(),
        _ => {}
    }
    s.write("books/b.txt", b"beside").unwrap();
    s.write("notes.txt", b"notes").unwrap();
    for n in 0..30 {
        s.write(&format!("books/f{n:02}.txt"), b"f").unwrap();
    }
    for n in (0..30).step_by(3) {
        s.remove(&format!("books/f{n:02}.txt")).unwrap();
    }
}

fn assert_beside(s: &dyn Storage, cut: usize) {
    assert_eq!(
        whole(s, "books/b.txt").as_deref(),
        Ok(&b"beside"[..]),
        "{cut}"
    );
    assert_eq!(whole(s, "notes.txt").as_deref(), Ok(&b"notes"[..]), "{cut}");
    for n in (0..30).filter(|n| n % 3 != 0) {
        let path = format!("books/f{n:02}.txt");
        assert_eq!(whole(s, &path).as_deref(), Ok(&b"f"[..]), "{cut}");
    }
}

/// After a cut: the next write of `path` and a sync leave a card the
/// checker finds nothing wrong with (see [`findings`]), and no name of
/// the moment is left.
fn assert_heals(image: &[u8], path: &str, cut: usize) {
    let bytes = card_of(image);
    let card = mount(&bytes);
    card.write(path, b"again").unwrap();
    card.sync().unwrap();
    assert_eq!(whole(&card, path).as_deref(), Ok(&b"again"[..]), "{cut}");
    drop(card);
    assert_eq!(findings(&bytes.0.borrow()), Vec::<String>::new(), "{cut}");
    let card = mount(&bytes);
    let mut all = Vec::new();
    for dir in ["", "books"] {
        card.list(dir, &mut |e| all.push(e.name.to_string()))
            .unwrap();
    }
    assert!(!all.iter().any(|n| n.contains("~pp")), "{cut}: {all:?}");
}

/// Runs `check` on the card after `op`, cut off after each block, for
/// every shift of the file's entry set; `setup` prepares the card.
fn each_cut(
    setup: impl Fn(&Card),
    op: impl Fn(&ExFatStorage<Recorded>),
    check: impl Fn(&Card, &str),
) {
    for shift in SHIFTS {
        let image = prepared(|s| {
            beside(s, shift);
            setup(s);
        });
        let cuts = cut_images(image, &op);
        assert!(cuts.len() > 2, "{} blocks written", cuts.len());
        for (n, image) in cuts.iter().enumerate() {
            let card = mount(&card_of(image));
            let at = format!("shift {shift}, cut {n} of {}", cuts.len() - 1);
            check(&card, &at);
            assert_beside(&card, n);
            drop(card);
            assert_heals(image, A, n);
        }
    }
}

#[test]
fn a_file_written_anew_is_old_or_new_after_any_cut() {
    let (old, new) = old_and_new();
    each_cut(
        |s| s.write(A, &old).unwrap(),
        |s| s.write(A, &new).unwrap(),
        |card, at| {
            let a = whole(card, A);
            assert!(
                a == Ok(old.clone()) || a == Ok(new.clone()),
                "{at}: {:?}",
                a.map(|a| a.len())
            );
        },
    );
}

#[test]
fn a_new_file_is_whole_or_not_there_after_any_cut() {
    let (_, new) = old_and_new();
    each_cut(
        |_| {},
        |s| s.write(A, &new).unwrap(),
        |card, at| {
            let a = whole(card, A);
            assert!(
                a == Ok(new.clone()) || a == Err(StorageError::NotFound),
                "{at}: {:?}",
                a.map(|a| a.len())
            );
        },
    );
}

#[test]
fn a_file_moved_is_at_one_place_whole_after_any_cut() {
    let (old, new) = old_and_new();
    for to in [
        "books/renamed.epub",
        "elsewhere/deeper/moved.epub",
        "notes.txt",
    ] {
        each_cut(
            |s| {
                s.write(A, &old).unwrap();
                s.write("elsewhere/x.txt", &new).unwrap();
            },
            |s| s.rename(A, to).unwrap(),
            |card, at| {
                let at_old = whole(card, A).map(|a| a.len());
                let at_new = whole(card, to).map(|a| a.len());
                let moved = at_old == Err(StorageError::NotFound) && at_new == Ok(old.len());
                let stayed = at_old == Ok(old.len());
                assert!(moved || stayed, "{to} {at}: {at_old:?} {at_new:?}");
                if moved {
                    assert_eq!(whole(card, to), Ok(old.clone()), "{to} {at}");
                }
                assert_eq!(whole(card, "elsewhere/x.txt"), Ok(new.clone()), "{at}");
                // Taken back to its old content for the files beside.
                if to == "notes.txt" {
                    card.write("notes.txt", b"notes").unwrap();
                }
            },
        );
    }
}

#[test]
fn a_file_removed_is_whole_or_gone_after_any_cut() {
    let (old, _) = old_and_new();
    each_cut(
        |s| s.write(A, &old).unwrap(),
        |s| s.remove(A).unwrap(),
        |card, at| {
            let a = whole(card, A).map(|a| a.len());
            assert!(
                a == Ok(old.len()) || a == Err(StorageError::NotFound),
                "{at}: {a:?}"
            );
        },
    );
}

#[test]
fn appended_to_is_as_before_or_longer_after_any_cut() {
    const LOG: &str = "books/a log with a name of three entries.log";
    each_cut(
        |s| s.append(LOG, &seq(500)).unwrap(),
        |s| {
            s.append(LOG, &seq(3000)).unwrap();
            s.sync().unwrap();
        },
        |card, at| {
            let log = whole(card, LOG).unwrap();
            assert!(log.starts_with(&seq(500)), "{at}: {}", log.len());
        },
    );
}

/// Whether a name of a replacement made whole (`~pp.new`) is in the
/// folder `books` of the card: read as a computer would, without any
/// settling.
fn has_new(image: &[u8]) -> bool {
    use hadris_fs::{DirCursor, MountOptions, Name};
    let (disk, _) = BlockDisk::find(card_of(image)).unwrap();
    let options = MountOptions::new().with_clock(&Later);
    let mut fs =
        hadris_fat::exfat::sync::ExFatFs::mount(ExDisk::new(SharedDisk::new(disk)), options)
            .unwrap();
    let root = fs.root();
    let Ok(books) = fs.lookup(root, Name::new("books")) else {
        return false;
    };
    let mut cursor = DirCursor::START;
    while let Ok(Some(entry)) = fs.readdir(books, cursor) {
        cursor = entry.next_cursor();
        if String::from_utf8_lossy(entry.name().as_bytes()).ends_with("~pp.new") {
            return true;
        }
    }
    false
}

/// A computer's clock: not the X4's 1980.
struct Later;

impl hadris_fs::Clock for Later {
    fn now(&self) -> hadris_fs::DateTime {
        hadris_fs::DateTime::from_unix_seconds(1_800_000_000).unwrap()
    }
}

/// What a computer does to `books/<name>` with the card put in it after
/// a cut: the file is written anew, with `data`, and the card unmounted
/// cleanly. Nothing of PaperPaw's settling runs.
fn changed_elsewhere(image: &mut Vec<u8>, name: &str, data: &[u8]) {
    use hadris_fs::{MountOptions, Name, OpenMode, SetAttr};
    let bytes = card_of(image);
    let (disk, _) = BlockDisk::find(bytes.clone()).unwrap();
    let shared = SharedDisk::new(disk);
    let options = MountOptions::new().with_clock(&Later);
    let mut fs =
        hadris_fat::exfat::sync::ExFatFs::mount(ExDisk::new(shared.clone()), options).unwrap();
    let root = fs.root();
    let books = fs.lookup(root, Name::new("books")).unwrap();
    // The file may be gone on the card (cut off in a replacement).
    let _ = fs.unlink(books, Name::new(name));
    let node = fs.create(books, Name::new(name), &SetAttr::new()).unwrap();
    fs.open(node, OpenMode::Write).unwrap();
    let mut done = 0;
    while done < data.len() {
        done += fs.write(node, done as u64, &data[done..]).unwrap();
    }
    fs.close(node).unwrap();
    fs.sync().unwrap();
    drop(fs);
    shared.flush_all().unwrap();
    *image = bytes.0.borrow().clone();
}

const A_NAME: &str = "A rather long name of a book that takes several entries.epub";

#[test]
fn a_replacement_cut_off_after_it_was_whole_takes_its_place() {
    let (old, new) = old_and_new();
    let mut seen = 0;
    for shift in SHIFTS {
        let image = prepared(|s| {
            beside(s, shift);
            s.write(A, &old).unwrap();
        });
        for (n, cut) in cut_images(image, |s| s.write(A, &new).unwrap())
            .iter()
            .enumerate()
        {
            if !has_new(cut) {
                continue;
            }
            seen += 1;
            let card = mount(&card_of(cut));
            let at = format!("shift {shift}, cut {n}");
            assert_eq!(whole(&card, A), Ok(new.clone()), "{at}");
            assert_beside(&card, n);
        }
    }
    assert!(seen > 16, "{seen} cuts with a replacement made whole");
}

/// Every cut of a replacement of `A`, the file written anew elsewhere
/// after it; with `damage_other`, a bad checksum in the set of a file
/// beside it as well (a torn set that is not `A`'s). How many times
/// theirs was kept, and the replacement made.
fn replacement_against_a_change_elsewhere(damage_other: bool) -> (usize, usize) {
    let (old, new) = old_and_new();
    let theirs: Vec<u8> = (0..5000u32).map(|i| (i % 199) as u8 ^ 0x33).collect();
    let (mut kept, mut replaced) = (0, 0);
    for shift in SHIFTS {
        let image = prepared(|s| {
            beside(s, shift);
            s.write(A, &old).unwrap();
        });
        for (n, cut) in cut_images(image, |s| s.write(A, &new).unwrap())
            .iter()
            .enumerate()
        {
            if !has_new(cut) {
                continue;
            }
            let mut edited = cut.clone();
            changed_elsewhere(&mut edited, A_NAME, &theirs);
            if damage_other {
                let at = set_offset(&edited, "b.txt");
                edited[at + 2] ^= 0xFF;
            }
            let bytes = card_of(&edited);
            let card = mount(&bytes);
            let at = format!("shift {shift}, cut {n}");
            // Theirs, or (where the replacement was cut off torn, which
            // is settled as it was) the replacement: whole, never a mix.
            let a = whole(&card, A);
            if a == Ok(theirs.clone()) {
                kept += 1;
            } else {
                assert_eq!(a, Ok(new.clone()), "{at}");
                replaced += 1;
            }
            assert_beside(&card, n);
            card.sync().unwrap();
            drop(card);
            assert_eq!(findings(&bytes.0.borrow()), Vec::<String>::new(), "{at}");
            let card = mount(&bytes);
            let mut all = Vec::new();
            for dir in ["", "books"] {
                card.list(dir, &mut |e| all.push(e.name.to_string()))
                    .unwrap();
            }
            assert!(!all.iter().any(|n| n.contains("~pp")), "{at}: {all:?}");
        }
    }
    assert!(kept > 16, "kept theirs {kept} times, replaced {replaced}");
    (kept, replaced)
}

#[test]
fn a_replacement_cut_off_does_not_overwrite_a_change_made_elsewhere() {
    replacement_against_a_change_elsewhere(false);
}

#[test]
fn a_torn_set_beside_does_not_end_the_protection_of_a_change_made_elsewhere() {
    // Only the file's own torn set (the replacement's cut) lets the
    // replacement take its place; another file's does not.
    assert_eq!(
        replacement_against_a_change_elsewhere(true),
        replacement_against_a_change_elsewhere(false)
    );
}

// A transient error while settling what a cut left.

/// A card that fails every read and write after the `left`-th, until
/// `left` is set to `usize::MAX` again: a card taken out, or a bus error.
#[derive(Clone)]
struct Flaky {
    bytes: Bytes,
    left: Rc<std::cell::Cell<usize>>,
}

impl Flaky {
    fn pass(&self) -> Result<(), paperpaw_fat::DiskError> {
        match self.left.get() {
            usize::MAX => Ok(()),
            0 => Err(paperpaw_fat::DiskError),
            n => {
                self.left.set(n - 1);
                Ok(())
            }
        }
    }
}

impl paperpaw_fat::Blocks for Flaky {
    fn read_block(
        &mut self,
        index: u32,
        block: &mut [u8; 512],
    ) -> Result<(), paperpaw_fat::DiskError> {
        self.pass()?;
        self.bytes.read_block(index, block)
    }

    fn write_block(
        &mut self,
        index: u32,
        block: &[u8; 512],
    ) -> Result<(), paperpaw_fat::DiskError> {
        self.pass()?;
        self.bytes.write_block(index, block)
    }

    fn blocks(&mut self) -> Result<u32, paperpaw_fat::DiskError> {
        self.bytes.blocks()
    }
}

/// On the card `image`, a first call (`first`) that fails at its `k`-th
/// block read or written, the card sound again, then `after` on the same
/// mount, without a remount. Whether the call came through, i.e. `k` was
/// past all its blocks.
fn fails_then_goes_on(
    image: &[u8],
    k: usize,
    first: impl Fn(&dyn Storage) -> bool,
    after: impl Fn(&ExFatStorage<Flaky>),
) -> bool {
    let flaky = Flaky {
        bytes: card_of(image),
        left: Rc::new(std::cell::Cell::new(usize::MAX)),
    };
    let (disk, _) = BlockDisk::find(flaky.clone()).unwrap();
    let card = ExFatStorage::mount(SharedDisk::new(disk)).unwrap();
    flaky.left.set(k);
    let came_through = first(&card);
    flaky.left.set(usize::MAX);
    after(&card);
    came_through
}

#[test]
fn a_replacement_is_settled_after_an_error_in_the_same_mount() {
    let (old, new) = old_and_new();
    let mut tried = 0;
    for shift in [0, 5, 11] {
        let image = prepared(|s| {
            beside(s, shift);
            s.write(A, &old).unwrap();
        });
        for (n, cut) in cut_images(image, |s| s.write(A, &new).unwrap())
            .iter()
            .enumerate()
        {
            if !has_new(cut) {
                continue;
            }
            for k in 0.. {
                let at = format!("shift {shift}, cut {n}, error at {k}");
                let through = fails_then_goes_on(
                    cut,
                    k,
                    |card| card.size(A).is_ok(),
                    |card| {
                        // The same mount goes on: the replacement is made.
                        assert_eq!(whole(card, A), Ok(new.clone()), "{at}");
                        assert_beside(card, n);
                        let mut all = Vec::new();
                        for dir in ["", "books"] {
                            card.list(dir, &mut |e| all.push(e.name.to_string()))
                                .unwrap();
                        }
                        assert!(!all.iter().any(|n| n.contains("~pp")), "{at}: {all:?}");
                    },
                );
                tried += 1;
                if through {
                    break;
                }
                assert!(k < 400, "{at}: never through");
            }
        }
    }
    assert!(tried > 50, "{tried} errors tried");
}

#[test]
fn a_move_is_settled_after_an_error_in_the_same_mount() {
    let (old, new) = old_and_new();
    let to = "elsewhere/deeper/moved.epub";
    let mut tried = 0;
    for shift in [0, 7] {
        let image = prepared(|s| {
            beside(s, shift);
            s.write(A, &old).unwrap();
            s.write("elsewhere/x.txt", &new).unwrap();
        });
        for (n, cut) in cut_images(image, |s| s.rename(A, to).unwrap())
            .iter()
            .enumerate()
        {
            for k in 0.. {
                let at = format!("shift {shift}, cut {n}, error at {k}");
                let through = fails_then_goes_on(
                    cut,
                    k,
                    |card| card.size("elsewhere/x.txt").is_ok(),
                    |card| {
                        let at_old = whole(card, A).map(|a| a.len());
                        let at_new = whole(card, to).map(|a| a.len());
                        let moved =
                            at_old == Err(StorageError::NotFound) && at_new == Ok(old.len());
                        let stayed =
                            at_old == Ok(old.len()) && at_new == Err(StorageError::NotFound);
                        assert!(moved || stayed, "{at}: {at_old:?} {at_new:?}");
                        assert_eq!(whole(card, "elsewhere/x.txt"), Ok(new.clone()), "{at}");
                        assert_beside(card, n);
                        // The journal is gone: the next write goes on.
                        card.write("books/after.txt", b"after").unwrap();
                        card.sync().unwrap();
                    },
                );
                tried += 1;
                if through {
                    break;
                }
                assert!(k < 400, "{at}: never through");
            }
        }
    }
    assert!(tried > 100, "{tried} errors tried");
}

// A set another system damaged.

/// The offset of the File entry set named `name` (short enough for one
/// name entry) in the root of the card `image`.
fn set_offset(image: &[u8], name: &str) -> usize {
    let units: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
    (0..image.len() - 96)
        .step_by(32)
        .find(|&p| {
            image[p] == 0x85
                && image[p + 32] == 0xC0
                && image[p + 64] == 0xC1
                && image[p + 66..p + 66 + units.len()] == units[..]
        })
        .expect("the set")
}

/// A card with `damaged.txt` and `kept.txt` whose set of the first has a
/// broken checksum and `change` made to its stream entry, and what the
/// storage then lists.
fn damaged(change: impl Fn(&mut [u8], u32)) -> (Vec<String>, u32, Bytes) {
    let bytes = small();
    let card = mount(&bytes);
    card.write("damaged.txt", &[7u8; 3000]).unwrap();
    card.write("kept.txt", b"kept").unwrap();
    card.sync().unwrap();
    drop(card);
    let clusters = u32::from_le_bytes(bytes.0.borrow()[92..96].try_into().unwrap());
    {
        let mut image = bytes.0.borrow_mut();
        let at = set_offset(&image, "damaged.txt");
        image[at + 2] ^= 0xFF;
        change(&mut image[at + 32..at + 64], clusters);
    }
    let card = mount(&bytes);
    let listed = names(&card, "");
    assert_eq!(whole(&card, "kept.txt").as_deref(), Ok(&b"kept"[..]));
    let left = card.left_alone();
    drop(card);
    (listed, left, bytes)
}

#[test]
fn a_set_with_a_bad_checksum_and_a_possible_stream_is_sealed_again() {
    // What a cut of the storage's own leaves: the control of the rest.
    let (listed, left, _) = damaged(|_, _| {});
    assert!(listed.contains(&"damaged.txt".to_string()), "{listed:?}");
    assert_eq!(left, 0);
}

/// A way to damage a stream extension entry, given the card's cluster count.
type Damage = fn(&mut [u8], u32);

#[test]
fn a_set_another_system_damaged_is_not_sealed_to_name_clusters_it_may_not_own() {
    let cases: [(&str, Damage); 4] = [
        ("first cluster beyond the card", |s, _| {
            s[20..24].copy_from_slice(&0xFFFF_FF00u32.to_le_bytes())
        }),
        ("first cluster the bitmap calls free", |s, clusters| {
            s[20..24].copy_from_slice(&(clusters + 1).to_le_bytes())
        }),
        ("valid length beyond the length", |s, _| {
            let len = u64::from_le_bytes(s[24..32].try_into().unwrap());
            s[8..16].copy_from_slice(&(len + 1).to_le_bytes())
        }),
        ("length beyond the card", |s, _| {
            s[24..32].copy_from_slice(&(1u64 << 40).to_le_bytes());
            s[8..16].copy_from_slice(&(1u64 << 40).to_le_bytes())
        }),
    ];
    for (what, change) in cases {
        let (listed, left, bytes) = damaged(change);
        assert!(
            !listed.contains(&"damaged.txt".to_string()),
            "{what}: {listed:?}"
        );
        assert_eq!(left, 1, "{what}");
        // The set is as it was, not made valid: still a bad checksum.
        let image = bytes.0.borrow();
        let at = set_offset(&image, "damaged.txt");
        assert_eq!(image[at], 0x85, "{what}");
    }
}
