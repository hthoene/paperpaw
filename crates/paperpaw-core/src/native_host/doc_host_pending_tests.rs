//! An open whose format plugin is parked (#248 D2, `doc_service::pending`):
//! the format's call answers `ERR_BUSY` (its start is asked for), the opener
//! gets a preparing document, never "busy", and the preparing loop sends the
//! one open once the format is made; a format that cannot be made fails the
//! document. The fixture's format answers `ERR_BUSY` as a parked one does.
use super::*;
use crate::native_host::{
    doc_service::{
        BACKGROUND, DETAILS, Document, FAILED, HOST_JOB, JobKind, JobStep, PREPARING, PrepareStep,
        READY, cover, details::DetailsJob,
    },
    fmt_call::FrameRoute,
};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_CANCELLED, ERR_NO_MEMORY};

const DOCX: u64 = 13;

fn fmt_state(state: u32, progress: u32) -> Vec<u8> {
    let mut bytes = vec![0_u8; 3840];
    let n = local::reply_encode(
        &[local::Record::FmtState(local::FmtState::new(
            0,
            state,
            progress,
            0,
            0,
            5,
            "epub.epub",
        ))],
        &mut bytes,
    )
    .unwrap();
    bytes.truncate(n);
    bytes
}

fn parked(w: &World<'_>) {
    *w.reply.borrow_mut() = Err(ERR_BUSY);
}

fn made(w: &World<'_>, state: u32) {
    *w.reply.borrow_mut() = Ok(fmt_state(state, 0));
}

fn step(w: &World<'_>) -> PrepareStep {
    w.state.prepare_step(&w.hosts, &FrameRoute(w))
}

fn ops(w: &World<'_>) -> Vec<u32> {
    w.calls.borrow().iter().map(|call| call.2).collect()
}

fn document(w: &World<'_>, id: u32) -> Document {
    w.state.table.lease(OPENER, id).unwrap()
}

/// The oldest `event.document_changed` of the opener: (document, state).
fn changed_event(w: &World<'_>) -> Option<(u32, u32)> {
    let mut out = [0_u8; 64];
    let length = w.state.next_event(OPENER, &mut out).unwrap()?;
    match crate::native_host::codec::decode_any_single(&out[..length]).unwrap() {
        Record::EventDocumentChanged(event) => Some((event.document, event.state)),
        other => panic!("{other:?}"),
    }
}

fn close(w: &World<'_>, document: u32) {
    w.call(
        AdmittedFeatures::NONE,
        18,
        &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
    )
    .unwrap();
}

#[test]
fn a_parked_format_opens_preparing_and_its_open_is_sent_once_when_it_is_made() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        parked(w);
        let id = w.open(1, "books/a.epub").expect("preparing, never busy");
        let pending = document(w, id);
        assert_eq!(pending.state, PREPARING);
        assert!(pending.open_pending);
        // The reader's requests meanwhile are the preparing book's answer and
        // send nothing to the format.
        for _ in 0..3 {
            let meta = w.call(
                AdmittedFeatures::NONE,
                5,
                &[Record::ServiceDocRef(wire::ServiceDocRef { document: id })],
            );
            assert_eq!(meta.unwrap_err(), ERR_BUSY);
        }
        assert_eq!(ops(w), [2], "only the open's own call");
        // Not made yet: later, and the open is still pending.
        assert_eq!(step(w), PrepareStep::Later);
        assert!(document(w, id).open_pending);
        // Made: the loop sends the open, the format answers ready.
        made(w, READY);
        assert_eq!(step(w), PrepareStep::More);
        let ready = document(w, id);
        assert_eq!((ready.state, ready.open_pending), (READY, false));
        assert_eq!(changed_event(w), Some((id, READY)));
        assert_eq!(step(w), PrepareStep::Idle, "no second open");
        assert_eq!(ops(w), [2, 2, 2]);
        assert_eq!(w.open_flags.borrow().last(), Some(&0));
    });
}

#[test]
fn an_open_answered_preparing_goes_on_with_prepare_steps() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        parked(w);
        let id = w.open(1, "books/a.epub").unwrap();
        made(w, PREPARING);
        assert_eq!(step(w), PrepareStep::More);
        assert!(!document(w, id).open_pending);
        made(w, READY);
        assert_eq!(step(w), PrepareStep::More);
        assert_eq!(document(w, id).state, READY);
        // The open once, then op 3 `prepare`.
        assert_eq!(ops(w), [2, 2, 3]);
    });
}

