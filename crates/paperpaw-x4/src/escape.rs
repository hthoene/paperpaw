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
//!    readings reset the processor ([`Step::Force`]). A request the main
//!    loop has not taken stands when the key is let go; one it took is
//!    watched only while the key is down.
//!    The reset leaves its place in RTC memory (`power::at::ESCAPE`) and,
//!    when [`CallWatch`] says a plugin's call has run past the grace, that
//!    call's plugin (the interface-7 loader's record of calls,
//!    `safe_mode`): the next start switches it off. A call that
//!    began after the grace started is not blamed (a long host step before
//!    it, a call that has only just begun).
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

/// Whom a reset by the escape blames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blame {
    /// No plugin: no plugin's own code has run for the whole grace (the host
    /// was working, or calls came and went).
    Nobody,
    /// The innermost plugin call: it has run unchanged for the whole grace.
    Innermost,
    /// The outermost plugin call: it has run for the whole grace, but the
    /// calls inside it came and went (a loop of nested calls).
    Outermost,
}

/// Which plugin call has been running, as the readings see it: the running
/// call's token and the outermost call's (each another value whenever a call
/// begins or ends, or only when a new outermost call begins), and since when
/// each is the same. Nothing is watched while the host works for a plugin (a
/// slot of the host is open: a slow card write, a large erase), so a plugin is
/// never blamed for waiting on the host. A reset by the escape blames a plugin
/// only if its call has run for the whole grace ([`GRACE_MS`]) with plugin
/// code running all that time, which a call that began after the 8 s mark
/// cannot have.
#[derive(Debug, Clone, Copy, Default)]
pub struct CallWatch {
    call: Option<u32>,
    call_since: u64,
    outer: Option<u32>,
    outer_since: u64,
}

impl CallWatch {
    /// No call seen.
    #[must_use]
    pub const fn new() -> Self {
        CallWatch {
            call: None,
            call_since: 0,
            outer: None,
            outer_since: 0,
        }
    }

    /// One reading at `now` (ms, increasing): `running` is the record of
    /// calls' `(call, outermost)` while plugin code runs, `None` while no call
    /// is open or the host works.
    pub fn observe(&mut self, now: u64, running: Option<(u32, u32)>) {
        let (call, outer) = match running {
            Some((call, outer)) => (Some(call), Some(outer)),
            None => (None, None),
        };
        if call != self.call {
            self.call = call;
            self.call_since = now;
        }
        if outer != self.outer {
            self.outer = outer;
            self.outer_since = now;
        }
    }

