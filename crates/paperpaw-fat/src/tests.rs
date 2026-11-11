extern crate std;

use super::*;
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use fatfs::{FormatVolumeOptions, FsOptions, StdIoWrapper};
use std::io::Cursor;

type Disk = StdIoWrapper<Cursor<Vec<u8>>>;

/// A fresh FAT disk of 4 MB, formatted.
fn storage() -> FatStorage<Disk> {
    let mut disk = StdIoWrapper::from(Cursor::new(vec![0u8; 4 << 20]));
    fatfs::format_volume(&mut disk, FormatVolumeOptions::new()).unwrap();
    disk.seek(SeekFrom::Start(0)).unwrap();
    let fs = FileSystem::new(disk, FsOptions::new()).unwrap();
    FatStorage::new(Box::leak(Box::new(fs)))
}

fn names(storage: &FatStorage<Disk>, dir: &str) -> Vec<(String, bool, u64)> {
    let mut out = Vec::new();
    storage
        .list(dir, &mut |e| {
            out.push((e.name.to_string(), e.is_dir, e.size))
        })
        .unwrap();
    out.sort();
    out
}

/// A card as blocks in memory, to look at as it is (after a cut),
/// with every block written to it in order.
#[derive(Clone)]
struct Cut {
    data: alloc::rc::Rc<RefCell<Vec<u8>>>,
    writes: alloc::rc::Rc<RefCell<Vec<Written>>>,
    /// Blocks read so far.
    reads: alloc::rc::Rc<core::cell::Cell<usize>>,
}

/// A block written to a card: its index, its bytes.
type Written = (u32, [u8; BLOCK]);

impl Cut {
    fn new(data: Vec<u8>) -> Self {
        Self {
            data: alloc::rc::Rc::new(RefCell::new(data)),
            writes: alloc::rc::Rc::default(),
            reads: alloc::rc::Rc::default(),
        }
    }

    /// A card of 4 MB, formatted with `options`.
    fn formatted(options: FormatVolumeOptions) -> Self {
        Self::formatted_of(4 << 20, options)
    }

    /// A card of `size` bytes, formatted with `options`.
    fn formatted_of(size: usize, options: FormatVolumeOptions) -> Self {
        let mut formatted = StdIoWrapper::from(Cursor::new(vec![0u8; size]));
        fatfs::format_volume(&mut formatted, options).unwrap();
        Self::new(formatted.into_inner().into_inner())
    }

    /// What it holds now.
    fn image(&self) -> Vec<u8> {
        self.data.borrow().clone()
    }

    /// The storage on it as on the X4: the disk shared, its changed
    /// blocks written back by the storage.
    fn storage(&self) -> FatStorage<SharedDisk<Cut>> {
        let disk = SharedDisk::new(BlockDisk::new(self.clone()).unwrap());
        let fs = FileSystem::new(disk.clone(), FsOptions::new()).unwrap();
        FatStorage::new(Box::leak(Box::new(fs))).with_disk(disk)
    }
}

impl Blocks for Cut {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        let at = index as usize * BLOCK;
        block.copy_from_slice(&self.data.borrow()[at..at + BLOCK]);
        self.reads.set(self.reads.get() + 1);
        Ok(())
    }

    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        let at = index as usize * BLOCK;
        self.data.borrow_mut()[at..at + BLOCK].copy_from_slice(block);
        self.writes.borrow_mut().push((index, *block));
        Ok(())
    }

    fn blocks(&mut self) -> Result<u32, DiskError> {
        Ok((self.data.borrow().len() / BLOCK) as u32)
    }
}

/// The storage on the card `bytes`.
fn mounted(bytes: Vec<u8>) -> FatStorage<Disk> {
    let fs = FileSystem::new(StdIoWrapper::from(Cursor::new(bytes)), FsOptions::new()).unwrap();
    FatStorage::new(Box::leak(Box::new(fs)))
}

/// The free bytes rust-fatfs counts on the card now (all of the FAT
/// read where it must).
fn counted(s: &FatStorage<SharedDisk<Cut>>) -> u64 {
    let stats = s.fs.stats().unwrap();
    u64::from(stats.free_clusters()) * u64::from(stats.cluster_size())
}

/// Asks for the free space until it is counted exactly, each ask
/// reading only a part of the FAT; how many asks it took.
fn count_out(card: &Cut, s: &FatStorage<SharedDisk<Cut>>) -> usize {
    for asks in 1..100 {
        let before = card.reads.get();
        s.free_space();
        let read = card.reads.get() - before;
        assert!(read <= free::STEP / BLOCK + 2, "{read} blocks at once");
        if s.free.borrow().exact() {
            return asks;
        }
    }
    panic!("never counted");
}

/// A FAT32 card of 40 MB in clusters of 512 bytes: its FAT holds
/// 320 KB, more than one ask reads.
fn fat32() -> Cut {
    let options = FormatVolumeOptions::new()
        .fat_type(fatfs::FatType::Fat32)
        .bytes_per_cluster(512);
    Cut::formatted_of(40 << 20, options)
}

/// The card's own word serves at once, the FAT is counted a part at a
/// time, and what is written and removed is followed.
#[test]
fn free_space_is_counted_a_part_at_a_time() {
    let card = fat32();
    let s = card.storage();
    let said = s.free_space().unwrap();
    assert_eq!(said, counted(&s));
    assert!(count_out(&card, &s) > 1);
    assert_eq!(s.free_space(), Some(counted(&s)));
    // Written, appended, made, replaced, moved and removed: followed
    // cluster by cluster, without counting again.
    s.write("books/a.bin", &vec![7u8; 64 * 1024]).unwrap();
    assert_eq!(s.free_space(), Some(counted(&s)));
    s.append("books/a.bin", &[1u8; 700]).unwrap();
    s.sync().unwrap();
    assert_eq!(s.free_space(), Some(counted(&s)));
    s.write("books/a.bin", b"short").unwrap();
    assert_eq!(s.free_space(), Some(counted(&s)));
    s.write("b.bin", &vec![1u8; 3000]).unwrap();
    s.rename("b.bin", "books/a.bin").unwrap();
    assert_eq!(s.free_space(), Some(counted(&s)));
    s.create_dir("x/y/z").unwrap();
    s.write("x/y/z/c.bin", &vec![2u8; 5000]).unwrap();
    assert_eq!(s.free_space(), Some(counted(&s)));
    s.remove_dir_all("x").unwrap();
    s.remove("books/a.bin").unwrap();
    assert_eq!(s.free_space(), Some(counted(&s)));
    // Asked on: nothing more to count.
    let before = card.reads.get();
    s.free_space();
    assert_eq!(card.reads.get(), before);
}

/// A card that was not unmounted (the X4 never unmounts): rust-fatfs
/// would read all of its FAT; here the count the card keeps, out of
/// date, serves first, and counting a part at a time puts it right.
#[test]
fn a_card_not_unmounted_is_counted_a_part_at_a_time() {
    let card = fat32();
    let first = card.storage();
    first.free_space();
    first.write("big.bin", &vec![7u8; 200 * 1024]).unwrap();
    first.sync().unwrap();
    // Mounted again from what is on the card.
    let card = Cut::new(card.image());
    let s = card.storage();
    let said = s.free_space().unwrap();
    count_out(&card, &s);
    let exact = s.free_space().unwrap();
    assert_eq!(exact, counted(&s));
    assert!(
        said > exact,
        "the count kept predates the file: {said} {exact}"
    );
}