#[test]
fn a_format_that_cannot_be_made_fails_the_document_and_the_opener_hears_it() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        parked(w);
        let id = w.open(1, "books/a.epub").unwrap();
        *w.reply.borrow_mut() = Err(ERR_CANCELLED);
        assert_eq!(step(w), PrepareStep::More);
        let failed = document(w, id);
        assert_eq!((failed.state, failed.status), (FAILED, ERR_CANCELLED));
        assert_eq!(changed_event(w), Some((id, FAILED)));
        // Not a loop: the failed document is not stepped again.
        assert_eq!(step(w), PrepareStep::Idle);
        // The format never had the document: its close sends nothing.
        close(w, id);
        assert_eq!(ops(w), [2, 2]);
    });
}

#[test]
fn a_pending_open_its_format_answers_without_memory_fails_at_once() {
    // Manager's decision 2 on #248: the format was made and its open found
    // no memory. Awake, the open fails then; pending, it did not end (the
    // loop tried it again each time).
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        parked(w);
        let id = w.open(1, "books/a.epub").unwrap();
        *w.reply.borrow_mut() = Err(ERR_NO_MEMORY);
        assert_eq!(step(w), PrepareStep::More);
        let failed = document(w, id);
        assert_eq!((failed.state, failed.status), (FAILED, ERR_NO_MEMORY));
        assert_eq!(changed_event(w), Some((id, FAILED)));
        assert_eq!(step(w), PrepareStep::Idle, "not tried again");
    });
}

#[test]
fn a_pending_open_the_host_had_no_room_to_send_gets_no_close() {
    // Senior, R1 on #1368: the host's own no-memory before the format is
    // called (the open or the CSS copied, the scope) fails the open, and the
    // format, which never saw it, gets no `close`. Each allocation of the
    // step fails in turn; those before the call must not mark it sent.
    let before_the_call = core::cell::Cell::new(0);
    for nth in 1..64 {
        world(|w| {
            w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
            parked(w);
            let id = w.open(1, "books/a.epub").unwrap();
            made(w, READY);
            let sent = ops(w).len();
            let fail = &crate::native_host::shell::tests::FAIL_NTH;
            fail.with(|n| n.set(nth));
            let _ = step(w);
            fail.with(|n| n.set(0));
            let failed = document(w, id);
            if ops(w).len() != sent || failed.state != FAILED {
                return; // the format was called, or the step is tried later
            }
            before_the_call.set(before_the_call.get() + 1);
            assert_eq!(failed.status, ERR_NO_MEMORY);
            close(w, id);
            assert_eq!(ops(w).len(), sent, "a close for an open never sent");
        });
    }
    assert!(before_the_call.get() > 0, "no refusal before the call");
}

/// A route whose format plugin gave up being made (Senior, R2 on #1365).
struct GaveUp<'a>(FrameRoute<'a>);
impl crate::native_host::fmt_call::Route for GaveUp<'_> {
    fn call(
        &self,
        target: crate::native_host::format_call::Target,
        payload: &[u8],
        out: &mut [u8],
    ) -> Result<usize, i32> {
        self.0.call(target, payload, out)
    }
    fn gave_up(&self, _plugin: u64) -> bool {
        true
    }
}

#[test]
fn a_pending_open_whose_format_gave_up_fails_without_memory_and_the_opener_hears_it() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        parked(w);
        let id = w.open(1, "books/a.epub").unwrap();
        // Still busy, but not given up: later, as before.
        assert_eq!(step(w), PrepareStep::Later);
        // Given up: the document fails, it does not wait for ever.
        assert_eq!(
            w.state.prepare_step(&w.hosts, &GaveUp(FrameRoute(w))),
            PrepareStep::More
        );
        let failed = document(w, id);
        assert_eq!((failed.state, failed.status), (FAILED, ERR_NO_MEMORY));
        assert_eq!(changed_event(w), Some((id, FAILED)));
        assert_eq!(step(w), PrepareStep::Idle);
        // The format never had it: no close.
        close(w, id);
        assert_eq!(ops(w), [2, 2, 2]);
    });
}

