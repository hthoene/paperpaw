#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The catalog's parts of the delete and import jobs over a card in memory:
//! the phases a delete runs through, a replay, and the importer record.
extern crate std;
use super::*;
use crate::native_host::lib_reads::tests::card;
use paperpaw_sdk::{library, positions};

#[test]
fn a_delete_runs_details_then_marks_then_is_done_and_a_replay_changes_nothing() {
    let card = card();
    library::set_field(&*card.storage, "books/c.epub", "stars.rating", "4");
    positions::save(
        &*card.storage,
        "books/c.epub",
        &positions::Position::new(paperpaw_sdk::document::Locator::new(2, 3), 12.0),
    )
    .unwrap();
    importer::set(&*card.storage, "books/c.epub", "news").unwrap();
    let jobs = CatalogJobs::new(&*card.storage);
    for _ in 0..2 {
        let mut phase = RemovalPhase::Details;
        let mut steps = 0;
        while phase != RemovalPhase::Done {
            phase = jobs.forget("books/c.epub", false, phase).unwrap();
            steps += 1;
        }
        assert_eq!(steps, 2, "details, then marks");
    }
    assert!(library::book(&*card.storage, "books/c.epub").is_none());
    assert!(positions::load(&*card.storage, "books/c.epub").is_none());
    assert_eq!(
        importer::is_by(&*card.storage, "books/c.epub", "news"),
        Ok(false)
    );
    assert_eq!(
        jobs.forget("books/c.epub", false, RemovalPhase::Done),
        Ok(RemovalPhase::Done)
    );
}

#[test]
fn an_import_keeps_who_imported_the_file_and_a_repeat_is_the_same() {
    let card = card();
    let jobs = CatalogJobs::new(&*card.storage);
    jobs.catalogue("books/c.epub", "news").unwrap();
    jobs.catalogue("books/c.epub", "news").unwrap();
    assert_eq!(
        importer::is_by(&*card.storage, "books/c.epub", "news"),
        Ok(true)
    );
    // A file that is not there has nothing to note: the card's error, repeated.
    assert!(jobs.catalogue("books/none.epub", "news").is_err());
}
