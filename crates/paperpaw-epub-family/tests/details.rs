//! A book opened only for its details (the library's walk, #98): its
//! metadata, chapters and cover read from the archive with nothing
//! written; the package goes into the cache the first time the book is
//! read, beside a cover made for the library before.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{Recording, blocks, container, opf, xhtml, zip};
use paperpaw_epub_family::EpubDocument;
use paperpaw_sdk::document::{Document, Locator};
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

const KAFKA: &[u8] = include_bytes!("../../../samples/library/books/die-verwandlung.epub");
const ALICE: &[u8] = include_bytes!("../../../samples/library/books/alice-in-wonderland.epub");

/// Where format-epub keeps the package of a book.
fn package_of(storage: &dyn Storage) -> Vec<String> {
    let mut found = Vec::new();
    files(storage, ".paperpaw/plugins/format-epub", &mut found);
    found.retain(|path| path.ends_with("/package.ini"));
    found
}

fn files(storage: &dyn Storage, dir: &str, out: &mut Vec<String>) {
    let mut entries = Vec::new();
    let _ = storage.list(dir, &mut |entry| {
        entries.push((entry.name.to_string(), entry.is_dir));
    });
    for (name, is_dir) in entries {
        let path = format!("{dir}/{name}");
        if is_dir {
            files(storage, &path, out);
        } else {
            out.push(path);
        }
    }
}

#[test]
fn opened_for_its_details_nothing_is_written() {
    let storage = Recording::new(MemoryStorage::new().with("books/kafka.epub", KAFKA));
    let doc = EpubDocument::open(&storage, "books/kafka.epub").unwrap();
    let metadata = doc.metadata();
    assert_eq!(metadata.title.as_deref(), Some("Die Verwandlung"));
    assert_eq!(metadata.authors, ["Franz Kafka"]);
    assert!(doc.sections() > 0);
    assert_eq!(*storage.written.borrow(), Vec::<String>::new());
    assert!(package_of(&storage).is_empty());
}

/// Read, the package goes into the cache, with the contents; opened
/// again, it is read from there and nothing is written.
#[test]
fn the_first_read_puts_the_package_in_the_cache() {
    let storage = Recording::new(MemoryStorage::new().with("books/kafka.epub", KAFKA));
    let doc = EpubDocument::open(&storage, "books/kafka.epub").unwrap();
    let first = blocks(&doc, &storage);
    assert!(!first.is_empty());
    assert_eq!(package_of(&storage).len(), 1);
    let toc = doc.toc_len(&storage);
    assert!(toc > 0);
    storage.written.borrow_mut().clear();
    let again = EpubDocument::open(&storage, "books/kafka.epub").unwrap();
    assert_eq!(again.metadata(), doc.metadata());
    assert_eq!(again.toc_len(&storage), toc);
    assert_eq!(blocks(&again, &storage), first);
    assert_eq!(*storage.written.borrow(), Vec::<String>::new());
}

/// The contents, wanted first (before any chapter), are there.
#[test]
fn the_contents_are_there_before_any_chapter() {
    let storage = MemoryStorage::new().with("books/alice.epub", ALICE);
    let doc = EpubDocument::open(&storage, "books/alice.epub").unwrap();
    let len = doc.toc_len(&storage);
    assert!(len > 0);
    let first = doc.toc_entry(&storage, 0).unwrap();
    assert!(!first.title.is_empty());
    assert!(doc.toc_target(&storage, len - 1).is_some());
}

/// A cover made for the library (before the book was read) stays when
/// the book is read: the library's details point at it.
#[test]
fn a_cover_made_for_the_library_stays_when_the_book_is_read() {
    let storage = MemoryStorage::new().with("books/alice.epub", ALICE);
    let doc = EpubDocument::open(&storage, "books/alice.epub").unwrap();
    let cover = doc.cover(&storage).unwrap().unwrap();
    assert!(package_of(&storage).is_empty());
    let read = EpubDocument::open(&storage, "books/alice.epub").unwrap();
    assert!(!blocks(&read, &storage).is_empty());
    assert!(storage.size(&cover.path).is_ok());
    let again = EpubDocument::open(&storage, "books/alice.epub").unwrap();
    assert_eq!(again.cover(&storage).unwrap(), Some(cover));
}