#[test]
fn a_pending_document_closed_before_its_format_is_made_sends_no_close() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        parked(w);
        let id = w.open(1, "books/a.epub").unwrap();
        close(w, id);
        assert_eq!(ops(w), [2], "no op 4 close");
        assert_eq!(step(w), PrepareStep::Idle);
    });
}

#[test]
fn the_cover_job_of_a_parked_format_waits_for_its_open() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.state.allow_cover_jobs();
        parked(w);
        let opener = Caller {
            owner: OPENER,
            plugin: "opener",
            features: AdmittedFeatures::NONE,
            hosts: &w.hosts,
        };
        let queued = cover::make(w, &opener, "books/a.epub", 0);
        assert!(matches!(queued, Ok(cover::Made::Queued)));
        let job = w
            .state
            .table
            .cover_job()
            .expect("the book is the cover job");
        assert!(job.open_pending);
        made(w, READY);
        assert_eq!(step(w), PrepareStep::More);
        let job = w.state.table.cover_job().unwrap();
        assert_eq!((job.state, job.open_pending), (READY, false));
        assert_eq!(ops(w), [2, 2]);
    });
}

#[test]
fn the_details_job_of_a_parked_prepared_format_waits_for_its_open_not_failed() {
    world(|w| {
        // `format.spec.flags` bit 0 `prepared`: a `details` open.
        w.install("docx", DOCX, &[format("docx", "docx", 1, 0)]);
        w.state.allow_cover_jobs();
        parked(w);
        let who = Caller {
            owner: HOST_JOB,
            plugin: "",
            features: AdmittedFeatures::NONE,
            hosts: &w.hosts,
        };
        DetailsJob::start(&*w.state, w, &who, &FrameRoute(w), "books/a.docx").unwrap();
        let job = w.state.table.cover_job().unwrap();
        assert_eq!(job.job_kind, JobKind::Details);
        assert!(job.details && job.open_pending);
        // The job waits for its open; it is not a details open answered
        // preparing (which would end as failed).
        let mut keep = |_: &str, _: &paperpaw_sdk::document::Metadata| true;
        let look = w.state.cover_job_step(&w.hosts, &FrameRoute(w), &mut keep);
        assert!(matches!(look, JobStep::Wait), "{look:?}");
        made(w, READY);
        assert_eq!(step(w), PrepareStep::More);
        let job = w.state.table.cover_job().unwrap();
        assert_eq!((job.state, job.open_pending), (READY, false));
        assert_eq!(w.open_flags.borrow().last(), Some(&(BACKGROUND | DETAILS)));
    });
}

/// The `doc_state` flags of `document` as the opener reads them (op 2).
fn state_flags(w: &World<'_>, document: u32) -> u32 {
    let reply = w
        .call(
            AdmittedFeatures::NONE,
            2,
            &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
        )
        .unwrap();
    match wire::record_decode_one(&reply).unwrap() {
        Record::ServiceDocState(state) => state.flags,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_pending_open_reports_no_book_flags_until_its_format_answers() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        parked(w);
        // An open that asks with flags: they are the request's, not the book's.
        let reply = w
            .call(
                AdmittedFeatures::test(1, FEATURES),
                1,
                &[Record::ServiceDocOpen(wire::ServiceDocOpen {
                    root: 1,
                    path: "books/a.epub",
                    format: "",
                    flags: 1,
                    image_side: 0,
                    cover_side: 0,
                    css_root: 0,
                    css_path: "",
                })],
            )
            .unwrap();
        let Record::ServiceDocState(opened) = wire::record_decode_one(&reply).unwrap() else {
            panic!("a state");
        };
        let id = opened.document;
        assert!(document(w, id).open_pending);
        // Bit 0 would read as "whole pages" (#248): nothing is claimed yet.
        assert_eq!(opened.flags, 0);
        assert_eq!(state_flags(w, id), 0);
        // The format answers: its flags are the book's from now on.
        made(w, READY);
        assert_eq!(step(w), PrepareStep::More);
        assert_eq!(state_flags(w, id), document(w, id).flags);
    });
}
