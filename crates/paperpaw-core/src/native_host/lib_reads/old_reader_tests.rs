#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! Beta 3's reader's own files of a book over a card in memory: moved where
//! they are shared at the book's first read, never over a shared file.
extern crate std;
use super::*;
use crate::native_host::lib_reads::CatalogReads;
use crate::native_host::lib_reads::tests::{Card, annotation_file, card};
use crate::native_host::lib_service::{BookRef, LibQuery};
use crate::native_host::registrations::Record;
use alloc::rc::Rc;
use paperpaw_sdk::annotations;
use std::format;
use std::string::String;

const A: BookRef<'static> = BookRef::Path("books/a.epub");

/// Beta 3's `progress.ini` (`plugins/reader/testdata/progress.ini.v1`).
const PROGRESS: &str = "[progress]\nsection = 7\noffset = 1234\npercent = 41.5\n";

fn old(name: &str) -> String {
    format!(
        "{BOOKS}/{}/{name}",
        paperpaw_sdk::library::key("books/a.epub")
    )
}

fn exists(card: &Card, file: &str) -> bool {
    card.storage.size(file).is_ok()
}

/// Op 13's answer: section, offset, progress in parts per million.
fn position(card: &Card) -> Result<[u32; 3], i32> {
    let mut row = None;
    card.reads().position(A, &mut |record| {
        if let Record::ServiceLibPosition(p) = record {
            row = Some([p.section, p.offset, p.progress_ppm]);
        }
    })?;
    Ok(row.unwrap())
}

/// Op 15's rows of the book (their count).
fn annotation_rows(card: &Card) -> usize {
    let mut rows = 0;
    card.reads()
        .annotations(A, 0, 0, &mut |_| {
            rows += 1;
            core::ops::ControlFlow::Continue(())
        })
        .unwrap();
    rows
}

#[test]
fn the_old_folder_is_the_one_beta_3s_reader_kept() {
    // `plugins/reader/src/screen/book.rs` `book_dir`: the reader's plugin
    // folder, `books`, the path's key.
    assert_eq!(
        old_file("books/a.epub", "progress.ini").unwrap(),
        format!(
            "{}/books/{}/progress.ini",
            paperpaw_sdk::Env::plugin_dir("reader"),
            paperpaw_sdk::library::key("books/a.epub")
        )
    );
    assert_eq!(
        kept_file(ANNOTATIONS, "books/a.epub").unwrap(),
        annotations::file("books/a.epub")
    );
}

#[test]
fn beta_3s_progress_is_the_books_position_and_moved_once() {
    let card = card();
    card.storage
        .write(&old("progress.ini"), PROGRESS.as_bytes())
        .unwrap();
    assert_eq!(position(&card), Ok([7, 1234, 415_000]));
    assert!(!exists(&card, &old("progress.ini")), "moved");
    assert!(exists(&card, &positions::file("books/a.epub")));
    // An interface 6 reader reads the moved file too.
    let kept = positions::load(&*card.storage, "books/a.epub").unwrap();
    assert_eq!((kept.at.section, kept.at.offset), (7, 1234));
    // Read again: the same place, from the shared file.
    assert_eq!(position(&card), Ok([7, 1234, 415_000]));
}

#[test]
fn a_shared_position_is_never_replaced_by_the_old_one() {
    let card = card();
    positions::save(
        &*card.storage,
        "books/a.epub",
        &positions::Position::new(paperpaw_sdk::document::Locator::new(2, 3), 12.0),
    )
    .unwrap();
    card.storage
        .write(&old("progress.ini"), PROGRESS.as_bytes())
        .unwrap();
    assert_eq!(position(&card), Ok([2, 3, 120_000]));
    assert!(exists(&card, &old("progress.ini")), "left as it is");
}

#[test]
fn no_old_file_and_no_position_is_not_found_and_writes_nothing() {
    let card = card();
    assert_eq!(
        position(&card),
        Err(paperpaw_native::host_glue::ERR_NOT_FOUND)
    );
    assert!(!exists(&card, &positions::file("books/a.epub")));
}

