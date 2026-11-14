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
    /// Of them, the reads of the book file itself.
    book: Cell<u32>,
}

impl Storage for Reads<'_> {
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
        self.count.set(self.count.get() + 1);
        if p == "book.epub" {
            self.book.set(self.book.get() + 1);
        }
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
        book: Cell::new(0),
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

#[test]
fn the_last_block_of_a_section_is_the_one_whose_end_is_the_sections_length() {
    // The provider says a block is the last without reading the next one: it
    // must say what asking for the next block would.
    for name in ["alice-in-wonderland", "die-verwandlung"] {
        let path = format!(
            "{}/../../samples/library/books/{name}.epub",
            env!("CARGO_MANIFEST_DIR")
        );
        let storage = MemoryStorage::new().with("book.epub", std::fs::read(path).unwrap());
        let doc = EpubDocument::open(&storage, "book.epub").unwrap();
        let mut lasts = 0;
        for (section, block) in blocks(&doc, &storage) {
            let past = doc
                .block_at(&storage, Locator::new(section, block.end))
                .unwrap()
                .is_none();
            assert_eq!(
                block.end >= doc.section_len(section),
                past,
                "{name} {section}"
            );
            lasts += usize::from(past);
        }
        assert!(lasts > 1, "{name}: {lasts} sections end");
    }
}

#[test]
fn a_book_opened_again_reads_nothing_of_its_zip_for_its_first_page_but_its_fingerprint() {
    for name in ["alice-in-wonderland", "die-verwandlung"] {
        let path = format!(
            "{}/../../samples/library/books/{name}.epub",
            env!("CARGO_MANIFEST_DIR")
        );
        let storage = MemoryStorage::new().with("book.epub", std::fs::read(path).unwrap());
        let first = EpubDocument::open(&storage, "book.epub").unwrap();
        let _ = blocks(&first, &storage);
        let counted = Reads {
            inner: &storage,
            count: Cell::new(0),
            book: Cell::new(0),
        };
        let doc = EpubDocument::open(&counted, "book.epub").unwrap();
        let mut at = Locator::new(1, 0);
        for _ in 0..25 {
            at = Locator::new(1, doc.block_at(&counted, at).unwrap().unwrap().end);
        }
        // Only the two reads of its fingerprint (the size, the head and the tail of
        // the file tell it is the same book); before (#250) 51 and 25 more reads of
        // the zip's directory.
        assert_eq!(counted.book.get(), 2, "{name}: reads of the book file");
    }
}

/// A card whose book file gives its first two reads (the fingerprint) and then fails.
struct Unreadable<'a> {
    inner: &'a MemoryStorage,
    allowed: Cell<u32>,
}

impl Storage for Unreadable<'_> {
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
        if p == "book.epub" {
            let left = self.allowed.get();
            if left == 0 {
                return Err(StorageError::Io);
            }
            self.allowed.set(left - 1);
        }
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

#[test]
fn a_zip_that_cannot_be_read_fails_where_it_is_first_needed_not_with_a_whole_cache() {
    let path = format!(
        "{}/../../samples/library/books/die-verwandlung.epub",
        env!("CARGO_MANIFEST_DIR")
    );
    let storage = MemoryStorage::new().with("book.epub", std::fs::read(path).unwrap());
    let doc = EpubDocument::open(&storage, "book.epub").unwrap();
    let all = blocks(&doc, &storage);
    // The cache is whole and the zip gives only the reads of the fingerprint:
    // the book opens and reads from the cache.
    let card = Unreadable {
        inner: &storage,
        allowed: Cell::new(2),
    };
    let again = EpubDocument::open(&card, "book.epub").unwrap();
    assert_eq!(blocks(&again, &card), all);
    // A chapter whose conversion is gone needs the zip: an error with a message,
    // no panic.
    let mut ends = Vec::new();
    let mut stack = vec![".paperpaw/plugins/format-epub".to_string()];
    while let Some(dir) = stack.pop() {
        let _ = storage.list(&dir, &mut |e| {
            let path = format!("{dir}/{}", e.name);
            if e.is_dir {
                stack.push(path);
            } else if path.ends_with("/sections/1/end") {
                ends.push(path);
            }
        });
    }
    assert_eq!(ends.len(), 1);
    storage.remove(&ends[0]).unwrap();
    let card = Unreadable {
        inner: &storage,
        allowed: Cell::new(2),
    };
    let third = EpubDocument::open(&card, "book.epub").unwrap();
    let error = third.block_at(&card, Locator::new(1, 0)).unwrap_err();
    assert!(!error.to_string().is_empty());
}
