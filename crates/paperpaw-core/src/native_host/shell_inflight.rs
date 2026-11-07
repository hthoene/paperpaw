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

/// What a watcher reads of the record: see [`Inflight::watch`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Watched {
    /// Another value after every change (a call or a host slot began or
    /// ended).
    pub call: u32,
    /// How many of the open calls have a slot of their own in `entries`
    /// (at most [`NESTING`]).
    pub open: u8,
    /// The innermost call is waiting for a host slot (its code is not running).
    pub host: bool,
    /// For each depth, how many calls have begun at it (wrapping); the
    /// deeper calls than [`NESTING`] count in the last. The call open at a
    /// depth is the same call as long as its count is.
    pub entries: [u32; NESTING],
}

/// The ids of the plugins whose code runs now, outermost first.
///
/// `head` packs, in one word: the number of open calls (bits 0-7), the number
/// of host slots open in the innermost call (bits 8-15) and a count of every
/// change (bits 16-31, wrapping). `entries` counts, for each depth, the calls
/// that began at it: the call open at a depth is the same one for as long as
/// that count is, however many calls begin and end deeper (a plugin that
/// loops on calls into another one is the deepest call that stays open, not
/// the plugin that called it). `head` and each count are stored with one
/// volatile store each and read by the X4's button interrupt with one
/// volatile load each ([`watch_at`](Self::watch_at)), so a reader that
/// interrupts the writer never sees half a word and the two sides do not race
/// in the language's sense (the X4 is a RISC-V C3: an aligned 32-bit access is
/// a single one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Inflight {
    head: u32,
    entries: [u32; NESTING],
    /// Host slots that were open in each outer call when a deeper plugin call
    /// began (the host called back into plugin code).
    outer_host: [u8; NESTING],
    ids: [[u8; ID_LEN]; NESTING],
}

impl Inflight {
    /// Nothing runs.
    pub const EMPTY: Self = Self {
        head: 0,
        entries: [0; NESTING],
        outer_host: [0; NESTING],
        ids: [[0; ID_LEN]; NESTING],
    };

    fn depth_of(head: u32) -> u8 {
        (head & 0xff) as u8
    }

    fn host_of(head: u32) -> u8 {
        ((head >> 8) & 0xff) as u8
    }

    /// `head` with these counts and the change counted.
    fn changed(head: u32, depth: u8, host: u8) -> u32 {
        let changes = (head >> 16).wrapping_add(1) & 0xffff;
        (changes << 16) | (u32::from(host) << 8) | u32::from(depth)
    }

    fn store_head(&mut self, head: u32) {
        // SAFETY: a valid, aligned word of `self`; volatile so that it is one
        // store the interrupt's volatile load matches.
        unsafe { core::ptr::write_volatile(core::ptr::addr_of_mut!(self.head), head) };
    }

    /// Counts a call that begins at depth `depth` (the last slot takes the
    /// deeper ones).
    fn count_entry(&mut self, depth: u8) {
        let slot = usize::from(depth).min(NESTING - 1);
        let next = self.entries[slot].wrapping_add(1);
        // SAFETY: as in `store_head`; `slot` is in bounds.
        unsafe { core::ptr::write_volatile(core::ptr::addr_of_mut!(self.entries[slot]), next) };
    }

