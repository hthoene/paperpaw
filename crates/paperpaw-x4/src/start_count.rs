//! Counting the starts of a firmware with native plugins that did not reach
//! their first frame (the test image's boot-loop guard, #2460, Senior's
//! conditions on #2462): after [`FAILED_STARTS`] of them in a row the next one
//! goes without native plugins (safe mode, `shells/xteink/src/safe_mode.rs`).
//!
//! The rules are here, on plain numbers, so that a computer tests them; the
//! firmware keeps the numbers in RTC memory, which a reset keeps and cutting
//! the power clears.

/// Failed starts with native plugins before one goes without them.
pub const FAILED_STARTS: u8 = 2;

/// The counts one start keeps.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Count {
    /// Starts with native plugins that did not reach their first frame.
    pub failed: u8,
    /// Whether this start was counted already (once per start).
    pub counted: bool,
}

/// Why a start goes without native plugins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The key for it was held at power-on.
    KeyHeld,
    /// [`FAILED_STARTS`] starts failed in a row, or a reset came in the
    /// middle of a call into a plugin.
    FailedStarts,
}

impl Count {
    /// At the start: whether this one goes without native plugins. `in_call`:
    /// the last reset came in the middle of a call into a plugin. The caller
    /// clears the count when this says yes (a start without native plugins
    /// counts anew).
    pub fn decide(&mut self, key_held: bool, in_call: bool) -> Option<Refusal> {
        self.counted = false;
        if key_held {
            Some(Refusal::KeyHeld)
        } else if self.failed >= FAILED_STARTS || in_call {
            Some(Refusal::FailedStarts)
        } else {
            None
        }
    }

    /// Native plugins are being loaded or hosted: the first time at a start
    /// counts it as one that may fail. Whether it was counted now.
    pub fn loading(&mut self) -> bool {
        if self.counted {
            return false;
        }
        self.counted = true;
        self.failed = self.failed.saturating_add(1);
        true
    }

    /// Native code cannot be run safely any more in this start (the band was
    /// lost after a sleep): the count says the starts failed, so that the
    /// start that follows the reset goes without native plugins, whether or
    /// not the first frame of this one was shown. Without it a reset would
    /// host them again and a nap could end in a reset each time.
    pub fn lost_the_band(&mut self) {
        self.failed = self.failed.max(FAILED_STARTS);
    }

    /// The first frame is shown: the start went well.
    pub fn started(&mut self) {
        *self = Self::default();
    }
}

/// Starts counted from the very first instruction, before the stack's band,
/// the card or anything that could fail has run: the owner's test image
/// panicked in its first flash read (#2556), before the count above (which
/// begins when native plugins load) knew anything. One word of RTC memory
/// holds it: [`early::next`] at the start, [`early::CLEARED`] at the first
/// frame.
pub mod early {
    use super::FAILED_STARTS;

    /// The word's high half: says the number was written by a start of this
    /// firmware, not left by chance (noise counts as no starts).
    const MAGIC: u32 = 0xEA51_0000;
    /// What the first frame leaves: no start that failed.
    pub const CLEARED: u32 = MAGIC;

    /// How many starts before this one did not reach their first frame, read
    /// from the word the last start left.
    pub fn previous(word: u32) -> u8 {
        if word & 0xFFFF_0000 == MAGIC {
            // Under 0xFF: more is as bad as that many.
            (word & 0xFF) as u8
        } else {
            0
        }
    }

    /// The word this start leaves, counting itself.
    pub fn next(word: u32) -> u32 {
        MAGIC | u32::from(previous(word).saturating_add(1))
    }

    /// The word a start that goes without native plugins because of
    /// `previous` leaves: it counts as the first one anew, so that the next
    /// start tries native again and a loop of early failures stays in the
    /// safe mode (the band is left off while it lasts).
    pub fn anew() -> u32 {
        MAGIC | 1
    }

