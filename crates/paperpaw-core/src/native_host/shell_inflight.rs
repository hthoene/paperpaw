//! Which plugin's code is running, kept in a fixed record a reset can leave
//! behind (the X4 puts it in RTC memory): a bounded stack of ids, one per
//! nested call into plugin code, so that a nested call returning (or being
//! refused) gives the outer plugin back instead of forgetting it.
//!
//! No allocation, no panic: the record is read after a crash from whatever
//! bits the reset left, so every index is bounded and every length saturates.

/// The longest plugin id kept (the catalogue's ids have at most 48 bytes).
pub const ID_LEN: usize = 48;
/// Nested calls kept apart; deeper ones are counted but share the last slot.
pub const NESTING: usize = 4;

/// The ids of the plugins whose code runs now, outermost first.
///
/// `head` packs the number of open calls (low 8 bits) with a count of every
/// change (the upper 24 bits, wrapping), so that one aligned word says both
/// and a reader that interrupts the writer (the X4's button interrupt) never
/// sees one without the other: a call that began or ended gives another
/// [`token`](Self::token).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Inflight {
    head: u32,
    ids: [[u8; ID_LEN]; NESTING],
}

impl Inflight {
    /// Nothing runs.
    pub const EMPTY: Self = Self {
        head: 0,
        ids: [[0; ID_LEN]; NESTING],
    };

    fn depth_of(head: u32) -> u8 {
        (head & 0xff) as u8
    }

    /// `head` with `depth` open calls and the change counted.
    fn changed(head: u32, depth: u8) -> u32 {
        (head & !0xff).wrapping_add(0x100) | u32::from(depth)
    }

    /// The code of plugin `id` is about to run (a call, nested or not).
    pub fn enter(&mut self, id: &str) {
        let depth = Self::depth_of(self.head);
        if let Some(slot) = self.ids.get_mut(usize::from(depth)) {
            *slot = [0; ID_LEN];
            let len = id.len().min(ID_LEN);
            slot[..len].copy_from_slice(&id.as_bytes()[..len]);
        }
        // One store: the id is in place before the call is counted (the fence
        // keeps the compiler from moving it above the copy).
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::Release);
        self.head = Self::changed(self.head, depth.saturating_add(1));
    }

    /// The innermost call returned (or was refused before it began): the plugin
    /// of the call around it is the running one again.
    pub fn leave(&mut self) {
        let depth = Self::depth_of(self.head);
        if depth == 0 {
            return;
        }
        // One store first, then the slot is cleared: a reader in between sees
        // the call gone and the slot of no call.
        self.head = Self::changed(self.head, depth - 1);
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::Release);
        if let Some(slot) = self.ids.get_mut(usize::from(depth - 1)) {
            *slot = [0; ID_LEN];
        }
    }

    /// How many calls are open.
    #[must_use]
    pub fn depth(&self) -> u8 {
        Self::depth_of(self.head)
    }

    /// Names the call that runs now: `None` when no call is open, otherwise a
    /// number that is another one after every call that begins or ends. The
    /// same number read twice means the same call ran in between (up to a
    /// wrap of 2^24 changes).
    #[must_use]
    pub fn token(&self) -> Option<u32> {
        (self.depth() > 0).then_some(self.head)
    }

    /// [`token`](Self::token) of the record at `this`, read with one volatile
    /// load of the word: for a reader that may interrupt the writer (the X4's
    /// button interrupt), which must not form a reference to a record the
    /// main loop is changing.
    ///
    /// # Safety
    /// `this` points to a valid, aligned `Inflight`.
    #[must_use]
    pub unsafe fn token_at(this: *const Self) -> Option<u32> {
        // SAFETY: the caller's contract; one aligned word.
        let head = unsafe { core::ptr::addr_of!((*this).head).read_volatile() };
        (Self::depth_of(head) > 0).then_some(head)
    }

    /// The plugin whose code runs now (the innermost call kept), if any.
    #[must_use]
    pub fn current(&self) -> Option<&str> {
        let top = usize::from(self.depth()).min(NESTING).checked_sub(1)?;
        let bytes = &self.ids[top];
        let len = bytes.iter().position(|&b| b == 0).unwrap_or(ID_LEN);
        let id = core::str::from_utf8(&bytes[..len]).ok()?;
        (!id.is_empty()).then_some(id)
    }
}

