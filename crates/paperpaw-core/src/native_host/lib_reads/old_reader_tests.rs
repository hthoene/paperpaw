#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! Beta 3's reader's own files of a book over a card in memory: moved where
//! they are shared at the book's first read, never over a shared file.
extern crate std;
use super::*;
use crate::native_host::lib_reads::tests::{Card, annotation_file, card};
use crate::native_host::lib_service::{BookRef, LibQuery};
use crate::native_host::registrations::Record;
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