/// The FAT is counted a sector at a time, into a buffer of that size:
/// nothing large is taken from the heap, which on the X4 is small and in
/// pieces while the radio is up.
#[test]
fn free_space_is_counted_without_a_large_buffer() {
    for card in [
        fat32(),
        Cut::formatted_of(
            8 << 20,
            FormatVolumeOptions::new().fat_type(fatfs::FatType::Fat16),
        ),
        Cut::formatted_of(
            8 << 20,
            FormatVolumeOptions::new().fat_type(fatfs::FatType::Fat12),
        ),
    ] {
        let image = card.image();
        let largest = alloc::rc::Rc::new(core::cell::Cell::new(0));
        let seen = largest.clone();
        let read = move |at: u64, buf: &mut [u8]| {
            seen.set(seen.get().max(buf.len()));
            let at = at as usize;
            buf.copy_from_slice(&image[at..at + buf.len()]);
            true
        };
        let mut free = free::Free::default();
        for _ in 0..100 {
            free.ask(&read);
            if free.exact() {
                break;
            }
        }
        assert!(free.exact());
        assert!(largest.get() <= BLOCK, "{} bytes at once", largest.get());
        let s = card.storage();
        assert_eq!(free.ask(&read), Some(counted(&s)));
    }
}

/// FAT12 and FAT16 keep no count: theirs is counted.
#[test]
fn small_cards_are_counted() {
    for fat in [fatfs::FatType::Fat12, fatfs::FatType::Fat16] {
        let card = Cut::formatted_of(8 << 20, FormatVolumeOptions::new().fat_type(fat));
        let s = card.storage();
        count_out(&card, &s);
        assert_eq!(s.free_space(), Some(counted(&s)));
        s.write("a.bin", &vec![1u8; 9000]).unwrap();
        assert_eq!(s.free_space(), Some(counted(&s)));
    }
}

/// The bytes for files in all, as rust-fatfs counts them, told from the
/// boot sector without reading the FAT; also on a card of its own.
#[test]
fn total_space_is_told_from_the_boot_sector() {
    for fat in [
        fatfs::FatType::Fat12,
        fatfs::FatType::Fat16,
        fatfs::FatType::Fat32,
    ] {
        let card = match fat {
            fatfs::FatType::Fat32 => fat32(),
            _ => Cut::formatted_of(8 << 20, FormatVolumeOptions::new().fat_type(fat)),
        };
        let s = card.storage();
        let before = card.reads.get();
        let total = s.total_space().unwrap();
        assert!(card.reads.get() - before <= 2, "{fat:?}");
        let stats = s.fs.stats().unwrap();
        assert_eq!(
            total,
            u64::from(stats.total_clusters()) * u64::from(stats.cluster_size())
        );
        assert!(s.free_space().unwrap() <= total);
        assert_eq!(mounted(card.image()).total_space(), Some(total));
    }
}

#[test]
fn synced_is_on_the_card() {
    let card = Cut::formatted(FormatVolumeOptions::new());
    let s = card.storage();
    // Emptied, then appended to in pieces (as files come over USB or
    // the network): the pieces stay in the open file until written
    // back.
    s.write("notes/a book.txt", b"").unwrap();
    s.append("notes/a book.txt", b"a high").unwrap();
    s.append("notes/a book.txt", b"light").unwrap();
    let before = card.image();
    s.sync().unwrap();
    // Not on the card before the sync: cut off then, it would be lost.
    assert_ne!(mounted(before).size("notes/a book.txt"), Ok(11));
    // The card as it is after it, as after a reset: the file is there.
    let mut read = [0u8; 32];
    let n = mounted(card.image())
        .read_at("notes/a book.txt", 0, &mut read)
        .unwrap();
    assert_eq!(&read[..n], b"a highlight");
}

/// What a call changes is on the card when it returns, but appends,
/// kept until written back.
#[test]
fn changes_are_on_the_card_when_calls_return() {
    let card = Cut::formatted(FormatVolumeOptions::new());
    let s = card.storage();
    let on_card = || contents(&mounted(card.image()), "");
    let file = |path: &str, data: &[u8]| (String::from(path), data.to_vec());
    s.write("a.txt", b"first").unwrap();
    assert_eq!(on_card(), [file("a.txt", b"first")]);
    s.write("a.txt", b"second").unwrap();
    assert_eq!(on_card(), [file("a.txt", b"second")]);
    s.rename("a.txt", "b.txt").unwrap();
    assert_eq!(on_card(), [file("b.txt", b"second")]);
    s.remove("b.txt").unwrap();
    assert_eq!(on_card(), []);
    s.create_dir("Books").unwrap();
    assert_eq!(
        mounted(card.image()).size("Books"),
        Err(StorageError::InvalidPath)
    );
    s.write("Books/c.txt", b"c").unwrap();
    s.remove_dir_all("Books").unwrap();
    assert!(!mounted(card.image()).exists("Books"));
    s.append("d.txt", b"appended").unwrap();
    assert_ne!(on_card(), [file("d.txt", b"appended")]);
    s.sync().unwrap();
    assert_eq!(on_card(), [file("d.txt", b"appended")]);
}

/// Every file in `dir` and below on `s`, with what it reads.
fn contents(s: &FatStorage<Disk>, dir: &str) -> Vec<(String, Vec<u8>)> {
    let mut found = Vec::new();
    s.list(dir, &mut |e| {
        found.push((e.name.to_string(), e.is_dir, e.size))
    })
    .unwrap();
    let mut out = Vec::new();
    for (name, is_dir, size) in found {
        let path = match dir {
            "" => name,
            _ => format!("{dir}/{name}"),
        };
        if is_dir {
            out.extend(contents(s, &path));
        } else {
            let mut data = vec![0u8; size as usize];
            let n = s.read_at(&path, 0, &mut data).unwrap();
            data.truncate(n);
            out.push((path, data));
        }
    }
    out
}

/// What a card holding `files` holds after `op`, cut off after each
/// block it writes (and before the first): the files on it, with what
/// they read. As on the X4: the disk shared, the storage writing back
/// its blocks.
fn cuts(
    files: &[(&str, &[u8])],
    op: impl FnOnce(&FatStorage<SharedDisk<Cut>>),
) -> Vec<Vec<(String, Vec<u8>)>> {
    let card = Cut::formatted(FormatVolumeOptions::new());
    let s = card.storage();
    for (path, data) in files {
        s.write(path, data).unwrap();
    }
    s.sync().unwrap();
    let mut image = card.image();
    card.writes.borrow_mut().clear();
    op(&s);
    let mut cuts = vec![contents(&mounted(image.clone()), "")];
    for (index, block) in card.writes.borrow().iter() {
        let at = *index as usize * BLOCK;
        image[at..at + BLOCK].copy_from_slice(block);
        cuts.push(contents(&mounted(image.clone()), ""));
    }
    cuts
}

/// What `path` reads in `files`, if it is there.
fn read<'a>(files: &'a [(String, Vec<u8>)], path: &str) -> Option<&'a [u8]> {
    files.iter().find(|(p, _)| p == path).map(|(_, d)| &d[..])
}

/// Two contents over several clusters (2 KB on this card).
fn old_and_new() -> (Vec<u8>, Vec<u8>) {
    let old = (0..7000u32).map(|i| (i % 251) as u8).collect();
    let new = (0..5000u32).map(|i| (i % 241) as u8 ^ 0x55).collect();
    (old, new)
}

/// Free clusters on `s`, as its FAT says.
fn free_clusters(s: &FatStorage<Disk>) -> u32 {
    s.fs.stats().unwrap().free_clusters()
}

