#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The catalog's side of a delete over a card in memory: what is gone after
//! each phase, that a repeat changes nothing, and that other books are left.
extern crate std;
use super::*;
use crate::native_host::lib_reads::tests::card;
use crate::native_host::lib_service::LibQuery;
use paperpaw_sdk::{annotations, positions};

/// A book with everything the two phases remove.
fn kept(card: &crate::native_host::lib_reads::tests::Card, path: &str) {
    library::set_field(&*card.storage, path, "stars.rating", "4");
    positions::save(
        &*card.storage,
        path,
        &positions::Position::new(paperpaw_sdk::document::Locator::new(2, 3), 12.0),
    )
    .unwrap();
    annotations::add(
        &*card.storage,
        path,
        annotations::Annotation::new(
            annotations::Mark::new(
                annotations::Kind::Highlight,
                paperpaw_sdk::document::Locator::new(1, 0),
                paperpaw_sdk::document::Locator::new(1, 4),
                false,
            ),
            1,
            1,
            1,
            "text",
            "",
        ),
    )
    .unwrap();
    importer::set(&*card.storage, path, "news").unwrap();
}

fn exists(card: &crate::native_host::lib_reads::tests::Card, file: &str) -> bool {
    card.storage.size(file).is_ok()
}

#[test]
fn the_annotation_file_is_the_one_the_sdk_keeps() {
    assert_eq!(
        kept_file(ANNOTATIONS, "books/c.epub").unwrap(),
        annotations::file("books/c.epub")
    );
}

#[test]
fn details_forgets_the_catalog_line_the_details_and_the_fields_but_not_the_marks() {
    let card = card();
    kept(&card, "books/c.epub");
    kept(&card, "books/a.epub");
    assert!(library::book(&*card.storage, "books/c.epub").is_some());
    details(&*card.storage, "books/c.epub").unwrap();
    assert!(library::book(&*card.storage, "books/c.epub").is_none());
    // The marks are the next phase's; the other book is untouched.
    assert!(exists(&card, &positions::file("books/c.epub")));
    assert!(exists(&card, &annotations::file("books/c.epub")));
    assert!(library::book(&*card.storage, "books/a.epub").is_some());
    // The book is out of the catalog: a read says so.
    let reads = card.reads();
    assert_eq!(
        reads.book(
            crate::native_host::lib_service::BookRef::Path("books/c.epub"),
            &mut |_| {}
        ),
        Err(paperpaw_native::host_glue::ERR_NOT_FOUND)
    );
}

#[test]
fn marks_removes_the_position_the_annotations_and_the_importer_record() {
    let card = card();
    kept(&card, "books/c.epub");
    kept(&card, "books/a.epub");
    marks(&*card.storage, "books/c.epub").unwrap();
    assert!(!exists(&card, &positions::file("books/c.epub")));
    assert!(!exists(&card, &annotations::file("books/c.epub")));
    assert_eq!(
        importer::is_by(&*card.storage, "books/c.epub", "news"),
        Ok(false)
    );
    assert!(exists(&card, &positions::file("books/a.epub")));
    assert!(exists(&card, &annotations::file("books/a.epub")));
    assert_eq!(
        importer::is_by(&*card.storage, "books/a.epub", "news"),
        Ok(true)
    );
}

#[test]
fn a_repeat_after_a_cut_finds_it_done_and_a_book_that_kept_nothing_is_no_error() {
    let card = card();
    kept(&card, "books/c.epub");
    for _ in 0..2 {
        details(&*card.storage, "books/c.epub").unwrap();
        marks(&*card.storage, "books/c.epub").unwrap();
    }
    // Book E kept nothing at all.
    details(&*card.storage, "books/e.epub").unwrap();
    marks(&*card.storage, "books/e.epub").unwrap();
    assert!(!exists(&card, &positions::file("books/c.epub")));
}
