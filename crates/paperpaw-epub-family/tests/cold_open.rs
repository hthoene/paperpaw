//! The cache of a book opened for the first time (#250): few, large appends
//! (each is a host call and a write to the card, about 27 ms on the X4), and a
//! cache cut short at any append is made again whole at the next open.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::blocks;
use paperpaw_epub_family::EpubDocument;
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};
use std::cell::Cell;

/// A storage that counts appends and refuses all of them from the `cut`-th on
/// (power lost: nothing after it reaches the card).
struct Card<'a> {
    inner: &'a MemoryStorage,
    appends: Cell<usize>,
    cut: usize,
}

impl<'a> Card<'a> {
    fn new(inner: &'a MemoryStorage, cut: usize) -> Self {
        Self {
            inner,
            appends: Cell::new(0),
            cut,
        }
    }
}

impl Storage for Card<'_> {
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
        self.inner.read_at(p, o, b)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        self.inner.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        if self.appends.get() >= self.cut {
            return Err(StorageError::Io);
        }
        self.inner.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.appends.set(self.appends.get() + 1);
        if self.appends.get() > self.cut {
            return Err(StorageError::Io);
        }
        self.inner.append(p, d)
    }
    fn list(&self, d: &str, v: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.inner.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.inner.remove(p)
    }
    fn rename(&self, a: &str, b: &str) -> Result<(), StorageError> {
        self.inner.rename(a, b)
    }
    fn create_dir(&self, p: &str) -> Result<(), StorageError> {
        self.inner.create_dir(p)
    }
    fn remove_dir_all(&self, p: &str) -> Result<(), StorageError> {
        self.inner.remove_dir_all(p)
    }
}

fn sample(name: &str) -> MemoryStorage {
    let path = format!(
        "{}/../../samples/library/books/{name}.epub",
        env!("CARGO_MANIFEST_DIR")
    );
    MemoryStorage::new().with("book.epub", std::fs::read(path).unwrap())
}

/// Appends of a first open that reads every block of the book.
fn cold_appends(name: &str) -> usize {
    let storage = sample(name);
    let card = Card::new(&storage, usize::MAX);
    let doc = EpubDocument::open(&card, "book.epub").unwrap();
    let _ = blocks(&doc, &card);
    card.appends.get()
}

#[test]
fn a_book_opened_for_the_first_time_appends_to_its_cache_in_few_calls() {
    // Before: 659 and 239 (flushes of 256 and 1,024 bytes); 279 and about 130 now.
    let alice = cold_appends("alice-in-wonderland");
    let gregor = cold_appends("die-verwandlung");
    assert!(alice <= 300, "Alice: {alice} appends");
    assert!(gregor <= 140, "Die Verwandlung: {gregor} appends");
}

#[test]
fn a_cache_cut_short_at_any_append_is_made_again_whole() {
    let reference = {
        let storage = sample("die-verwandlung");
        let doc = EpubDocument::open(&storage, "book.epub").unwrap();
        blocks(&doc, &storage)
    };
    let total = cold_appends("die-verwandlung");
    assert!(total > 10);
    for cut in 0..total {
        let storage = sample("die-verwandlung");
        // Power lost after `cut` appends: the open may fail or give a part.
        let card = Card::new(&storage, cut);
        if let Ok(doc) = EpubDocument::open(&card, "book.epub") {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = blocks(&doc, &card);
            }));
        }
        // The card back: the next open reads the book whole.
        let again = Card::new(&storage, usize::MAX);
        let doc = EpubDocument::open(&again, "book.epub").unwrap();
        assert_eq!(blocks(&doc, &again), reference, "cut after {cut} appends");
    }
}
