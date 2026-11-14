//! A book opened again reads its cache with few reads (#250): each read is a
//! call to the card (about 1 ms on the X4, more with the host's own work),
//! and a page is laid out from some twenty to forty blocks.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::blocks;
use paperpaw_epub_family::EpubDocument;
use paperpaw_sdk::document::{Document, Locator};
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};
use std::cell::Cell;

/// A storage that counts the reads.
struct Reads<'a> {
    inner: &'a MemoryStorage,
    count: Cell<u32>,
}

impl Storage for Reads<'_> {
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
        self.count.set(self.count.get() + 1);
        self.inner.read_at(p, o, b)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        self.inner.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.inner.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
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

/// Reads of the cache for the first `page` blocks of section 1 and for all the
/// blocks of the book, of a book opened again (its cache converted before).
fn warm(name: &str, page: usize) -> (u32, (usize, u32)) {
    let path = format!(
        "{}/../../samples/library/books/{name}.epub",
        env!("CARGO_MANIFEST_DIR")
    );
    let storage = MemoryStorage::new().with("book.epub", std::fs::read(path).unwrap());
    let doc = EpubDocument::open(&storage, "book.epub").unwrap();
    let _ = blocks(&doc, &storage);
    let counted = Reads {
        inner: &storage,
        count: Cell::new(0),
    };
    let doc = EpubDocument::open(&counted, "book.epub").unwrap();
    let open = counted.count.get();
    let mut at = Locator::new(1, 0);
    for _ in 0..page {
        match doc.block_at(&counted, at).unwrap() {
            Some(block) => at = Locator::new(1, block.end),
            None => break,
        }
    }
    let first_page = counted.count.get() - open;
    let all = blocks(&doc, &counted).len();
    (first_page, (all, counted.count.get() - open - first_page))
}

#[test]
fn a_page_of_a_book_opened_again_is_read_with_about_one_read_a_block() {
    // Before (#250): 81 and 83 reads for 25 blocks, about 3.2 a block.
    for (name, most) in [("alice-in-wonderland", 40), ("die-verwandlung", 40)] {
        let (page, _) = warm(name, 25);
        assert!(page <= most, "{name}: {page} reads for 25 blocks");
    }
}

#[test]
fn a_book_is_read_through_with_one_read_a_block_and_a_few_for_its_index() {
    for name in ["alice-in-wonderland", "die-verwandlung"] {
        let (_, (blocks, reads)) = warm(name, 25);
        assert!(blocks > 100, "{name}: {blocks} blocks");
        // The records one read each, the index one in thirty, and a few for each
        // chapter's files (before: about 3.2 a block).
        assert!(
            reads as usize <= blocks + blocks / 4,
            "{name}: {reads} reads for {blocks} blocks"
        );
    }
}
