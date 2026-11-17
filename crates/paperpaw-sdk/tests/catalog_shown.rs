//! A first walk shows the books it found so far (#108): where there is no
//! catalog yet, the books found are put into one at checkpoints (20, then
//! each time twice as many, or every two minutes); the catalog it ends
//! with is the one it would have put in place, byte for byte; cut off at
//! any step, the catalog left holds books found, and the next walk makes
//! it whole.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::borrow::Cow;

use paperpaw_sdk::document::DocumentError;
use paperpaw_sdk::library;
use paperpaw_sdk::library::catalog::{self, Walk};
use paperpaw_sdk::{
    DirEntry, Env, FormatSpec, Formats, Id, Instant, MemoryStorage, Params, Storage, StorageError,
};

const CATALOG: &str = ".paperpaw/library/catalog.txt";
const BUILDING: &str = ".paperpaw/library/catalog.tmp";

fn harness(storage: MemoryStorage) -> common::Harness {
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
    h.storage = storage;
    h
}

/// A card of `n` books in a few folders.
fn library(n: usize) -> MemoryStorage {
    (0..n).fold(MemoryStorage::new(), |storage, i| {
        storage.with(
            &format!("books/f{}/b{i:04}.txt", i % 5),
            format!("book {i}"),
        )
    })
}

/// The books of the catalog, by path.
fn books(storage: &dyn Storage) -> Vec<String> {
    let mut books = Vec::new();
    catalog::each(storage, &mut |r| books.push(r.path));
    books.sort();
    books
}

/// Every file of `storage`, copied: the card as it is at a cut.
fn snapshot(storage: &MemoryStorage) -> MemoryStorage {
    fn copy(from: &MemoryStorage, to: MemoryStorage, dir: &str) -> MemoryStorage {
        let mut entries = Vec::new();
        from.list(dir, &mut |e| entries.push((e.name.to_string(), e.is_dir)))
            .unwrap();
        entries.into_iter().fold(to, |to, (name, is_dir)| {
            let path = if dir.is_empty() {
                name
            } else {
                format!("{dir}/{name}")
            };
            if is_dir {
                copy(from, to, &path)
            } else {
                to.with(&path, from.get(&path).unwrap())
            }
        })
    }
    copy(storage, MemoryStorage::new(), "")
}

fn walk(env: &Env<'_>) {
    let mut walk = Walk::start(env, Vec::new()).unwrap();
    while !walk.step(env).unwrap() {}
}

fn walking(env: &Env<'_>) -> bool {
    let key = Id::new_static("core.library-walking");
    library::service(env)
        .extended(env, &key, &Params::new())
        .is_some_and(|p| p.get("walking") == Some("true"))
}

/// Walked step by step: at each checkpoint the catalog lists exactly the
/// books found until then (20, then twice as many each time); it ends as
/// the building file was, byte for byte (what a walk that never shows
/// its books puts in place); while it goes on, the library says so.
#[test]
fn a_first_walk_shows_the_books_found_so_far() {
    let h = harness(library(200));
    let env = h.env();
    let mut walk = Walk::start(&env, Vec::new()).unwrap();
    let (mut shown, mut building) = (Vec::new(), None);
    loop {
        let done = walk.step(&env).unwrap();
        if let Some(bytes) = h.storage.get(BUILDING) {
            building = Some(bytes);
        }
        if walk.published() {
            let listed = books(&h.storage);
            let text = String::from_utf8(h.storage.get(CATALOG).unwrap()).unwrap();
            assert!(text.ends_with('\n'), "whole lines");
            assert!(listed.len() <= walk.found());
            shown.push(listed.len());
        }
        if done {
            break;
        }
        assert!(walking(&env) || !h.storage.exists(BUILDING));
    }
    assert!(!walking(&env));
    assert_eq!(shown.len(), 4, "{shown:?}");
    assert!((20..36).contains(&shown[0]), "{shown:?}");
    for pair in shown.windows(2) {
        assert!(pair[1] >= 2 * pair[0], "{shown:?}");
    }
    assert_eq!(h.storage.get(CATALOG), building, "byte for byte");
    assert!(!h.storage.exists(BUILDING));
    assert_eq!(books(&h.storage).len(), 200);
}

