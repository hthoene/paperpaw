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
    LibraryHost::new(card.storage.clone(), card.env.clone())
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
    assert_eq!(
        host.set_status(BookRef::Path("books/a.epub"), 1, 0),
        Err(ERR_UNSUPPORTED)
    );
    assert_eq!(host.delete("books/a.epub").0, ERR_UNSUPPORTED);
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
    let host = LibraryHost::new(card.storage.clone(), card.env.clone());
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
