//! The reading session (`document.md` 2.4, `core-events.md` 3.2): what a
//! report raises, in which order, and what leaves the session alone.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
extern crate std;
use super::*;
use alloc::string::String;

/// One event the pump would take: its tag, `now_ms`, `reason` (37 has none:
/// 0), the path, title and chapter and the section.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Seen {
    pub(crate) tag: u32,
    pub(crate) now_ms: u64,
    pub(crate) reason: u32,
    pub(crate) path: String,
    pub(crate) title: String,
    pub(crate) chapter: String,
    pub(crate) section: u32,
}

/// Every event waiting, taken in order.
pub(crate) fn drain(reading: &Reading) -> Vec<Seen> {
    let mut seen = Vec::new();
    while let Some(edge) = reading.take_edge() {
        let (now_ms, reason, path, title, chapter, section) =
            match wire::records_decode_single(&edge.bytes).unwrap() {
                Record::EventPageTurned(e) => (e.now_ms, 0, e.path, e.title, e.chapter, e.section),
                Record::EventBookOpened(e) => {
                    (e.now_ms, e.reason, e.path, e.title, e.chapter, e.section)
                }
                Record::EventBookClosed(e) => {
                    (e.now_ms, e.reason, e.path, e.title, e.chapter, e.section)
                }
                Record::EventBookFinished(e) => {
                    (e.now_ms, e.reason, e.path, e.title, e.chapter, e.section)
                }
                other => panic!("{other:?}"),
            };
        seen.push(Seen {
            tag: edge.tag,
            now_ms,
            reason,
            path: path.into(),
            title: title.into(),
            chapter: chapter.into(),
            section,
        });
    }
    seen
}

const OPENED: u32 = wire::EVENT_BOOK_OPENED_TAG;
const CLOSED_TAG: u32 = wire::EVENT_BOOK_CLOSED_TAG;
const FINISHED_TAG: u32 = wire::EVENT_BOOK_FINISHED_TAG;
const TURNED: u32 = wire::EVENT_PAGE_TURNED_TAG;

fn report<'a>(document: u32, section: u32, chapter: &'a str) -> wire::ServiceDocReport<'a> {
    wire::ServiceDocReport {
        document,
        section,
        offset: 0,
        insert: 0,
        progress_ppm: 1000 * section,
        flags: 0,
        title: "Title",
        chapter,
        page: None,
        pages: None,
        chapter_left: None,
    }
}

fn facts(document: u32, path: &str) -> Facts<'_> {
    Facts {
        document,
        counts: true,
        path,
    }
}

fn tags(seen: &[Seen]) -> Vec<(u32, u32)> {
    seen.iter().map(|s| (s.tag, s.reason)).collect()
}

#[test]
fn the_first_report_opens_the_book_and_a_repeat_is_free() {
    let reading = Reading::new();
    let f = facts(1, "books/a.epub");
    reading.report(100, &f, &report(1, 0, "One")).unwrap();
    let seen = drain(&reading);
    assert_eq!(tags(&seen), [(OPENED, 0)]);
    // The host stamps the time and the path; the plugin gives neither.
    assert_eq!(seen[0].now_ms, 100);
    assert_eq!(
        (seen[0].path.as_str(), seen[0].title.as_str()),
        ("books/a.epub", "Title")
    );
    reading.report(200, &f, &report(1, 0, "One")).unwrap();
    assert!(drain(&reading).is_empty());
    assert!(!reading.waiting());
}

#[test]
fn a_changed_report_turns_the_page_and_a_title_alone_does_not() {
    let reading = Reading::new();
    let f = facts(1, "a");
    reading.report(1, &f, &report(1, 0, "One")).unwrap();
    drain(&reading);
    // Each field of the list raises, one at a time.
    let mut r = report(1, 0, "One");
    r.offset = 5;
    reading.report(2, &f, &r).unwrap();
    r.insert = 3;
    reading.report(3, &f, &r).unwrap();
    r.progress_ppm += 1;
    reading.report(4, &f, &r).unwrap();
    r.page = Some(1);
    r.pages = Some(9);
    reading.report(5, &f, &r).unwrap();
    r.chapter_left = Some(2);
    reading.report(6, &f, &r).unwrap();
    r.chapter = "Two";
    reading.report(7, &f, &r).unwrap();
    r.section = 2;
    reading.report(8, &f, &r).unwrap();
    let seen = drain(&reading);
    assert_eq!(tags(&seen), [(TURNED, 0); 7]);
    assert_eq!(seen[5].chapter, "Two");
    assert_eq!(seen[6].section, 2);
    // The book's title is not in the list: no event, and it is not kept.
    r.title = "Another";
    reading.report(9, &f, &r).unwrap();
    assert!(drain(&reading).is_empty());
}