/// Asserts that no two files on `s` share a cluster and that no
/// cluster is lost but up to `taken` bytes in whole clusters: once all
/// is removed, all others are free again. (FAT takes a cluster before
/// an entry holds it, and the patched rust-fatfs removes an entry before
/// it frees its clusters: cut off between, they stay taken by none.)
fn assert_nothing_lost(s: &FatStorage<Disk>, taken: u32, cut: usize) {
    assert_eq!(cross_linked(s), Vec::<String>::new(), "{cut}");
    for (name, is_dir, _) in names(s, "") {
        match is_dir {
            true => s.remove_dir_all(&name).unwrap(),
            false => s.remove(&name).unwrap(),
        }
    }
    // The record of moves, the storage's own.
    match s.root().remove(JOURNAL) {
        Ok(()) | Err(fatfs::Error::NotFound) => {}
        Err(e) => panic!("{e:?}"),
    }
    let fresh = mounted(Cut::formatted(FormatVolumeOptions::new()).image());
    let lost = free_clusters(&fresh) - free_clusters(s);
    assert!(lost <= taken.div_ceil(s.fs.cluster_size()), "{cut}: {lost}");
}

/// A file removed, cut off at any point, is whole or gone.
#[test]
fn a_file_removed_is_whole_or_gone_after_any_cut() {
    let (old, _) = old_and_new();
    let files = [("Books/a.txt", &old[..]), ("notes.txt", b"notes")];
    let cuts = cuts(&files, |s| s.remove("Books/a.txt").unwrap());
    for (n, files) in cuts.iter().enumerate() {
        assert_eq!(read(files, "notes.txt"), Some(&b"notes"[..]), "{n}");
        let a = read(files, "Books/a.txt");
        assert!(a.is_none_or(|a| a == old), "{n}");
    }
    assert_eq!(read(&cuts[cuts.len() - 1], "Books/a.txt"), None);
}

/// The file written in the tests of cuts below: its name takes more
/// entries than the gaps [`beside`] leaves, so it is not written where
/// its temporary file was.
const A: &str = "Books/A rather long name.txt";

/// A card prepared by `setup`, as on the X4, synced.
fn prepared(setup: impl FnOnce(&FatStorage<SharedDisk<Cut>>)) -> Vec<u8> {
    let card = Cut::formatted(FormatVolumeOptions::new());
    let s = card.storage();
    setup(&s);
    s.sync().unwrap();
    card.image()
}

/// The card `image` after `op` on it, cut off after each block it
/// writes (and before the first).
fn cut_images(image: Vec<u8>, op: impl FnOnce(&FatStorage<SharedDisk<Cut>>)) -> Vec<Vec<u8>> {
    let card = Cut::new(image);
    let s = card.storage();
    let mut image = card.image();
    card.writes.borrow_mut().clear();
    op(&s);
    let mut cuts = vec![image.clone()];
    for (index, block) in card.writes.borrow().iter() {
        let at = *index as usize * BLOCK;
        image[at..at + BLOCK].copy_from_slice(block);
        cuts.push(image.clone());
    }
    cuts
}

/// All of the file at `path`, as its size says.
fn whole(s: &dyn Storage, path: &str) -> Result<Vec<u8>, StorageError> {
    let mut data = vec![0u8; s.size(path)? as usize];
    let n = s.read_at(path, 0, &mut data)?;
    data.truncate(n);
    Ok(data)
}

/// Files in folder `Books` beside the one written: few, or so many
/// that its entries take several blocks, with gaps between.
fn beside(s: &FatStorage<SharedDisk<Cut>>, crowded: bool) {
    s.write("Books/b.txt", b"beside").unwrap();
    s.write("notes.txt", b"notes").unwrap();
    if crowded {
        for n in 0..40 {
            s.write(&format!("Books/f{n:02}.txt"), b"f").unwrap();
        }
        for n in (0..40).step_by(3) {
            s.remove(&format!("Books/f{n:02}.txt")).unwrap();
        }
    }
}

/// Asserts that the files [`beside`] wrote are there, whole.
fn assert_beside(s: &dyn Storage, crowded: bool, cut: usize) {
    assert_eq!(
        whole(s, "Books/b.txt").as_deref(),
        Ok(&b"beside"[..]),
        "{cut}"
    );
    assert_eq!(whole(s, "notes.txt").as_deref(), Ok(&b"notes"[..]), "{cut}");
    if crowded {
        for n in (0..40).filter(|n| n % 3 != 0) {
            let path = format!("Books/f{n:02}.txt");
            assert_eq!(whole(s, &path).as_deref(), Ok(&b"f"[..]), "{cut}");
        }
    }
}

/// The files on `s` that share a cluster with another.
fn cross_linked(s: &FatStorage<Disk>) -> Vec<String> {
    let mut clusters = Vec::new();
    let mut dirs = vec![(String::new(), s.root())];
    while let Some((path, dir)) = dirs.pop() {
        for entry in dir.iter().map(Result::unwrap) {
            let name = entry.file_name();
            let path = format!("{path}/{name}");
            if entry.is_dir() && name != "." && name != ".." {
                dirs.push((path, entry.to_dir()));
            } else if entry.is_file() {
                for extent in entry.to_file().extents() {
                    clusters.push((extent.unwrap().offset, path.clone()));
                }
            }
        }
    }
    clusters.sort();
    clusters
        .windows(2)
        .filter(|w| w[0].0 == w[1].0)
        .map(|w| format!("{} {}", w[0].1, w[1].1))
        .collect()
}

/// A file written anew, cut off at any point, is found with its old
/// or its new content, whole, however full its folder.
#[test]
fn a_file_written_anew_is_found_after_any_cut() {
    let (old, new) = old_and_new();
    for crowded in [false, true] {
        let image = prepared(|s| {
            beside(s, crowded);
            s.write(A, &old).unwrap();
        });
        let cuts = cut_images(image, |s| s.write(A, &new).unwrap());
        for (n, image) in cuts.into_iter().enumerate() {
            let s = mounted(image);
            let a = whole(&s, A);
            assert!(
                a == Ok(old.clone()) || a == Ok(new.clone()),
                "{crowded} {n}"
            );
            assert_beside(&s, crowded, n);
        }
    }
}

/// Cut off at any point of a write, the card takes the next writes
/// without harm: the file is written anew whole, other files keep
/// theirs, no two share a cluster, no temporary file is left.
#[test]
fn a_write_after_a_cut_harms_nothing() {
    let (old, new) = old_and_new();
    let again: Vec<u8> = (0..6000u32).map(|i| (i % 233) as u8 ^ 0x33).collect();
    for crowded in [false, true] {
        let image = prepared(|s| {
            beside(s, crowded);
            s.write(A, &old).unwrap();
        });
        let cuts = cut_images(image, |s| s.write(A, &new).unwrap());
        for (n, image) in cuts.into_iter().enumerate() {
            let card = Cut::new(image);
            let s = card.storage();
            s.write(A, &again).unwrap();
            s.write("Books/other.bin", &[9u8; 8000]).unwrap();
            s.write("other.bin", &[7u8; 8000]).unwrap();
            s.sync().unwrap();
            let s = mounted(card.image());
            assert_eq!(whole(&s, A), Ok(again.clone()), "{n}");
            assert_eq!(whole(&s, "Books/other.bin"), Ok(vec![9u8; 8000]), "{n}");
            assert_eq!(whole(&s, "other.bin"), Ok(vec![7u8; 8000]), "{n}");
            assert_beside(&s, crowded, n);
            assert_eq!(cross_linked(&s), Vec::<String>::new(), "{n}");
            let left: Vec<_> = names(&s, "Books")
                .into_iter()
                .filter(|(name, ..)| name.starts_with('~'))
                .collect();
            assert_eq!(left, [], "{n}");
        }
    }
}

