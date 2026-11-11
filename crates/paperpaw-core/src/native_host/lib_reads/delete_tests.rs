#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! Op 9 over a catalog on a card in memory: what is a book, what is gone, and
//! the facts of what is kept for it.
extern crate std;
use super::super::tests::card;
use super::*;
use crate::native_host::lib_service::{BookRef, LibWrite};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NOT_FOUND};
use paperpaw_sdk::annotations::{Annotation, Kind, Mark};
use paperpaw_sdk::document::Locator;
use paperpaw_sdk::{Storage, library};

#[test]
fn a_book_with_nothing_kept_is_deletable_with_no_facts() {
    let card = card();
    let reads = card.reads();
    assert_eq!(reads.can_delete("books/c.epub"), (OK, 0));
}

#[test]
fn the_facts_say_what_is_kept_for_the_book() {
    let card = card();
    let reads = card.reads();
    // An annotation on C, a position on A; plugins keep data for E.
    paperpaw_sdk::annotations::add(
        &*card.storage,
        "books/c.epub",
        Annotation::new(
            Mark::new(
                Kind::Highlight,
                Locator::new(1, 0),
                Locator::new(1, 5),
                false,
            ),
            10,
            1,
            1,
            "text",
            "",
        ),
    )
    .unwrap();
    reads
        .write_position(
            BookRef::Path("books/a.epub"),
            &crate::native_host::registrations::ServiceLibPosition {
                section: 1,
                offset: 2,
                insert: 0,
                progress_ppm: 100_000,
                conversion: 0,
                updated: 0,
                xpointer: "",
                flags: 0,
            },
            false,
        )
        .unwrap();
    assert_eq!(reads.can_delete("books/c.epub"), (OK, HAS_ANNOTATIONS));
    assert_eq!(reads.can_delete("books/a.epub"), (OK, HAS_POSITION));
    assert_eq!(reads.can_delete("books/e.epub"), (OK, HAS_BOOK_DATA));
}

#[test]
fn what_is_no_book_is_invalid_and_a_file_that_is_gone_is_not_found() {
    let card = card();
    let reads = card.reads();
    // An unknown format, no extension, and the host's own folder.
    for path in [
        "books/d.txt",
        "books/readme",
        ".paperpaw/library/catalog.txt",
    ] {
        assert_eq!(reads.can_delete(path), (ERR_INVALID, 0), "{path}");
    }
    assert_eq!(reads.can_delete(".PaperPaw/x.epub"), (ERR_INVALID, 0));
    assert_eq!(reads.can_delete("books/none.epub"), (ERR_NOT_FOUND, 0));
    // B is in the catalog but its file is not on the card.
    assert_eq!(reads.can_delete("books/b.epub"), (ERR_NOT_FOUND, 0));
}

#[test]
fn it_changes_nothing() {
    let card = card();
    let reads = card.reads();
    let before = card.storage.size("books/c.epub").unwrap();
    let _ = reads.can_delete("books/c.epub");
    assert_eq!(card.storage.size("books/c.epub"), Ok(before));
    assert!(library::book(&*card.storage, "books/c.epub").is_none());
}