#[test]
fn finished_is_raised_once_after_the_page_turn() {
    let reading = Reading::new();
    let f = facts(1, "a");
    let mut r = report(1, 3, "End");
    reading.report(1, &f, &r).unwrap();
    drain(&reading);
    r.section = 4;
    r.flags = FINISHED;
    reading.report(2, &f, &r).unwrap();
    assert_eq!(
        tags(&drain(&reading)),
        [(TURNED, 0), (FINISHED_TAG, 0)],
        "the turn first"
    );
    // Reported again, or with a later turn: nothing more of it.
    reading.report(3, &f, &r).unwrap();
    r.section = 5;
    reading.report(4, &f, &r).unwrap();
    assert_eq!(tags(&drain(&reading)), [(TURNED, 0)]);
    // Finished on the first report: opened, then finished.
    let other = Reading::new();
    let mut first = report(1, 0, "");
    first.flags = FINISHED;
    other.report(1, &facts(1, "b"), &first).unwrap();
    assert_eq!(tags(&drain(&other)), [(OPENED, 0), (FINISHED_TAG, 0)]);
}

#[test]
fn another_book_closes_the_old_one_before_it_opens() {
    let reading = Reading::new();
    reading
        .report(1, &facts(1, "a"), &report(1, 2, "One"))
        .unwrap();
    drain(&reading);
    reading
        .report(5, &facts(2, "b"), &report(2, 0, "Intro"))
        .unwrap();
    let seen = drain(&reading);
    assert_eq!(tags(&seen), [(CLOSED_TAG, 0), (OPENED, 0)]);
    // The close carries the old book's last raised fields.
    assert_eq!(
        (
            seen[0].path.as_str(),
            seen[0].chapter.as_str(),
            seen[0].section
        ),
        ("a", "One", 2)
    );
    assert_eq!(seen[1].path, "b");
}

#[test]
fn a_document_that_ends_closes_its_session_and_only_its_own() {
    let reading = Reading::new();
    reading
        .report(1, &facts(1, "a"), &report(1, 0, "One"))
        .unwrap();
    drain(&reading);
    reading.document_ended(9, 7);
    assert!(drain(&reading).is_empty());
    reading.document_ended(10, 1);
    assert_eq!(tags(&drain(&reading)), [(CLOSED_TAG, 0)]);
    // Ended twice: nothing more. The next report opens a session again.
    reading.document_ended(11, 1);
    assert!(drain(&reading).is_empty());
    reading
        .report(12, &facts(1, "a"), &report(1, 0, "One"))
        .unwrap();
    assert_eq!(tags(&drain(&reading)), [(OPENED, 0)]);
}

#[test]
fn sleep_closes_with_reason_one_and_the_same_book_opens_again_with_it() {
    let reading = Reading::new();
    reading.sleep(1);
    assert!(drain(&reading).is_empty());
    reading
        .report(1, &facts(1, "a"), &report(1, 0, "One"))
        .unwrap();
    drain(&reading);
    reading.sleep(50);
    assert_eq!(tags(&drain(&reading)), [(CLOSED_TAG, 1)]);
    // After the wake (the old document may be another id): the same path.
    reading
        .report(60, &facts(2, "a"), &report(2, 0, "One"))
        .unwrap();
    assert_eq!(tags(&drain(&reading)), [(OPENED, 1)]);
    // Only the first: a later session of it opens as usual.
    reading.document_ended(70, 2);
    drain(&reading);
    reading
        .report(80, &facts(3, "a"), &report(3, 0, "One"))
        .unwrap();
    assert_eq!(tags(&drain(&reading)), [(OPENED, 0)]);
    // Another book after a sleep opens with reason 0 and forgets the old one.
    reading.sleep(90);
    drain(&reading);
    reading
        .report(100, &facts(4, "b"), &report(4, 0, "One"))
        .unwrap();
    assert_eq!(tags(&drain(&reading)), [(OPENED, 0)]);
    reading.document_ended(110, 4);
    drain(&reading);
    reading
        .report(120, &facts(5, "a"), &report(5, 0, "One"))
        .unwrap();
    assert_eq!(tags(&drain(&reading)), [(OPENED, 0)]);
}

