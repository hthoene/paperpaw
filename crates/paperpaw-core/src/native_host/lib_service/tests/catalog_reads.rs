//! The catalog reads (`lib_reads`) through the service: the schema and the
//! service's checks of the host's answers accept what the adapter makes.
use super::*;
use crate::native_host::lib_reads::tests::{Host, card, long_title};
use crate::native_host::registrations::ServiceLibRef as Ref;

fn host(card: &crate::native_host::lib_reads::tests::Card) -> Rc<dyn LibHost> {
    Rc::new(Host(card.reads()))
}
fn path_ref(path: &str) -> Record<'_> {
    Record::ServiceLibRef(Ref {
        path,
        generation: 0,
        key: None,
    })
}

#[test]
fn every_read_of_the_adapter_passes_the_services_checks() {
    let card = card();
    let host = host(&card);
    let Record::ServiceLibState(state) =
        wire::record_decode_one(&call(&host, 1, &[]).unwrap()).unwrap()
    else {
        panic!("a state")
    };
    assert_eq!((state.state, state.books), (2, 4));
    // By path and by the handle and generation the state told.
    let a = u32::try_from(card.a).unwrap();
    let by_key = Record::ServiceLibRef(Ref {
        path: "",
        generation: state.generation,
        key: Some(a),
    });
    for reference in [path_ref("books/a.epub"), by_key] {
        let reply = call(&host, 3, &[reference]).unwrap();
        let Record::ServiceLibBook(book) = wire::record_decode_one(&reply).unwrap() else {
            panic!("a book")
        };
        assert_eq!((book.key, book.title, book.flags), (a, "Title A", 0b11));
    }
    // The long book: the cut title is flagged, so the checks accept it.
    let reply = call(&host, 3, &[path_ref("books/b.epub")]).unwrap();
    let Record::ServiceLibBook(book) = wire::record_decode_one(&reply).unwrap() else {
        panic!("a book")
    };
    assert_eq!((book.title_bytes, book.flags & 0b1110_0000), (2100, 1 << 5));
    // Its title in pieces, each as the text rules want it.
    let mut got = std::string::String::new();
    let mut offset = 0_u32;
    while (offset as usize) < long_title().len() {
        let ask = Record::ServiceLibTextAt(wire::ServiceLibTextAt {
            kind: 0,
            index: 0,
            offset,
        });
        let reply = call(&host, 4, &[path_ref("books/b.epub"), ask]).unwrap();
        let Record::ServiceLibText(piece) = wire::record_decode_one(&reply).unwrap() else {
            panic!("a text")
        };
        assert!(!piece.text.is_empty());
        got.push_str(piece.text);
        offset += u32::try_from(piece.text.len()).unwrap();
    }
    assert_eq!(got, long_title());
    let reply = call(&host, 25, &[path_ref("books/a.epub")]).unwrap();
    let Record::ServiceLibKey(key) = wire::record_decode_one(&reply).unwrap() else {
        panic!("a key")
    };
    assert_eq!(
        (key.key.len(), key.fingerprint.len(), key.size),
        (16, 32, 3000)
    );
    // A position, checked by the service as the host's answer.
    assert_eq!(
        call(&host, 13, &[path_ref("books/a.epub")]),
        Err(paperpaw_native::host_glue::ERR_NOT_FOUND)
    );
    let kept =
        paperpaw_sdk::positions::Position::new(paperpaw_sdk::document::Locator::new(2, 9), 12.5)
            .conversion(3)
            .xpointer(Some("/body/p[1]".into()));
    paperpaw_sdk::positions::save(&*card.storage, "books/a.epub", &kept).unwrap();
    let reply = call(&host, 13, &[path_ref("books/a.epub")]).unwrap();
    let Record::ServiceLibPosition(place) = wire::record_decode_one(&reply).unwrap() else {
        panic!("a position")
    };
    assert_eq!(
        (place.section, place.offset, place.progress_ppm, place.flags),
        (2, 9, 125_000, 0b11)
    );
    // A book that is not there is the host's own status.
    assert_eq!(
        call(&host, 3, &[path_ref("books/zz.epub")]),
        Err(paperpaw_native::host_glue::ERR_NOT_FOUND)
    );
}

