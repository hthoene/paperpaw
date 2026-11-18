//! The reading session of the document service (`docs/abi/document.md` 2.4,
//! `core-events.md` 3.1 and 3.2): one device-wide record of the book the user
//! is reading, kept from the reader's reports, and the events it raises
//! (`page_turned` 37, `book_opened` 65, `book_closed` 66, `book_finished`
//! 67).
//!
//! The session never calls a plugin and never reaches the kernel's queues: a
//! report (or the end of its document, or the device going to sleep) encodes
//! the events it raises into the outbox, and the kernel's pump takes them
//! between callbacks ([`Reading::take_edge`]) and queues them for the
//! subscribers, in the order they were raised (`reading_events.rs`).
//!
//! Which reports count is the caller's ([`Facts`]): a book of the library
//! (root 1, not a background open) that the reader on screen reports. Any
//! other report is `OK` and never touches the session.
//!
//! Memory: the session is one record, its texts (the path, the title and the
//! chapter, at most 1,024 bytes together) in one buffer reserved at that full
//! size when the session starts, so a later title or chapter never grows it;
//! it is freed when the session ends. The outbox holds the events a report
//! raised (at most [`EDGES_MAX`], each encoded at its exact size, under
//! 1.2 KB) until the pump has taken them: the pump runs right after the
//! callback that reported, so it is empty between reports.
use super::{registrations as wire, registrations::Record};
use crate::native_host::codec;
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_NO_MEMORY};

/// The most events waiting for the pump (a report raises at most three, and a
/// new book's report ends the old session first).
pub(crate) const EDGES_MAX: usize = 8;
/// The path, the title and the chapter together.
const TEXTS_MAX: usize = 1024;
/// `doc_report.flags` bit 0: the user reached the end.
pub(crate) const FINISHED: u32 = 1;

/// `book_closed.reason`: the book was closed.
const CLOSED: u32 = 0;
/// `book_closed.reason` and `book_opened.reason`: the device slept or woke.
pub(super) const SLEPT: u32 = 1;

/// One event for the kernel's pump: the event's wire tag and its record as a
/// one-record batch.
pub(crate) struct Edge {
    pub(crate) tag: u32,
    pub(crate) bytes: Vec<u8>,
}

/// What the document service knows of a report's document.
pub(crate) struct Facts<'a> {
    pub(crate) document: u32,
    /// A book of the library (root 1, not a background open) and the report
    /// comes from the reader on screen.
    pub(crate) counts: bool,
    /// The book's path as `doc_open` named it.
    pub(crate) path: &'a str,
}

/// The location and counters of a report (what the events carry).
#[derive(Clone, Copy, PartialEq, Eq)]
struct Place {
    section: u32,
    offset: u32,
    insert: u32,
    progress_ppm: u32,
    page: Option<u32>,
    pages: Option<u32>,
    chapter_left: Option<u32>,
}

impl Place {
    fn of(report: &wire::ServiceDocReport<'_>) -> Self {
        Self {
            section: report.section,
            offset: report.offset,
            insert: report.insert,
            progress_ppm: report.progress_ppm,
            page: report.page,
            pages: report.pages,
            chapter_left: report.chapter_left,
        }
    }
}

/// The session: the last raised report and whether `finished` was raised.
struct Session {
    document: u32,
    /// The path, the title and the chapter, in this order, in a buffer of
    /// [`TEXTS_MAX`] bytes reserved when the session started.
    texts: Vec<u8>,
    path_len: usize,
    title_len: usize,
    place: Place,
    finished: bool,
}

impl Session {
    fn path(&self) -> &str {
        self.text(0, self.path_len)
    }
    fn title(&self) -> &str {
        self.text(self.path_len, self.title_len)
    }
    fn chapter(&self) -> &str {
        self.text(
            self.path_len + self.title_len,
            self.texts.len() - self.path_len - self.title_len,
        )
    }
    fn text(&self, at: usize, len: usize) -> &str {
        self.texts
            .get(at..at + len)
            .and_then(|bytes| core::str::from_utf8(bytes).ok())
            .unwrap_or("")
    }

    /// Replaces the title and the chapter, within the room reserved.
    fn set_texts(&mut self, title: &str, chapter: &str) {
        self.texts.truncate(self.path_len);
        self.texts.extend_from_slice(title.as_bytes());
        self.texts.extend_from_slice(chapter.as_bytes());
        self.title_len = title.len();
    }
}

/// The device's reading session and the events waiting for the pump.
pub(crate) struct Reading {
    session: RefCell<Option<Session>>,
    edges: RefCell<Vec<Edge>>,
    /// The path of the book that was open when the device went to sleep: the
    /// next session of it opens with `reason` 1.
    slept: RefCell<Option<alloc::string::String>>,
    /// The latest time stamped: an event is never stamped before an earlier
    /// one (a clock read late cannot put a close before the last page turn).
    clock: Cell<u64>,
}

