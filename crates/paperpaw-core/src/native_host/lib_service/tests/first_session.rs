//! A library's first session (#257): the book the reader opened is on the card
//! but not in the catalog yet (no view of the library asked for the walk).
//! Its annotations and its place are kept by its path all the same, as in
//! Beta 3; a path the card does not hold is still `ERR_NOT_FOUND`.
use super::*;
use crate::native_host::lib_host::LibraryHost;
use crate::native_host::lib_reads::tests::{Env, card};
use crate::native_host::registrations::{
    ServiceLibEdit as Edit, ServiceLibEditHead as Head, ServiceLibPosition as Position,
    ServiceLibPositionSet as PositionSet, ServiceLibRef as Ref, ServiceLibTransactionRef as TRef,
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NOT_FOUND};
use paperpaw_platform::Storage as _;
use paperpaw_sdk::MemoryStorage;

const BOOK: &str = "books/new.epub";

fn by_path(path: &str) -> Record<'_> {
    Record::ServiceLibRef(Ref {
        path,
        generation: 0,
        key: None,
    })
}

/// A bookmark: begin (op 17), one edit (18), commit (20); the edits added.
fn mark(host: &Rc<dyn LibHost>, path: &str) -> Result<u32, i32> {
    let head = Record::ServiceLibEditHead(Head {
        annotation_generation: 1,
    });
    let reply = call(host, 17, &[by_path(path), head])?;
    let Record::ServiceLibTransaction(t) = wire::record_decode_one(&reply).unwrap() else {
        panic!("a transaction")
    };
    let edit = Edit {
        action: 0,
        id: 0,
        kind: 0,
        start_section: 1,
        start_offset: 5,
        end_section: 1,
        end_offset: 5,
        flags: 0,
        percent: 10,
        conversion: 0,
        created: 1,
        updated: 1,
        client_key: "",
        text: "x",
        note: "",
        text_bytes: 0,
        note_bytes: 0,
    };
    let at = || {
        Record::ServiceLibTransactionRef(TRef {
            transaction: t.transaction,
        })
    };
    call(host, 18, &[at(), Record::ServiceLibEdit(edit)])?;
    let reply = call(host, 20, &[at()])?;
    let Record::ServiceLibCommitted(done) = wire::record_decode_one(&reply).unwrap() else {
        panic!("committed")
    };
    Ok(done.added)
}

/// The place set (op 14), then read back (op 13): its section.
fn place(host: &Rc<dyn LibHost>, path: &str) -> Result<u32, i32> {
    let at = Record::ServiceLibPosition(Position {
        section: 3,
        offset: 40,
        insert: 0,
        progress_ppm: 250_000,
        conversion: 0,
        updated: 1_760_000_000,
        xpointer: "",
        flags: 0,
    });
    let set = Record::ServiceLibPositionSet(PositionSet { flags: 0 });
    call(host, 14, &[by_path(path), at, set])?;
    let reply = call(host, 13, &[by_path(path)])?;
    let Record::ServiceLibPosition(read) = wire::record_decode_one(&reply).unwrap() else {
        panic!("a position")
    };
    Ok(read.section)
}

fn host_on(storage: Rc<MemoryStorage>) -> Rc<dyn LibHost> {
    let env = Rc::new(Env {
        walking: core::cell::Cell::new(None),
        today: core::cell::Cell::new(None),
    });
    Rc::new(LibraryHost::new(storage, env, None))
}

#[test]
fn a_book_of_a_librarys_first_session_keeps_its_marks_and_place() {
    // No catalog at all: the walk never ran on this card.
    let fresh = Rc::new(MemoryStorage::new().with(BOOK, std::vec![7_u8; 100]));
    // A catalog that has not caught up with a book copied on since.
    let behind = card().storage;
    behind.write(BOOK, &[7_u8; 100]).unwrap();
    for storage in [fresh, behind] {
        let host = host_on(storage);
        assert_eq!(mark(&host, BOOK), Ok(1));
        assert_eq!(place(&host, BOOK), Ok(3));
        // A book the card does not hold is none.
        assert_eq!(mark(&host, "books/gone.epub"), Err(ERR_NOT_FOUND));
        assert_eq!(place(&host, "books/gone.epub"), Err(ERR_NOT_FOUND));
    }
}

