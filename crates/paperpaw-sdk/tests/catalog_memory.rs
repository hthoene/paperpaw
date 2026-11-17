//! Making the catalog of a large library takes a few kilobytes, whatever
//! its size, on a FAT card as the X4's: a walk reads the old catalog along
//! with it instead of holding it (and takes every book from it), lists
//! folders a piece a step, and writes a few books a step; catching up
//! copies the old catalog a piece at a time. With a long log of status
//! changes and full lists of favourites and books opened (the X4 has
//! about 192 KiB of heap in all, the library's index of 2000 books 48 KB
//! of it).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::borrow::Cow;
use std::io::Cursor;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use paperpaw_fat::FatStorage;
use paperpaw_fat::fatfs::{
    self, FileSystem, FormatVolumeOptions, FsOptions, Seek, SeekFrom, StdIoWrapper,
};
use paperpaw_sdk::document::DocumentError;
use paperpaw_sdk::library::catalog::{self, MAX_LOG, Record, Rewrite, Status, Walk};
use paperpaw_sdk::library::{Book, sorted};
use paperpaw_sdk::{Env, FormatSpec, Formats, Id, Storage};

/// Counts the bytes allocated now, and the most since the peak was reset.
struct Counting;

static NOW: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

// SAFETY: passes every call on to the system's allocator.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as the caller asked.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let now = NOW.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: as the caller asked.
        unsafe { System.dealloc(ptr, layout) };
        NOW.fetch_sub(layout.size(), Ordering::Relaxed);
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// One test runs at a time (the counts are the whole process's).
static TURN: Mutex<()> = Mutex::new(());

fn turn() -> std::sync::MutexGuard<'static, ()> {
    TURN.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The most the heap grows above where it is while `run` runs, and what
/// `run` gives.
fn growth<T>(run: impl FnOnce() -> T) -> (usize, T) {
    let before = NOW.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    let out = run();
    (PEAK.load(Ordering::Relaxed) - before, out)
}

/// Books in the library, in a few folders.
const BOOKS: usize = 2400;
/// The most making a catalog may take beside what is there.
const BOUND: usize = 12 * 1024;

/// A FAT card of 64 MB in memory, formatted (its bytes allocated before
/// anything is counted, never grown).
fn card() -> FatStorage<StdIoWrapper<Cursor<Vec<u8>>>> {
    let mut disk = StdIoWrapper::from(Cursor::new(vec![0u8; 64 << 20]));
    fatfs::format_volume(&mut disk, FormatVolumeOptions::new()).unwrap();
    disk.seek(SeekFrom::Start(0)).unwrap();
    let fs = FileSystem::new(disk, FsOptions::new()).unwrap();
    FatStorage::new(Box::leak(Box::new(fs)))
}

fn path(n: usize) -> String {
    format!("books/Shelf {}/A rather long book title {n:04}.txt", n % 6)
}

/// A walk to its end: how many books it took from the old catalog.
fn walk(env: &Env<'_>) -> usize {
    let mut walk = Walk::start(env, Vec::new()).unwrap();
    while !walk.step(env).unwrap() {}
    walk.from_old()
}

/// A harness whose formats open `.txt` books (as having no details).
fn harness() -> common::Harness {
    let mut h = common::Harness::new();
    let mut formats = Formats::new();
    formats
        .add(FormatSpec::new(
            Id::new_static("test.txt"),
            "TXT",
            vec![Cow::Borrowed("txt")],
            |_, _, _| Err(DocumentError::UnsupportedFormat),
        ))
        .unwrap();
    h.formats = formats;
    h
}

#[test]
fn making_the_catalog_of_a_large_library_takes_a_few_kilobytes() {
    let _turn = turn();
    let h = harness();
    let storage = card();
    storage.create_dir("books").unwrap();
    for shelf in 0..6 {
        storage.create_dir(&format!("books/Shelf {shelf}")).unwrap();
    }
    for n in 0..BOOKS {
        storage.write(&path(n), b"text").unwrap();
    }
    // The lists as full as they get: 500 favourites, 100 books opened.
    storage.create_dir(".paperpaw").unwrap();
    storage.create_dir(".paperpaw/library").unwrap();
    let list = |count: usize| (0..count).map(|n| path(n * 3) + "\n").collect::<String>();
    storage
        .write(".paperpaw/library/favourites.txt", list(500).as_bytes())
        .unwrap();
    storage
        .write(".paperpaw/library/opened.txt", list(100).as_bytes())
        .unwrap();
    let env = h.env_on(&storage);
    // The first walk reads every book's details (none were known).
    let (first, _) = growth(|| walk(&env));
    let mut count = 0;
    catalog::each(&storage, &mut |_| count += 1);
    assert_eq!(count, BOOKS);
    // A log as long as it gets before it is folded in.
    for n in 0..MAX_LOG {
        let status = Status::new(Some(50), true, true);
        catalog::note(&storage, &path(n * 7 % BOOKS), status).unwrap();
    }
    assert!(catalog::log_is_long(&storage));
    // Again: every book from the old catalog, read along.
    let (again, from_old) = growth(|| walk(&env));
    assert_eq!(from_old, BOOKS, "books taken from the old catalog");
    // Caught up: the old catalog copied a piece a step.
    for n in 0..MAX_LOG {
        let status = Status::new(Some(100), false, true);
        catalog::note(&storage, &path(n * 11 % BOOKS), status).unwrap();
    }
    let (caught_up, ()) = growth(|| {
        let mut rewrite = Rewrite::start(&env, vec![path(0)]).unwrap();
        while !rewrite.step(&env).unwrap() {}
    });
    for (what, took) in [
        ("first walk", first),
        ("walk", again),
        ("catch-up", caught_up),
    ] {
        eprintln!("2,400 books: {what} took {took} B");
        assert!(took <= BOUND, "{what} took {took} bytes");
    }
}

