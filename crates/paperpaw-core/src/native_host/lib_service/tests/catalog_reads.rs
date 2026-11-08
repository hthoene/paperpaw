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
