//! Books opened last that an existing catalog lacks (#253): appended to
//! it at the start, before the walk ends, without rewriting it; a cut
//! off mid-append leaves it readable; a start that has nothing to catch
//! up reads none of it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

extern crate std;
#[allow(
    unused_imports,
    reason = "the names of std the tests had as SDK integration tests, not all used"
)]
use std::{
    borrow::ToOwned,
    boxed::Box,
    format,
    rc::Rc,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use std::borrow::Cow;
use std::cell::Cell;

use super::common;
use crate::kit::library::Book;
use crate::kit::library::catalog::{self, CatchUp, Record, Walk};
use paperpaw_sdk::document::DocumentError;
use paperpaw_sdk::{DirEntry, FormatSpec, Formats, Id, MemoryStorage, Storage, StorageError};

const CATALOG: &str = ".paperpaw/library/catalog.txt";
const BUILDING: &str = ".paperpaw/library/catalog.tmp";
const SEEN: &str = ".paperpaw/library/catalog.seen";
const OPENED: &str = ".paperpaw/library/opened.txt";
const NEW: &str = "books/new.txt";

/// A card that counts what is read of the catalog and written to it, and
/// where an append to the catalog can be cut off after some bytes (the
/// power gone).
struct Spy {
    card: Rc<MemoryStorage>,
    read: Cell<u64>,
    writes: Cell<u32>,
    appended: Cell<u64>,
    cut_after: Cell<Option<usize>>,
}

impl Spy {
    fn new(card: Rc<MemoryStorage>) -> Rc<Spy> {
        Rc::new(Spy {
            card,
            read: Cell::new(0),
            writes: Cell::new(0),
            appended: Cell::new(0),
            cut_after: Cell::new(None),
        })
    }
}

impl Storage for Spy {
    fn read_at(&self, path: &str, at: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let n = self.card.read_at(path, at, buf)?;
        if path == CATALOG {
            self.read.set(self.read.get() + n as u64);
        }
        Ok(n)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.card.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if path == CATALOG || path == BUILDING {
            self.writes.set(self.writes.get() + 1);
        }
        self.card.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if path == BUILDING {
            self.writes.set(self.writes.get() + 1);
        }
        if path == CATALOG {
            if let Some(cut) = self.cut_after.take() {
                self.card.append(path, &data[..cut.min(data.len())])?;
                return Err(StorageError::Io);
            }
            self.appended.set(self.appended.get() + data.len() as u64);
        }
        self.card.append(path, data)
    }
    fn truncate(&self, path: &str, len: u64) -> Result<(), StorageError> {
        self.card.truncate(path, len)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.card.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.card.remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        if to == CATALOG {
            self.writes.set(self.writes.get() + 1);
        }
        self.card.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.card.create_dir(path)
    }
}

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

fn books(card: &MemoryStorage) -> Vec<String> {
    let mut books = Vec::new();
    catalog::each(card, &mut |r| books.push(r.path));
    books.sort();
    books
}

/// A card whose catalog was walked, then loses the line of `books/new.txt`
/// and has it opened last, as after a book was opened (a download, a copy
/// over Wi-Fi) before the catalog knew it. `n` books beside it; the
/// catalog is that of `n` synthetic books where `n` is large.
fn card(n: usize) -> (common::Harness, Rc<MemoryStorage>) {
    let mut h = harness();
    let mut card = MemoryStorage::new().with(NEW, "new book");
    for i in 0..n.min(10) {
        card = card.with(&format!("books/b{i:04}.txt"), format!("book {i}"));
    }
    let card = Rc::new(card);
    h.set_storage(MemoryStorage::new());
    // Walked on the card itself, so that the new book has its details.
    let spy = Spy::new(card.clone());
    {
        let env = h.env_on(spy);
        let mut walk = Walk::start(&env, Vec::new()).unwrap();
        while !walk.step(&env).unwrap() {}
    }
    // The catalog before the book came: the books of the others, and any
    // number of synthetic ones.
    let mut records: Vec<Record> = Vec::new();
    catalog::each(&*card, &mut |r| {
        if r.path != NEW {
            records.push(r);
        }
    });
    for i in 10..n {
        records.push(Record::of(
            &format!("books/f{}/synthetic-book-{i:05}.txt", i % 7),
            &Book::default(),
        ));
    }
    catalog::write(&*card, &records).unwrap();
    card.write(OPENED, format!("{NEW}\nbooks/b0001.txt\n").as_bytes())
        .unwrap();
    (h, card)
}

fn catch_up(h: &mut common::Harness, spy: &Rc<Spy>) -> Result<CatchUp, StorageError> {
    let env = h.env_on(spy.clone());
    let mut catch_up = CatchUp::start(&env)?;
    while !catch_up.step(&env)? {}
    Ok(catch_up)
}

/// The book opened last that the catalog lacks is in it at the start,
/// before any walk step; the catalog before it is as it was and nothing
/// was written anew.
#[test]
fn a_missing_opened_book_appears_before_the_walk_ends() {
    let (mut h, card) = card(40);
    let before = card.get(CATALOG).unwrap();
    assert!(!books(&card).contains(&NEW.to_string()));
    let spy = Spy::new(card.clone());
    let done = catch_up(&mut h, &spy).unwrap();
    assert!(done.appended());
    assert!(books(&card).contains(&NEW.to_string()));
    let after = card.get(CATALOG).unwrap();
    assert!(after.starts_with(&before));
    assert_eq!(spy.writes.get(), 0, "the catalog is not written anew");
    assert!(!card.exists(BUILDING));
    // The record has the opened mark and the book once.
    let mut found = Vec::new();
    catalog::each(&*card, &mut |r| found.push(r));
    let record = found.iter().find(|r| r.path == NEW).unwrap();
    assert!(record.status.opened);
    assert_eq!(found.iter().filter(|r| r.path == NEW).count(), 1);
    // The walk that follows folds it in: the same books, once each.
    let env = h.env_on(spy);
    let mut walk = Walk::start(&env, Vec::new()).unwrap();
    while !walk.step(&env).unwrap() {}
    let all = books(&card);
    assert!(all.contains(&NEW.to_string()));
    let mut once = all.clone();
    once.dedup();
    assert_eq!(all, once);
}

/// Cut off mid-append, at any byte: the books before are whole, a next
/// start cuts off what was left of the line and appends it whole, and
/// the walk after makes the same catalog.
#[test]
fn a_cut_mid_append_leaves_the_catalog_readable() {
    let (mut h, card) = card(40);
    let before = card.get(CATALOG).unwrap();
    let kept = books(&card);
    let line_length = {
        let spy = Spy::new(card.clone());
        catch_up(&mut h, &spy).unwrap();
        card.get(CATALOG).unwrap().len() - before.len()
    };
    for cut in [0, 1, 5, line_length / 2, line_length - 2, line_length - 1] {
        card.write(CATALOG, &before).unwrap();
        card.remove(SEEN).unwrap_or(());
        let spy = Spy::new(card.clone());
        spy.cut_after.set(Some(cut));
        assert!(catch_up(&mut h, &spy).is_err(), "{cut}");
        let torn = card.get(CATALOG).unwrap();
        assert!(torn.starts_with(&before), "{cut}: what was there is whole");
        assert!(!card.exists(SEEN), "{cut}: not noted as caught up");
        for book in &kept {
            assert!(books(&card).contains(book), "{cut}: {book}");
        }
        // The next start: the new book once, no line cut short left.
        let healthy = Spy::new(card.clone());
        assert!(catch_up(&mut h, &healthy).unwrap().appended(), "{cut}");
        let whole = card.get(CATALOG).unwrap();
        assert!(whole.ends_with(b"\n"), "{cut}");
        let found = books(&card);
        assert_eq!(
            found.iter().filter(|b| b.as_str() == NEW).count(),
            1,
            "{cut}"
        );
        assert_eq!(found.len(), kept.len() + 1, "{cut}: no line cut short");
        assert_eq!(whole.len(), before.len() + line_length, "{cut}");
    }
}

/// A start in a 5,000-book catalog: one pass over it to find what is
/// missing, one line appended, nothing written anew; the next start,
/// where all the opened books are known to be there, reads none of the
/// books of the catalog.
#[test]
fn the_start_cost_in_a_large_catalog_is_one_read_and_no_rewrite() {
    let (mut h, card) = card(5_000);
    let size = card.get(CATALOG).unwrap().len() as u64;
    assert!(size > 300_000, "{size}");
    let spy = Spy::new(card.clone());
    catch_up(&mut h, &spy).unwrap();
    // The catalog read once (and the line it appended not at all).
    assert!(
        spy.read.get() <= size + size / 16,
        "{} of {size}",
        spy.read.get()
    );
    assert_eq!(spy.writes.get(), 0);
    assert!(spy.appended.get() > 0 && spy.appended.get() < 1_024);
    assert!(!card.exists(BUILDING));
    // The next start: opened as before, caught up: not a book of it read
    // (the header only), nothing written to it.
    let again = Spy::new(card.clone());
    let done = catch_up(&mut h, &again).unwrap();
    assert!(!done.appended());
    assert!(again.read.get() <= 128, "{}", again.read.get());
    assert_eq!(again.writes.get() + again.appended.get() as u32, 0);
    // A book opened since is looked for alone: one more pass, nothing
    // appended where it is there.
    card.write(
        OPENED,
        format!("{NEW}\nbooks/f1/synthetic-book-00001.txt\n").as_bytes(),
    )
    .unwrap();
    let third = Spy::new(card.clone());
    assert!(!catch_up(&mut h, &third).unwrap().appended());
    assert_eq!(third.appended.get(), 0);
}
