//! Op 22 `report` of the production host (`document.md` 2.4): the checks of a
//! report, which documents and which reader drive the session, and the events
//! the session raises.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
use super::*;
use crate::native_host::doc_reading::tests::{Seen, drain};
use crate::native_host::stages_wire::core_format_v1 as local;
use alloc::vec::Vec;
use paperpaw_native::host_glue::{ERR_DENIED, ERR_NO_MEMORY};

const OTHER: u64 = 8;
const OPENED: u32 = wire::EVENT_BOOK_OPENED_TAG;
const CLOSED: u32 = wire::EVENT_BOOK_CLOSED_TAG;
const FINISHED: u32 = wire::EVENT_BOOK_FINISHED_TAG;
const TURNED: u32 = wire::EVENT_PAGE_TURNED_TAG;

fn meta(sections: u32) -> Vec<u8> {
    let mut bytes = vec![0_u8; 3840];
    let n = local::reply_encode(
        &[local::Record::FmtMeta(local::FmtMeta::new(
            "Title", "en", "", "", 0, 0, sections, 0, 0, 0, 0,
        ))],
        &mut bytes,
    )
    .unwrap();
    bytes.truncate(n);
    bytes
}

fn report<'a>(document: u32, section: u32) -> wire::ServiceDocReport<'a> {
    wire::ServiceDocReport {
        document,
        section,
        offset: 0,
        insert: 0,
        progress_ppm: section.min(1000) * 1000,
        flags: 0,
        title: "Title",
        chapter: "One",
        page: None,
        pages: None,
        chapter_left: None,
    }
}

impl World<'_> {
    fn report(&self, report: wire::ServiceDocReport<'_>) -> Result<(), i32> {
        self.call(
            AdmittedFeatures::NONE,
            22,
            &[Record::ServiceDocReport(report)],
        )
        .map(|reply| assert!(reply.is_empty()))
    }

    /// The format plugin, and the caller (`opener`) as a reading app.
    fn readers(&self) {
        let has = |id: &str| self.plugins.borrow().iter().any(|p| p.id == id);
        if !has("epub") {
            self.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        }
        if !has("opener") {
            self.install(
                "opener",
                OPENER,
                &[
                    Record::ScreenSpec(wire::ScreenSpec {
                        name: "main",
                        title: "t",
                        token: 1,
                        flags: 0,
                    }),
                    Record::ReaderSpec(wire::ReaderSpec {
                        name: "app",
                        screen: "opener.main",
                        formats: "",
                        flags: 0,
                    }),
                ],
            );
        }
    }

    /// A book of the card, opened and ready, its format counting `sections`.
    fn book(&self, path: &str, sections: u32) -> u32 {
        self.readers();
        *self.reply.borrow_mut() = Ok(state_reply());
        let document = self.open(1, path).unwrap();
        *self.reply.borrow_mut() = Ok(meta(sections));
        document
    }

    fn seen(&self) -> Vec<(u32, u32)> {
        let seen: Vec<Seen> = drain(&self.state.reading);
        seen.iter().map(|s| (s.tag, s.reason)).collect()
    }
}

#[test]
fn a_reading_session_goes_from_the_first_report_to_the_close() {
    world(|w| {
        let document = w.book("books/a.epub", 5);
        w.state.now.set(1_000);
        w.report(report(document, 0)).unwrap();
        // The format was asked once, for the section count.
        assert_eq!(w.calls.borrow().last(), Some(&(EPUB, 5, 5)));
        let seen = drain(&w.state.reading);
        assert_eq!(seen.len(), 1);
        assert_eq!(
            (seen[0].tag, seen[0].now_ms, seen[0].path.as_str()),
            (OPENED, 1_000, "books/a.epub")
        );
        // A page turn costs no plugin call; an identical report raises nothing.
        let calls = w.calls.borrow().len();
        w.state.now.set(2_000);
        w.report(report(document, 0)).unwrap();
        w.report(report(document, 1)).unwrap();
        assert_eq!(w.calls.borrow().len(), calls);
        let seen = drain(&w.state.reading);
        assert_eq!(seen.len(), 1);
        assert_eq!((seen[0].tag, seen[0].now_ms), (TURNED, 2_000));
        // The end: the turn, then the finish, once.
        let mut last = report(document, 4);
        last.flags = 1;
        w.report(last).unwrap();
        w.report(last).unwrap();
        assert_eq!(w.seen(), [(TURNED, 0), (FINISHED, 0)]);
        // Closing the document closes the book, at the frame's clock.
        w.state.now.set(3_000);
        w.call(
            AdmittedFeatures::NONE,
            18,
            &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
        )
        .unwrap();
        let seen = drain(&w.state.reading);
        assert_eq!(
            (seen[0].tag, seen[0].reason, seen[0].now_ms),
            (CLOSED, 0, 3_000)
        );
        assert_eq!(seen[0].section, 4);
    });
}