/// Few books: no checkpoint, the catalog put in place at the end as
/// always; a walk with a catalog there shows nothing before its end, nor
/// says its books are those found so far.
#[test]
fn only_a_first_walk_of_enough_books_shows_them_before_its_end() {
    let h = harness(library(19));
    let env = h.env();
    let mut walk = Walk::start(&env, Vec::new()).unwrap();
    while !walk.step(&env).unwrap() {
        assert!(!walk.published());
        assert!(!h.storage.exists(CATALOG));
    }
    assert_eq!(books(&h.storage).len(), 19);
    // More books, the catalog there: compared with it, none shown early.
    for i in 19..120 {
        h.storage
            .write(&format!("books/g/c{i}.txt"), b"more")
            .unwrap();
    }
    let mut walk = Walk::start(&env, Vec::new()).unwrap();
    while !walk.step(&env).unwrap() {
        assert!(!walk.published());
        assert!(!walking(&env));
        assert_eq!(books(&h.storage).len(), 19);
    }
    assert_eq!(books(&h.storage).len(), 120);
}

/// Every two minutes at least, once 50 books more were found.
#[test]
fn a_slow_walk_shows_its_books_every_two_minutes() {
    let mut h = harness(library(150));
    let mut walk = Walk::start(&h.env(), Vec::new()).unwrap();
    let mut shown = Vec::new();
    loop {
        // A book every two seconds.
        h.now = Instant(h.now.0 + 2_000);
        let env = h.env();
        let done = walk.step(&env).unwrap();
        if walk.published() {
            shown.push(books(&h.storage).len());
        }
        if done {
            break;
        }
    }
    // 20, 40, 80, then 130 or so after two minutes (not 160: 150 books).
    assert!(shown.len() >= 4, "{shown:?}");
    assert!(shown[3] < 160, "{shown:?}");
}

/// Cut off after any step (the power gone), a piece of a checkpoint
/// maybe half written: the catalog left lists books of the library and
/// no other, and the next walk makes it whole, its books said to be
/// those found so far until it ends.
#[test]
fn a_cut_leaves_books_found_and_the_next_walk_makes_them_all() {
    let h = harness(library(100));
    let env = h.env();
    let all = {
        let whole = harness(library(100));
        walk(&whole.env());
        books(&whole.storage)
    };
    let mut walk_on = Walk::start(&env, Vec::new()).unwrap();
    let mut cuts = 0;
    loop {
        let done = walk_on.step(&env).unwrap();
        let _ = walk_on.published();
        let Some(catalog) = h.storage.get(CATALOG) else {
            if done {
                break;
            }
            continue;
        };
        // Cut off as it was, and with its last line cut short.
        for cut in [catalog.len(), catalog.len().saturating_sub(7)] {
            let card = snapshot(&h.storage);
            card.write(CATALOG, &catalog[..cut]).unwrap();
            let after = harness(card);
            let left = books(&after.storage);
            assert!(left.iter().all(|b| all.contains(b)), "{cut}: {left:?}");
            let env = after.env();
            let mut next = Walk::start(&env, Vec::new()).unwrap();
            while !next.step(&env).unwrap() {
                let under_way = after.storage.exists(BUILDING);
                assert_eq!(walking(&env), !done && under_way, "{cut}");
            }
            assert!(!walking(&env), "{cut}");
            assert_eq!(books(&after.storage), all, "{cut}");
            cuts += 1;
        }
        if done {
            break;
        }
    }
    assert!(cuts > 10, "{cuts}");
}

/// Cut off while the first checkpoint began the catalog (the power gone
/// with the file made, its header not or half written): the next walk
/// takes it for no catalog or a damaged one and ends with all the books.
#[test]
fn a_cut_in_the_first_header_leaves_a_catalog_the_next_walk_makes_whole() {
    let h = harness(library(60));
    let env = h.env();
    let (all, bytes) = {
        let whole = harness(library(60));
        walk(&whole.env());
        (books(&whole.storage), whole.storage.get(CATALOG).unwrap())
    };
    let mut first = Walk::start(&env, Vec::new()).unwrap();
    while h.storage.get(CATALOG).is_none() {
        assert!(!first.step(&env).unwrap());
    }
    let catalog = h.storage.get(CATALOG).unwrap();
    let header = catalog.iter().position(|&b| b == b'\n').unwrap() + 1;
    for cut in [0, 1, header / 2, header - 1, header] {
        let card = snapshot(&h.storage);
        card.write(CATALOG, &catalog[..cut]).unwrap();
        let after = harness(card);
        walk(&after.env());
        assert_eq!(books(&after.storage), all, "{cut}");
        // Byte for byte but the header, whose generation counts up as
        // with any catalog made anew where there was one.
        let made = after.storage.get(CATALOG).unwrap();
        let books_of = |c: &[u8]| c[c.iter().position(|&b| b == b'\n').unwrap()..].to_vec();
        assert_eq!(books_of(&made), books_of(&bytes), "{cut}: byte for byte");
        assert!(!after.storage.exists(BUILDING), "{cut}");
    }
}