/// A file moved over another, cut off at any point, leaves the other
/// with its old content or the new one, whole, and the new content is
/// never lost: where the other is still the old one, it is still where
/// it was, or beside the other as its new content, until the next
/// change settles it. Settled, the new content is at one place, no two
/// files share a cluster and none is lost (but the old content's, cut
/// off between its entry removed and its clusters freed).
#[test]
fn a_file_replaced_by_rename_is_found_after_any_cut() {
    let (old, new) = old_and_new();
    for (from, crowded) in [
        ("Books/a.part", false),
        ("Books/a.part", true),
        ("Inbox/a.part", false),
        ("Inbox/a.part", true),
    ] {
        let image = prepared(|s| {
            beside(s, crowded);
            s.write(A, &old).unwrap();
            s.write(from, &new).unwrap();
            // A move before: the card holds its record of moves.
            s.write("Inbox/d.txt", b"d").unwrap();
            s.rename("Inbox/d.txt", "Inbox/e.txt").unwrap();
        });
        let cuts = cut_images(image, |s| s.rename(from, A).unwrap());
        let last = cuts.len() - 1;
        let new_beside = format!("Books/{}", Temps::of(name(A)).ready());
        for (n, image) in cuts.into_iter().enumerate() {
            let s = mounted(image);
            let a = whole(&s, A);
            assert!(a == Ok(old.clone()) || a == Ok(new.clone()), "{from} {n}");
            if a == Ok(old.clone()) {
                let kept = [whole(&s, from), whole(&s, &new_beside)];
                assert!(kept.contains(&Ok(new.clone())), "{from} {n}");
            }
            assert_beside(&s, crowded, n);
            // The next change settles it.
            s.write("other.txt", b"other").unwrap();
            let found = (whole(&s, A), whole(&s, from).ok());
            let settled =
                found == (Ok(new.clone()), None) || found == (Ok(old.clone()), Some(new.clone()));
            assert!(settled, "{from} {n}");
            if n == last {
                assert_eq!(found, (Ok(new.clone()), None));
            }
            assert_beside(&s, crowded, n);
            assert_nothing_lost(&s, old.len() as u32, n);
        }
    }
}

/// Temporary files left by cuts are taken away again: writes go on
/// in a folder however often they were cut off there, or however many
/// an earlier version left.
#[test]
fn leftover_temporaries_do_not_pile_up() {
    let (old, new) = old_and_new();
    let mut image = prepared(|s| s.write(A, &old).unwrap());
    for round in 0..70 {
        let data = if round % 2 == 0 { &new } else { &old };
        let cuts = cut_images(image, |s| s.write(A, data).unwrap());
        // Cut off once the temporary file is written, before it took
        // the old one's place.
        image = cuts[cuts.len() / 2].clone();
    }
    let s = mounted(image.clone());
    s.write(A, b"last").unwrap();
    assert_eq!(names(&s, "Books"), [entry(name(A), false, 4)]);
    // Those of an earlier version, which named them by number.
    let s = mounted(image);
    for n in 0..64 {
        s.write(&format!("Books/~pp{n}.tmp"), &new).unwrap();
    }
    s.write(A, b"last").unwrap();
    assert_eq!(whole(&s, A).as_deref(), Ok(&b"last"[..]));
}

/// Writes file `name` in folder `dir` of `s` as rust-fatfs does, past
/// the storage (as an earlier version wrote its temporary files).
fn write_raw(s: &FatStorage<Disk>, dir: &str, name: &str, data: &[u8]) {
    let mut file = s.dir(dir).unwrap().create_file(name).unwrap();
    file.write_all(data).unwrap();
    file.flush().unwrap();
}

/// A file whose clusters were freed but whose entry stayed (cut off
/// in a remove by an earlier version) is not read short as if whole:
/// its size is its entry's, reading past its clusters fails.
#[test]
fn a_file_whose_clusters_were_freed_is_not_read_short() {
    let (old, _) = old_and_new();
    let image = card_with_files(FormatVolumeOptions::new(), &[("a.bin", &old)]);
    let card = Cut::new(image.clone());
    let s = card.storage();
    s.fs.root_dir().remove("a.bin").unwrap();
    s.sync().unwrap();
    let freed = card.image();
    // The FATs of the card after the remove, the folders of before.
    let reserved = usize::from(u16::from_le_bytes([image[14], image[15]]));
    let per_fat = usize::from(u16::from_le_bytes([image[22], image[23]]));
    let fats = reserved * BLOCK..(reserved + usize::from(image[16]) * per_fat) * BLOCK;
    let mut damaged = image;
    damaged[fats.clone()].copy_from_slice(&freed[fats]);
    let s = mounted(damaged);
    assert_eq!(s.size("a.bin"), Ok(old.len() as u64));
    let mut buf = vec![0u8; old.len()];
    assert_eq!(s.read_at("a.bin", 0, &mut buf), Err(StorageError::Io));
    assert_eq!(s.read_at("a.bin", 6000, &mut buf), Err(StorageError::Io));
    assert_eq!(s.append("a.bin", b"more"), Err(StorageError::Io));
}

/// Changed blocks reach the card in the order they were changed: cut
/// off after any block written, the card holds the changes up to one
/// of them, none after.
#[test]
fn blocks_reach_the_card_in_the_order_changed() {
    // More blocks than are kept, changed here and there, in part and
    // whole, read between.
    let blocks = 2 * disk::SLOTS;
    let card = Cut::new(vec![0; blocks * BLOCK]);
    let mut disk = BlockDisk::new(card.clone()).unwrap();
    let mut model = vec![0u8; blocks * BLOCK];
    // The disk after each change.
    let mut states = vec![model.clone()];
    let mut seed = 7u32;
    let mut next = |below: usize| {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        (seed >> 8) as usize % below
    };
    for change in 0..600 {
        // Most often a few blocks, as the FAT and a folder.
        let block = match next(3) {
            0 => next(blocks),
            _ => next(4),
        };
        let (at, len) = match next(4) {
            0 => (block * BLOCK, BLOCK),
            _ => {
                let offset = next(BLOCK);
                (block * BLOCK + offset, 1 + next(BLOCK - offset))
            }
        };
        let byte = (change % 255 + 1) as u8;
        disk.seek(SeekFrom::Start(at as u64)).unwrap();
        disk.write_all(&vec![byte; len]).unwrap();
        model[at..at + len].fill(byte);
        states.push(model.clone());
        if next(8) == 0 {
            let mut read = vec![0u8; 3 * BLOCK];
            let at = next(blocks - 3) * BLOCK;
            disk.seek(SeekFrom::Start(at as u64)).unwrap();
            disk.read_exact(&mut read).unwrap();
            assert_eq!(read, model[at..at + read.len()]);
        }
        if next(100) == 0 {
            disk.flush().unwrap();
        }
    }
    disk.flush().unwrap();
    let mut image = vec![0u8; blocks * BLOCK];
    let mut state = 0;
    for (k, (index, block)) in card.writes.borrow().iter().enumerate() {
        let at = *index as usize * BLOCK;
        image[at..at + BLOCK].copy_from_slice(block);
        while states.get(state).is_some_and(|s| *s != image) {
            state += 1;
        }
        assert!(state < states.len(), "cut after write {k}: no state");
    }
    assert_eq!(image, model);
}

#[test]
fn a_rename_of_nothing_keeps_what_is_there() {
    let s = storage();
    s.write("notes/kept.txt", b"highlights").unwrap();
    assert_eq!(
        s.rename("old/gone.txt", "notes/kept.txt"),
        Err(StorageError::NotFound)
    );
    assert_eq!(s.size("notes/kept.txt"), Ok(10));
}

fn entry(name: &str, is_dir: bool, size: u64) -> (String, bool, u64) {
    (String::from(name), is_dir, size)
}

/// Moved to another case of its own name, a file or a folder stays,
/// spelt as asked.
#[test]
fn a_rename_to_another_case_keeps_it() {
    let s = storage();
    s.write("Books/book.epub", b"a book").unwrap();
    s.rename("Books/book.epub", "books/Book.epub").unwrap();
    assert_eq!(names(&s, "Books"), [entry("Book.epub", false, 6)]);
    assert_eq!(s.size("Books/Book.epub"), Ok(6));
    // Its folder alone in another case: nothing to do.
    s.rename("BOOKS/Book.epub", "books/Book.epub").unwrap();
    assert_eq!(names(&s, "Books"), [entry("Book.epub", false, 6)]);
    // A folder, with all in it.
    s.rename("Books", "BOOKS").unwrap();
    assert_eq!(names(&s, ""), [entry("BOOKS", true, 0)]);
    assert_eq!(s.size("BOOKS/Book.epub"), Ok(6));
    // Exactly as it is: nothing to do.
    s.rename("BOOKS", "BOOKS").unwrap();
    assert_eq!(names(&s, ""), [entry("BOOKS", true, 0)]);
}

