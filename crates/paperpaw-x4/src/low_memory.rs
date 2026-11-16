//! The one-time low-memory notice (#248, Senior's recommendation for Beta 4:
//! no hard limit): when the X4's largest free block is under
//! [`LARGEST_BLOCK_MIN`] with store plugins loaded, the user is told once
//! that memory is short and where to switch a plugin off. This is the rule,
//! on plain numbers so that a computer tests it; the firmware
//! (`shells/xteink/src/firmware.rs`) takes the heap's reading, asks the
//! kernel to show the notice (only over the home screen) and keeps the set
//! it was shown for on the card, so that a restart does not show it again.
//!
//! "Once per plugin set": the set is the plugins running (a fingerprint of
//! their ids, [`fingerprint`]); the notice comes again only where the set
//! differs from the one it was last shown for.

/// The largest free block under which the memory counts as short: about what
/// a book's first page and a cover's decode ask for together on the X4.
pub const LARGEST_BLOCK_MIN: usize = 40 * 1024;

/// A fingerprint of the ids of the plugins running, independent of their
/// order (FNV-1a of each id, summed).
pub fn fingerprint<'a>(ids: impl IntoIterator<Item = &'a str>) -> u32 {
    ids.into_iter().fold(0u32, |sum, id| {
        let hash = id.bytes().fold(0x811c_9dc5u32, |h, b| {
            (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
        });
        sum.wrapping_add(hash)
    })
}

/// What the card keeps: the fingerprint last shown, as eight hex digits.
pub fn parse(text: &str) -> Option<u32> {
    u32::from_str_radix(text.trim(), 16).ok()
}

/// The text kept for `set` (see [`parse`]).
pub fn render(set: u32) -> [u8; 8] {
    let mut out = [b'0'; 8];
    for (i, digit) in out.iter_mut().enumerate() {
        let nibble = (set >> (28 - 4 * i)) & 0xf;
        *digit = b"0123456789abcdef"[nibble as usize];
    }
    out
}

/// Where the notice stands: four bytes of state each way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Notice {
    /// The set it was last shown for (what the card keeps).
    shown: Option<u32>,
    /// The set the heap was last looked at for and found roomy.
    roomy: Option<u32>,
    /// The set the notice is due for, until the kernel shows it.
    due: Option<u32>,
}

impl Notice {
    /// With what the card kept (`None` where it kept nothing).
    pub const fn new(shown: Option<u32>) -> Self {
        Self {
            shown,
            roomy: None,
            due: None,
        }
    }

    /// Whether the heap is to be looked at now, which is dear (a search for
    /// the largest block): store plugins are loaded, and this set was neither
    /// shown nor found roomy, nor is the notice already due.
    pub fn wants_look(&self, store_plugins: bool, set: u32) -> bool {
        store_plugins && self.shown != Some(set) && self.roomy != Some(set) && self.due != Some(set)
    }

    /// The heap was looked at: the largest block is `largest`. From now on
    /// [`Notice::due`] says whether to show it.
    pub fn looked(&mut self, set: u32, largest: usize) {
        if largest < LARGEST_BLOCK_MIN {
            self.due = Some(set);
        } else {
            self.roomy = Some(set);
        }
    }

    /// The set to show the notice for, where it is due.
    pub fn due(&self) -> Option<u32> {
        self.due.filter(|set| self.shown != Some(*set))
    }

    /// The kernel showed it: kept as shown, no longer due.
    pub fn showed(&mut self, set: u32) -> u32 {
        self.shown = Some(set);
        self.due = None;
        set
    }

    /// The plugins changed while it was due: it is due for the new set only
    /// after a new look.
    pub fn set_changed(&mut self, set: u32) {
        if self.due.is_some_and(|due| due != set) {
            self.due = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: u32 = 11;
    const B: u32 = 22;

    /// One pass of the firmware's rule: the notice is shown (`true`) or not.
    fn pass(notice: &mut Notice, store: bool, set: u32, largest: usize) -> bool {
        notice.set_changed(set);
        if notice.wants_look(store, set) {
            notice.looked(set, largest);
        }
        match notice.due() {
            Some(due) => {
                notice.showed(due);
                true
            }
            None => false,
        }
    }

    #[test]
    fn under_the_threshold_with_store_plugins_it_shows_once() {
        let mut notice = Notice::new(None);
        assert!(pass(&mut notice, true, A, LARGEST_BLOCK_MIN - 1));
        assert!(!pass(&mut notice, true, A, LARGEST_BLOCK_MIN - 1));
        assert!(!pass(&mut notice, true, A, 1_000));
    }

    #[test]
    fn a_changed_set_shows_it_again() {
        let mut notice = Notice::new(None);
        assert!(pass(&mut notice, true, A, 10_000));
        assert!(pass(&mut notice, true, B, 10_000));
        assert!(!pass(&mut notice, true, B, 10_000));
    }

    #[test]
    fn at_or_above_the_threshold_or_without_store_plugins_it_does_not_show() {
        let mut notice = Notice::new(None);
        assert!(!pass(&mut notice, true, A, LARGEST_BLOCK_MIN));
        assert!(!pass(&mut notice, false, B, 1_000));
    }

    #[test]
    fn what_the_card_kept_is_not_shown_again_after_a_restart() {
        let mut notice = Notice::new(parse(core::str::from_utf8(&render(A)).unwrap()));
        assert!(!pass(&mut notice, true, A, 1_000));
        assert!(pass(&mut notice, true, B, 1_000));
    }

    #[test]
    fn a_set_changed_before_the_kernel_showed_it_is_looked_at_anew() {
        let mut notice = Notice::new(None);
        notice.looked(A, 1_000);
        notice.set_changed(B);
        assert_eq!(notice.due(), None);
        assert!(notice.wants_look(true, B));
    }

    #[test]
    fn the_fingerprint_follows_the_set_and_not_its_order() {
        assert_eq!(fingerprint(["a.b", "c.d"]), fingerprint(["c.d", "a.b"]));
        assert_ne!(fingerprint(["a.b", "c.d"]), fingerprint(["a.b"]));
        assert_eq!(
            parse(core::str::from_utf8(&render(0xdead_beef)).unwrap()),
            Some(0xdead_beef)
        );
    }
}