#[test]
fn an_uncatalogued_path_keeps_the_path_rules_and_its_data_once_catalogued() {
    let storage = Rc::new(
        MemoryStorage::new()
            .with(BOOK, std::vec![7_u8; 100])
            .with(".paperpaw/x.epub", std::vec![7_u8; 10])
            .with("plugins/p/a.epub", std::vec![7_u8; 10]),
    );
    let host = host_on(storage.clone());
    // The path rules hold for a file the catalog does not know too.
    for path in [".paperpaw/x.epub", "plugins/p/a.epub", "books/a\nb.epub"] {
        assert_eq!(mark(&host, path), Err(ERR_INVALID), "{path}");
        assert_eq!(place(&host, path), Err(ERR_INVALID), "{path}");
    }
    assert_eq!(place(&host, BOOK), Ok(3));
    assert_eq!(mark(&host, BOOK), Ok(1));
    // The walk catalogues it later: the place kept by its path is its own.
    storage
        .write(
            ".paperpaw/library/catalog.txt",
            b"\tcatalog\t2\t1\nbooks/new.epub\tNew\tN\t\t\t\t\t\t\t\t1\n",
        )
        .unwrap();
    let reply = call(&host, 13, &[by_path(BOOK)]).unwrap();
    let Record::ServiceLibPosition(read) = wire::record_decode_one(&reply).unwrap() else {
        panic!("a position")
    };
    assert_eq!(read.section, 3);
    // And it is the catalog's book now (op 3 answers from the catalog).
    assert!(call(&host, 3, &[by_path(BOOK)]).is_ok());
}

#[test]
fn an_uncatalogued_books_annotation_texts_and_anchors_are_read_by_its_path() {
    let storage = Rc::new(
        MemoryStorage::new()
            .with(BOOK, std::vec![7_u8; 100])
            .with("books/folder/x.epub", std::vec![7_u8; 10]),
    );
    let host = host_on(storage);
    assert_eq!(mark(&host, BOOK), Ok(1));
    let page = || {
        Record::ServiceLibPage(wire::ServiceLibPage {
            cursor: 0,
            annotation_generation: 0,
        })
    };
    // Ops 15 and 16 answer too, with the bookmark among the annotations.
    let rows = call(&host, 15, &[by_path(BOOK), page()]).unwrap();
    let (mut at, mut id) = (0, None);
    while at < rows.len() {
        let (record, used) = wire::record_decode_prefix(&rows[at..]).unwrap();
        at += used;
        if let Record::ServiceLibAnnotation(row) = record {
            id = Some(row.id);
        }
    }
    let id = id.expect("the bookmark is listed");
    let text = || {
        Record::ServiceLibTextAt(wire::ServiceLibTextAt {
            kind: 5,
            index: id,
            offset: 0,
        })
    };
    // Op 4: the annotation's own text (by its id), from its file.
    let reply = call(&host, 4, &[by_path(BOOK), text()]).unwrap();
    let Record::ServiceLibText(read) = wire::record_decode_one(&reply).unwrap() else {
        panic!("a text")
    };
    assert_eq!(read.text, "x");
    assert!(call(&host, 16, &[by_path(BOOK), page()]).is_ok());
    // A folder is no book: not found, as a missing file.
    for op in [4, 15, 16] {
        let input = if op == 4 { text() } else { page() };
        assert_eq!(
            call(&host, op, &[by_path("books/folder"), input]),
            Err(ERR_NOT_FOUND),
            "op {op}"
        );
    }
    assert_eq!(place(&host, "books/folder"), Err(ERR_NOT_FOUND));
    assert_eq!(mark(&host, "books/folder"), Err(ERR_NOT_FOUND));
}