/// Folders in a library of a folder an author, two levels deep.
const FOLDERS: usize = 2000;

#[test]
fn a_walk_of_thousands_of_folders_takes_a_few_kilobytes() {
    let _turn = turn();
    let h = harness();
    let storage = card();
    let folder = |n: usize| format!("books/Shelf {}/An author of the name {n:04}", n % 4);
    storage.create_dir("books").unwrap();
    for shelf in 0..4 {
        storage.create_dir(&format!("books/Shelf {shelf}")).unwrap();
    }
    for n in 0..FOLDERS {
        storage.create_dir(&folder(n)).unwrap();
        storage
            .write(&format!("{}/A book {n:04}.txt", folder(n)), b"text")
            .unwrap();
    }
    let env = h.env_on(&storage);
    for what in ["first walk", "walk"] {
        let (took, _) = growth(|| walk(&env));
        eprintln!("2,000 folders: {what} took {took} B");
        assert!(took <= BOUND, "{what} took {took} bytes");
        let mut paths = Vec::new();
        catalog::each(&storage, &mut |record| paths.push(record.path));
        paths.sort();
        let mut expected: Vec<String> = (0..FOLDERS)
            .map(|n| format!("{}/A book {n:04}.txt", folder(n)))
            .collect();
        expected.sort();
        assert_eq!(paths, expected, "every book once after the {what}");
    }
    assert!(!storage.exists(".paperpaw/library/walk.dirs"));
}

/// A library as people keep them: shelves, authors, series, volumes (four
/// levels deep), names of up to 60 letters, paths of up to 270.
#[test]
fn a_walk_of_a_deep_library_takes_a_few_kilobytes() {
    let _turn = turn();
    let h = harness();
    let storage = card();
    let long = "Of the very long name of a folder";
    let mut books = Vec::new();
    storage.create_dir("books").unwrap();
    for shelf in 0..12 {
        let shelf = format!("books/Shelf {shelf:02} {long}");
        storage.create_dir(&shelf).unwrap();
        for author in 0..30 {
            let author = format!("{shelf}/Ørsted Ågård the {author:03}rd {long}");
            storage.create_dir(&author).unwrap();
            books.push(format!("{author}/A book by itself.txt"));
            let series = format!("{author}/The series of it all {long}");
            storage.create_dir(&series).unwrap();
            let volume = format!("{series}/Volume one");
            storage.create_dir(&volume).unwrap();
            books.push(format!("{volume}/The first part of it.txt"));
        }
    }
    for book in &books {
        storage.write(book, b"text").unwrap();
    }
    let env = h.env_on(&storage);
    for what in ["first walk", "walk"] {
        let (took, _) = growth(|| walk(&env));
        let mut count = 0;
        catalog::each(&storage, &mut |_| count += 1);
        assert_eq!(count, books.len(), "every book after the {what}");
        eprintln!("deep library: {what} took {took} B");
        assert!(took <= BOUND, "{what} took {took} bytes");
    }
}

/// Books sorted for the view of them on the X4.
const SORTED: usize = 10_000;
/// The most sorting them may take beside what is there.
const SORT_BOUND: usize = 24 * 1024;

#[test]
fn sorting_the_books_of_a_large_library_takes_a_bounded_part_of_the_heap() {
    let _turn = turn();
    let mut h = harness();
    h.device = &common::SMALL_DEVICE;
    let storage = card();
    storage.create_dir(".paperpaw").unwrap();
    storage.create_dir(".paperpaw/library").unwrap();
    // As many series as the X4 counts (and more), two books each.
    let records: Vec<Record> = (0..SORTED)
        .map(|n| {
            let book = Book::default()
                .title(Some(format!("A rather long book title {n:05}")))
                .author(Some(format!("Somebody Writing {:04}", n % 900)))
                .series(Some(format!("A series of books {:04}", n / 2)))
                .series_index(Some(format!("{}", n % 2 + 1)));
            Record::of(&path(n), &book).with_status(Status::new(
                Some((n % 100) as u8),
                false,
                n % 7 == 0,
            ))
        })
        .collect();
    catalog::write(&storage, &records).unwrap();
    drop(records);
    let env = h.env_on(&storage);
    let (took, ()) = growth(|| {
        let mut build = sorted::Build::start(&env, "").unwrap();
        while !build.step(&env).unwrap() {}
    });
    let file = format!(
        ".paperpaw/library/sorted-{:08x}.idx",
        paperpaw_sdk::library::index::hash("")
    );
    assert!(storage.exists(&file));
    eprintln!("sorting 10,000 books took {took} B");
    assert!(took <= SORT_BOUND, "sorting took {took} bytes");
}
