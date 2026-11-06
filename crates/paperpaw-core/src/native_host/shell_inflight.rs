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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Inflight {
    depth: u8,
    ids: [[u8; ID_LEN]; NESTING],
}

impl Inflight {
    /// Nothing runs.
    pub const EMPTY: Self = Self {
        depth: 0,
        ids: [[0; ID_LEN]; NESTING],
    };

    /// The code of plugin `id` is about to run (a call, nested or not).
    pub fn enter(&mut self, id: &str) {
        if let Some(slot) = self.ids.get_mut(usize::from(self.depth)) {
            *slot = [0; ID_LEN];
            let len = id.len().min(ID_LEN);
            slot[..len].copy_from_slice(&id.as_bytes()[..len]);
        }
        self.depth = self.depth.saturating_add(1);
    }

    /// The innermost call returned (or was refused before it began): the plugin
    /// of the call around it is the running one again.
    pub fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
        if let Some(slot) = self.ids.get_mut(usize::from(self.depth)) {
            *slot = [0; ID_LEN];
        }
    }

    /// How many calls are open.
    #[must_use]
    pub fn depth(&self) -> u8 {
        self.depth
    }

    /// The plugin whose code runs now (the innermost call kept), if any.
    #[must_use]
    pub fn current(&self) -> Option<&str> {
        let top = usize::from(self.depth).min(NESTING).checked_sub(1)?;
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
        assert_eq!(inflight, Inflight::EMPTY);
    }

    #[test]
    fn a_refused_nested_call_is_an_enter_and_a_leave_that_changes_nothing() {
        let mut inflight = Inflight::EMPTY;
        inflight.enter("a");
        let before = inflight;
        inflight.enter("b");
        inflight.leave();
        assert_eq!(inflight, before);
        assert_eq!(inflight.current(), Some("a"));
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
        assert_eq!(inflight, Inflight::EMPTY);
    }

    #[test]
    fn leaving_with_nothing_open_stays_empty_and_garbage_depth_is_bounded() {
        let mut inflight = Inflight::EMPTY;
        inflight.leave();
        assert_eq!(inflight, Inflight::EMPTY);
        let mut garbage = Inflight::EMPTY;
        garbage.depth = 255;
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
}
