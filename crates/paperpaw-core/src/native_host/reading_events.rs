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
use crate::Kernel;

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
        self.native_reading_queue(&edge);
        true
    }

    /// The device goes to sleep or off at `now_ms`: the open book closes, and
    /// the event is queued at once.
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
        let Ok(record) = super::registrations::records_decode_single(&edge.bytes) else {
            return;
        };
        let tag = edge.tag;
        self.native_queue_event(tag, &|_| true, &|queue| queue.push_record(tag, &record));
    }
}
