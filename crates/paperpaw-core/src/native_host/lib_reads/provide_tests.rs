#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! `provide` over a catalog on a card in memory: what is kept, and what is
//! answered where the book or the card is not as it should be.
extern crate std;
use super::super::tests::card;
use super::*;
use crate::native_host::lib_service::LibWrite;
use paperpaw_native::host_glue::{ERR_NOT_FOUND, ERR_STATE};
use paperpaw_platform::Storage;
use paperpaw_sdk::library;

fn record(
    flags: u32,
    texts: [&'static str; 4],
    tags: &'static str,
) -> wire::ServiceLibProvided<'static> {
    wire::ServiceLibProvided {
        title: texts[0],
        author: texts[1],
        series: texts[2],
        number: texts[3],
        tags,
        flags,
    }
}

#[test]
fn what_a_source_provides_is_kept_for_the_book_and_the_rest_is_left_alone() {
    let card = card();
    let reads = card.reads();
    // Book C names its own author "Cy" and title "C" in the catalog; the
    // details know nothing yet.
    reads
        .provide(
            BookRef::Path("books/c.epub"),
            &record(0b00101, ["Provided title", "", "Sagas", ""], ""),
        )
        .unwrap();
    let book = library::book(&*card.storage, "books/c.epub").unwrap();
    assert_eq!(book.title.as_deref(), Some("Provided title"));
    assert_eq!(book.series.as_deref(), Some("Sagas"));
    assert_eq!(book.author, None, "a field that is not provided is not set");
    assert!(!book.indexed, "the file is read anew");
    // A later provider changes what it names; the tags are one list that
    // the last provider replaces.
    reads
        .provide(
            BookRef::Path("books/c.epub"),
            &record(0b11000, ["", "", "", "7"], "one\ntwo"),
        )
        .unwrap();
    reads
        .provide(
            BookRef::Path("books/c.epub"),
            &record(0b10000, ["", "", "", ""], "three"),
        )
        .unwrap();
    let book = library::book(&*card.storage, "books/c.epub").unwrap();
    assert_eq!(book.series_index.as_deref(), Some("7"));
    assert_eq!(book.tags, ["three"], "replaced, not added to");
    assert_eq!(book.title.as_deref(), Some("Provided title"), "kept");
}

#[test]
fn a_book_by_its_handle_is_provided_too_and_a_stale_or_unknown_one_is_refused() {
    let card = card();
    let reads = card.reads();
    let by_handle = BookRef::Key {
        key: u32::try_from(card.a).unwrap(),
        generation: card.generation(),
    };
    reads
        .provide(by_handle, &record(0b1, ["By handle", "", "", ""], ""))
        .unwrap();
    assert_eq!(
        library::book(&*card.storage, "books/a.epub")
            .unwrap()
            .title
            .as_deref(),
        Some("By handle")
    );
    let stale = BookRef::Key {
        key: u32::try_from(card.a).unwrap(),
        generation: card.generation().wrapping_add(1).max(1),
    };
    assert_eq!(
        reads.provide(stale, &record(0b1, ["x", "", "", ""], "")),
        Err(ERR_STATE)
    );
    // Not in the catalog; removed from it by the log; a handle that starts
    // no line.
    for path in ["books/zz.epub", "books/d.txt"] {
        assert_eq!(
            reads.provide(BookRef::Path(path), &record(0b1, ["x", "", "", ""], "")),
            Err(ERR_NOT_FOUND),
            "{path}"
        );
    }
    let no_line = BookRef::Key {
        key: 3,
        generation: card.generation(),
    };
    assert_eq!(
        reads.provide(no_line, &record(0b1, ["x", "", "", ""], "")),
        Err(ERR_NOT_FOUND)
    );
}

#[test]
fn a_book_gone_from_the_card_is_not_found_and_nothing_is_written() {
    let card = card();
    let reads = card.reads();
    // B is in the catalog; its file is not on the card.
    assert!(card.storage.size("books/b.epub").is_err());
    assert_eq!(
        reads.provide(
            BookRef::Path("books/b.epub"),
            &record(0b1, ["x", "", "", ""], "")
        ),
        Err(ERR_NOT_FOUND)
    );
    assert!(library::book(&*card.storage, "books/b.epub").is_none());
}
