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
    /// The plugin call open at this depth (0: the outermost): the deepest call
    /// that has stayed open for the whole grace. Calls deeper than it may have
    /// come and gone meanwhile (a loop of calls into another plugin), or it
    /// may be the innermost and have made host calls.
    Depth(u8),
}

/// How many depths of nested calls are told apart (the record of calls keeps
/// as many; deeper calls count in the last).
pub const DEPTHS: usize = 4;

/// What one reading of the record of calls says, while a call is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Running {
    /// Another value whenever a call or a host slot begins or ends.
    pub call: u32,
    /// How many of the open calls have a count in `entries` (at most
    /// [`DEPTHS`]).
    pub open: u8,
    /// The innermost call waits for a host slot (its code is not running).
    pub host: bool,
    /// For each depth, how many calls have begun at it: the call open at a
    /// depth is the same one as long as its count is.
    pub entries: [u32; DEPTHS],
}

/// Which plugin call has been running, as the readings see it: for each depth
/// the count of calls that began there, and since when it has been the same
/// (that call has been open since); the running call's token (another value
/// whenever a call or a host slot begins or ends) and whether the innermost
/// call waits for a host slot.
///
/// A reset by the escape blames a plugin only if its call has run for the
/// whole grace ([`GRACE_MS`]), which a call that began after the 8 s mark
/// cannot have, and then the **deepest** call that has stayed open for the
/// whole grace: the innermost one, also while it makes one host call after the
/// other (the plugin whose code keeps the device busy, not the plugin that
/// called it); or, when calls keep beginning and ending inside it, the call
/// they are made from (a loop of nested calls: A calls B, B loops on calls
/// into C, and the device is stuck in B's call, not in A's). Only **one** host
/// call that has stayed open for the whole grace blames nobody (a slow card
/// write is the host's time, not the plugin's), and the times then start again
/// where that call returns.
#[derive(Debug, Clone, Copy, Default)]
pub struct CallWatch {
    /// The count of each depth's call as last read, `None` where no call is
    /// open, and since when it has been the same.
    entries: [Option<u32>; DEPTHS],
    since: [u64; DEPTHS],
    /// The reading of the host call that is open now, and since when.
    host: Option<u32>,
    host_since: u64,
}

impl CallWatch {
    /// No call seen.
    #[must_use]
    pub const fn new() -> Self {
        CallWatch {
            entries: [None; DEPTHS],
            since: [0; DEPTHS],
            host: None,
            host_since: 0,
        }
    }

    /// One reading at `now` (ms, increasing) of the record of calls: `None`
    /// when no call is open.
    pub fn observe(&mut self, now: u64, running: Option<Running>) {
        let Some(running) = running else {
            *self = CallWatch::new();
            return;
        };
        let open = usize::from(running.open).min(DEPTHS);
        let mut changed = false;
        for depth in 0..DEPTHS {
            let seen = (depth < open).then_some(running.entries[depth]);
            if self.entries[depth] != seen {
                self.entries[depth] = seen;
                self.since[depth] = now;
                changed = true;
            }
        }
        if changed {
            self.host = None;
        }
        if running.host {
            // The plugin's own code is not running; a host call open for the
            // whole grace is the host's time, whoever called it.
            if self.host != Some(running.call) {
                self.host = Some(running.call);
                self.host_since = now;
            }
            if now.saturating_sub(self.host_since) >= GRACE_MS {
                self.since = [now; DEPTHS];
            }
        } else {
            self.host = None;
        }
    }

