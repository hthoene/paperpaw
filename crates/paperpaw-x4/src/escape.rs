//! The Power key's escape (G12): holding Power for [`HOLD_MS`] gets the
//! reader out of whatever the device is doing, also when the main loop or a
//! plugin's callback has stopped coming by. No power mode, `controls` bit,
//! guard, filter, key map, confirmation or plugin can stop it: it is timed
//! from the readings of the button's own timer interrupt
//! (`shells/xteink/src/input.rs`, every 5 ms), never from the main loop.
//!
//! Two steps, so that a device that still answers goes the clean way:
//!
//! 1. At [`HOLD_MS`] the readings ask the main loop to switch off (the
//!    firmware's own path, [`Step::Ask`]): it saves what it keeps, puts the
//!    panel to sleep and lets the power go.
//! 2. If the main loop has not taken the request [`GRACE_MS`] later (it hangs,
//!    or a plugin does), or took it but the switch-off has not begun
//!    [`CLEAN_MS`] after the request (a plugin hangs in `core.suspend`), the
//!    readings reset the processor ([`Step::Force`]).
//!    RTC memory survives the reset, so the next start knows which plugin's
//!    call was running (`safe_mode`) and switches it off.
//!
//! A button held from before the start, or from before a light sleep ended,
//! counts only after it was let go once (the caller's `armed`).
//!
//! Not stopped by this: a hang with interrupts masked (a critical section
//! that never ends) and a flash erase or write in progress, which masks them
//! for its own duration (some hundreds of milliseconds for an erase block):
//! the escape comes at the end of it, or the watchdogs' reset does.

/// How long Power is held for the escape.
pub const HOLD_MS: u64 = 8_000;
/// How long the main loop has to take the request before the processor is
/// reset.
pub const GRACE_MS: u64 = 1_500;
/// How long the clean way out may take once the main loop took the request
/// (the plugins hear `core.suspend`, the work is finished, the state
/// written): if the switch-off has not begun by then (`enabled` false), the
/// processor is reset all the same.
pub const CLEAN_MS: u64 = 6_000;

/// What a reading makes of the hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Nothing (yet, or any more).
    Idle,
    /// The hold is long enough: ask the main loop to switch off. Said once
    /// per hold.
    Ask,
    /// The main loop did not take the request in time: reset now.
    Force,
}

/// The hold of Power, fed by the readings.
#[derive(Debug, Clone, Copy, Default)]
pub struct Escape {
    /// When the current hold began (the reading it was first down).
    since: Option<u64>,
    /// When the request was made, if it was.
    asked: Option<u64>,
}

impl Escape {
    /// No hold.
    #[must_use]
    pub const fn new() -> Self {
        Escape {
            since: None,
            asked: None,
        }
    }