#[test]
fn beta_3s_own_annotations_are_listed_and_moved_before_an_edit() {
    let card = card();
    card.storage
        .write(&old("annotations.txt"), annotation_file("n").as_bytes())
        .unwrap();
    // Op 15: the four annotations of the fixture.
    assert_eq!(annotation_rows(&card), 4);
    assert!(!exists(&card, &old("annotations.txt")), "moved");
    assert!(exists(&card, &annotations::file("books/a.epub")));
    // Before an edit (op 17): moved where none is shared yet, else left.
    card.storage
        .write(&old("annotations.txt"), b"# format 2\n")
        .unwrap();
    card.reads().adopt_old_annotations("books/a.epub");
    assert!(exists(&card, &old("annotations.txt")), "a shared file wins");
    assert_eq!(annotation_rows(&card), 4);
}

/// The card of [`card`], whose renames fail while `refuse` is set.
struct Refusing {
    card: Rc<paperpaw_sdk::MemoryStorage>,
    refuse: core::cell::Cell<bool>,
}

impl Storage for Refusing {
    fn read_at(&self, p: &str, a: u64, b: &mut [u8]) -> Result<usize, StorageError> {
        self.card.read_at(p, a, b)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        self.card.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.card.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        self.card.append(p, d)
    }
    fn list(
        &self,
        d: &str,
        v: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), StorageError> {
        self.card.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.card.remove(p)
    }
    fn entry_identity(&self, p: &str) -> Result<Option<u64>, StorageError> {
        self.card.entry_identity(p)
    }
    fn create_dir(&self, p: &str) -> Result<(), StorageError> {
        self.card.create_dir(p)
    }
    fn rename(&self, f: &str, t: &str) -> Result<(), StorageError> {
        if self.refuse.get() {
            return Err(StorageError::Io);
        }
        self.card.rename(f, t)
    }
}

#[test]
fn a_rename_the_card_refuses_fails_no_read_and_is_tried_again() {
    let card = card();
    card.storage
        .write(&old("progress.ini"), PROGRESS.as_bytes())
        .unwrap();
    let refusing = Rc::new(Refusing {
        card: card.storage.clone(),
        refuse: core::cell::Cell::new(true),
    });
    let reads = CatalogReads::new(refusing.clone(), card.env.clone());
    let read = |reads: &CatalogReads| {
        let mut row = None;
        let answer = reads.position(A, &mut |record| {
            if let Record::ServiceLibPosition(p) = record {
                row = Some([p.section, p.offset]);
            }
        });
        answer.map(|()| row.unwrap())
    };
    // Not moved: the read answers as for a book with no position, no error.
    assert_eq!(read(&reads), Err(paperpaw_native::host_glue::ERR_NOT_FOUND));
    assert!(exists(&card, &old("progress.ini")), "left");
    // The same reads again, the card well: moved at the next read.
    refusing.refuse.set(false);
    assert_eq!(read(&reads), Ok([7, 1234]));
    assert!(!exists(&card, &old("progress.ini")), "moved");
}

#[test]
fn a_settled_book_is_not_looked_at_again() {
    let card = card();
    let refusing = Rc::new(Refusing {
        card: card.storage.clone(),
        refuse: core::cell::Cell::new(false),
    });
    let reads = CatalogReads::new(refusing.clone(), card.env.clone());
    // No old file: settled at the first read.
    let _ = reads.position(A, &mut |_| {});
    // One that comes later is not looked for by the same reads (the memo):
    // only a Beta 3 reader writes it, and that one is gone.
    card.storage
        .write(&old("progress.ini"), PROGRESS.as_bytes())
        .unwrap();
    let _ = reads.position(A, &mut |_| {});
    assert!(exists(&card, &old("progress.ini")), "not looked at");
    // Another book in between takes the memo's place: looked at afresh.
    let _ = reads.position(BookRef::Path("books/b.epub"), &mut |_| {});
    let _ = reads.position(A, &mut |_| {});
    assert!(!exists(&card, &old("progress.ini")), "moved");
}