#[test]
fn annotations_anchors_and_their_texts_pass_the_services_checks() {
    use crate::native_host::lib_reads::tests::{annotation_file, put_annotations};
    let card = card();
    let host = host(&card);
    put_annotations(&card, &annotation_file(&"€".repeat(700)));
    let page = |cursor, generation| {
        Record::ServiceLibPage(wire::ServiceLibPage {
            cursor,
            annotation_generation: generation,
        })
    };
    // The rows then the end, as the service built them.
    let rows = |op, cursor, generation| {
        let reply = call(
            &host,
            op,
            &[path_ref("books/a.epub"), page(cursor, generation)],
        )
        .unwrap();
        let (mut at, mut ids, mut end) = (0, std::vec::Vec::new(), None);
        while at < reply.len() {
            let (record, used) = wire::record_decode_prefix(&reply[at..]).unwrap();
            at += used;
            match record {
                Record::ServiceLibAnnotation(a) => ids.push(a.id),
                Record::ServiceLibAnchor(a) => ids.push(a.id),
                Record::ServiceEnd(e) => end = Some((e.total, e.generation, e.next, e.flags)),
                _ => panic!("a row"),
            }
        }
        (ids, end.unwrap())
    };
    // Four annotations fit a page; from the cursor 2 on, the last two.
    let (ids, end) = rows(15, 0, 0);
    assert_eq!((ids.as_slice(), end), (&[1, 2, 3, 4][..], (4, 1, 4, 1)));
    let (ids, end) = rows(15, 2, 1);
    assert_eq!((ids.as_slice(), end), (&[3, 4][..], (4, 1, 4, 1)));
    let (ids, end) = rows(16, 0, 0);
    assert_eq!((ids.as_slice(), end), (&[1, 3][..], (2, 1, 2, 1)));
    // The note of the second, whole by id.
    let ask = Record::ServiceLibTextAt(wire::ServiceLibTextAt {
        kind: 6,
        index: 2,
        offset: 1023,
    });
    let reply = call(&host, 4, &[path_ref("books/a.epub"), ask]).unwrap();
    let Record::ServiceLibText(part) = wire::record_decode_one(&reply).unwrap() else {
        panic!("a text")
    };
    assert_eq!((part.total, part.text.len()), (2100, 1023));
}

#[test]
fn a_query_of_the_adapter_passes_the_services_checks_and_pages() {
    let card = card();
    let host = host(&card);
    let ask = |cursor, generation| {
        Record::ServiceLibQuery(wire::ServiceLibQuery {
            show: 0,
            sort: 0,
            flags: 0,
            words: "",
            author: "",
            series: "",
            tag: "",
            folder: "",
            added_since: 0,
            cursor,
            generation,
            facet: 0,
        })
    };
    let rows = |cursor, generation| {
        let reply = call(&host, 2, &[ask(cursor, generation)]).unwrap();
        let (mut at, mut titles, mut end) = (0, std::vec::Vec::new(), None);
        while at < reply.len() {
            let (record, used) = wire::record_decode_prefix(&reply[at..]).unwrap();
            at += used;
            match record {
                Record::ServiceLibEntry(e) => titles.push(std::string::String::from(e.title)),
                Record::ServiceEnd(e) => end = Some((e.total, e.next, e.flags)),
                _ => panic!("a row"),
            }
        }
        (titles, end.unwrap())
    };
    // The four books of the card by title (D is removed): all fit a page.
    let (titles, end) = rows(0, 0);
    assert_eq!(titles.len(), 4);
    // By title, regardless of case: C, E, Title A, then the long one.
    assert_eq!(titles[..3], ["C", "E", "Title A"]);
    assert_eq!(end, (4, 4, 1));
    let Record::ServiceLibState(state) =
        wire::record_decode_one(&call(&host, 1, &[]).unwrap()).unwrap()
    else {
        panic!("a state")
    };
    // From the cursor 3 on, under the generation the state told.
    let (tail, end) = rows(3, state.generation);
    assert_eq!((tail.as_slice(), end), (&titles[3..], (4, 4, 1)));
    // A stale generation is the host's `ERR_STATE`.
    assert_eq!(
        call(&host, 2, &[ask(1, state.generation ^ 1)]),
        Err(paperpaw_native::host_glue::ERR_STATE)
    );
}

