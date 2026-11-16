//! The one-time low-memory notice (#248, Senior's recommendation for Beta 4:
//! no hard limit): when the X4's largest free block is under
//! [`LARGEST_BLOCK_MIN`] with store plugins loaded, the user is told once
//! that memory is short and where to switch a plugin off. The heap is looked
//! at in one state only ([`Facts::ready`]): Home on top, no radio on, the
//! start settled, store plugins running; before that nothing is looked at and
//! nothing remembered. This is the rule,
//! on plain numbers so that a computer tests it; the firmware
//! (`shells/xteink/src/firmware.rs`) takes the heap's reading, asks the
//! kernel to show the notice (only over the home screen) and keeps the set
//! it was shown for on the card, so that a restart does not show it again.
//!
//! "Once per plugin set": the set is the plugins running (a fingerprint of
//! their ids, [`fingerprint`]); the notice comes again only where the set
//! differs from the one it was last shown for.

/// The largest free block under which the memory counts as short, on a
/// settled Home with no radio on. An ESTIMATE from single device runs (#248,
/// before a book's picture: 65.5 / 60.7 / 46.9 / 42.4 KB with 0 / 1 / 3 / 5
/// store plugins), set between one plugin and three; the owner's later device
/// run with this build's log line ("low memory: largest block ...") settles it.
pub const LARGEST_BLOCK_MIN: usize = 50 * 1024;

/// The start has settled this long after it (covers, indexes and the plugins'
/// first work done): the heap's reading means something from here on.
pub const SETTLED_MS: u64 = 15_000;

/// What the firmware knows when it asks.
#[derive(Clone, Copy, Debug)]
pub struct Facts {
    /// Plugins from the store (card code) are running.
    pub store_plugins: bool,
    /// The set of plugins running ([`fingerprint`]).
    pub set: u32,
    /// The home screen is on top, with nothing over it.
    pub on_home: bool,
    /// Wi-Fi or Bluetooth is on (a store page, a sync: the heap is not Home's).
    pub radio_on: bool,
    /// Milliseconds since the start.
    pub up_ms: u64,
    /// The device is going to sleep, off or restart, or the call showed grays.
    pub busy: bool,
}

impl Facts {
    /// The one state the heap is looked at and the notice shown in.
    pub fn ready(&self) -> bool {
        self.store_plugins
            && self.on_home
            && !self.radio_on
            && !self.busy
            && self.up_ms >= SETTLED_MS
    }
}

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

/// Where the notice stands: a few bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Notice {
    /// The card's note was read (only where store plugins run).
    loaded: bool,
    /// The set it was last shown for (what the card keeps).
    shown: Option<u32>,
    /// The set found roomy in the ready state.
    roomy: Option<u32>,
    /// The set the notice is due for, until the kernel shows it.
    due: Option<u32>,
}

impl Notice {
    /// Whether the card's note is to be read now: store plugins run and it
    /// was not read yet (nothing is read where none runs).
    pub fn needs_card(&self, store_plugins: bool) -> bool {
        store_plugins && !self.loaded
    }

    /// What the card kept (`None` where it kept nothing).
    pub fn kept(&mut self, shown: Option<u32>) {
        self.loaded = true;
        self.shown = shown;
    }

    /// The rule: the set to show the notice for, if it is to be shown now.
    /// Not ready: `None`, nothing looked at, nothing remembered. Ready: the
    /// heap is looked at (`look` gives the largest block) once for each set;
    /// a roomy block is remembered for the set, a short one makes the notice
    /// due until [`Notice::showed`].
    pub fn step(&mut self, facts: &Facts, look: impl FnOnce() -> usize) -> Option<u32> {
        if !facts.ready() || !self.loaded {
            return None;
        }
        let set = facts.set;
        if self.due.is_some_and(|due| due != set) {
            self.due = None;
        }
        if self.shown == Some(set) || self.roomy == Some(set) {
            return None;
        }
        if self.due != Some(set) {
            if look() >= LARGEST_BLOCK_MIN {
                self.roomy = Some(set);
                return None;
            }
            self.due = Some(set);
        }
        Some(set)
    }

    /// The kernel showed it: kept as shown, no longer due.
    pub fn showed(&mut self, set: u32) {
        self.shown = Some(set);
        self.due = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: u32 = 11;
    const B: u32 = 22;
    const READY: Facts = Facts {
        store_plugins: true,
        set: A,
        on_home: true,
        radio_on: false,
        up_ms: SETTLED_MS,
        busy: false,
    };

    fn loaded() -> Notice {
        let mut notice = Notice::default();
        notice.kept(None);
        notice
    }

    /// One pass of the firmware's rule: shown (`true`) or not; the kernel
    /// shows it where asked.
    fn pass(notice: &mut Notice, facts: Facts, largest: usize) -> bool {
        match notice.step(&facts, || largest) {
            Some(set) => {
                notice.showed(set);
                true
            }
            None => false,
        }
    }

    #[test]
    fn under_the_limit_in_the_ready_state_it_shows_once() {
        let mut notice = loaded();
        assert!(pass(&mut notice, READY, LARGEST_BLOCK_MIN - 1));
        assert!(!pass(&mut notice, READY, 1_000));
    }

    #[test]
    fn at_the_limit_the_set_is_roomy_and_looked_at_once() {
        let mut notice = loaded();
        assert!(!pass(&mut notice, READY, LARGEST_BLOCK_MIN));
        assert_eq!(
            notice.step(&READY, || unreachable!("looked at twice")),
            None
        );
    }

    #[test]
    fn a_changed_set_shows_it_again() {
        let mut notice = loaded();
        assert!(pass(&mut notice, READY, 10_000));
        assert!(pass(&mut notice, Facts { set: B, ..READY }, 10_000));
        assert!(!pass(&mut notice, Facts { set: B, ..READY }, 10_000));
    }

    #[test]
    fn outside_the_ready_state_nothing_is_looked_at_or_remembered() {
        for not_ready in [
            Facts {
                store_plugins: false,
                ..READY
            },
            Facts {
                on_home: false,
                ..READY
            },
            Facts {
                radio_on: true,
                ..READY
            },
            Facts {
                busy: true,
                ..READY
            },
            Facts {
                up_ms: SETTLED_MS - 1,
                ..READY
            },
        ] {
            let mut notice = loaded();
            assert_eq!(notice.step(&not_ready, || unreachable!("looked at")), None);
            // Later, ready: it looks now (a short block shows).
            assert!(pass(&mut notice, READY, 1_000));
        }
    }

    #[test]
    fn the_card_is_read_once_and_only_where_store_plugins_run() {
        let mut notice = Notice::default();
        assert!(!notice.needs_card(false));
        assert!(notice.needs_card(true));
        // Not asked before the note is read.
        assert_eq!(notice.step(&READY, || unreachable!("looked at")), None);
        notice.kept(Some(A));
        assert!(!notice.needs_card(true));
        assert!(
            !pass(&mut notice, READY, 1_000),
            "kept: not shown again after a restart"
        );
        assert!(pass(&mut notice, Facts { set: B, ..READY }, 1_000));
    }

    #[test]
    fn a_notice_the_kernel_did_not_show_stays_due_without_a_second_look() {
        let mut notice = loaded();
        assert_eq!(notice.step(&READY, || 1_000), Some(A));
        assert_eq!(
            notice.step(&READY, || unreachable!("looked at twice")),
            Some(A)
        );
        // The set changes meanwhile: due for the new set after its own look.
        assert_eq!(notice.step(&Facts { set: B, ..READY }, || 2_000), Some(B));
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