/// A folder is not moved into itself (it would be cut off from the
/// root), by any case of its name.
#[test]
fn a_folder_is_not_moved_into_itself() {
    let s = storage();
    s.write("Books/a.txt", b"a").unwrap();
    for to in ["Books/x", "books/x", "BOOKS/x/y"] {
        assert_eq!(s.rename("Books", to), Err(StorageError::InvalidPath));
    }
    assert_eq!(names(&s, ""), [entry("Books", true, 0)]);
    assert_eq!(names(&s, "Books"), [entry("a.txt", false, 1)]);
    // A name that only begins as it does is another folder.
    s.rename("Books", "Books2/x").unwrap();
    assert_eq!(s.size("books2/X/a.txt"), Ok(1));
}

/// A file open under one case of its path, written anew or removed
/// under another, is not read or written where it was: those
/// clusters are free, or another file's.
#[test]
fn open_files_and_kept_folders_follow_any_case() {
    let s = storage();
    s.write("Books/x.epub", &[1; 5000]).unwrap();
    let mut buf = [0u8; 4];
    assert_eq!(s.read_at("Books/x.epub", 0, &mut buf), Ok(4));
    s.write("books/X.epub", &[2; 3000]).unwrap();
    assert_eq!(s.size("Books/x.epub"), Ok(3000));
    assert_eq!(s.read_at("Books/x.epub", 0, &mut buf), Ok(4));
    assert_eq!(buf, [2; 4]);
    // Appended to, removed under another case, its clusters given to
    // another file: not written back over them.
    s.append("Books/x.epub", &[3; 100]).unwrap();
    s.remove("BOOKS/x.EPUB").unwrap();
    s.write("Books/other.bin", &[4; 8000]).unwrap();
    s.sync().unwrap();
    assert_eq!(names(&s, "Books"), [entry("other.bin", false, 8000)]);
    let mut other = vec![0u8; 8000];
    assert_eq!(s.read_at("Books/other.bin", 0, &mut other), Ok(8000));
    assert!(other.iter().all(|&b| b == 4));
    // A kept folder, removed and made again under another case.
    s.write("A/b/one.txt", b"1").unwrap();
    assert_eq!(s.size("A/b/one.txt"), Ok(1));
    s.remove_dir_all("a/B").unwrap();
    assert_eq!(s.size("A/b/one.txt"), Err(StorageError::NotFound));
    s.write("a/b/two.txt", b"22").unwrap();
    assert_eq!(s.size("A/b/two.txt"), Ok(2));
    // Moved under another case.
    s.rename("a/B", "a/c").unwrap();
    assert_eq!(s.size("A/b/two.txt"), Err(StorageError::NotFound));
    assert_eq!(s.size("A/C/two.txt"), Ok(2));
}

#[test]
fn paths_compared_as_fat_does() {
    assert!(same_path("Books/x.epub", "books/X.EPUB"));
    assert!(same_path("B\u{fc}cher/a", "B\u{dc}CHER/A"));
    assert!(!same_path("Books/x.epub", "Books/x.epu"));
    assert!(!same_path("B\u{fc}cher/a", "B\u{fc}cher/a/b"));
    assert!(is_within("Books/Sub/x.txt", "books/sub"));
    assert!(is_within("books/sub", "Books/Sub"));
    assert!(!is_within("books/subway", "books/sub"));
    assert!(!is_within("books", "books/sub"));
}

#[test]
fn long_names_written_read_and_listed() {
    let s = storage();
    let path = "Books/Lewis Carroll/Alice's Adventures in Wonderland.epub";
    s.write(path, b"the whole book").unwrap();
    assert_eq!(s.size(path), Ok(14));
    let mut buf = [0u8; 5];
    assert_eq!(s.read_at(path, 4, &mut buf), Ok(5));
    assert_eq!(&buf, b"whole");
    assert_eq!(s.read_at(path, 12, &mut buf), Ok(2));
    assert_eq!(&buf[..2], b"ok");
    assert_eq!(
        names(&s, "Books/Lewis Carroll"),
        [(
            String::from("Alice's Adventures in Wonderland.epub"),
            false,
            14
        )]
    );
    assert_eq!(names(&s, ""), [(String::from("Books"), true, 0)]);
    // Hidden folders with dots, as PaperPaw keeps its data.
    s.write(".paperpaw/plugins/reader/annotations.txt", b"a")
        .unwrap();
    assert!(s.exists(".paperpaw/plugins/reader/annotations.txt"));
    assert!(!s.exists("missing.txt"));
}

/// Folders kept from before stay right when folders are removed,
/// made again, renamed, or more are used than are kept.
#[test]
fn kept_folders_follow_changes() {
    let s = storage();
    s.write("a/b/one.txt", b"1").unwrap();
    assert_eq!(s.size("a/b/one.txt"), Ok(1));
    // Removed and made again under the same name: another folder.
    s.remove_dir_all("a/b").unwrap();
    assert_eq!(s.size("a/b/one.txt"), Err(StorageError::NotFound));
    s.write("a/b/two.txt", b"22").unwrap();
    assert_eq!(s.size("a/b/two.txt"), Ok(2));
    assert_eq!(s.size("a/b/one.txt"), Err(StorageError::NotFound));
    // Renamed: found at the new path only.
    s.rename("a/b", "a/c").unwrap();
    assert_eq!(s.size("a/c/two.txt"), Ok(2));
    assert_eq!(s.size("a/b/two.txt"), Err(StorageError::NotFound));
    s.write("a/b/three.txt", b"333").unwrap();
    assert_eq!(names(&s, "a/b").len(), 1);
    // More folders than are kept, used by turns.
    for round in 0..2 {
        for i in 0..FOLDERS * 2 {
            let path = format!("many/f{i}/x.txt");
            if round == 0 {
                s.write(&path, format!("{i}").as_bytes()).unwrap();
            }
            let mut buf = [0u8; 2];
            let n = s.read_at(&path, 0, &mut buf).unwrap();
            assert_eq!(&buf[..n], format!("{i}").as_bytes());
        }
    }
}

/// A disk of blocks in memory.
struct Card {
    data: Vec<u8>,
}

impl Blocks for Card {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        let at = index as usize * BLOCK;
        block.copy_from_slice(self.data.get(at..at + BLOCK).ok_or(DiskError)?);
        Ok(())
    }
    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError> {
        let at = index as usize * BLOCK;
        self.data
            .get_mut(at..at + BLOCK)
            .ok_or(DiskError)?
            .copy_from_slice(block);
        Ok(())
    }
    fn blocks(&mut self) -> Result<u32, DiskError> {
        Ok((self.data.len() / BLOCK) as u32)
    }
}

#[test]
fn whole_blocks_pass_by_the_kept_ones() {
    let mut disk = BlockDisk::new(Card {
        data: vec![0; 64 * BLOCK],
    })
    .unwrap();
    // Block 1 kept, changed, not yet written back.
    disk.seek(SeekFrom::Start(BLOCK as u64 + 10)).unwrap();
    disk.write_all(&[7; 100]).unwrap();
    // Four whole blocks over it: three pass by, one goes to its slot.
    let data: Vec<u8> = (0..4 * BLOCK).map(|i| (i % 251) as u8).collect();
    disk.seek(SeekFrom::Start(0)).unwrap();
    disk.write_all(&data).unwrap();
    let mut back = vec![0u8; 4 * BLOCK];
    disk.seek(SeekFrom::Start(0)).unwrap();
    disk.read_exact(&mut back).unwrap();
    assert_eq!(back, data);
    disk.flush().unwrap();
    assert_eq!(&disk.disk().data[..4 * BLOCK], &data[..]);
}