/// Op 5 over the adapter: every list passes the service's checks of a page
/// (whole rows, then the end that counts them).
#[test]
fn the_facets_of_the_adapter_pass_the_services_checks() {
    let card = card();
    let host = host(&card);
    let list = |facet: u32| {
        let ask = Record::ServiceLibQuery(wire::ServiceLibQuery {
            show: 0,
            sort: 0,
            flags: 0,
            words: "",
            author: "",
            series: "",
            tag: "",
            folder: "",
            added_since: 0,
            cursor: 0,
            generation: 0,
            facet,
        });
        let reply = call(&host, 5, &[ask]).unwrap();
        let (mut rows, mut end) = (std::vec::Vec::new(), None);
        let mut at = 0;
        while at < reply.len() {
            let (record, used) = wire::record_decode_prefix(&reply[at..]).unwrap();
            at += used;
            match record {
                Record::ServiceLibFacet(row) => {
                    rows.push((std::string::String::from(row.text), row.count, row.first));
                }
                Record::ServiceEnd(last) => end = Some((last.flags, last.total, last.next)),
                other => panic!("{other:?}"),
            }
        }
        (rows, end)
    };
    let names = |facet: u32| -> std::vec::Vec<std::string::String> {
        list(facet).0.into_iter().map(|row| row.0).collect()
    };
    let (authors, end) = list(0);
    let jumps: std::vec::Vec<u32> = authors.iter().map(|row| row.2).collect();
    assert_eq!(names(0), ["Ada", "Bea", "Cy", "Eve"]);
    assert_eq!(jumps, [0, 1, 2, 3]);
    // The last page: flag bit 0, and the end counts the four names.
    assert_eq!(end, Some((1, 4, 4)));
    assert_eq!(names(1), ["Series"]);
    assert_eq!(names(2), ["x", "y", "z"]);
    assert_eq!(names(3), ["#", "C", "E", "T"]);
    assert_eq!(
        list(4).0,
        [(std::string::String::from("books"), 4, u32::MAX)]
    );
}

/// One call through a frame that lends `declarations` (none: it lends none).
fn declared_call(
    host: &Rc<dyn LibHost>,
    declarations: Option<&crate::native_host::declaration_store::DeclarationStore>,
    id: u32,
    records: &[Record<'_>],
) -> std::vec::Vec<u8> {
    let op = schema_op(id);
    let bytes = input(id, records).unwrap();
    let mut out = std::vec![0; op.max_reply];
    let length = with_declared(host.clone(), declarations, |frame| {
        frame.run(op, &bytes, &mut out)
    })
    .unwrap();
    out.truncate(length);
    validate_reply(&out, op).unwrap();
    out
}

/// The status of the first `lib_result` of an op 9 or 10 reply.
fn first_status(reply: &[u8]) -> i32 {
    let (Record::ServiceLibResult(row), _) = wire::record_decode_prefix(reply).unwrap() else {
        panic!("a lib_result")
    };
    row.status
}

/// #487: what is a book and its format are the kernel's, from the formats the
/// plugins declared (the shells know none): the delete preflight takes a file
/// a declared format reads for a book, the book names that format, and a
/// delete removes it as a book. Without the declaration it is a plain file.
#[test]
fn the_declared_formats_tell_a_book_its_format_and_its_delete() {
    use crate::native_host::native_book_formats::tests::{Plugin, declare};
    let card = card();
    let moves = Rc::new(crate::native_host::lib_moves_service::LibraryMoves::new(
        card.storage.clone(),
    ));
    let host: Rc<dyn LibHost> = Rc::new(crate::native_host::lib_host::LibraryHost::new(
        card.storage.clone(),
        card.env.clone(),
        Some(moves.clone()),
    ));
    let mut store = crate::native_host::declaration_store::DeclarationStore::default();
    let plugin = Rc::new(Plugin {
        id: "format-books",
        live: core::cell::Cell::new(true),
    });
    declare(&mut store, &plugin, "books", "epub");
    let c = Record::ServiceLibPaths(wire::ServiceLibPaths {
        paths: "books/c.epub",
        flags: 0,
    });
    let preflight = |declarations| first_status(&declared_call(&host, declarations, 9, &[c]));
    assert_eq!(preflight(None), paperpaw_native::host_glue::ERR_INVALID);
    assert_eq!(preflight(Some(&store)), paperpaw_native::host_glue::OK);
    // A file no declared format reads stays a plain file.
    let d = Record::ServiceLibPaths(wire::ServiceLibPaths {
        paths: "books/d.txt",
        flags: 0,
    });
    assert_eq!(
        first_status(&declared_call(&host, Some(&store), 9, &[d])),
        paperpaw_native::host_glue::ERR_INVALID
    );
    let format = |declarations| {
        let reply = declared_call(&host, declarations, 3, &[path_ref("books/c.epub")]);
        let Record::ServiceLibBook(book) = wire::record_decode_one(&reply).unwrap() else {
            panic!("a book")
        };
        std::string::String::from(book.format)
    };
    assert_eq!(format(None), "");
    assert_eq!(format(Some(&store)), "format-books.books");
    // The delete is the library's: the file goes and the catalog's part is
    // the move job's.
    let reply = declared_call(&host, Some(&store), 10, &[c]);
    assert_eq!(first_status(&reply), paperpaw_native::host_glue::OK);
    assert!(paperpaw_platform::Storage::size(&*card.storage, "books/c.epub").is_err());
    assert!(moves.pending_delete("books/c.epub"));
}