    /// Whom a reset at `now` blames.
    #[must_use]
    pub fn blame(&self, now: u64) -> Blame {
        (0..DEPTHS)
            .rev()
            .find(|&depth| {
                self.entries[depth].is_some() && now.saturating_sub(self.since[depth]) >= GRACE_MS
            })
            .map_or(Blame::Nobody, |depth| Blame::Depth(depth as u8))
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

    /// A reading of the record of calls: `open` calls, the counts at each
    /// depth, the host flag.
    fn run(call: u32, open: u8, entries: [u32; DEPTHS], host: bool) -> Option<Running> {
        Some(Running {
            call,
            open,
            host,
            entries,
        })
    }

    /// Feeds readings every 5 ms in `[from, to)` of the record of calls.
    fn watch(call: &mut CallWatch, from: u64, to: u64, running: Option<Running>) {
        for now in (from..to).step_by(5) {
            call.observe(now, running);
        }
    }

    const FORCED: u64 = HOLD_MS + GRACE_MS;

    #[test]
    fn a_call_that_was_running_at_the_mark_is_blamed_at_the_reset() {
        let mut call = CallWatch::new();
        // The call began shortly before the 8 s mark and never returned.
        let one = run(7, 1, [1, 0, 0, 0], false);
        watch(&mut call, 7_000, FORCED, one);
        call.observe(FORCED, one);
        assert_eq!(call.blame(FORCED), Blame::Depth(0));
    }

    #[test]
    fn a_call_that_began_after_the_mark_is_not_blamed() {
        let mut call = CallWatch::new();
        // A long host step (no plugin call) up to just after the mark; then a
        // call begins and is the one running when the reset comes.
        watch(&mut call, 7_000, HOLD_MS + 10, None);
        let one = run(9, 1, [2, 0, 0, 0], false);
        watch(&mut call, HOLD_MS + 10, FORCED, one);
        call.observe(FORCED, one);
        assert_eq!(call.blame(FORCED), Blame::Nobody);
    }

    #[test]
    fn a_plugin_inside_one_slow_host_call_is_not_blamed() {
        let mut call = CallWatch::new();
        // The plugin's call began before the mark; its card write began at 7.9
        // s and is open through the reset: one host call, open for the grace.
        watch(&mut call, 7_000, 7_900, run(7, 1, [1, 0, 0, 0], false));
        watch(&mut call, 7_900, FORCED, run(8, 1, [1, 0, 0, 0], true));
        call.observe(FORCED, run(8, 1, [1, 0, 0, 0], true));
        assert_eq!(call.blame(FORCED), Blame::Nobody);
        // The write returned just before the reset: the plugin runs again as
        // another reading, and its call's time starts there.
        call.observe(FORCED + 5, run(9, 1, [1, 0, 0, 0], false));
        assert_eq!(call.blame(FORCED + 5), Blame::Nobody);
    }

    #[test]
    fn a_loop_of_short_host_calls_is_blamed_on_the_plugin_that_makes_them() {
        let mut call = CallWatch::new();
        // `loop { text_measure() }`: the call never returns; a host slot opens
        // and closes every reading, so the reading changes all the time and
        // no host call stays open, but no plugin call begins or ends.
        for (n, now) in (0u32..).zip((7_000..FORCED + 5).step_by(5)) {
            call.observe(now, run(n, 1, [3, 0, 0, 0], n % 2 == 1));
        }
        assert_eq!(call.blame(FORCED), Blame::Depth(0));
    }

    #[test]
    fn a_host_call_shorter_than_the_grace_does_not_hide_a_stuck_call() {
        let mut call = CallWatch::new();
        // The call has run since 7 s; a host call opens at 8.9 s and is still
        // open at the reset (1.1 s < the grace).
        watch(&mut call, 7_000, 8_900, run(7, 1, [1, 0, 0, 0], false));
        watch(&mut call, 8_900, FORCED, run(8, 1, [1, 0, 0, 0], true));
        assert_eq!(call.blame(FORCED), Blame::Depth(0));
    }

    #[test]
    fn a_nested_or_another_call_starts_the_count_anew() {
        let mut call = CallWatch::new();
        watch(&mut call, 7_000, 9_000, run(1, 1, [1, 0, 0, 0], false));
        // The first call ends and another begins (even the same plugin): the
        // count of its depth is another one.
        watch(&mut call, 9_000, FORCED, run(3, 1, [2, 0, 0, 0], false));
        call.observe(FORCED, run(3, 1, [2, 0, 0, 0], false));
        assert_eq!(call.blame(FORCED), Blame::Nobody);
        assert_eq!(call.blame(9_000 + GRACE_MS), Blame::Depth(0));
    }

    #[test]
    fn a_loop_of_nested_calls_under_one_stuck_call_blames_that_call() {
        let mut call = CallWatch::new();
        // Plugin A's call began at 7 s and never returned; inside it, calls
        // into B begin and end every reading (B's count moves all the time,
        // A's does not).
        for (n, now) in (0u32..).zip((7_000..FORCED + 5).step_by(5)) {
            let open = 1 + (n % 2) as u8;
            call.observe(now, run(n, open, [1, n + 1, 0, 0], false));
        }
        assert_eq!(call.blame(FORCED), Blame::Depth(0));
        // A stuck inner call is the innermost, though.
        let mut call = CallWatch::new();
        watch(&mut call, 7_000, FORCED + 5, run(5, 2, [1, 1, 0, 0], false));
        assert_eq!(call.blame(FORCED), Blame::Depth(1));
    }

    #[test]
    fn a_plugin_looping_on_calls_into_another_is_blamed_not_the_reader_that_called_it() {
        // The reader A began its call at 7 s and called plugin B at 7.1 s; B
        // loops on calls into C (C's depth counts every one). A's and B's
        // calls have stayed open for the whole grace: the deepest of them, B,
        // is blamed, and A, the reader, is not switched off.
        let mut call = CallWatch::new();
        watch(&mut call, 7_000, 7_100, run(1, 1, [1, 0, 0, 0], false));
        for (n, now) in (0u32..).zip((7_100..FORCED + 5).step_by(5)) {
            let open = 2 + (n % 2) as u8;
            call.observe(now, run(10 + n, open, [1, 1, n + 1, 0], false));
        }
        assert_eq!(call.blame(FORCED), Blame::Depth(1));
        // The same with B's calls made one level deeper (D inside C), and a
        // loop in the last slot, where deeper calls all count: the deepest
        // slot that stays put is the one blamed.
        let mut call = CallWatch::new();
        for (n, now) in (0u32..).zip((7_000..FORCED + 5).step_by(5)) {
            let open = 3 + (n % 2) as u8;
            call.observe(now, run(n, open, [1, 1, 1, n + 1], false));
        }
        assert_eq!(call.blame(FORCED), Blame::Depth(2));
    }

    #[test]
    fn a_nested_plugin_looping_on_host_calls_is_blamed_not_the_plugin_that_called_it() {
        // Plugin A (the reader) began its call at 7 s and called plugin B
        // inside it at 7.1 s; B loops on host calls (`call` changes at every
        // reading, the counts do not).
        let mut call = CallWatch::new();
        watch(&mut call, 7_000, 7_100, run(1, 1, [1, 0, 0, 0], false));
        for (n, now) in (10u32..).zip((7_100..FORCED + 5).step_by(5)) {
            call.observe(now, run(n, 2, [1, 1, 0, 0], n % 2 == 1));
        }
        // The innermost call (B) is the one blamed; A is not switched off.
        assert_eq!(call.blame(FORCED), Blame::Depth(1));
    }

    #[test]
    fn a_nested_plugin_inside_one_slow_host_call_blames_nobody() {
        let mut call = CallWatch::new();
        watch(&mut call, 7_000, 7_100, run(1, 1, [1, 0, 0, 0], false));
        watch(&mut call, 7_100, 7_900, run(2, 2, [1, 1, 0, 0], false));
        watch(&mut call, 7_900, FORCED, run(3, 2, [1, 1, 0, 0], true));
        call.observe(FORCED, run(3, 2, [1, 1, 0, 0], true));
        assert_eq!(call.blame(FORCED), Blame::Nobody);
    }

    #[test]
    fn a_nested_call_that_returned_leaves_the_outer_plugin_to_blame() {
        // B ran 7.0-7.3 s and returned; A runs on and never returns.
        let mut call = CallWatch::new();
        watch(&mut call, 7_000, 7_300, run(2, 2, [1, 1, 0, 0], false));
        watch(&mut call, 7_300, FORCED + 5, run(3, 1, [1, 1, 0, 0], false));
        // A has run since 7 s, and B's depth is closed.
        assert_eq!(call.blame(FORCED), Blame::Depth(0));
        assert_eq!(call.blame(7_300 + GRACE_MS - 5), Blame::Depth(0));
        // B's depth counts again from the next call into it.
        call.observe(FORCED + 10, run(4, 2, [1, 2, 0, 0], false));
        assert_eq!(call.blame(FORCED + 10), Blame::Depth(0));
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
        let one = run(11, 1, [6, 0, 0, 0], false);
        watch(&mut call, asked + 100, asked + CLEAN_MS, one);
        call.observe(asked + CLEAN_MS, one);
        assert_eq!(call.blame(asked + CLEAN_MS), Blame::Depth(0));
    }
}
