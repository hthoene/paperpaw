//! A format that asks before it reads a book (#242, `doc_service::asked`):
//! it is asked (op 1 `claimed`, root 3 the book) before its open; a pass
//! hands the book to the next format of the extension; one not made yet is
//! asked at depth 0 by the pump, the open waiting as a pending one does.
use super::*;
use crate::native_host::{
    doc_service::{FAILED, PREPARING, PrepareStep, READY},
    fmt_call::FrameRoute,
};

/// The format plugin that reads only some EPUBs (`format-epub-plus`).
const PLUS: u64 = 21;
/// `format.spec.flags` bit 5 `asks`.
const ASKS: u32 = 1 << 5;

fn both(w: &World<'_>) {
    w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
    w.install("plus", PLUS, &[format("epub", "epub", ASKS, 10)]);
}

/// (owner, fmt op) of each call made to a format.
fn calls(w: &World<'_>) -> Vec<(u64, u32)> {
    w.calls.borrow().iter().map(|c| (c.0, c.2)).collect()
}

fn format_of(w: &World<'_>, id: u32) -> String {
    let document = w.state.table.lease(OPENER, id).unwrap();
    let mut name = String::new();
    w.state
        .index()
        .unwrap()
        .id(document.format, &mut |id| name.push_str(id))
        .unwrap();
    name
}

#[test]
fn a_format_that_asks_and_says_no_hands_the_book_to_the_next() {
    world(|w| {
        both(w);
        let id = w.open(1, "books/a.epub").unwrap();
        // Asked first, the book its root 3; then the built-in format opens it.
        assert_eq!(calls(w), [(PLUS, 1), (EPUB, 2)]);
        assert_eq!(w.scoped.borrow()[0].0, "books/a.epub");
        assert_eq!(format_of(w, id), "epub.epub");
        assert_eq!(w.state.table.lease(OPENER, id).unwrap().state, READY);
    });
}

#[test]
fn a_format_that_asks_and_says_yes_reads_the_book() {
    world(|w| {
        both(w);
        w.sniffs.borrow_mut().push(PLUS);
        let id = w.open(1, "books/a.epub").unwrap();
        assert_eq!(calls(w), [(PLUS, 1), (PLUS, 2)]);
        assert_eq!(format_of(w, id), "plus.epub");
    });
}

#[test]
fn a_book_every_format_passes_on_fails_as_unsupported() {
    world(|w| {
        w.install("plus", PLUS, &[format("epub", "epub", ASKS, 10)]);
        let id = w.open(1, "books/a.epub").unwrap();
        let document = w.state.table.lease(OPENER, id).unwrap();
        assert_eq!(
            (document.state, document.status),
            (FAILED, paperpaw_native::host_glue::ERR_UNSUPPORTED)
        );
        assert_eq!(calls(w), [(PLUS, 1)]);
    });
}

#[test]
fn a_parked_format_is_asked_by_the_pump_and_its_pass_opens_the_next() {
    world(|w| {
        both(w);
        w.parked.borrow_mut().push(PLUS);
        let id = w.open(1, "books/a.epub").expect("preparing, never busy");
        let waiting = w.state.table.lease(OPENER, id).unwrap();
        assert!(waiting.asking && waiting.open_pending);
        assert_eq!(waiting.state, PREPARING);
        // The preparing loop leaves it alone; the pump asks again later.
        let ask = |w: &World<'_>| w.state.settle_asks(w, &w.hosts, &FrameRoute(w));
        assert_eq!(
            w.state.prepare_step(&w.hosts, &FrameRoute(w)),
            PrepareStep::Idle
        );
        assert_eq!(ask(w), PrepareStep::Later);
        // Made, it says no: the next format reads it, its open sent by the loop.
        w.parked.borrow_mut().clear();
        assert_eq!(ask(w), PrepareStep::More);
        let moved = w.state.table.lease(OPENER, id).unwrap();
        assert!(!moved.asking && moved.open_pending);
        assert_eq!(format_of(w, id), "epub.epub");
        assert_eq!(
            w.state.prepare_step(&w.hosts, &FrameRoute(w)),
            PrepareStep::More
        );
        assert_eq!(w.state.table.lease(OPENER, id).unwrap().state, READY);
        assert_eq!(
            calls(w),
            [(PLUS, 1), (PLUS, 1), (PLUS, 1), (EPUB, 2)],
            "asked until made, then the next format's open once"
        );
        assert_eq!(ask(w), PrepareStep::Idle);
    });
}