/// Another book copied over one whose details were read (its cover made,
/// its package never written): its own metadata and cover, and its own
/// chapters once read.
#[test]
fn a_book_replaced_after_its_details_were_read_is_read_anew() {
    let storage = MemoryStorage::new().with("books/book.epub", ALICE);
    let alice = EpubDocument::open(&storage, "books/book.epub").unwrap();
    let alice_cover = alice.cover(&storage).unwrap().unwrap();
    let alice_picture = storage.size(&alice_cover.path).unwrap();
    drop(alice);
    storage.write("books/book.epub", KAFKA).unwrap();
    let kafka = EpubDocument::open(&storage, "books/book.epub").unwrap();
    assert_eq!(kafka.metadata().title.as_deref(), Some("Die Verwandlung"));
    let kafka_cover = kafka.cover(&storage).unwrap().unwrap();
    let kafka_picture = storage.size(&kafka_cover.path).unwrap();
    assert!(
        (kafka_cover.width, kafka_cover.height, kafka_picture)
            != (alice_cover.width, alice_cover.height, alice_picture),
        "the old book's cover stays: {kafka_cover:?}"
    );
    let text = blocks(&kafka, &storage);
    assert!(
        text.iter()
            .any(|(_, block)| block.text.contains("Gregor") || block.text.contains("Samsa"))
    );
    // Read, then replaced again: the package and chapters go.
    storage.write("books/book.epub", ALICE).unwrap();
    let alice = EpubDocument::open(&storage, "books/book.epub").unwrap();
    assert!(package_of(&storage).is_empty());
    assert!(
        blocks(&alice, &storage)
            .iter()
            .all(|(_, block)| !block.text.contains("Samsa"))
    );
}

/// Its cover made during the walk, a book is replaced on the card and
/// read before its cover is asked for: the cover is the new book's.
#[test]
fn a_book_replaced_and_read_before_its_cover_shows_its_own() {
    let storage = MemoryStorage::new().with("books/book.epub", ALICE);
    let alice = EpubDocument::open(&storage, "books/book.epub").unwrap();
    let alice_cover = alice.cover(&storage).unwrap().unwrap();
    let alice_picture = storage.size(&alice_cover.path).unwrap();
    drop(alice);
    storage.write("books/book.epub", KAFKA).unwrap();
    let kafka = EpubDocument::open(&storage, "books/book.epub").unwrap();
    assert!(!blocks(&kafka, &storage).is_empty());
    let cover = kafka.cover(&storage).unwrap().unwrap();
    let picture = storage.size(&cover.path).unwrap();
    assert!(
        (cover.width, cover.height, picture)
            != (alice_cover.width, alice_cover.height, alice_picture),
        "the old book's cover stays: {cover:?}"
    );
}

/// A book of one chapter whose cover is `cover`.
fn with_cover(cover: Vec<u8>) -> Vec<u8> {
    let package = opf(
        "<dc:title>Covered</dc:title><meta name=\"cover\" content=\"cov\"/>",
        &[
            ("cov", "cover.png", "image/png", "cover-image"),
            ("a", "a.xhtml", "application/xhtml+xml", ""),
        ],
        &["a"],
    );
    zip(
        &[
            (
                "META-INF/container.xml",
                container("OEBPS/content.opf").into_bytes(),
            ),
            ("OEBPS/content.opf", package.into_bytes()),
            ("OEBPS/cover.png", cover),
            ("OEBPS/a.xhtml", xhtml("<p>Text</p>").into_bytes()),
        ],
        false,
    )
}

/// The cover of a book could not be made; a good copy of it replaces it on
/// the card: the walk makes its cover, and the book read keeps it.
#[test]
fn a_cover_that_failed_is_made_for_the_book_that_replaced_it() {
    let storage = MemoryStorage::new().with("books/book.epub", with_cover(vec![1, 2, 3]));
    let broken = EpubDocument::open(&storage, "books/book.epub").unwrap();
    assert!(broken.cover(&storage).is_err());
    assert_eq!(broken.cover(&storage).unwrap(), None);
    drop(broken);
    storage.write("books/book.epub", ALICE).unwrap();
    let walked = EpubDocument::open(&storage, "books/book.epub").unwrap();
    let cover = walked.cover(&storage).unwrap().unwrap();
    drop(walked);
    let read = EpubDocument::open(&storage, "books/book.epub").unwrap();
    assert!(!blocks(&read, &storage).is_empty());
    assert_eq!(read.cover(&storage).unwrap(), Some(cover));
}

/// A removal of a book's cache cut off (power gone) left its chapters
/// without its package: they go, and another book copied there is read as
/// itself.
#[test]
fn chapters_left_without_their_package_go() {
    let storage = MemoryStorage::new().with("books/book.epub", ALICE);
    let alice = EpubDocument::open(&storage, "books/book.epub").unwrap();
    let _ = alice.cover(&storage).unwrap();
    let alice_text = blocks(&alice, &storage);
    drop(alice);
    let package = package_of(&storage);
    assert_eq!(package.len(), 1);
    storage.remove(&package[0]).unwrap();
    storage.write("books/book.epub", KAFKA).unwrap();
    let kafka = EpubDocument::open(&storage, "books/book.epub").unwrap();
    let text = blocks(&kafka, &storage);
    assert_ne!(text, alice_text);
    assert!(text.iter().all(|(_, block)| !block.text.contains("Alice")));
    assert!(
        text.iter()
            .any(|(_, block)| block.text.contains("Gregor") || block.text.contains("Samsa"))
    );
}

