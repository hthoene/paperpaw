#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! Op 23 over a catalog on a card in memory: what is kept with the book, what
//! a declared field is, and what is answered where the book or the card is
//! not as it should be.
extern crate std;
use super::super::tests::card;
use super::*;
use crate::native_host::lib_service::LibWrite;
use paperpaw_native::host_glue::ERR_NOT_FOUND;
use paperpaw_sdk::library;

// Book C has its file on the card; B has none.
const B: BookRef<'static> = BookRef::Path("books/c.epub");

#[test]
fn a_field_is_kept_with_the_book_replaced_and_removed_by_an_empty_value() {
    let card = card();
    let reads = card.reads();
    reads.set_field(B, "stars.rating", "4").unwrap();
    let book = library::book(&*card.storage, "books/c.epub").unwrap();
    let field = |book: &library::Book, id: &str| {
        book.fields
            .iter()
            .find(|(key, _)| key == id)
            .map(|(_, value)| value.clone())
    };
    assert_eq!(field(&book, "stars.rating").as_deref(), Some("4"));
    reads.set_field(B, "other.note", "n").unwrap();
    reads.set_field(B, "stars.rating", "5").unwrap();
    let book = library::book(&*card.storage, "books/c.epub").unwrap();
    assert_eq!(field(&book, "stars.rating").as_deref(), Some("5"));
    assert_eq!(
        book.fields
            .iter()
            .filter(|(key, _)| key == "stars.rating")
            .count(),
        1,
        "replaced, not added to"
    );
    reads.set_field(B, "stars.rating", "").unwrap();
    let book = library::book(&*card.storage, "books/c.epub").unwrap();
    assert_eq!(field(&book, "stars.rating"), None);
    assert_eq!(field(&book, "other.note").as_deref(), Some("n"));
}

#[test]
fn a_declared_field_says_where_it_is_kept_and_an_undeclared_one_is_not_found() {
    let card = card();
    let reads = card.reads();
    assert_eq!(
        reads.field_kind("stars.rating"),
        Ok(FieldKind { in_catalog: false })
    );
    assert_eq!(
        reads.field_kind("stars.tag"),
        Ok(FieldKind { in_catalog: true })
    );
    assert_eq!(reads.field_kind("stars.other"), Err(ERR_NOT_FOUND));
    assert_eq!(reads.field_kind(""), Err(ERR_NOT_FOUND));
}

#[test]
fn a_book_that_is_not_in_the_catalog_is_not_found_and_nothing_is_written() {
    let card = card();
    let reads = card.reads();
    assert_eq!(
        reads.set_field(BookRef::Path("books/none.epub"), "stars.rating", "4"),
        Err(ERR_NOT_FOUND)
    );
    // D was removed by the log: not in the catalog either.
    assert_eq!(
        reads.set_field(BookRef::Path("books/d.txt"), "stars.rating", "4"),
        Err(ERR_NOT_FOUND)
    );
    assert!(library::book(&*card.storage, "books/none.epub").is_none());
}

#[test]
fn a_field_of_one_plugin_never_touches_another_plugins() {
    let card = card();
    let reads = card.reads();
    reads.set_field(B, "stars.rating", "4").unwrap();
    reads.set_field(B, "stars.tag", "x").unwrap();
    reads.set_field(B, "stars.tag", "").unwrap();
    let book = library::book(&*card.storage, "books/c.epub").unwrap();
    assert!(
        book.fields
            .iter()
            .any(|(key, value)| key == "stars.rating" && value == "4")
    );
    assert!(book.fields.iter().all(|(key, _)| key != "stars.tag"));
}

#[test]
fn a_book_in_use_is_busy_and_nothing_is_kept() {
    let card = card();
    let reads = card.reads();
    assert_eq!(
        reads.set_field_unless(B, "stars.rating", "4", &|path| path == "books/c.epub"),
        Err(paperpaw_native::host_glue::ERR_BUSY)
    );
    let book = library::book(&*card.storage, "books/c.epub");
    assert!(book.is_none_or(|b| b.fields.iter().all(|(k, _)| k != "stars.rating")));
    // Another book is not held by the same job.
    reads
        .set_field_unless(B, "stars.rating", "4", &|path| path == "books/a.epub")
        .unwrap();
}
