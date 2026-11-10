#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The production library host: the catalog reads, no writes, the host's
//! folders excluded.
extern crate std;
use super::*;
use crate::native_host::lib_reads::tests::card;
use crate::native_host::registrations::Record;
use paperpaw_native::host_glue::ERR_UNSUPPORTED;

fn host() -> LibraryHost {
    let card = card();
    LibraryHost::new(card.storage.clone(), card.env.clone(), None)
}

#[test]
fn it_reads_the_catalog() {
    let mut books = None;
    host()
        .state(&mut |record| {
            if let Record::ServiceLibState(state) = record {
                books = Some(state.books);
            }
        })
        .unwrap();
    assert_eq!(books, Some(4));
}

#[test]
fn it_has_no_writes_and_says_so() {
    let host = host();
    assert_eq!(host.delete(&["books/a.epub"], false), Err(ERR_UNSUPPORTED));
    assert_eq!(
        host.move_book("books/a.epub", "books/z.epub", false),
        Err(ERR_UNSUPPORTED)
    );
}

#[test]
fn the_hosts_folders_are_excluded() {
    let host = host();
    for name in ["plugins", "PLUGINS", ".paperpaw", "paperpaw.ini"] {
        assert!(host.excluded(name), "{name}");
    }
    assert!(!host.excluded("books"));
    // A `~` that names nothing the host holds is not an alias of it.
    assert!(!host.excluded("BOOKS~1"));
}

#[test]
fn provide_reaches_the_catalog() {
    let card = card();
    let host = LibraryHost::new(card.storage.clone(), card.env.clone(), None);
    host.provide(
        BookRef::Path("books/c.epub"),
        &wire::ServiceLibProvided {
            title: "Provided",
            author: "",
            series: "",
            number: "",
            tags: "",
            flags: 0b1,
        },
    )
    .unwrap();
    let book = paperpaw_sdk::library::book(&*card.storage, "books/c.epub").unwrap();
    assert_eq!(book.title.as_deref(), Some("Provided"));
}

#[test]
fn provide_is_busy_for_a_book_the_library_move_carries() {
    let card = card();
    let moves = Rc::new(LibraryMoves::new(card.storage.clone()));
    moves.begin("books/c.epub", "moved/c.epub", false).unwrap();
    let host = LibraryHost::new(card.storage.clone(), card.env.clone(), Some(moves));
    let provided = wire::ServiceLibProvided {
        title: "Provided",
        author: "",
        series: "",
        number: "",
        tags: "",
        flags: 0b1,
    };
    // The old name is closed while the job runs; nothing was kept.
    assert_eq!(
        host.provide(BookRef::Path("books/c.epub"), &provided),
        Err(paperpaw_native::host_glue::ERR_BUSY)
    );
    let book = paperpaw_sdk::library::book(&*card.storage, "books/c.epub");
    assert!(book.is_none_or(|book| book.title.as_deref() != Some("Provided")));
    // Another book is not the job's.
    host.provide(BookRef::Path("books/a.epub"), &provided)
        .unwrap();
}

#[test]
fn a_position_is_written_and_read_through_the_host() {
    let card = card();
    let host = LibraryHost::new(card.storage.clone(), card.env.clone(), None);
    let book = BookRef::Path("books/a.epub");
    assert_eq!(host.stored_position(book), Ok(None));
    host.write_position(
        book,
        &wire::ServiceLibPosition {
            section: 3,
            offset: 40,
            insert: 0,
            progress_ppm: 250_000,
            conversion: 0,
            updated: 1_760_000_000,
            xpointer: "",
            flags: 0,
        },
        false,
    )
    .unwrap();
    let mut seen = None;
    host.position(book, &mut |record| {
        if let Record::ServiceLibPosition(p) = record {
            seen = Some((p.section, p.offset, p.progress_ppm));
        }
    })
    .unwrap();
    assert_eq!(seen, Some((3, 40, 250_000)));
    assert_eq!(
        host.stored_position(book).unwrap().unwrap().updated,
        1_760_000_000
    );
}

#[test]
fn a_field_is_set_through_the_host_and_is_busy_for_a_book_the_move_carries() {
    let card = card();
    let moves = Rc::new(LibraryMoves::new(card.storage.clone()));
    moves.begin("books/c.epub", "moved/c.epub", false).unwrap();
    let host = LibraryHost::new(card.storage.clone(), card.env.clone(), Some(moves));
    assert!(!host.field_kind("stars.rating").unwrap().in_catalog);
    assert_eq!(
        host.set_field(BookRef::Path("books/c.epub"), "stars.rating", "4"),
        Err(paperpaw_native::host_glue::ERR_BUSY)
    );
    host.set_field(BookRef::Path("books/a.epub"), "stars.rating", "4")
        .unwrap();
    let book = paperpaw_sdk::library::book(&*card.storage, "books/a.epub").unwrap();
    assert!(
        book.fields
            .iter()
            .any(|(k, v)| k == "stars.rating" && v == "4")
    );
}

#[test]
fn an_unconfirmed_sync_position_is_busy_for_a_book_the_move_carries_and_a_local_one_is_not() {
    let card = card();
    let moves = Rc::new(LibraryMoves::new(card.storage.clone()));
    moves.begin("books/c.epub", "moved/c.epub", false).unwrap();
    let host = LibraryHost::new(card.storage.clone(), card.env.clone(), Some(moves));
    let at = wire::ServiceLibPosition {
        section: 3,
        offset: 40,
        insert: 0,
        progress_ppm: 250_000,
        conversion: 0,
        updated: 1_760_000_000,
        xpointer: "",
        flags: 0,
    };
    let book = BookRef::Path("books/c.epub");
    assert_eq!(
        host.write_position(book, &at, true),
        Err(paperpaw_native::host_glue::ERR_BUSY)
    );
    assert_eq!(host.stored_position(book), Ok(None), "nothing was written");
    // A local write is the user's and is kept; another book's sync write is
    // not the job's.
    host.write_position(book, &at, false).unwrap();
    host.write_position(BookRef::Path("books/a.epub"), &at, true)
        .unwrap();
}

#[test]
fn the_delete_preflight_is_the_cards() {
    let host = host();
    assert_eq!(
        host.can_delete("books/c.epub"),
        (paperpaw_native::host_glue::OK, 0)
    );
    assert_eq!(
        host.can_delete("books/none.epub").0,
        paperpaw_native::host_glue::ERR_NOT_FOUND
    );
    assert_eq!(
        host.can_delete("books/d.txt").0,
        paperpaw_native::host_glue::ERR_INVALID
    );
}

#[test]
fn a_status_is_set_through_the_host_and_is_busy_for_a_book_the_move_carries() {
    let card = card();
    let moves = Rc::new(LibraryMoves::new(card.storage.clone()));
    moves.begin("books/c.epub", "moved/c.epub", false).unwrap();
    let host = LibraryHost::new(card.storage.clone(), card.env.clone(), Some(moves));
    assert_eq!(
        host.set_status(BookRef::Path("books/c.epub"), 0b010, 0),
        Err(paperpaw_native::host_glue::ERR_BUSY)
    );
    assert!(paperpaw_sdk::library::favourites(&*card.storage).is_empty());
    host.set_status(BookRef::Path("books/a.epub"), 0b010, 0)
        .unwrap();
    assert_eq!(
        paperpaw_sdk::library::favourites(&*card.storage),
        ["books/a.epub"]
    );
}