impl Reading {
    pub(crate) fn new() -> Self {
        Self {
            session: RefCell::new(None),
            edges: RefCell::new(Vec::new()),
            slept: RefCell::new(None),
            clock: Cell::new(0),
        }
    }

    /// `now_ms`, or the latest time stamped if that is later.
    fn stamp(&self, now_ms: u64) -> u64 {
        let now = now_ms.max(self.clock.get());
        self.clock.set(now);
        now
    }

    /// Whether events wait for the pump.
    pub(crate) fn waiting(&self) -> bool {
        self.edges
            .try_borrow()
            .map_or(true, |edges| !edges.is_empty())
    }

    /// The oldest event for the pump, taken.
    pub(crate) fn take_edge(&self) -> Option<Edge> {
        let mut edges = self.edges.try_borrow_mut().ok()?;
        (!edges.is_empty()).then(|| edges.remove(0))
    }

    /// A checked report of the document `facts` names, at `now_ms`
    /// (`document.md` 2.4).
    ///
    /// # Errors
    /// `ERR_NO_MEMORY` when the session or an event cannot be had (nothing is
    /// raised for this report; when it was another book's, the old session has
    /// ended with its `book_closed` first); `ERR_BUSY` when the session is in
    /// use or the pump is `EDGES_MAX` events behind (nothing is changed).
    pub(crate) fn report(
        &self,
        now_ms: u64,
        facts: &Facts<'_>,
        report: &wire::ServiceDocReport<'_>,
    ) -> Result<(), i32> {
        if !facts.counts {
            return Ok(());
        }
        let now_ms = self.stamp(now_ms);
        let mut session = self.session.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        // Another book's report: the old book closes before the new one
        // opens (`core-events.md` 3.2).
        if session
            .as_ref()
            .is_some_and(|s| s.document != facts.document)
        {
            self.end(&mut session, now_ms, CLOSED);
        }
        let place = Place::of(report);
        let finished = report.flags & FINISHED != 0;
        let Some(open) = session.as_mut() else {
            return self.start(&mut session, now_ms, facts, report, place);
        };
        let turned = place != open.place || report.chapter != open.chapter();
        let finishing = finished && !open.finished;
        if !turned && !finishing {
            return Ok(());
        }
        // The events of this report, built before anything changes.
        let mut raised: [Option<Edge>; 2] = [None, None];
        if turned {
            raised[0] = Some(encode(
                wire::EVENT_PAGE_TURNED_TAG,
                now_ms,
                0,
                (facts.path, report.title, report.chapter),
                place,
            )?);
        }
        if finishing {
            raised[1] = Some(encode(
                wire::EVENT_BOOK_FINISHED_TAG,
                now_ms,
                0,
                (facts.path, report.title, report.chapter),
                place,
            )?);
        }
        self.push(&mut raised)?;
        if turned {
            open.place = place;
            open.set_texts(report.title, report.chapter);
        }
        open.finished |= finished;
        Ok(())
    }

    /// The first report of a document: the session starts and `book_opened`
    /// is raised (`book_finished` after it where the report says finished).
    fn start(
        &self,
        session: &mut Option<Session>,
        now_ms: u64,
        facts: &Facts<'_>,
        report: &wire::ServiceDocReport<'_>,
        place: Place,
    ) -> Result<(), i32> {
        let mut texts = Vec::new();
        texts
            .try_reserve_exact(TEXTS_MAX)
            .map_err(|_| ERR_NO_MEMORY)?;
        texts.extend_from_slice(facts.path.as_bytes());
        texts.extend_from_slice(report.title.as_bytes());
        texts.extend_from_slice(report.chapter.as_bytes());
        if texts.len() > TEXTS_MAX {
            return Err(ERR_INVALID);
        }
        // After a sleep, the same book opens again with reason 1.
        let reason = {
            let slept = self.slept.try_borrow().map_err(|_| ERR_BUSY)?;
            u32::from(slept.as_deref() == Some(facts.path)) * SLEPT
        };
        let texts_of = (facts.path, report.title, report.chapter);
        let mut raised: [Option<Edge>; 2] = [
            Some(encode(
                wire::EVENT_BOOK_OPENED_TAG,
                now_ms,
                reason,
                texts_of,
                place,
            )?),
            None,
        ];
        let finished = report.flags & FINISHED != 0;
        if finished {
            raised[1] = Some(encode(
                wire::EVENT_BOOK_FINISHED_TAG,
                now_ms,
                0,
                texts_of,
                place,
            )?);
        }
        self.push(&mut raised)?;
        if let Ok(mut slept) = self.slept.try_borrow_mut() {
            *slept = None;
        }
        *session = Some(Session {
            document: facts.document,
            texts,
            path_len: facts.path.len(),
            title_len: report.title.len(),
            place,
            finished,
        });
        Ok(())
    }

