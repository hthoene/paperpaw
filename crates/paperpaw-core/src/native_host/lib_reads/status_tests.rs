#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! Op 8 over a catalog on a card in memory: what each bit changes, both masks
//! together, idempotence, and what is answered where the book or a list is not
//! as it should be.
extern crate std;
use super::super::tests::card;
use super::*;
use crate::native_host::lib_service::LibWrite;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_IO, ERR_NOT_FOUND};
use paperpaw_platform::Storage;
use paperpaw_sdk::library;
use std::format;

const C: BookRef<'static> = BookRef::Path("books/c.epub");
const E: BookRef<'static> = BookRef::Path("books/e.epub");

const FINISHED: u32 = 1;
const FAVOURITE: u32 = 2;
const OPENED: u32 = 4;

#[test]
fn favourite_and_opened_are_set_and_cleared_in_their_lists() {
    let card = card();
    let reads = card.reads();
    reads.set_status(C, FAVOURITE | OPENED, 0).unwrap();
    assert_eq!(library::favourites(&*card.storage), ["books/c.epub"]);
    assert_eq!(library::opened_books(&*card.storage), ["books/c.epub"]);
    // Opened is the order of the books opened last: the newest first, once.
    reads.set_status(E, OPENED, 0).unwrap();
    reads.set_status(C, OPENED, 0).unwrap();
    reads.set_status(C, OPENED, 0).unwrap();
    assert_eq!(
        library::opened_books(&*card.storage),
        ["books/c.epub", "books/e.epub"]
    );
    reads.set_status(C, 0, FAVOURITE | OPENED).unwrap();
    assert!(library::favourites(&*card.storage).is_empty());
    assert_eq!(library::opened_books(&*card.storage), ["books/e.epub"]);
    // Clearing what is not set is no error.
    reads.set_status(C, 0, FAVOURITE | OPENED).unwrap();
}

#[test]
fn finished_is_the_hundred_percent_of_the_details_and_clearing_marks_it_unread() {
    let card = card();
    let reads = card.reads();
    reads.set_status(E, FINISHED, 0).unwrap();
    assert_eq!(
        library::book(&*card.storage, "books/e.epub")
            .unwrap()
            .progress,
        Some(100)
    );
    reads.set_status(E, 0, FINISHED).unwrap();
    assert_eq!(
        library::book(&*card.storage, "books/e.epub")
            .unwrap()
            .progress,
        None
    );
}

#[test]
fn both_masks_are_applied_together_and_a_bit_in_neither_is_left_alone() {
    let card = card();
    let reads = card.reads();
    reads.set_status(C, FAVOURITE | OPENED, 0).unwrap();
    reads.set_status(C, FINISHED, FAVOURITE).unwrap();
    assert!(library::favourites(&*card.storage).is_empty());
    assert_eq!(library::opened_books(&*card.storage), ["books/c.epub"]);
    assert_eq!(
        library::book(&*card.storage, "books/c.epub")
            .unwrap()
            .progress,
        Some(100)
    );
}

#[test]
fn the_catalog_says_the_new_status_at_once() {
    let card = card();
    let reads = card.reads();
    reads.set_status(E, FAVOURITE | OPENED, 0).unwrap();
    let status = library::catalog::changes(&*card.storage)
        .status(paperpaw_sdk::digest::fnv1a64("books/e.epub"));
    let status = status.unwrap();
    assert!(status.favourite && status.opened);
}

#[test]
fn a_list_keeps_the_newest_up_to_its_cap() {
    let card = card();
    // 101 books opened one after the other: the list holds the last 100.
    for n in 0..101 {
        let path = format!("books/n{n}.epub");
        card.storage.write(&path, b"x").unwrap();
        library::try_set_status(&*card.storage, &path, OPENED, 0).unwrap();
    }
    let opened = library::opened_books(&*card.storage);
    assert_eq!(opened.len(), 100);
    assert_eq!(opened[0], "books/n100.epub");
    assert_eq!(opened[99], "books/n1.epub");
}

#[test]
fn a_book_not_in_the_catalog_is_not_found_and_nothing_is_written() {
    let card = card();
    let reads = card.reads();
    assert_eq!(
        reads.set_status(BookRef::Path("books/none.epub"), FAVOURITE, 0),
        Err(ERR_NOT_FOUND)
    );
    assert!(library::favourites(&*card.storage).is_empty());
}

#[test]
fn a_list_that_is_no_text_is_not_written_over() {
    let card = card();
    let reads = card.reads();
    card.storage
        .write(".paperpaw/library/favourites.txt", &[0xff, 0xfe, 0x00])
        .unwrap();
    assert!(reads.set_status(C, FAVOURITE, 0).is_err());
    assert_eq!(
        card.storage.size(".paperpaw/library/favourites.txt"),
        Ok(3),
        "the list is as it was"
    );
}

#[test]
fn a_book_in_use_is_busy_and_nothing_changes() {
    let card = card();
    let reads = card.reads();
    assert_eq!(
        reads.set_status_unless(C, FAVOURITE, 0, &|path| path == "books/c.epub"),
        Err(ERR_BUSY)
    );
    assert!(library::favourites(&*card.storage).is_empty());
    let _ = ERR_IO;
}

#[test]
fn a_list_is_edited_in_place_whatever_its_line_endings_blanks_and_marks() {
    let card = card();
    let file = ".paperpaw/library/opened.txt";
    // CRLF, a blank line and the path twice.
    card.storage
        .write(
            file,
            b"books/a.epub\r\n\nbooks/c.epub\nbooks/e.epub\nbooks/c.epub\n",
        )
        .unwrap();
    let reads = card.reads();
    reads.set_status(C, OPENED, 0).unwrap();
    assert_eq!(
        library::opened_books(&*card.storage),
        ["books/c.epub", "books/a.epub", "books/e.epub"]
    );
    reads.set_status(C, 0, OPENED).unwrap();
    assert_eq!(
        library::opened_books(&*card.storage),
        ["books/a.epub", "books/e.epub"]
    );
    // The file is exactly the lines, with no trailing line break.
    let mut text = std::vec![0_u8; 64];
    let n = card.storage.read_at(file, 0, &mut text).unwrap();
    assert_eq!(&text[..n], b"books/a.epub\nbooks/e.epub");
    // A file that begins with a byte order mark is read without it.
    card.storage
        .write(file, "\u{feff}books/e.epub\n".as_bytes())
        .unwrap();
    reads.set_status(C, OPENED, 0).unwrap();
    assert_eq!(
        library::opened_books(&*card.storage),
        ["books/c.epub", "books/e.epub"]
    );
}
