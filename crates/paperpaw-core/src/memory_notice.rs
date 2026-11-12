//! "Not enough memory for this" (#248): the notice shown when a screen
//! cannot open for a lack of memory, such as a parked plugin's that could
//! not be made again (and, with parking, when its waiting actions are
//! dropped for the lack). A notice made then would meet the same lack (`notify`
//! allocates without a check), so this one is made ahead, at start and again
//! when the language or the size of the interface changes, and kept: showing
//! it allocates nothing, and it comes back when it ends.
use super::*;
use crate::native_host::notice::Notice;
use paperpaw_sdk::builtin::params as notice_params;

/// Its words, in the host's catalog.
pub(crate) const KEY: &str = "core.screen.no-memory";

impl Kernel {
    /// Makes the notice ahead, in place of the one before (which goes first,
    /// so that the two are never held at once).
    pub(crate) fn reserve_memory_notice(&mut self) {
        self.waiting_notices.keep_memory_notice(None);
        let words = self.shared.translator.tr(KEY);
        let params = Params::new().with(notice_params::TEXT, words);
        let notice: Box<dyn Screen> = Box::new(Notice::new(&self.shared.env(self.now), &params));
        self.waiting_notices.keep_memory_notice(Some(notice));
    }

    /// Made again where one is made: the language, the interface's size or
    /// its font changed (before start there is none yet).
    pub(crate) fn renew_memory_notice(&mut self) {
        if self.waiting_notices.has_memory_notice() {
            self.reserve_memory_notice();
        }
    }

    /// Shows the notice made ahead, without allocating: in place of the
    /// notice showing, never over a guarding screen (the lack is in the log
    /// then), and not again while it shows.
    pub(crate) fn show_memory_notice(&mut self) {
        if self.held_by_guard() || !self.waiting_notices.memory_notice_ready() {
            return;
        }
        // The notice showing ends first: ended while this one showed, it
        // would be kept in its place.
        self.end_notice();
        let Some(notice) = self.waiting_notices.show_memory_notice() else {
            return;
        };
        self.place_notice(notice);
    }
}