#[test]
fn a_report_that_does_not_count_never_touches_the_session() {
    let reading = Reading::new();
    let mut other = facts(2, "b");
    other.counts = false;
    // No session: nothing starts.
    reading.report(1, &other, &report(2, 0, "x")).unwrap();
    assert!(drain(&reading).is_empty());
    reading
        .report(2, &facts(1, "a"), &report(1, 1, "One"))
        .unwrap();
    drain(&reading);
    // Another document, or the same book from a reader that is not on top: no
    // close, no turn, no finish.
    let mut r = report(2, 9, "Far");
    r.flags = FINISHED;
    reading.report(3, &other, &r).unwrap();
    let mut same = facts(1, "a");
    same.counts = false;
    reading.report(4, &same, &r).unwrap();
    assert!(drain(&reading).is_empty());
    // The session is as it was: its own identical report raises nothing, and
    // its next turn is measured from its last raised place.
    reading
        .report(5, &facts(1, "a"), &report(1, 1, "One"))
        .unwrap();
    assert!(drain(&reading).is_empty());
    reading
        .report(6, &facts(1, "a"), &report(1, 2, "One"))
        .unwrap();
    assert_eq!(tags(&drain(&reading)), [(TURNED, 0)]);
}

#[test]
fn an_event_is_never_stamped_before_an_earlier_one() {
    let reading = Reading::new();
    reading
        .report(100, &facts(1, "a"), &report(1, 0, "One"))
        .unwrap();
    // A clock read late (the pump's) is behind the report's.
    reading.document_ended(40, 1);
    let seen = drain(&reading);
    assert_eq!(
        seen.iter().map(|s| s.now_ms).collect::<Vec<_>>(),
        [100, 100]
    );
}

#[test]
fn the_pump_that_is_behind_refuses_the_report_and_changes_nothing() {
    let reading = Reading::new();
    let f = facts(1, "a");
    reading.report(1, &f, &report(1, 0, "One")).unwrap();
    for section in 1..EDGES_MAX as u32 {
        reading.report(2, &f, &report(1, section, "One")).unwrap();
    }
    assert_eq!(reading.report(3, &f, &report(1, 20, "One")), Err(ERR_BUSY));
    assert_eq!(drain(&reading).len(), EDGES_MAX);
    // Nothing of the refused report was kept: the same report goes through.
    reading.report(4, &f, &report(1, 20, "One")).unwrap();
    let seen = drain(&reading);
    assert_eq!((seen.len(), seen[0].section), (1, 20));
}

#[test]
fn the_texts_are_one_record_reserved_at_their_full_size() {
    let reading = Reading::new();
    let path = "p".repeat(512);
    let title = "t".repeat(256);
    let chapter = "c".repeat(256);
    let mut r = report(1, 0, "");
    r.title = &title;
    reading.report(1, &facts(1, &path), &r).unwrap();
    let reserved = reading.session.borrow().as_ref().unwrap().texts.capacity();
    assert!(reserved >= TEXTS_MAX);
    // A chapter that appears later and one that changes grow nothing.
    r.chapter = &chapter;
    r.section = 1;
    reading.report(2, &facts(1, &path), &r).unwrap();
    r.chapter = "short";
    r.section = 2;
    reading.report(3, &facts(1, &path), &r).unwrap();
    let session = reading.session.borrow();
    let session = session.as_ref().unwrap();
    assert_eq!(session.texts.capacity(), reserved);
    assert_eq!(session.chapter(), "short");
    assert_eq!(session.path().len(), 512);
}
