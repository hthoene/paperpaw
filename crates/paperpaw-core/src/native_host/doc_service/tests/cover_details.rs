#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The library's cover while the host's details job holds the one background
//! slot (`doc_details_job`): a preparing book's cover is busy, no event while
//! the details job runs, and the screens are told to ask again when it ends.
use super::*;
use crate::native_host::{
    admitted_features::AdmittedFeatures,
    doc_service::table::JobKind,
    host_services::Caller,
    lib_cover::{CoverCx, path_key},
    lib_host::LibraryHost,
    lib_reads::tests::card,
    lib_service::{BookRef, Caller as Who, LibQuery, LibWrite},
    ui_store::HOST_OWNER,
};

const BOOK: &str = "books/a.epub";

#[test]
fn a_cover_asked_while_the_details_job_runs_is_busy_and_asked_again_when_it_ends() {
    let card = card();
    let library = LibraryHost::new(card.storage.clone(), card.env.clone(), None);
    let fake = Fake::new(Limits::X4);
    with(&fake, |view| {
        // The host's own document of another book is the details job.
        let (details, _) = fake
            .table
            .open(HOST_OWNER, false, FormatRef(0), PREPARING, 0, &|_| false)
            .unwrap();
        assert!(fake.table.start_job(HOST_OWNER, details, JobKind::Details));
        let who = Who {
            owner: OWNER,
            plugin: "home",
            storage: Err(ERR_DENIED),
        };
        let host = Caller {
            owner: OWNER,
            plugin: "home",
            features: AdmittedFeatures::NONE,
            hosts: &view.hosts,
        };
        let cx = CoverCx {
            frame: view,
            caller: &host,
        };
        // The book prepares: no place for its cover job now.
        fake.queue.borrow_mut().push(Ok(state_answer(1, 0)));
        let busy = library.cover(&who, BookRef::Path(BOOK), 0, &cx, &mut |_| {});
        assert_eq!(busy, Err(ERR_BUSY));
        assert!(!library.take_changed(&mut |_, _, _| {}), "nothing yet");
        let job = fake.table.cover_job().unwrap();
        assert_eq!((job.id, job.job_kind), (details, JobKind::Details));
        assert_eq!(fake.table.len(), 1, "the cover's document closed again");
        // The details job ends: the screens ask again (bit 6), once.
        library.details_job_ended(0);
        let mut told = None;
        assert!(library.take_changed(&mut |what, _, path| {
            told = Some((what, std::string::String::from(path)));
        }));
        assert_eq!(told, Some((1 << 6, std::string::String::new())));
        library.details_job_ended(0);
        assert!(!library.take_changed(&mut |_, _, _| {}), "nothing waited");
        // A details job's failed book is the library's failed book: no cover.
        library.details_job_ended(path_key(BOOK));
        assert!(library.book_failed(path_key(BOOK)));
        let calls = fake.calls.borrow().len();
        let none = library.cover(&who, BookRef::Path(BOOK), 0, &cx, &mut |_| {});
        assert_eq!(none, Err(ERR_NOT_FOUND));
        assert_eq!(fake.calls.borrow().len(), calls, "nothing opened");
    });
}