/// A disk of blocks in memory that counts its commands.
struct Counted {
    data: Vec<u8>,
    commands: usize,
}

impl Blocks for Counted {
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError> {
        self.read_blocks(index, block)
    }
    fn read_blocks(&mut self, index: u32, data: &mut [u8]) -> Result<(), DiskError> {
        self.commands += 1;
        let at = index as usize * BLOCK;
        data.copy_from_slice(self.data.get(at..at + data.len()).ok_or(DiskError)?);
        Ok(())
    }
    fn write_block(&mut self, _: u32, _: &[u8; BLOCK]) -> Result<(), DiskError> {
        Err(DiskError)
    }
    fn blocks(&mut self) -> Result<u32, DiskError> {
        Ok((self.data.len() / BLOCK) as u32)
    }
}

#[test]
fn blocks_read_one_after_the_other_are_read_ahead() {
    let data: Vec<u8> = (0..40 * BLOCK)
        .map(|i| (i / BLOCK * 7 + i % 13) as u8)
        .collect();
    let mut disk = BlockDisk::new(Counted {
        data: data.clone(),
        commands: 0,
    })
    .unwrap();
    // A folder's entries, 32 bytes at a time over 12 blocks: the first
    // block alone, then eight at once, then the rest of eight.
    let mut entry = [0u8; 32];
    for at in (0..12 * BLOCK).step_by(32) {
        disk.read_exact(&mut entry).unwrap();
        assert_eq!(&entry[..], &data[at..at + 32]);
    }
    assert_eq!(disk.disk().commands, 3);
    // Here and there: a block at a time.
    for block in [30u64, 20, 25] {
        disk.seek(SeekFrom::Start(block * BLOCK as u64)).unwrap();
        disk.read_exact(&mut entry).unwrap();
        assert_eq!(&entry[..], &data[block as usize * BLOCK..][..32]);
    }
    assert_eq!(disk.disk().commands, 6);
    // Not past the disk's end.
    disk.seek(SeekFrom::Start(37 * BLOCK as u64)).unwrap();
    let mut rest = vec![0u8; 3 * BLOCK];
    for chunk in rest.chunks_mut(100) {
        disk.read_exact(chunk).unwrap();
    }
    assert_eq!(&rest[..], &data[37 * BLOCK..]);
}

#[test]
fn on_a_card_of_blocks() {
    let mut card = BlockDisk::new(Card {
        data: vec![0; 2 << 20],
    })
    .unwrap();
    fatfs::format_volume(&mut card, FormatVolumeOptions::new()).unwrap();
    card.flush().unwrap();
    card.seek(SeekFrom::Start(0)).unwrap();
    let fs = FileSystem::new(card, FsOptions::new()).unwrap();
    let s = FatStorage::new(Box::leak(Box::new(fs)));
    let text: Vec<u8> = (0..3000u32).map(|i| (i % 251) as u8).collect();
    s.write("Books/A long name.txt", &text).unwrap();
    let mut buf = [0u8; 700];
    assert_eq!(s.read_at("Books/A long name.txt", 1000, &mut buf), Ok(700));
    assert_eq!(&buf[..], &text[1000..1700]);
    assert_eq!(s.size("Books/A long name.txt"), Ok(3000));
}

/// A card of `blocks` blocks with a FAT volume formatted at block
/// `first` and `table` written into its first blocks.
fn card_with(blocks: usize, first: usize, table: &[(usize, &[u8])]) -> Card {
    let mut volume = StdIoWrapper::from(Cursor::new(vec![0u8; (blocks - first) * BLOCK]));
    fatfs::format_volume(&mut volume, FormatVolumeOptions::new()).unwrap();
    let mut data = vec![0u8; blocks * BLOCK];
    data[first * BLOCK..].copy_from_slice(&volume.into_inner().into_inner());
    for (at, bytes) in table {
        data[*at..*at + bytes.len()].copy_from_slice(bytes);
    }
    Card { data }
}

/// A partition table entry of an MBR (`kind` e.g. 0x0C, FAT32).
fn mbr_entry(kind: u8, first: u32, count: u32) -> [u8; 16] {
    let mut entry = [0u8; 16];
    entry[4] = kind;
    entry[8..12].copy_from_slice(&first.to_le_bytes());
    entry[12..16].copy_from_slice(&count.to_le_bytes());
    entry
}

fn opens(card: Card) -> Result<(), VolumeError> {
    let disk = BlockDisk::volume(card)?;
    let fs = FileSystem::new(disk, FsOptions::new()).unwrap();
    let s = FatStorage::new(Box::leak(Box::new(fs)));
    s.write("Books/a.txt", b"found").unwrap();
    assert_eq!(s.size("Books/a.txt"), Ok(5));
    Ok(())
}

#[test]
fn volumes_found_in_partitions() {
    let blocks = 8192;
    // The file system from the first block, without a table.
    assert_eq!(opens(card_with(blocks, 0, &[])), Ok(()));
    // An MBR, as on most cards, its partition after an empty one.
    let first = 2048;
    let entry = mbr_entry(0x0C, first as u32, (blocks - first) as u32);
    let table = [(446 + 16, &entry[..]), (510, &[0x55, 0xAA][..])];
    assert_eq!(opens(card_with(blocks, first, &table)), Ok(()));
    // Its table saying more blocks than the card has, as an X4 Pro's
    // card as sold.
    let entry = mbr_entry(0x0C, first as u32, (blocks - first + 512) as u32);
    let table = [(446, &entry[..]), (510, &[0x55, 0xAA][..])];
    assert_eq!(opens(card_with(blocks, first, &table)), Ok(()));
    // A GPT: a protective MBR, the header in block 1, entries in 2.
    let protective = mbr_entry(0xEE, 1, (blocks - 1) as u32);
    let mut header = [0u8; 92];
    header[..8].copy_from_slice(b"EFI PART");
    header[72..80].copy_from_slice(&2u64.to_le_bytes());
    header[80..84].copy_from_slice(&4u32.to_le_bytes());
    header[84..88].copy_from_slice(&128u32.to_le_bytes());
    let mut entry = [0u8; 48];
    entry[..16].copy_from_slice(&[0xA2; 16]);
    entry[32..40].copy_from_slice(&(first as u64).to_le_bytes());
    entry[40..48].copy_from_slice(&((blocks - 1) as u64).to_le_bytes());
    let table = [
        (446, &protective[..]),
        (510, &[0x55, 0xAA][..]),
        (BLOCK, &header[..]),
        (2 * BLOCK + 128, &entry[..]),
    ];
    assert_eq!(opens(card_with(blocks, first, &table)), Ok(()));
    // exFAT, as cards above 32 GB come.
    let mut card = card_with(
        blocks,
        first,
        &[
            (446, &mbr_entry(0x07, 2048, 6144)[..]),
            (510, &[0x55, 0xAA][..]),
        ],
    );
    card.data[first * BLOCK + 3..first * BLOCK + 11].copy_from_slice(b"EXFAT   ");
    assert_eq!(opens(card).unwrap_err(), VolumeError::ExFat);
    // Nothing at all.
    let card = Card {
        data: vec![0; blocks * BLOCK],
    };
    assert_eq!(opens(card).unwrap_err(), VolumeError::NoFat);
}