/// A slow walk of a large library: its end, the rest appended in a few
/// steps, is no checkpoint (views hear of the catalog made once, as of
/// any walk's end, not of its books just before).
#[test]
fn the_end_of_a_slow_first_walk_is_no_checkpoint() {
    let mut h = harness(library(1_500));
    let mut walk = Walk::start(&h.env(), Vec::new()).unwrap();
    let mut shown = Vec::new();
    loop {
        h.now = Instant(h.now.0 + 2_000);
        let env = h.env();
        let done = walk.step(&env).unwrap();
        if walk.published() {
            shown.push(books(&h.storage).len());
        }
        if done {
            break;
        }
    }
    assert!(shown.len() > 4, "{shown:?}");
    assert!(shown.last() < Some(&1_500), "{shown:?}");
    assert_eq!(books(&h.storage).len(), 1_500);
}

/// A card that takes no file named `walk.first` (full, say, as the first
/// walk began).
struct NoNote<'a>(&'a MemoryStorage);

impl Storage for NoNote<'_> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if path.ends_with("/walk.first") {
            return Err(StorageError::Full);
        }
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(from, to)
    }
}

/// The note that the books are those found so far cannot be written:
/// the first walk goes on all the same, shows its books and ends whole;
/// they are just not said to be found so far.
#[test]
fn a_first_walk_without_its_note_goes_on() {
    let h = harness(library(100));
    let card = NoNote(&h.storage);
    let env = h.env_on(&card);
    let mut walk = Walk::start(&env, Vec::new()).unwrap();
    let mut shown = 0;
    while !walk.step(&env).unwrap() {
        shown += usize::from(walk.published());
        assert!(!walking(&env));
    }
    assert!(shown >= 2, "{shown}");
    assert_eq!(books(&h.storage).len(), 100);
    assert!(!h.storage.exists(BUILDING));
}

/// A slow walk of a large library: each checkpoint sorts all books found,
/// so those after two minutes also want a quarter more books than shown
/// (not as many sorts as the walk takes minutes).
#[test]
fn checkpoints_of_a_large_slow_walk_grow_by_a_quarter() {
    let mut h = harness(library(1_500));
    let mut walk = Walk::start(&h.env(), Vec::new()).unwrap();
    let mut shown = Vec::new();
    loop {
        h.now = Instant(h.now.0 + 2_000);
        let env = h.env();
        let done = walk.step(&env).unwrap();
        if walk.published() {
            shown.push(books(&h.storage).len());
        }
        if done {
            break;
        }
    }
    for pair in shown.windows(2) {
        let grown = pair[1] >= 2 * pair[0] || pair[1] >= pair[0] + (pair[0] / 4).max(50);
        assert!(grown, "{shown:?}");
    }
    // Every two minutes would be about 25 (a book a step, two seconds).
    assert!((5..16).contains(&shown.len()), "{shown:?}");
}

/// A catalog without books (the card was empty when it was made) is as
/// none (#109): the books copied since show at checkpoints, the library
/// says they are those found so far, and it ends with all of them.
#[test]
fn a_walk_over_a_catalog_without_books_shows_them_as_a_first_walk() {
    let h = harness(MemoryStorage::new().with("books/readme.md", "no book"));
    let env = h.env();
    walk(&env);
    assert!(
        library::emptiness::is_empty(&h.storage),
        "a catalog without books"
    );
    for i in 0..200 {
        h.storage
            .write(&format!("books/f{}/b{i:04}.txt", i % 5), b"a book")
            .unwrap();
    }
    let mut walk = Walk::start(&env, Vec::new()).unwrap();
    assert!(!h.storage.exists(CATALOG), "walked as where there is none");
    let mut shown = Vec::new();
    while !walk.step(&env).unwrap() {
        if walk.published() {
            shown.push(books(&h.storage).len());
        }
        assert!(walking(&env) || !h.storage.exists(BUILDING));
    }
    assert!(!walking(&env));
    assert!(shown.len() >= 2 && shown[0] < 200, "{shown:?}");
    assert_eq!(books(&h.storage).len(), 200);
    // With books, the catalog stays while a walk goes on.
    let mut walk = Walk::start(&env, Vec::new()).unwrap();
    assert!(h.storage.exists(CATALOG));
    while !walk.step(&env).unwrap() {
        assert!(!walk.published());
    }
}