    /// The code of plugin `id` is about to run (a call, nested or not).
    pub fn enter(&mut self, id: &str) {
        let (depth, host) = (Self::depth_of(self.head), Self::host_of(self.head));
        if let Some(slot) = self.ids.get_mut(usize::from(depth)) {
            *slot = [0; ID_LEN];
            let len = id.len().min(ID_LEN);
            slot[..len].copy_from_slice(&id.as_bytes()[..len]);
        }
        if let Some(saved) = self.outer_host.get_mut(usize::from(depth)) {
            // Host work open around this call is waited for, not shown by it.
            *saved = host;
        }
        self.count_entry(depth);
        // One store: the id and the count are in place before the call is
        // seen (the fence keeps the compiler from moving them below).
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::Release);
        self.store_head(Self::changed(self.head, depth.saturating_add(1), 0));
    }

    /// The innermost call returned (or was refused before it began): the plugin
    /// of the call around it is the running one again.
    pub fn leave(&mut self) {
        let depth = Self::depth_of(self.head);
        if depth == 0 {
            return;
        }
        // The host work that was open around the call is open again.
        let host = self
            .outer_host
            .get(usize::from(depth - 1))
            .copied()
            .unwrap_or(0);
        // One store first, then the slot is cleared: a reader in between sees
        // the call gone and the slot of no call.
        self.store_head(Self::changed(self.head, depth - 1, host));
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::Release);
        if let Some(slot) = self.ids.get_mut(usize::from(depth - 1)) {
            *slot = [0; ID_LEN];
        }
        if let Some(saved) = self.outer_host.get_mut(usize::from(depth - 1)) {
            *saved = 0;
        }
    }

    /// The running call is waiting for the host: a plugin's call into one of
    /// the host's slots began.
    pub fn host_enter(&mut self) {
        let (depth, host) = (Self::depth_of(self.head), Self::host_of(self.head));
        self.store_head(Self::changed(self.head, depth, host.saturating_add(1)));
    }

    /// The host slot returned.
    pub fn host_leave(&mut self) {
        let (depth, host) = (Self::depth_of(self.head), Self::host_of(self.head));
        self.store_head(Self::changed(self.head, depth, host.saturating_sub(1)));
    }

    /// How many calls are open.
    #[must_use]
    pub fn depth(&self) -> u8 {
        Self::depth_of(self.head)
    }

    /// How many host slots are open in the innermost call.
    #[must_use]
    pub fn host_slots(&self) -> u8 {
        Self::host_of(self.head)
    }

    /// What a watcher reads: `Some(..)` while a call is open, `None`
    /// otherwise (see [`Watched`]).
    #[must_use]
    pub fn watch(&self) -> Option<Watched> {
        Self::watched(self.head, self.entries)
    }

    fn watched(head: u32, entries: [u32; NESTING]) -> Option<Watched> {
        (Self::depth_of(head) > 0).then_some(Watched {
            call: head,
            open: Self::depth_of(head).min(NESTING as u8),
            host: Self::host_of(head) > 0,
            entries,
        })
    }

    /// [`watch`](Self::watch) of the record at `this`, read with one volatile
    /// load of each word: for a reader that may interrupt the writer (the
    /// X4's button interrupt), which must not form a reference to a record the
    /// main loop is changing.
    ///
    /// # Safety
    /// `this` points to a valid, aligned `Inflight`.
    #[must_use]
    pub unsafe fn watch_at(this: *const Self) -> Option<Watched> {
        // SAFETY: the caller's contract; aligned words.
        let head = unsafe { core::ptr::addr_of!((*this).head).read_volatile() };
        let mut entries = [0; NESTING];
        for (slot, entry) in entries.iter_mut().enumerate() {
            // SAFETY: as above; `slot` is in bounds.
            *entry = unsafe { core::ptr::addr_of!((*this).entries[slot]).read_volatile() };
        }
        Self::watched(head, entries)
    }

    /// Keeps only the calls down to depth `depth` recorded (0: the outermost;
    /// the next start blames the plugin of the deepest one kept): for a reset
    /// that found that call stuck while the calls inside it came and went.
    pub fn keep_depth(&mut self, depth: u8) {
        let keep = depth.saturating_add(1);
        if Self::depth_of(self.head) > keep {
            self.store_head(Self::changed(self.head, keep, 0));
        }
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
        assert_eq!(Inflight::EMPTY.watch(), None);
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
        assert_ne!(inflight.watch(), before.watch(), "it was another call");
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
        let a = inflight.watch();
        assert!(a.is_some());
        assert_eq!(inflight.watch(), a, "unchanged while it runs");
        inflight.enter("b");
        let b = inflight.watch();
        assert_ne!(b, a);
        // The nested call returned: the outer plugin runs again, but it is not
        // the same reading as before the nested call began or while it ran.
        inflight.leave();
        assert_ne!(inflight.watch(), b);
        assert_ne!(inflight.watch().map(|w| w.call), a.map(|w| w.call));
        inflight.leave();
        assert_eq!(inflight.watch(), None);
        // The same plugin calling again is another call.
        inflight.enter("a");
        assert_ne!(inflight.watch(), a);
    }

    #[test]
    fn each_depth_counts_the_calls_that_began_at_it() {
        let mut inflight = Inflight::EMPTY;
        inflight.enter("a");
        let a = inflight.watch().unwrap().entries;
        assert_eq!(a, [1, 0, 0, 0]);
        // B calls C over and over: depth 1 is B's one call, depth 2 is each
        // of C's; A's and B's counts do not move.
        inflight.enter("b");
        let b = inflight.watch().unwrap().entries;
        assert_eq!(b, [1, 1, 0, 0]);
        for round in 1..=5 {
            inflight.enter("c");
            inflight.leave();
            let now = inflight.watch().unwrap().entries;
            assert_eq!(now, [1, 1, round, 0], "only the depth of C moves");
        }
        assert_eq!(inflight.current(), Some("b"));
        // A calls B again: B's depth moves, A's does not; a new outermost
        // call moves depth 0.
        inflight.leave();
        inflight.enter("b");
        assert_eq!(inflight.watch().unwrap().entries, [1, 2, 5, 0]);
        inflight.leave();
        inflight.leave();
        inflight.enter("a");
        assert_eq!(inflight.watch().unwrap().entries, [2, 2, 5, 0]);
    }

    #[test]
    fn calls_deeper_than_kept_count_in_the_last_slot_and_open_is_bounded() {
        let mut inflight = Inflight::EMPTY;
        for id in ["p0", "p1", "p2", "p3", "p4", "p5"] {
            inflight.enter(id);
        }
        let w = inflight.watch().unwrap();
        assert_eq!((w.open, usize::from(w.open)), (4, NESTING));
        assert_eq!(w.entries, [1, 1, 1, 3], "p3, p4 and p5 share the last");
        let mut garbage = Inflight::EMPTY;
        garbage.head = 255;
        assert_eq!(garbage.watch().unwrap().open, 4);
    }

    #[test]
    fn host_slots_move_the_head_and_not_the_counts() {
        let mut inflight = Inflight::EMPTY;
        inflight.enter("a");
        inflight.enter("b");
        let before = inflight.watch().unwrap();
        // B keeps making host calls: the head moves, the counts do not.
        for _ in 0..5 {
            inflight.host_enter();
            inflight.host_leave();
        }
        let after = inflight.watch().unwrap();
        assert_ne!(after.call, before.call, "host slots are changes");
        assert_eq!(after.entries, before.entries, "but not plugin calls");
        // A plugin call that begins moves its depth's count; one that ends
        // takes its slot out of `open`.
        inflight.enter("c");
        let in_c = inflight.watch().unwrap();
        assert_eq!((in_c.open, in_c.entries[2]), (3, 1));
        inflight.leave();
        assert_eq!(inflight.watch().unwrap().open, 2);
    }

    #[test]
    fn a_host_slot_is_read_as_the_call_waiting_for_the_host() {
        let mut inflight = Inflight::EMPTY;
        inflight.enter("a");
        let before = inflight.watch().unwrap();
        assert!(!before.host);
        inflight.host_enter();
        let during = inflight.watch().unwrap();
        assert!(during.host, "the host is working");
        assert_ne!(during.call, before.call, "a slot began: another reading");
        assert_eq!(during.entries, before.entries, "the call is the same one");
        assert_eq!(inflight.host_slots(), 1);
        inflight.host_enter();
        inflight.host_leave();
        assert!(
            inflight.watch().unwrap().host,
            "still inside the first slot"
        );
        inflight.host_leave();
        let after = inflight.watch().unwrap();
        assert!(!after.host);
        assert_ne!(
            after.call, before.call,
            "the plugin runs again as another reading"
        );
        assert_eq!(inflight.current(), Some("a"));
        // A host slot with no call open is read as nothing.
        inflight.leave();
        inflight.host_enter();
        assert_eq!(inflight.watch(), None);
        inflight.host_leave();
        assert_eq!(inflight.depth(), 0);
    }

    #[test]
    fn a_plugin_the_host_called_back_into_is_watched_and_the_host_work_around_it_returns() {
        let mut inflight = Inflight::EMPTY;
        inflight.enter("a");
        inflight.host_enter();
        // The host slot calls plugin B (an event, a service callback).
        inflight.enter("b");
        assert_eq!(
            inflight.watch().map(|w| w.host),
            Some(false),
            "B's own code runs"
        );
        assert_eq!(inflight.current(), Some("b"));
        inflight.leave();
        assert_eq!(
            inflight.watch().map(|w| w.host),
            Some(true),
            "back in the host's slot"
        );
        assert_eq!(inflight.host_slots(), 1);
        inflight.host_leave();
        assert_eq!(inflight.watch().map(|w| w.host), Some(false));
    }

    #[test]
    fn keeping_a_depth_blames_the_plugin_of_that_call() {
        let mut inflight = Inflight::EMPTY;
        for id in ["a", "b", "c"] {
            inflight.enter(id);
        }
        assert_eq!(inflight.current(), Some("c"));
        // The deepest call kept is the innermost already: nothing to cut.
        let whole = inflight;
        inflight.keep_depth(2);
        assert_eq!(inflight, whole);
        inflight.keep_depth(1);
        assert_eq!((inflight.current(), inflight.depth()), (Some("b"), 2));
        inflight.keep_depth(0);
        assert_eq!((inflight.current(), inflight.depth()), (Some("a"), 1));
        inflight.keep_depth(0);
        assert_eq!(inflight.depth(), 1);
        // Deeper than kept: the last kept slot is the one blamed.
        let mut deep = Inflight::EMPTY;
        for id in ["p0", "p1", "p2", "p3", "p4", "p5"] {
            deep.enter(id);
        }
        deep.keep_depth(NESTING as u8 - 1);
        assert_eq!((deep.depth(), deep.current()), (4, Some("p3")));
    }

    #[test]
    fn the_token_wraps_without_confusing_depth() {
        let mut inflight = Inflight::EMPTY;
        inflight.head = !0xffff;
        inflight.enter("a");
        assert_eq!(inflight.depth(), 1);
        assert_eq!(inflight.current(), Some("a"));
        inflight.leave();
        assert_eq!(inflight.depth(), 0);
    }
}