#[test]
fn many_blocks_and_files_by_turns() {
    // More blocks than are kept, written and read by turns from more
    // files than stay open, on a card of blocks.
    let mut card = BlockDisk::new(Card {
        data: vec![0; 4 << 20],
    })
    .unwrap();
    fatfs::format_volume(&mut card, FormatVolumeOptions::new()).unwrap();
    card.flush().unwrap();
    card.seek(SeekFrom::Start(0)).unwrap();
    let fs = FileSystem::new(card, FsOptions::new()).unwrap();
    let s = FatStorage::new(Box::leak(Box::new(fs)));
    let content =
        |n: u32| -> Vec<u8> { (0..20_000u32).map(|i| ((i * 7 + n) % 251) as u8).collect() };
    let names: Vec<String> = (0..6).map(|n| alloc::format!("book/{n}.txt")).collect();
    for (n, name) in names.iter().enumerate() {
        s.write(name, &content(n as u32)).unwrap();
    }
    let mut buf = [0u8; 300];
    for round in 0..40u32 {
        let n = (round * 5 % 6) as usize;
        let at = u64::from(round * 487 % 19_000);
        assert_eq!(s.read_at(&names[n], at, &mut buf), Ok(300));
        let at = at as usize;
        assert_eq!(
            &buf[..],
            &content(n as u32)[at..at + 300],
            "{} at {at}",
            names[n]
        );
    }
    // Written again while open: read as written.
    s.write(&names[0], b"short").unwrap();
    assert_eq!(s.read_at(&names[0], 0, &mut buf), Ok(5));
}

#[test]
fn read_on_and_back_across_clusters() {
    // In clusters of 512 bytes, laid by turns with those of another file:
    // a cluster taken wrongly on a seek reads the other file.
    let s = fat32().storage();
    let data: Vec<u8> = (0..40 * 512u32).map(|i| (i * 7 % 251) as u8).collect();
    for part in data.chunks(512) {
        s.append("a.bin", part).unwrap();
        s.append("b.bin", &[1; 512]).unwrap();
    }
    let mut buf = [0u8; 600];
    let ats = [
        0, 511, 512, 513, 2560, 2559, 9000, 10_240, 1536, 19_968, 19_880, 1024, 20_479,
    ];
    for at in ats {
        let n = s.read_at("a.bin", at, &mut buf).unwrap();
        let at = at as usize;
        assert_eq!(&buf[..n], &data[at..data.len().min(at + 600)], "at {at}");
    }
    // Grown from its end, read on past where it ended.
    s.append("a.bin", &[9; 700]).unwrap();
    assert_eq!(s.read_at("a.bin", 20_380, &mut buf), Ok(600));
    assert_eq!(&buf[..100], &data[20_380..]);
    assert!(buf[100..].iter().all(|&b| b == 9));
}

#[test]
fn appended_by_turns_and_written_back() {
    let s = storage();
    s.write("book.epub", &[7u8; 5000]).unwrap();
    let files: Vec<String> = (0..OPEN_FILES + 2)
        .map(|n| alloc::format!(".cache/{n}.bin"))
        .collect();
    let mut buf = [0u8; 100];
    // More files than stay open, appended to by turns, with a book
    // read between.
    for round in 0..30u8 {
        for (n, name) in files.iter().enumerate() {
            s.append(name, &[round, n as u8]).unwrap();
        }
        assert_eq!(
            s.read_at("book.epub", u64::from(round) * 100, &mut buf),
            Ok(100)
        );
    }
    for (n, name) in files.iter().enumerate() {
        assert_eq!(s.size(name), Ok(60));
        let mut all = [0u8; 60];
        assert_eq!(s.read_at(name, 0, &mut all), Ok(60));
        let want: Vec<u8> = (0..30u8).flat_map(|r| [r, n as u8]).collect();
        assert_eq!(&all[..], &want[..], "{name}");
    }
    // Listed with their sizes as appended.
    assert!(names(&s, ".cache").iter().all(|(_, _, size)| *size == 60));
    // Written back on sync: opened anew, a file has it all.
    s.append(&files[0], b"end").unwrap();
    s.sync().unwrap();
    let mut fresh = s.root().open_file(&files[0]).unwrap();
    assert_eq!(fresh.seek(SeekFrom::End(0)).unwrap(), 63);
    // And moved with it all.
    s.append(&files[1], b"end").unwrap();
    s.rename(&files[1], "moved.bin").unwrap();
    assert_eq!(s.size("moved.bin"), Ok(63));
    let mut fresh = s.root().open_file("moved.bin").unwrap();
    assert_eq!(fresh.seek(SeekFrom::End(0)).unwrap(), 63);
}

#[test]
fn written_anew_appended_moved_and_removed() {
    let s = storage();
    s.write("a.txt", b"first, longer content").unwrap();
    // The file read last is open: a new content is seen anyway.
    let mut buf = [0u8; 32];
    assert_eq!(s.read_at("a.txt", 0, &mut buf), Ok(21));
    s.write("a.txt", b"second").unwrap();
    assert_eq!(s.read_at("a.txt", 0, &mut buf), Ok(6));
    assert_eq!(&buf[..6], b"second");
    s.append("a.txt", b" and more").unwrap();
    assert_eq!(s.size("a.txt"), Ok(15));
    assert_eq!(
        names(&s, ""),
        [(String::from("a.txt"), false, 15)],
        "no temporary left"
    );
    // Moved over another file, into a new folder.
    s.write("b/c.txt", b"old").unwrap();
    s.rename("a.txt", "b/c.txt").unwrap();
    assert!(!s.exists("a.txt"));
    assert_eq!(s.size("b/c.txt"), Ok(15));
    s.remove("b/c.txt").unwrap();
    s.remove("b/c.txt").unwrap();
    assert!(names(&s, "b").is_empty());
    // Folders with all in them.
    s.write("d/e/f.txt", b"x").unwrap();
    s.create_dir("d/g").unwrap();
    s.create_dir("d/g").unwrap();
    s.remove_dir_all("d").unwrap();
    s.remove_dir_all("d").unwrap();
    assert_eq!(names(&s, ""), [(String::from("b"), true, 0)]);
    assert_eq!(
        s.read_at("../x", 0, &mut buf),
        Err(StorageError::InvalidPath)
    );
}

/// The short name FAT keeps beside `long` in folder `dir`.
fn short_name(s: &FatStorage<Disk>, dir: &str, long: &str) -> String {
    s.dir(dir)
        .unwrap()
        .iter()
        .map(Result::unwrap)
        .find(|entry| entry.file_name() == long)
        .unwrap()
        .short_file_name()
}