#[test]
fn a_bad_report_is_invalid_and_raises_nothing() {
    world(|w| {
        let document = w.book("books/a.epub", 3);
        let ok = report(document, 1);
        let counted = |page, pages, left| {
            let mut r = ok;
            r.page = page;
            r.pages = pages;
            r.chapter_left = left;
            r
        };
        let mut bad = std::vec::Vec::new();
        let mut r = ok;
        r.progress_ppm = 1_000_001;
        bad.push(r);
        r = ok;
        r.flags = 2;
        bad.push(r);
        // Counters: both or neither; from 1; not past the end.
        bad.push(counted(Some(1), None, None));
        bad.push(counted(None, Some(9), None));
        bad.push(counted(Some(0), Some(9), None));
        bad.push(counted(Some(10), Some(9), None));
        // The countdown: with the counters, with a chapter, within the rest.
        bad.push(counted(None, None, Some(1)));
        bad.push(counted(Some(2), Some(9), Some(8)));
        r = counted(Some(2), Some(9), Some(1));
        r.chapter = "";
        bad.push(r);
        // The section is below the document's count.
        bad.push(report(document, 3));
        bad.push(report(document, u32::MAX));
        for r in bad {
            assert_eq!(w.report(r), Err(ERR_INVALID), "{r:?}");
        }
        assert!(drain(&w.state.reading).is_empty());
        // The edges of the same rules are fine; no session was started.
        w.report(counted(Some(2), Some(9), Some(7))).unwrap();
        w.report(counted(Some(9), Some(9), Some(0))).unwrap();
        assert_eq!(w.seen(), [(OPENED, 0), (TURNED, 0)]);
    });
}

#[test]
fn a_report_of_another_plugins_or_an_unknown_document_is_refused() {
    world(|w| {
        let document = w.book("books/a.epub", 3);
        let caller = Caller {
            owner: OTHER,
            plugin: "other",
            features: AdmittedFeatures::NONE,
            hosts: &w.hosts,
        };
        let schema = services_wire::service_schema("core.document", 1).unwrap();
        let op = schema.op(22).unwrap();
        let mut input = vec![0; 4096];
        let used = wire::records_encode_counted(
            &[Record::ServiceDocReport(report(document, 0))],
            usize::from(op.input_rows),
            op.max_input,
            &mut input,
        )
        .unwrap();
        let mut out = vec![0; op.max_reply];
        assert_eq!(
            doc_service::backend(w, &caller, op, &input[..used], &mut out),
            Err(ERR_DENIED)
        );
        assert_eq!(w.report(report(document + 40, 0)), Err(ERR_NOT_FOUND));
        assert!(drain(&w.state.reading).is_empty());
    });
}

