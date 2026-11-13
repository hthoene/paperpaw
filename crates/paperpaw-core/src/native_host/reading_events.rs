//! The reading session's events to the plugins (`docs/abi/core-events.md`
//! 3.1 and 3.2): `page_turned` 37 and `book_opened`, `book_closed`,
//! `book_finished` 65 to 67, raised by the document service from the
//! reader's reports (`doc_reading.rs`) and queued here, between callbacks, for
//! every running plugin that subscribed, in the order they were raised, in the
//! same per-plugin queue as every other kernel event ([`Kernel::native_core_event`]
//! tells them).
//!
//! The service never reaches the queues itself: it holds the encoded events
//! (at most eight, taken at the end of the call that raised them) and the pump
//! takes them. When the device goes to sleep or off, the book that is open
//! closes first ([`Kernel::native_reading_sleep`]), so `book_closed` (reason 1)
//! is queued before `suspend` or `stopping`.
//!
//! The kernel follows the reading as it followed the interface-6 reader's
//! events (#2449): each event is raised on the kernel's side too, as the
//! interface-6 event of the same name, but not queued for the native plugins a
//! second time ([`Kernel::native_reading_kernel`]).
use super::registrations as wire;
use crate::Kernel;
use paperpaw_sdk::Command;
use paperpaw_sdk::builtin::events::{self, Reading};

/// Most events queued in one pass (the service holds eight at most).
const PER_PASS: usize = 8;

impl Kernel {
    /// Takes the reading session's events from the document service and queues
    /// them for the subscribers. `true` while more may wait (another pass).
    pub(crate) fn native_reading_pump(&mut self) -> bool {
        (0..PER_PASS).all(|_| self.native_reading_take())
    }

    /// The oldest event of the session, queued; whether there was one.
    fn native_reading_take(&mut self) -> bool {
        let Some(edge) = self
            .shared
            .native_instances
            .hosts()
            .and_then(|hosts| hosts.documents())
            .and_then(|documents| documents.0.take_reading_edge())
        else {
            return false;
        };
        self.native_reading_tell(&edge);
        true
    }

    /// One event of the session: queued for the native plugins that
    /// subscribed, and raised on the kernel's side.
    fn native_reading_tell(&mut self, edge: &super::doc_reading::Edge) {
        self.native_reading_queue(edge);
        self.native_reading_kernel(edge);
    }

    /// The device goes to sleep or off at `now_ms`: the open book closes, and
    /// the event is queued at once. Called inside the kernel's `emit_from` of
    /// the sleep or stop event, so an event of the session that still waited
    /// is raised on the kernel's side one call deeper (once: the sleep's own
    /// close, reason 1, is not raised there).
    pub(crate) fn native_reading_sleep(&mut self, now_ms: u64) {
        let Some(documents) = self
            .shared
            .native_instances
            .hosts()
            .and_then(|hosts| hosts.documents())
        else {
            return;
        };
        documents.0.reading_sleep(now_ms);
        while self.native_reading_take() {}
    }

    /// One event for the plugins that subscribed to its tag. A tag no plugin
    /// subscribed to is left out: nothing is held for nobody. One that does not
    /// fit a queue is dropped and logged there ([`Self::native_queue_event`]).
    // Its own frame: the page turn's record is the largest (under 1.1 KB).
    #[inline(never)]
    fn native_reading_queue(&mut self, edge: &super::doc_reading::Edge) {
        let Ok(record) = super::codec::decode_single(&edge.bytes) else {
            return;
        };
        let tag = edge.tag;
        self.native_queue_event(tag, &|_| true, &|queue| queue.push_record(tag, &record));
    }

    /// The kernel's own following of the reading (`core_plugin`'s
    /// `register_reading`: the books opened last, how far a book is read,
    /// finished; the sleep timer, declared work, books' tasks), as the
    /// interface-6 reader's events drove it in Beta 3: the same event, raised
    /// on the kernel's side alone, as the reader raised it then (one per page
    /// turned; the library writes only a changed percentage). A book closed
    /// for sleep and opened again after it (reason 1) is not told: Beta 3's
    /// reader raised neither, and each would write the list of books opened.
    #[inline(never)]
    fn native_reading_kernel(&mut self, edge: &super::doc_reading::Edge) {
        let Ok(record) = super::codec::decode_single(&edge.bytes) else {
            return;
        };
        let Some((event, reading, reason)) = kernel_reading(&record) else {
            return;
        };
        let opened_or_closed = matches!(
            record,
            wire::Record::EventBookOpened(_) | wire::Record::EventBookClosed(_)
        );
        if opened_or_closed && reason == super::doc_reading::SLEPT {
            return;
        }
        if let Command::Emit(id, params) = event.emit(reading) {
            self.emit_from(&id, &params, None, false);
        }
    }
}

/// The interface-6 event of a reading event's record, its reading and its
/// reason (0 for a page turned); `None` for any other record.
fn kernel_reading(
    record: &wire::Record<'_>,
) -> Option<(paperpaw_sdk::event::Event<Reading>, Reading, u32)> {
    macro_rules! reading {
        ($event:expr, $r:expr, $reason:expr) => {{
            let r = $r;
            let reading = Reading::new(
                r.path.into(),
                r.title.into(),
                percent(r.progress_ppm),
                r.section,
                r.offset,
                r.chapter.into(),
            )
            .page(r.page)
            .pages(r.pages)
            .chapter_left(r.chapter_left);
            Some(($event, reading, $reason))
        }};
    }
    match record {
        wire::Record::EventPageTurned(r) => reading!(events::PAGE_TURNED, r, 0),
        wire::Record::EventBookOpened(r) => reading!(events::BOOK_OPENED, r, r.reason),
        wire::Record::EventBookClosed(r) => reading!(events::BOOK_CLOSED, r, r.reason),
        wire::Record::EventBookFinished(r) => reading!(events::BOOK_FINISHED, r, r.reason),
        _ => None,
    }
}

/// How far in, in percent with one decimal, as the interface-6 reader gave it
/// (1,000,000 parts per million is 100, read to the end).
fn percent(ppm: u32) -> f32 {
    (ppm.min(1_000_000).saturating_add(500) / 1_000) as f32 / 10.0
}

#[cfg(test)]
#[path = "reading_events_tests.rs"]
mod tests;