    /// Whom a reset at `now` blames.
    #[must_use]
    pub fn blame(&self, now: u64) -> Blame {
        let long = |since: u64| now.saturating_sub(since) >= GRACE_MS;
        if self.call.is_some() && long(self.call_since) {
            Blame::Innermost
        } else if self.outer.is_some() && long(self.outer_since) {
            Blame::Outermost
        } else {
            Blame::Nobody
        }
    }
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
        if !enabled {
            *self = Escape::new();
            return Step::Idle;
        }
        match self.asked {
            // A request the main loop has not taken stands whether the key is
            // still down or not: a user who lets go when nothing happens
            // must not cancel the way out of a hung main loop.
            Some(asked) if !taken => {
                if now.saturating_sub(asked) >= GRACE_MS {
                    Step::Force
                } else {
                    Step::Idle
                }
            }
            // Taken: the main loop is on its clean way out and does not come
            // back (it ends in the power-off or a reset). The key is the
            // user's watch over it: while it is down the clean way has
            // [`CLEAN_MS`]; let go, the watchdogs are its guard, and a new
            // hold counts from its start.
            Some(_) if !down => {
                *self = Escape::new();
                Step::Idle
            }
            Some(asked) => {
                if now.saturating_sub(asked) >= CLEAN_MS {
                    Step::Force
                } else {
                    Step::Idle
                }
            }
            None if !down => {
                *self = Escape::new();
                Step::Idle
            }
            None => {
                let since = *self.since.get_or_insert(now);
                if now.saturating_sub(since) >= HOLD_MS {
                    self.asked = Some(now);
                    Step::Ask
                } else {
                    Step::Idle
                }
            }
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
    fn a_request_the_main_loop_has_not_taken_stands_when_the_key_is_let_go() {
        let mut escape = Escape::new();
        let (asked, step) = hold(&mut escape, 0, 20_000, false).unwrap();
        assert_eq!((asked, step), (HOLD_MS, Step::Ask));
        // Let go at once: a hung main loop is still reset after the grace.
        assert_eq!(escape.update(asked + 100, false, true, false), Step::Idle);
        assert_eq!(
            escape.update(asked + GRACE_MS, false, true, false),
            Step::Force
        );
        // Taken in time, the request is the main loop's; no reset follows.
        let mut escape = Escape::new();
        let (asked, _) = hold(&mut escape, 0, 20_000, false).unwrap();
        assert_eq!(escape.update(asked + 100, false, true, true), Step::Idle);
        assert_eq!(escape.update(asked + 20_000, false, true, true), Step::Idle);
    }

    #[test]
    fn a_taken_request_is_watched_only_while_the_key_is_down() {
        let mut escape = Escape::new();
        let (asked, _) = hold(&mut escape, 0, 20_000, false).unwrap();
        assert_eq!(escape.update(asked + 100, true, true, true), Step::Idle);
        // Let go: the clean way goes on, the watchdogs guard it.
        assert_eq!(escape.update(asked + 200, false, true, true), Step::Idle);
        assert_eq!(
            escape.update(asked + CLEAN_MS + 50, true, true, true),
            Step::Idle
        );
        // A new hold counts from its own start, and asks again after 8 s.
        let from = asked + CLEAN_MS + 50;
        assert_eq!(escape.update(from + HOLD_MS, true, true, true), Step::Ask);
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

    /// Feeds readings every 5 ms in `[from, to)` of the running call
    /// `(call, outermost)`.
    fn watch(call: &mut CallWatch, from: u64, to: u64, running: Option<(u32, u32)>) {
        for now in (from..to).step_by(5) {
            call.observe(now, running);
        }
    }

    #[test]
    fn a_call_that_was_running_at_the_mark_is_blamed_at_the_reset() {
        let mut call = CallWatch::new();
        // The call began shortly before the 8 s mark and never returned.
        watch(&mut call, 7_000, HOLD_MS + GRACE_MS, Some((7, 1)));
        let forced = HOLD_MS + GRACE_MS;
        call.observe(forced, Some((7, 1)));
        assert_eq!(call.blame(forced), Blame::Innermost);
    }

    #[test]
    fn a_call_that_began_after_the_mark_is_not_blamed() {
        let mut call = CallWatch::new();
        // A long host step (no plugin call) up to just after the mark; then a
        // call begins and is the one running when the reset comes.
        watch(&mut call, 7_000, HOLD_MS + 10, None);
        watch(&mut call, HOLD_MS + 10, HOLD_MS + GRACE_MS, Some((9, 2)));
        let forced = HOLD_MS + GRACE_MS;
        call.observe(forced, Some((9, 2)));
        assert_eq!(call.blame(forced), Blame::Nobody);
    }

    #[test]
    fn a_plugin_inside_a_slow_host_write_is_not_blamed() {
        let mut call = CallWatch::new();
        // The plugin's call began before the mark; its card write began just
        // before it and runs through the reset: nothing is watched meanwhile.
        watch(&mut call, 7_000, 7_900, Some((7, 1)));
        watch(&mut call, 7_900, HOLD_MS + GRACE_MS, None);
        let forced = HOLD_MS + GRACE_MS;
        call.observe(forced, None);
        assert_eq!(call.blame(forced), Blame::Nobody);
        // The write returned just before the reset: the plugin runs again as
        // another reading, which has not run for the grace.
        call.observe(forced + 5, Some((9, 1)));
        assert_eq!(call.blame(forced + 5), Blame::Nobody);
    }

    #[test]
    fn a_nested_or_another_call_starts_the_count_anew() {
        let mut call = CallWatch::new();
        watch(&mut call, 7_000, 9_000, Some((1, 1)));
        // The first call ends and another begins (even the same plugin): the
        // token is another one.
        watch(&mut call, 9_000, HOLD_MS + GRACE_MS, Some((3, 2)));
        let forced = HOLD_MS + GRACE_MS;
        call.observe(forced, Some((3, 2)));
        assert_eq!(call.blame(forced), Blame::Nobody);
        assert_eq!(call.blame(9_000 + GRACE_MS), Blame::Innermost);
    }

    #[test]
    fn a_loop_of_nested_calls_under_one_stuck_outer_call_blames_the_outermost() {
        let mut call = CallWatch::new();
        // Plugin A's call began at 7 s and never returned; inside it, nested
        // calls begin and end every reading (the innermost token changes all
        // the time, the outermost does not).
        for (n, now) in (0u32..).zip((7_000..HOLD_MS + GRACE_MS + 5).step_by(5)) {
            call.observe(now, Some((n, 4)));
        }
        let forced = HOLD_MS + GRACE_MS;
        assert_eq!(call.blame(forced), Blame::Outermost);
        // A stuck inner call is the innermost, though.
        let mut call = CallWatch::new();
        watch(&mut call, 7_000, forced + 5, Some((5, 4)));
        assert_eq!(call.blame(forced), Blame::Innermost);
    }

    #[test]
    fn no_call_means_nobody_is_blamed() {
        let mut call = CallWatch::new();
        watch(&mut call, 0, 20_000, None);
        assert_eq!(call.blame(20_000), Blame::Nobody);
        assert_eq!(CallWatch::new().blame(100_000), Blame::Nobody);
    }

    #[test]
    fn a_call_in_the_clean_way_that_hangs_is_blamed_after_the_grace() {
        // The main loop took the request; core.suspend then hangs in a plugin:
        // the reset 6 s after the request finds a call that has run 6 s.
        let mut call = CallWatch::new();
        let asked = HOLD_MS;
        watch(&mut call, asked + 100, asked + CLEAN_MS, Some((11, 6)));
        call.observe(asked + CLEAN_MS, Some((11, 6)));
        assert_eq!(call.blame(asked + CLEAN_MS), Blame::Innermost);
    }
}
