//! The one-time low-memory notice (#248, Senior's recommendation, no hard
//! limit for Beta 4): where a shell finds the memory short with store plugins
//! loaded (the X4 shell's rule, `paperpaw_x4::low_memory`), it asks the kernel
//! to say so once. The notice is a kernel-owned dialog on the host's
//! `ConfirmBox`: its words, *Plugins* (opens the plugin manager's list) and
//! *Not now* (or *Back*). The kernel shows it only over the home screen,
//! never over another screen (the first-run setup, a book, a dialog, the
//! sleep screen), and nothing of it is kept once it closes.
use crate::{Effects, Kernel};
use paperpaw_platform::Instant;
use paperpaw_sdk::builtin::slots;
use paperpaw_sdk::{Id, Params};

/// The screen's id: a host screen, opened only by the kernel.
pub(crate) const SCREEN: Id = Id::new_static("core.low-memory");
pub(crate) const TITLE: &str = "title";
pub(crate) const TEXT: &str = "text";
pub(crate) const PLUGINS_WORD: &str = "plugins";
pub(crate) const LATER_WORD: &str = "later";

impl Kernel {
    /// Whether the home screen is on top, with nothing over it.
    pub fn home_on_top(&self) -> bool {
        self.stack
            .last()
            .is_some_and(|top| top.slot.as_ref() == Some(&slots::HOME) && top.id != SCREEN)
    }

    /// Whether the home screen is on top and no document is open: no book,
    /// no cover being made, no background job (their memory is not Home's).
    pub fn calm_home(&self) -> bool {
        self.home_on_top()
            && !self
                .shared
                .native_instances
                .hosts()
                .and_then(|hosts| hosts.documents())
                .is_some_and(|documents| documents.0.documents_open())
    }

    /// Shows the low-memory notice, once the caller has decided it is due
    /// (the shell's rule); `true` where it shows. Only over the home screen
    /// with nothing above it and nothing guarding the device: anywhere else
    /// (or where the screen could not open) it does nothing and the caller
    /// asks again later.
    pub fn show_low_memory_notice(&mut self, now: Instant) -> (bool, Effects) {
        if !self.home_on_top()
            || self.held_by_guard()
            || self.notice.is_some()
            || self.waiting_notices.start_notice_shown
        {
            return (false, Effects::default());
        }
        self.now = now;
        let tr = &self.shared.translator;
        let params = Params::new()
            .with(TITLE, tr.tr("core.low-memory.title"))
            .with(TEXT, tr.tr("core.low-memory.text"))
            .with(PLUGINS_WORD, tr.tr("core.low-memory.plugins"))
            .with(LATER_WORD, tr.tr("core.low-memory.later"));
        self.open(SCREEN, None, &params);
        let shown = self.stack.last().is_some_and(|top| top.id == SCREEN);
        self.waiting_notices.start_notice_shown |= shown;
        (shown, self.finish())
    }
}
