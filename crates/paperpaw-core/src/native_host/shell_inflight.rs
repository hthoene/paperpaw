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
/// `head` packs, in one word: the number of open calls (bits 0-7), the number
/// of host slots open in the innermost call (bits 8-15) and a count of every
/// change (bits 16-31, wrapping). `root` counts the calls that began with no
/// other open (the outermost ones). `calls` counts every plugin call that
/// began or ended, and nothing else (no host slot): it stays the same while
/// one plugin's code keeps making host calls, which `head` does not. All
/// three are stored with one volatile store each and read by the X4's button
/// interrupt with one volatile load each
/// ([`watch_at`](Self::watch_at)), so a reader that interrupts the writer
/// never sees half a word and the two sides do not race in the language's
/// sense (the X4 is a RISC-V C3: an aligned 32-bit access is a single one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Inflight {
    head: u32,
    root: u32,
    calls: u32,
    /// Host slots that were open in each outer call when a deeper plugin call
    /// began (the host called back into plugin code).
    outer_host: [u8; NESTING],
    ids: [[u8; ID_LEN]; NESTING],
}

impl Inflight {
    /// Nothing runs.
    pub const EMPTY: Self = Self {
        head: 0,
        root: 0,
        calls: 0,
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

    fn store_root(&mut self, root: u32) {
        // SAFETY: as in `store_head`.
        unsafe { core::ptr::write_volatile(core::ptr::addr_of_mut!(self.root), root) };
    }

    fn store_calls(&mut self, calls: u32) {
        // SAFETY: as in `store_head`.
        unsafe { core::ptr::write_volatile(core::ptr::addr_of_mut!(self.calls), calls) };
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
        if depth == 0 {
            self.store_root(self.root.wrapping_add(1));
        }
        self.store_calls(self.calls.wrapping_add(1));
        // One store: the id is in place before the call is counted (the fence
        // keeps the compiler from moving it above the copy).
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
        self.store_calls(self.calls.wrapping_add(1));
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

    /// What a watcher reads: `Some((call, calls, outermost, host))` while a
    /// call is open, `None` otherwise. `call` is another value after every
    /// change (a call or a host slot began or ended); `calls` is another
    /// value only when a plugin call began or ended, so it stays the same
    /// while the innermost call keeps making host calls; `outermost` is
    /// another value only when a new outermost call begins; `host` says the
    /// innermost call is waiting for a host slot (its code is not running).
    #[must_use]
    pub fn watch(&self) -> Option<(u32, u32, u32, bool)> {
        Self::watched(self.head, self.calls, self.root)
    }

    fn watched(head: u32, calls: u32, root: u32) -> Option<(u32, u32, u32, bool)> {
        (Self::depth_of(head) > 0).then_some((head, calls, root, Self::host_of(head) > 0))
    }

    /// [`watch`](Self::watch) of the record at `this`, read with one volatile
    /// load of each word: for a reader that may interrupt the writer (the
    /// X4's button interrupt), which must not form a reference to a record the
    /// main loop is changing.
    ///
    /// # Safety
    /// `this` points to a valid, aligned `Inflight`.
    #[must_use]
    pub unsafe fn watch_at(this: *const Self) -> Option<(u32, u32, u32, bool)> {
        // SAFETY: the caller's contract; aligned words.
        let head = unsafe { core::ptr::addr_of!((*this).head).read_volatile() };
        // SAFETY: as above.
        let calls = unsafe { core::ptr::addr_of!((*this).calls).read_volatile() };
        // SAFETY: as above.
        let root = unsafe { core::ptr::addr_of!((*this).root).read_volatile() };
        Self::watched(head, calls, root)
    }

    /// Keeps only the outermost call recorded (the next start blames its
    /// plugin): for a reset that found the outermost call stuck while the
    /// calls inside it came and went.
    pub fn keep_outermost(&mut self) {
        if Self::depth_of(self.head) > 1 {
            self.store_head(Self::changed(self.head, 1, 0));
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
        assert_ne!(inflight.watch().map(|w| w.0), a.map(|w| w.0));
        inflight.leave();
        assert_eq!(inflight.watch(), None);
        // The same plugin calling again is another call.
        inflight.enter("a");
        assert_ne!(inflight.watch(), a);
    }

    #[test]
    fn the_outermost_token_changes_only_with_a_new_outermost_call() {
        let mut inflight = Inflight::EMPTY;
        inflight.enter("a");
        let root = inflight.watch().map(|w| w.2);
        for _ in 0..5 {
            inflight.enter("b");
            inflight.leave();
        }
        assert_eq!(inflight.watch().map(|w| w.2), root, "nested calls under A");
        inflight.leave();
        inflight.enter("a");
        assert_ne!(inflight.watch().map(|w| w.2), root, "A called again");
    }

    #[test]
    fn the_plugin_call_count_changes_with_calls_and_not_with_host_slots() {
        let mut inflight = Inflight::EMPTY;
        inflight.enter("a");
        inflight.enter("b");
        let (call, calls, ..) = inflight.watch().unwrap();
        // B keeps making host calls: the head moves, the call count does not.
        for _ in 0..5 {
            inflight.host_enter();
            inflight.host_leave();
        }
        let (call_after, calls_after, ..) = inflight.watch().unwrap();
        assert_ne!(call_after, call, "host slots are changes");
        assert_eq!(calls_after, calls, "but not plugin calls");
        // A plugin call that begins or ends moves it, a refused one too.
        inflight.enter("c");
        assert_ne!(inflight.watch().unwrap().1, calls);
        let in_c = inflight.watch().unwrap().1;
        inflight.leave();
        assert_ne!(inflight.watch().unwrap().1, in_c);
    }

    #[test]
    fn a_host_slot_is_read_as_the_call_waiting_for_the_host() {
        let mut inflight = Inflight::EMPTY;
        inflight.enter("a");
        let (before, _, root, host) = inflight.watch().unwrap();
        assert!(!host);
        inflight.host_enter();
        let (during, _, same_root, host) = inflight.watch().unwrap();
        assert!(host, "the host is working");
        assert_ne!(during, before, "a slot began: another reading");
        assert_eq!(same_root, root, "the outermost call is the same one");
        assert_eq!(inflight.host_slots(), 1);
        inflight.host_enter();
        inflight.host_leave();
        assert!(inflight.watch().unwrap().3, "still inside the first slot");
        inflight.host_leave();
        let (after, _, _, host) = inflight.watch().unwrap();
        assert!(!host);
        assert_ne!(after, before, "the plugin runs again as another reading");
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
            inflight.watch().map(|w| w.3),
            Some(false),
            "B's own code runs"
        );
        assert_eq!(inflight.current(), Some("b"));
        inflight.leave();
        assert_eq!(
            inflight.watch().map(|w| w.3),
            Some(true),
            "back in the host's slot"
        );
        assert_eq!(inflight.host_slots(), 1);
        inflight.host_leave();
        assert_eq!(inflight.watch().map(|w| w.3), Some(false));
    }

    #[test]
    fn keeping_the_outermost_call_blames_its_plugin() {
        let mut inflight = Inflight::EMPTY;
        inflight.enter("a");
        inflight.enter("b");
        assert_eq!(inflight.current(), Some("b"));
        inflight.keep_outermost();
        assert_eq!(inflight.current(), Some("a"));
        assert_eq!(inflight.depth(), 1);
        inflight.keep_outermost();
        assert_eq!(inflight.depth(), 1);
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