/// A short name stands for nothing where its entry has a long name:
/// `PAPERP~1` does not reach PaperPaw's data, by any call.
#[test]
fn short_names_of_long_ones_reach_nothing() {
    let s = storage();
    s.write(".paperpaw/secrets.txt", b"password").unwrap();
    s.write(".paperpaw/plugins/wifi/networks.txt", b"home")
        .unwrap();
    s.write("Books/A long title.epub", b"a book").unwrap();
    assert_eq!(short_name(&s, "", ".paperpaw"), "PAPERP~1");
    assert_eq!(short_name(&s, "Books", "A long title.epub"), "ALONGT~1.EPU");
    let alias = "PAPERP~1/secrets.txt";
    let mut buf = [0u8; 16];
    // Not read, nor found at all.
    assert_eq!(s.read_at(alias, 0, &mut buf), Err(StorageError::NotFound));
    assert_eq!(s.size(alias), Err(StorageError::NotFound));
    assert!(!s.exists("PAPERP~1"));
    assert!(!s.exists("paperp~1/secrets.txt"));
    assert_eq!(s.list("PAPERP~1", &mut |_| {}), Err(StorageError::NotFound));
    assert_eq!(
        s.list("PAPERP~1/plugins", &mut |_| {}),
        Err(StorageError::NotFound)
    );
    assert_eq!(s.size("Books/ALONGT~1.EPU"), Err(StorageError::NotFound));
    // Not written, appended to, made, moved or removed through it.
    assert_eq!(s.write(alias, b"stolen"), Err(StorageError::InvalidPath));
    assert_eq!(
        s.append("PAPERP~1/firmware-keys.txt", b"key"),
        Err(StorageError::InvalidPath)
    );
    assert_eq!(
        s.create_dir("PAPERP~1/plugins"),
        Err(StorageError::InvalidPath)
    );
    assert_eq!(s.write("PAPERP~1", b"x"), Err(StorageError::InvalidPath));
    assert_eq!(s.create_dir("PAPERP~1"), Err(StorageError::InvalidPath));
    assert_eq!(s.rename(alias, "stolen.txt"), Err(StorageError::NotFound));
    assert_eq!(s.rename("PAPERP~1", "stolen"), Err(StorageError::NotFound));
    assert_eq!(
        s.rename("Books/A long title.epub", "PAPERP~1/a.epub"),
        Err(StorageError::InvalidPath)
    );
    assert_eq!(
        s.rename("Books/A long title.epub", "PAPERP~1"),
        Err(StorageError::InvalidPath)
    );
    s.remove(alias).unwrap();
    s.remove("PAPERP~1").unwrap();
    s.remove_dir_all("PAPERP~1").unwrap();
    s.remove_dir_all("PAPERP~1/plugins").unwrap();
    // All as it was, nothing left beside.
    assert_eq!(s.read_at(".paperpaw/secrets.txt", 0, &mut buf), Ok(8));
    assert_eq!(&buf[..8], b"password");
    assert_eq!(s.size(".paperpaw/plugins/wifi/networks.txt"), Ok(4));
    assert_eq!(
        names(&s, ".paperpaw"),
        [
            (String::from("plugins"), true, 0),
            (String::from("secrets.txt"), false, 8)
        ]
    );
    assert_eq!(
        names(&s, ""),
        [
            (String::from(".paperpaw"), true, 0),
            (String::from("Books"), true, 0)
        ]
    );
    assert_eq!(s.size("Books/A long title.epub"), Ok(6));
    // A long name reaches its entry, without regard to case.
    assert_eq!(s.size(".PaperPaw/SECRETS.TXT"), Ok(8));
    assert_eq!(s.size("books/a LONG title.epub"), Ok(6));
}

/// A card of 4 MB formatted with `options`, `files` written on it.
fn card_with_files(options: FormatVolumeOptions, files: &[(&str, &[u8])]) -> Vec<u8> {
    let card = Cut::formatted(options);
    let s = card.storage();
    for (path, data) in files {
        s.write(path, data).unwrap();
    }
    s.sync().unwrap();
    card.image()
}

/// A card with `files` written, then their long names taken away, as
/// systems write names that fit in 8.3.
fn short_names_only(files: &[(&str, &[u8])]) -> FatStorage<Disk> {
    let mut bytes = card_with_files(FormatVolumeOptions::new(), files);
    // Every long name entry (attributes 0x0F, type 0, no cluster)
    // marked removed.
    for entry in bytes.as_chunks_mut::<32>().0 {
        if entry[11] == 0x0F && entry[12] == 0 && entry[26..28] == [0, 0] && entry[0] != 0 {
            entry[0] = 0xE5;
        }
    }
    mounted(bytes)
}

#[test]
fn folders_end_after_the_most_entries_fat_holds() {
    // Endless, as the entries of a folder whose clusters run in a loop.
    let endless = core::iter::repeat_with(|| Ok(()));
    assert_eq!(bounded(endless).position(|r| r.is_err()), Some(ENTRIES));
    let few = [Ok(1), Ok(2)].into_iter();
    assert_eq!(bounded(few).collect::<Vec<_>>(), [Ok(1), Ok(2)]);
}

/// A damaged card, a folder's clusters in a loop: reading the folder
/// fails instead of going on for ever.
#[test]
fn folders_in_a_loop_end() {
    // Clusters of one block: 16 entries. `.`, `..` and seven files
    // of two entries each (a long name and a short one) fill the
    // first; the eighth file takes another.
    let files: Vec<(String, &[u8])> = (0..8)
        .map(|n| (alloc::format!("Loop/f{n}.txt"), &b"x"[..]))
        .collect();
    let files: Vec<(&str, &[u8])> = files.iter().map(|(p, d)| (p.as_str(), *d)).collect();
    let options = FormatVolumeOptions::new()
        .fat_type(fatfs::FatType::Fat16)
        .bytes_per_cluster(BLOCK as u32);
    let mut bytes = card_with_files(options, &files);
    // The folder's first cluster, from its short entry in the root.
    let entry = bytes
        .as_chunks::<32>()
        .0
        .iter()
        .find(|e| &e[..11] == b"LOOP       " && e[11] & 0x10 != 0)
        .unwrap();
    let cluster = usize::from(u16::from_le_bytes([entry[26], entry[27]]));
    // Its next cluster in both FATs: itself.
    let reserved = usize::from(u16::from_le_bytes([bytes[14], bytes[15]]));
    let per_fat = usize::from(u16::from_le_bytes([bytes[22], bytes[23]]));
    for fat in 0..usize::from(bytes[16]) {
        let at = (reserved + fat * per_fat) * BLOCK + cluster * 2;
        bytes[at..at + 2].copy_from_slice(&(cluster as u16).to_le_bytes());
    }
    let s = mounted(bytes);
    assert_eq!(s.size("Loop/missing.txt"), Err(StorageError::Io));
    assert_eq!(s.list("Loop", &mut |_| {}), Err(StorageError::Io));
    assert_eq!(s.write("Loop/new.txt", b"new"), Err(StorageError::Io));
    assert_eq!(s.remove_dir_all("Loop"), Err(StorageError::Io));
}

/// An entry with a short name alone is found by it.
#[test]
fn short_names_alone_are_names() {
    let s = short_names_only(&[("BOOKS/README.TXT", b"read me")]);
    assert!(
        s.dir("BOOKS")
            .unwrap()
            .iter()
            .map(Result::unwrap)
            .all(|entry| entry.long_file_name_as_ucs2_units().is_none())
    );
    assert_eq!(names(&s, ""), [(String::from("BOOKS"), true, 0)]);
    assert_eq!(s.size("BOOKS/README.TXT"), Ok(7));
    assert_eq!(s.size("books/readme.txt"), Ok(7));
    s.write("books/readme.txt", b"again").unwrap();
    s.rename("Books/ReadMe.txt", "books/notes.txt").unwrap();
    assert_eq!(names(&s, "BOOKS"), [(String::from("notes.txt"), false, 5)]);
    s.remove("BOOKS/NOTES.TXT").unwrap();
    assert!(names(&s, "books").is_empty());
}

mod after_cuts;
mod damaged;
mod discard;
mod failing_writes;
mod listed;
mod noted;

/// The time a file was changed is the same listed and asked for, and
/// read from FAT's date and time as seconds since 1970.
#[test]
fn files_tell_when_they_were_changed() {
    let storage = storage();
    storage.write("books/a.txt", b"x").unwrap();
    let mut listed = None;
    storage.list("books", &mut |e| listed = e.modified).unwrap();
    assert!(listed.is_some());
    assert_eq!(storage.modified("books/a.txt"), listed);
    assert_eq!(storage.modified("/books/a.txt"), listed);
    assert_eq!(storage.modified("books/none.txt"), None);
    let at = |y, mo, d, h, mi, s| {
        seconds(fatfs::DateTime::new(
            fatfs::Date::new(y, mo, d),
            fatfs::Time::new(h, mi, s, 0),
        ))
    };
    assert_eq!(at(1980, 1, 1, 0, 0, 0), 315_532_800);
    assert_eq!(at(2026, 10, 24, 12, 30, 5), 1_792_845_005);
}
