//! The page after the one on show, laid out in the quiet time after a turn, and
//! the count of the pages, which waits for the same quiet time.
use super::*;

/// The page after the one on show is laid out this long after a turn (the
/// screen is drawn and refreshing by then), so the next turn only takes it.
pub(super) const KEY: &str = "ahead";
pub(super) const AFTER_MS: u32 = 300;

impl NativeReader {
    /// The quiet time of the work `key` has come: the next page is laid out,
    /// or a step of the count of the pages is made.
    pub(super) fn quiet_step(&mut self, host: &Host<'_>, key: &str) {
        match key {
            KEY => self.ahead_step(host),
            _ => self.count_step(host),
        }
    }

    /// The next page is laid out now, as the count's steps wait while another
    /// screen covers the reader.
    fn ahead_step(&mut self, host: &Host<'_>) {
        if self.ui.is_some()
            && self.stack_top(host).ok().flatten() != self.screens.top_instance(SCREEN)
        {
            return;
        }
        if let (Some(view), Some(docs)) = (self.screens.top(SCREEN), &self.docs) {
            view.lay_ahead(host, docs);
        }
    }

    /// The pages of the reading screen on top are counted in the background:
    /// a step now, then one after the other.
    pub(super) fn start_count(&mut self, host: &Host<'_>) {
        if self
            .screens
            .top(SCREEN)
            .is_some_and(|view| view.counting() || view.marks_due())
        {
            self.declare_work(
                host,
                (native_index::WORK_KEY, WORK_DUE),
                native_index::DELAY_MS,
            );
        }
    }

    /// One step of the count of the pages; the next is declared while pages remain.
    /// The count waits while another screen covers the reading screen: its
    /// layouts would add to that screen's peak (#248, Manager's decision 1).
    /// `reading_shown` starts it again.
    pub(super) fn count_step(&mut self, host: &Host<'_>) {
        if self.ui.is_some()
            && self.stack_top(host).ok().flatten() != self.screens.top_instance(SCREEN)
        {
            return;
        }
        let more = match (self.screens.top(SCREEN), &self.docs) {
            (Some(view), Some(docs)) => {
                // The first page's marks are asked here, after it is shown.
                view.marks_step(host, docs);
                view.count_step(host, docs)
            }
            _ => None,
        };
        if let Some(delay) = more {
            self.declare_work(host, (native_index::WORK_KEY, WORK_DUE), delay);
        }
    }
}