    /// Whether this start goes without native plugins and without arming the
    /// band (the band's own trap may be what the starts before it died of).
    pub fn refuses(previous: u8) -> bool {
        previous >= FAILED_STARTS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One start that loads native plugins and either reaches its first
    /// frame or does not.
    fn start(count: &mut Count, reaches_first_frame: bool) -> Option<Refusal> {
        if let Some(refusal) = count.decide(false, false) {
            *count = Count::default();
            return Some(refusal);
        }
        count.loading();
        count.loading();
        if reaches_first_frame {
            count.started();
        }
        None
    }

    #[test]
    fn two_failed_starts_in_a_row_make_the_third_go_without_native_plugins() {
        let mut count = Count::default();
        assert_eq!(start(&mut count, false), None);
        assert_eq!(start(&mut count, false), None);
        assert_eq!(start(&mut count, false), Some(Refusal::FailedStarts));
        // The start without native plugins counts anew: native is tried again.
        assert_eq!(start(&mut count, false), None);
    }

    #[test]
    fn a_start_is_counted_once_however_many_plugins_load() {
        let mut count = Count::default();
        assert!(count.loading());
        assert!(!count.loading());
        assert_eq!(count.failed, 1);
    }

    #[test]
    fn a_first_frame_clears_the_count() {
        let mut count = Count::default();
        assert_eq!(start(&mut count, false), None);
        assert_eq!(start(&mut count, true), None);
        assert_eq!(count, Count::default());
        assert_eq!(start(&mut count, false), None);
        assert_eq!(start(&mut count, false), None);
        assert_eq!(start(&mut count, false), Some(Refusal::FailedStarts));
    }

    #[test]
    fn the_key_and_a_reset_inside_a_call_go_without_at_once() {
        let mut count = Count::default();
        assert_eq!(count.decide(true, false), Some(Refusal::KeyHeld));
        assert_eq!(count.decide(false, true), Some(Refusal::FailedStarts));
        assert_eq!(count.decide(false, false), None);
    }

    #[test]
    fn a_lost_band_after_the_first_frame_sends_the_next_start_without_native() {
        let mut count = Count::default();
        // A start that went well (first frame shown), then the band is lost.
        assert_eq!(start(&mut count, true), None);
        count.lost_the_band();
        // The reset that follows: no native plugins, however clean the last
        // start looked.
        assert_eq!(count.decide(false, false), Some(Refusal::FailedStarts));
        // That start counts anew: native is tried again at the one after.
        count = Count::default();
        assert_eq!(start(&mut count, true), None);
    }

    #[test]
    fn the_count_cannot_overflow() {
        let mut count = Count {
            failed: u8::MAX,
            counted: false,
        };
        count.loading();
        assert_eq!(count.failed, u8::MAX);
    }

    #[test]
    fn starts_that_die_before_anything_else_are_counted_from_the_first_word() {
        // Power was cut: noise counts as no failed start.
        assert_eq!(early::previous(0), 0);
        assert_eq!(early::previous(0xFFFF_FFFF), 0);
        assert_eq!(early::previous(early::CLEARED), 0);
        let one = early::next(0);
        assert_eq!(early::previous(one), 1);
        assert!(!early::refuses(early::previous(one)));
        let two = early::next(one);
        assert_eq!(early::previous(two), 2);
        // The third start goes without native plugins and without the band.
        assert!(early::refuses(early::previous(two)));
        // It counts as the first anew, so that the next one does not refuse
        // for the same two, and the one after a refused one does again.
        assert_eq!(early::previous(early::anew()), 1);
        assert!(!early::refuses(early::previous(early::anew())));
        assert!(early::refuses(early::previous(early::next(early::anew()))));
    }

    #[test]
    fn the_early_count_does_not_run_over() {
        let mut word = early::CLEARED;
        for _ in 0..1000 {
            word = early::next(word);
        }
        assert_eq!(early::previous(word), 255);
    }
}
