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
