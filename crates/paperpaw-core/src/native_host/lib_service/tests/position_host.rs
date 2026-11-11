//! `set_position` (op 14) through the service over the production host and its
//! card, as the reader saves its place on a page turn.
use super::*;
use crate::native_host::lib_host::LibraryHost;
use crate::native_host::lib_moves_service::LibraryMoves;
use crate::native_host::lib_reads::tests::card;
use crate::native_host::registrations::{
    ServiceLibPosition as Position, ServiceLibPositionSet as PositionSet, ServiceLibRef as Ref,
};
use paperpaw_native::host_glue::{ERR_NOT_FOUND, OK};
use std::string::String;

fn set(host: &Rc<dyn LibHost>, book: Ref<'_>, section: u32) -> Result<Vec<u8>, i32> {
    call(
        host,
        14,
        &[
            Record::ServiceLibRef(book),
            Record::ServiceLibPosition(Position {
                section,
                offset: 40,
                insert: 0,
                progress_ppm: 250_000,
                conversion: 0,
                updated: 1_760_000_000,
                xpointer: "",
                flags: 0,
            }),
            Record::ServiceLibPositionSet(PositionSet { flags: 0 }),
        ],
    )
}
fn by_path(path: &str) -> Ref<'_> {
    Ref {
        path,
        generation: 0,
        key: None,
    }
}
fn result(reply: &[u8]) -> (String, i32) {
    let Record::ServiceLibResult(row) = wire::record_decode_one(reply).unwrap() else {
        panic!("a lib_result")
    };
    (row.path.into(), row.status)
}
fn stored_section(host: &LibraryHost) -> Option<u32> {
    let mut seen = None;
    host.position(BookRef::Path("books/a.epub"), &mut |record| {
        if let Record::ServiceLibPosition(p) = record {
            seen = Some(p.section);
        }
    })
    .unwrap();
    seen
}

#[test]
fn a_position_set_through_the_service_answers_ok_and_is_kept() {
    let card = card();
    let host: Rc<dyn LibHost> = Rc::new(LibraryHost::new(
        card.storage.clone(),
        card.env.clone(),
        None,
    ));
    let reply = set(&host, by_path("books/a.epub"), 3).unwrap();
    assert_eq!(result(&reply), ("books/a.epub".into(), OK));
    // By the handle the state told: the reply names the catalog's path.
    let Record::ServiceLibState(state) =
        wire::record_decode_one(&call(&host, 1, &[]).unwrap()).unwrap()
    else {
        panic!("a state")
    };
    let by_key = Ref {
        path: "",
        generation: state.generation,
        key: Some(u32::try_from(card.a).unwrap()),
    };
    let reply = set(&host, by_key, 4).unwrap();
    assert_eq!(result(&reply), ("books/a.epub".into(), OK));
    // Kept on the card: a host made afresh (the next start) reads it.
    let again = LibraryHost::new(card.storage.clone(), card.env.clone(), None);
    assert_eq!(stored_section(&again), Some(4));
    // A book the catalog does not know is refused, as for the reads.
    assert_eq!(
        set(&host, by_path("books/none.epub"), 1),
        Err(ERR_NOT_FOUND)
    );
}

#[test]
fn a_position_for_a_book_a_pending_delete_holds_is_not_found() {
    let card = card();
    let moves = Rc::new(LibraryMoves::new(card.storage.clone()));
    let host = LibraryHost::new(card.storage.clone(), card.env.clone(), Some(moves.clone()));
    assert_eq!(host.delete(&["books/e.epub"], false).unwrap()[0], OK);
    assert!(moves.pending_delete("books/e.epub"));
    let host: Rc<dyn LibHost> = Rc::new(host);
    // The file is gone, the catalog's part waits for the pump: hidden, as
    // for the reads.
    assert_eq!(set(&host, by_path("books/e.epub"), 1), Err(ERR_NOT_FOUND));
    // Another book is not the job's.
    let reply = set(&host, by_path("books/a.epub"), 2).unwrap();
    assert_eq!(result(&reply), ("books/a.epub".into(), OK));
}