    /// One reading at `now` (ms, increasing): `down`, whether Power is held
    /// and counts (read down, and let go once since the start); `enabled`,
    /// false while the device is on its way to switch off by itself (the
    /// escape would only start it again); `taken`, whether the main loop
    /// took the request.
    pub fn update(&mut self, now: u64, down: bool, enabled: bool, taken: bool) -> Step {
        if !down || !enabled {
            *self = Escape::new();
            return Step::Idle;
        }
        let since = *self.since.get_or_insert(now);
        match self.asked {
            None if now.saturating_sub(since) >= HOLD_MS => {
                self.asked = Some(now);
                Step::Ask
            }
            Some(asked) if now.saturating_sub(asked) >= if taken { CLEAN_MS } else { GRACE_MS } => {
                Step::Force
            }
            _ => Step::Idle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds one reading every 5 ms from `from` to `to` (exclusive), the
    /// button down, and returns the first step that is not idle with its time.
    fn hold(escape: &mut Escape, from: u64, to: u64, taken: bool) -> Option<(u64, Step)> {
        (from..to)
            .step_by(5)
            .map(|now| (now, escape.update(now, true, true, taken)))
            .find(|(_, step)| *step != Step::Idle)
    }

    #[test]
    fn eight_seconds_of_hold_ask_once_and_not_before() {
        let mut escape = Escape::new();
        assert_eq!(hold(&mut escape, 1_000, 8_995, true), None);
        assert_eq!(escape.update(9_000, true, true, true), Step::Ask);
        // Said once per hold, whatever follows.
        assert_eq!(escape.update(9_005, true, true, true), Step::Idle);
        assert_eq!(hold(&mut escape, 9_010, 9_000 + CLEAN_MS, true), None);
    }

    #[test]
    fn a_main_loop_that_does_not_answer_is_reset_after_the_grace() {
        let mut escape = Escape::new();
        let (asked, step) = hold(&mut escape, 0, 20_000, false).unwrap();
        assert_eq!((asked, step), (HOLD_MS, Step::Ask));
        let forced = hold(&mut escape, asked + 5, 20_000, false).unwrap();
        assert_eq!(forced, (asked + GRACE_MS, Step::Force));
        // And stays forced until the reset happens.
        assert_eq!(escape.update(forced.0 + 5, true, true, false), Step::Force);
    }

    #[test]
    fn a_request_the_main_loop_took_is_not_forced_while_it_works_but_after_the_clean_time() {
        let mut escape = Escape::new();
        let (asked, _) = hold(&mut escape, 0, 20_000, false).unwrap();
        // Taken: the clean way has CLEAN_MS from the request.
        assert_eq!(hold(&mut escape, asked + 5, asked + CLEAN_MS, true), None);
        assert_eq!(
            hold(&mut escape, asked + CLEAN_MS, 30_000, true),
            Some((asked + CLEAN_MS, Step::Force))
        );
        // The switch-off began (the escape disabled): nothing is forced.
        let mut escape = Escape::new();
        let (asked, _) = hold(&mut escape, 0, 20_000, false).unwrap();
        assert_eq!(escape.update(asked + 5, true, false, true), Step::Idle);
        assert_eq!(escape.update(asked + 20_000, true, false, true), Step::Idle);
    }

    #[test]
    fn letting_go_starts_the_count_anew() {
        let mut escape = Escape::new();
        assert_eq!(hold(&mut escape, 0, 7_000, false), None);
        assert_eq!(escape.update(7_000, false, true, false), Step::Idle);
        // 7 s again does not add to the 7 s before.
        assert_eq!(hold(&mut escape, 7_005, 14_000, false), None);
        let (at, step) = hold(&mut escape, 14_000, 30_000, false).unwrap();
        assert_eq!((at, step), (7_005 + HOLD_MS, Step::Ask));
    }

    #[test]
    fn a_hold_that_is_let_go_after_the_request_is_over() {
        let mut escape = Escape::new();
        hold(&mut escape, 0, 20_000, false).unwrap();
        assert_eq!(escape.update(9_100, false, true, false), Step::Idle);
        // A new hold asks again after its own 8 s.
        assert_eq!(escape.update(9_105, true, true, false), Step::Idle);
        assert_eq!(escape.update(9_105 + HOLD_MS, true, true, false), Step::Ask);
    }

    #[test]
    fn nothing_happens_while_the_device_switches_off_by_itself() {
        let mut escape = Escape::new();
        for now in (0..30_000).step_by(5) {
            assert_eq!(escape.update(now, true, false, false), Step::Idle);
        }
        // And a hold that began before is dropped when it is disabled.
        let mut escape = Escape::new();
        assert_eq!(hold(&mut escape, 0, 7_000, false), None);
        assert_eq!(escape.update(7_000, true, false, false), Step::Idle);
        assert_eq!(escape.update(15_000, true, true, false), Step::Idle);
    }

    #[test]
    fn a_time_that_does_not_advance_cannot_ask() {
        let mut escape = Escape::new();
        for _ in 0..100 {
            assert_eq!(escape.update(5_000, true, true, false), Step::Idle);
        }
        // A clock that steps back (it must not) neither asks nor panics.
        assert_eq!(escape.update(1_000, true, true, false), Step::Idle);
    }
}