#[test]
fn only_a_book_of_the_library_raises_events() {
    world(|w| {
        w.readers();
        // The opener's own data and an index build (background) are accepted
        // and raise nothing, so a reader reports every document the same way.
        let data = w.open(0, "a.epub").unwrap();
        let background = w
            .call(
                AdmittedFeatures::test(1, FEATURES),
                1,
                &[Record::ServiceDocOpen(wire::ServiceDocOpen {
                    root: 1,
                    path: "books/bg.epub",
                    format: "",
                    flags: 1 << 2,
                    image_side: 0,
                    cover_side: 0,
                    css_root: 0,
                    css_path: "",
                })],
            )
            .map(|reply| match wire::record_decode_one(&reply).unwrap() {
                Record::ServiceDocState(state) => state.document,
                other => panic!("{other:?}"),
            })
            .unwrap();
        *w.reply.borrow_mut() = Ok(meta(3));
        w.report(report(data, 1)).unwrap();
        w.report(report(background, 1)).unwrap();
        assert!(drain(&w.state.reading).is_empty());
        // A book of the library does (an opener keeps two documents).
        for document in [data, background] {
            w.call(
                AdmittedFeatures::NONE,
                18,
                &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
            )
            .unwrap();
        }
        let book = w.book("books/a.epub", 3);
        w.report(report(book, 0)).unwrap();
        assert_eq!(w.seen(), [(OPENED, 0)]);
    });
}

#[test]
fn a_plugin_that_is_no_reading_app_raises_no_events() {
    world(|w| {
        // A plugin with a screen on top that opens a library book is no
        // reader: its report is accepted and raises nothing.
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        *w.reply.borrow_mut() = Ok(state_reply());
        let book = w.open(1, "books/a.epub").unwrap();
        *w.reply.borrow_mut() = Ok(meta(3));
        w.report(report(book, 0)).unwrap();
        w.report(report(book, 1)).unwrap();
        assert!(drain(&w.state.reading).is_empty());
        // Once it declares a reading app, the same book drives the session.
        w.readers();
        w.report(report(book, 2)).unwrap();
        assert_eq!(w.seen(), [(OPENED, 0)]);
    });
}

#[test]
fn a_report_of_a_reader_that_is_not_on_top_leaves_the_session_alone() {
    world(|w| {
        let first = w.book("books/a.epub", 9);
        let second = w.book("books/b.epub", 9);
        w.report(report(first, 2)).unwrap();
        assert_eq!(w.seen(), [(OPENED, 0)]);
        // Another plugin's screen is on top: the reports are accepted and
        // raise nothing; they never start, replace, advance or end the
        // session (no close of the first book, no finish).
        w.top.set(OTHER);
        let mut away = report(first, 8);
        away.flags = 1;
        w.report(away).unwrap();
        w.report(report(second, 0)).unwrap();
        assert!(drain(&w.state.reading).is_empty());
        // Back on top: the session is where it was.
        w.top.set(OPENED_BY);
        w.report(report(first, 2)).unwrap();
        assert!(drain(&w.state.reading).is_empty());
        w.report(report(first, 3)).unwrap();
        assert_eq!(w.seen(), [(TURNED, 0)]);
        // Another book on top of the stack takes over: the close comes first.
        w.report(report(second, 0)).unwrap();
        assert_eq!(w.seen(), [(CLOSED, 0), (OPENED, 0)]);
    });
}

const OPENED_BY: u64 = OPENER;

#[test]
fn the_opener_stopping_closes_the_book_and_the_sleep_closes_it_with_reason_one() {
    world(|w| {
        let document = w.book("books/a.epub", 3);
        w.report(report(document, 0)).unwrap();
        drain(&w.state.reading);
        // The device sleeps: the book closes with reason 1 ...
        w.state.reading.sleep(10);
        assert_eq!(w.seen(), [(CLOSED, 1)]);
        // ... and the same book reported after the wake opens with reason 1.
        w.report(report(document, 0)).unwrap();
        assert_eq!(w.seen(), [(OPENED, 1)]);
        // The opener stops: its documents end, the book closes.
        doc_service::opener_stopped(&*w.state, OPENER).unwrap();
        assert_eq!(w.seen(), [(CLOSED, 0)]);
    });
}

#[test]
fn a_refused_allocation_raises_nothing_and_the_next_report_tries_again() {
    world(|w| {
        let document = w.book("books/a.epub", 3);
        // The section count is known: the first allocation is the session's.
        w.state.table.set_sections(document, 3);
        w.fail.set(1);
        assert_eq!(w.report(report(document, 0)), Err(ERR_NO_MEMORY));
        assert!(drain(&w.state.reading).is_empty());
        w.fail.set(0);
        w.report(report(document, 0)).unwrap();
        assert_eq!(w.seen(), [(OPENED, 0)]);
    });
}
