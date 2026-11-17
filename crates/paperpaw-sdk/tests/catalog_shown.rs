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
use paperpaw_sdk::library::catalog::{self, Walk};
use paperpaw_sdk::{Env, FormatSpec, Formats, Id, Instant, MemoryStorage, Storage};

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