    /// The document `document` ended (closed, its opener or format stopped, the
    /// host closed it): its session ends with `book_closed` (reason 0).
    pub(crate) fn document_ended(&self, now_ms: u64, document: u32) {
        let Ok(mut session) = self.session.try_borrow_mut() else {
            return;
        };
        let now_ms = self.stamp(now_ms);
        if session.as_ref().is_some_and(|s| s.document == document) {
            self.end(&mut session, now_ms, CLOSED);
        }
    }

    /// The device goes to sleep or off: the session ends with `book_closed`
    /// (reason 1), and the next report of the same book opens it again with
    /// `book_opened` (reason 1).
    pub(crate) fn sleep(&self, now_ms: u64) {
        let Ok(mut session) = self.session.try_borrow_mut() else {
            return;
        };
        let now_ms = self.stamp(now_ms);
        let Some(path) = session.as_ref().map(|s| s.path()) else {
            return;
        };
        let mut kept = alloc::string::String::new();
        let kept = kept.try_reserve_exact(path.len()).ok().map(|()| {
            kept.push_str(path);
            kept
        });
        self.end(&mut session, now_ms, SLEPT);
        if let Ok(mut slept) = self.slept.try_borrow_mut() {
            *slept = kept;
        }
    }

    /// Ends the session with `book_closed` (`reason`, the last raised fields).
    /// An event the pump has no room for is lost (the session ends all the
    /// same: a book that is closed stays closed).
    fn end(&self, session: &mut Option<Session>, now_ms: u64, reason: u32) {
        let Some(old) = session.take() else {
            return;
        };
        let closed = encode(
            wire::EVENT_BOOK_CLOSED_TAG,
            now_ms,
            reason,
            (old.path(), old.title(), old.chapter()),
            old.place,
        );
        if let Ok(edge) = closed {
            let _ = self.push(&mut [Some(edge), None]);
        }
    }

    /// Queues `raised` for the pump, all or none.
    fn push(&self, raised: &mut [Option<Edge>; 2]) -> Result<(), i32> {
        let mut edges = self.edges.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        let count = raised.iter().flatten().count();
        if edges.len() + count > EDGES_MAX {
            return Err(ERR_BUSY);
        }
        edges.try_reserve(count).map_err(|_| ERR_NO_MEMORY)?;
        edges.extend(raised.iter_mut().filter_map(Option::take));
        Ok(())
    }
}

/// One event as a one-record batch, at its exact size. Its own frame: the
/// page turn's record is the largest (under 1.1 KB) and must not widen the
/// caller's.
#[inline(never)]
fn encode(
    tag: u32,
    now_ms: u64,
    reason: u32,
    (path, title, chapter): (&str, &str, &str),
    place: Place,
) -> Result<Edge, i32> {
    let Place {
        section,
        offset,
        insert,
        progress_ppm,
        page,
        pages,
        chapter_left,
    } = place;
    let (insert, page, pages, chapter_left) = (Some(insert), page, pages, chapter_left);
    let record = match tag {
        wire::EVENT_BOOK_OPENED_TAG => Record::EventBookOpened(wire::EventBookOpened {
            now_ms,
            section,
            offset,
            progress_ppm,
            path,
            title,
            chapter,
            reason,
            page,
            pages,
            insert,
            chapter_left,
        }),
        wire::EVENT_BOOK_CLOSED_TAG => Record::EventBookClosed(wire::EventBookClosed {
            now_ms,
            section,
            offset,
            progress_ppm,
            path,
            title,
            chapter,
            reason,
            page,
            pages,
            insert,
            chapter_left,
        }),
        wire::EVENT_BOOK_FINISHED_TAG => Record::EventBookFinished(wire::EventBookFinished {
            now_ms,
            section,
            offset,
            progress_ppm,
            path,
            title,
            chapter,
            reason,
            page,
            pages,
            insert,
            chapter_left,
        }),
        _ => Record::EventPageTurned(wire::EventPageTurned {
            now_ms,
            section,
            offset,
            progress_ppm,
            path,
            title,
            chapter,
            page,
            pages,
            insert,
            chapter_left,
        }),
    };
    let length = codec::framed_len(&record)?
        .checked_add(1)
        .ok_or(ERR_INVALID)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(|_| ERR_NO_MEMORY)?;
    bytes.resize(length, 0);
    codec::encode_batch(core::slice::from_ref(&record), &mut bytes)?;
    Ok(Edge { tag, bytes })
}

#[cfg(test)]
#[path = "doc_reading_tests.rs"]
pub(crate) mod tests;