/// Contents that cannot be written into the cache (here the card refuses
/// every write): the book's details are there, a chapter is an error, and
/// it is not tried again on every look until the book is opened again.
#[test]
fn a_package_that_cannot_be_cached_is_not_tried_again_and_again() {
    let storage = Recording::limited(MemoryStorage::new().with("books/alice.epub", ALICE), 0);
    let doc = EpubDocument::open(&storage, "books/alice.epub").unwrap();
    assert!(doc.metadata().title.is_some());
    assert!(doc.block_at(&storage, Locator::new(0, 0)).is_err());
    let tried = storage.written.borrow().len();
    assert!(tried > 0);
    assert!(doc.block_at(&storage, Locator::new(0, 0)).is_err());
    assert_eq!(doc.toc_len(&storage), 0);
    assert!(doc.toc_entry(&storage, 0).is_none());
    assert!(doc.fonts(&storage).is_empty());
    assert_eq!(storage.written.borrow().len(), tried);
}

/// A broken NCX (and no navigation document): the book opens with its
/// details and is read, with no contents.
#[test]
fn a_broken_ncx_reads_without_contents() {
    let package = opf(
        "<dc:title>No contents</dc:title>",
        &[
            ("ncx", "toc.ncx", "application/x-dtbncx+xml", ""),
            ("a", "a.xhtml", "application/xhtml+xml", ""),
        ],
        &["a"],
    )
    .replace("<spine>", "<spine toc=\"ncx\">");
    let epub = zip(
        &[
            (
                "META-INF/container.xml",
                container("OEBPS/content.opf").into_bytes(),
            ),
            ("OEBPS/content.opf", package.into_bytes()),
            (
                "OEBPS/toc.ncx",
                b"<ncx><navMap><navPoint><navLabel><te".to_vec(),
            ),
            ("OEBPS/a.xhtml", xhtml("<p>Text</p>").into_bytes()),
        ],
        false,
    );
    let storage = MemoryStorage::new().with("books/book.epub", epub);
    let doc = EpubDocument::open(&storage, "books/book.epub").unwrap();
    assert_eq!(doc.metadata().title.as_deref(), Some("No contents"));
    assert_eq!(doc.toc_len(&storage), 0);
    let text = blocks(&doc, &storage);
    assert_eq!(text.len(), 1);
    assert_eq!(text[0].1.text, "Text");
}

/// A card where another book's cache folder cannot be removed (damaged,
/// #388), nor listed if `unlisted`: counted writes, nothing else changed.
struct Stuck {
    inner: MemoryStorage,
    writes: std::cell::Cell<usize>,
    unlisted: bool,
}

impl Storage for Stuck {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.inner.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.writes.set(self.writes.get() + 1);
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.writes.set(self.writes.get() + 1);
        self.inner.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        match self.unlisted {
            true => Err(StorageError::Io),
            false => self.inner.list(dir, visit),
        }
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.writes.set(self.writes.get() + 1);
        self.inner.create_dir(path)
    }
    fn remove_dir_all(&self, _: &str) -> Result<(), StorageError> {
        Err(StorageError::InvalidPath)
    }
}

/// Another book's cache that cannot be removed, listed or not (#397 N4):
/// the book replacing it shows its details, and nothing of the folder
/// (chapters, contents, printed pages, cover) is read as its own, nor
/// written there.
#[test]
fn a_cache_that_cannot_be_cleared_is_never_read_from() {
    for unlisted in [false, true] {
        let storage = MemoryStorage::new().with("books/book.epub", ALICE);
        let alice = EpubDocument::open(&storage, "books/book.epub").unwrap();
        let _ = alice.cover(&storage).unwrap();
        assert!(!blocks(&alice, &storage).is_empty());
        drop(alice);
        storage.write("books/book.epub", KAFKA).unwrap();
        let stuck = Stuck {
            inner: storage,
            writes: std::cell::Cell::new(0),
            unlisted,
        };
        let kafka = EpubDocument::open(&stuck, "books/book.epub").unwrap();
        assert_eq!(kafka.metadata().title.as_deref(), Some("Die Verwandlung"));
        let start = Locator::new(0, 0);
        assert!(kafka.block_at(&stuck, start).is_err(), "{unlisted}");
        assert!(kafka.cover(&stuck).is_err());
        assert_eq!(kafka.toc_len(&stuck), 0);
        assert!(kafka.toc_entry(&stuck, 0).is_none());
        assert!(kafka.page_label(&stuck, start).is_none());
        assert!(kafka.fonts(&stuck).is_empty());
        assert_eq!(stuck.writes.get(), 0);
    }
}