impl Default for Inflight {
    fn default() -> Self {
        Self::EMPTY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_record_runs_nothing() {
        assert_eq!(Inflight::EMPTY.current(), None);
        assert_eq!(Inflight::EMPTY.depth(), 0);
        assert_eq!(Inflight::EMPTY.token(), None);
    }

    #[test]
    fn a_nested_call_returning_gives_the_outer_plugin_back() {
        let mut inflight = Inflight::EMPTY;
        inflight.enter("a");
        inflight.enter("b");
        assert_eq!(inflight.current(), Some("b"));
        inflight.leave();
        assert_eq!(inflight.current(), Some("a"), "A is not forgotten");
        inflight.leave();
        assert_eq!((inflight.depth(), inflight.current()), (0, None));
        assert_eq!(inflight.ids, Inflight::EMPTY.ids, "no id is left behind");
    }

    #[test]
    fn a_refused_nested_call_is_an_enter_and_a_leave_that_changes_nothing_but_the_token() {
        let mut inflight = Inflight::EMPTY;
        inflight.enter("a");
        let before = inflight;
        inflight.enter("b");
        inflight.leave();
        assert_eq!(inflight.ids, before.ids);
        assert_eq!(inflight.current(), Some("a"));
        assert_ne!(inflight.token(), before.token(), "it was another call");
    }

    #[test]
    fn deeper_than_kept_is_counted_and_unwinds_to_the_right_plugin() {
        let mut inflight = Inflight::EMPTY;
        let ids = ["p0", "p1", "p2", "p3", "p4", "p5"];
        for id in ids {
            inflight.enter(id);
        }
        assert_eq!(inflight.depth(), 6);
        assert_eq!(inflight.current(), Some("p3"), "the last kept slot");
        for _ in 0..2 {
            inflight.leave();
        }
        assert_eq!(inflight.current(), Some("p3"));
        inflight.leave();
        assert_eq!(inflight.current(), Some("p2"));
        for _ in 0..3 {
            inflight.leave();
        }
        assert_eq!((inflight.depth(), inflight.current()), (0, None));
    }

    #[test]
    fn leaving_with_nothing_open_stays_empty_and_garbage_depth_is_bounded() {
        let mut inflight = Inflight::EMPTY;
        inflight.leave();
        assert_eq!(inflight, Inflight::EMPTY);
        let mut garbage = Inflight::EMPTY;
        garbage.head = 255;
        assert_eq!(garbage.current(), None, "an unset slot");
        garbage.enter("x");
        assert_eq!(garbage.depth(), 255);
        garbage.leave();
        assert_eq!(garbage.depth(), 254);
    }

    #[test]
    fn a_long_id_is_cut_and_never_overruns() {
        let mut inflight = Inflight::EMPTY;
        let long = "x".repeat(ID_LEN + 10);
        inflight.enter(&long);
        assert_eq!(inflight.current().map(str::len), Some(ID_LEN));
    }

    #[test]
    fn every_call_that_begins_or_ends_gives_another_token() {
        let mut inflight = Inflight::EMPTY;
        inflight.enter("a");
        let a = inflight.token();
        assert!(a.is_some());
        assert_eq!(inflight.token(), a, "unchanged while it runs");
        inflight.enter("b");
        let b = inflight.token();
        assert_ne!(b, a);
        // The nested call returned: the outer plugin runs again, but it is not
        // the same reading as before the nested call began or while it ran.
        inflight.leave();
        assert_ne!(inflight.token(), b);
        assert_ne!(inflight.token(), a);
        inflight.leave();
        assert_eq!(inflight.token(), None);
        // The same plugin calling again is another call.
        inflight.enter("a");
        assert_ne!(inflight.token(), a);
    }

    #[test]
    fn the_token_wraps_without_confusing_depth() {
        let mut inflight = Inflight::EMPTY;
        inflight.head = !0xff;
        inflight.enter("a");
        assert_eq!(inflight.depth(), 1);
        assert_eq!(inflight.current(), Some("a"));
        inflight.leave();
        assert_eq!(inflight.depth(), 0);
    }
}
