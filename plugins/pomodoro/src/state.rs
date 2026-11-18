//! What every part of the plugin shares: the phase, whether it runs and when it
//! ends, the rounds of the set and the day's counts ([`Timer`]), and the small
//! file that keeps it, so that switching the device off does not lose a round.
//!
//! Nothing here asks a host: the callers give the time (the host's monotonic
//! milliseconds, the Unix time if the device knows it, today's number) and the
//! lengths the settings say. Nothing allocates.

use core::fmt::Write;

use paperpaw_ui_kit::widget::settings::Stack;

use crate::phase::{DAYS, Lengths, Log, Phase};

/// The file's name in the plugin's data folder (storage root 0).
pub const FILE: &str = "state.txt";
/// The most bytes the kept text has; a longer file is not ours.
pub const FILE_MAX: usize = 160;

/// What the kept text said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kept {
    phase: Phase,
    running: bool,
    end: i64,
    left: u64,
    done: u32,
    day: i32,
    counts: [u16; DAYS],
}

impl Kept {
    /// Reads the kept text; `None` where it is not ours or damaged.
    #[must_use]
    pub fn parse(text: &str) -> Option<Kept> {
        let mut parts = text.trim().split('|');
        let mut number = || parts.next()?.trim().parse::<i64>().ok();
        let phase = number()?;
        let running = number()?;
        let end = number()?;
        let left = number()?;
        let done = number()?;
        let day = number()?;
        let mut counts = [0u16; DAYS];
        let mut items = parts.next()?.split(',');
        for slot in &mut counts {
            *slot = items.next()?.trim().parse().ok()?;
        }
        if items.next().is_some() || parts.next().is_some() {
            return None;
        }
        Some(Kept {
            phase: Phase::from_code(phase),
            running: running == 1,
            end,
            left: u64::try_from(left).unwrap_or(0),
            done: u32::try_from(done.clamp(0, 99)).unwrap_or(0),
            day: i32::try_from(day).ok()?,
            counts,
        })
    }
}

/// What reading the kept state back made of a round that was running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Restored {
    /// Nothing runs: a phase not begun, or one paused.
    Idle,
    /// The round goes on: its end is in this many milliseconds.
    Running(u64),
    /// Its time ran out while the device was off or asleep: it is to be finished.
    Over,
}

/// The timer, shared by the screen, the widgets and the work that ends a phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timer {
    pub phase: Phase,
    pub running: bool,
    /// When the phase ends while running, on the host's monotonic clock.
    end_ms: u64,
    /// Seconds since 1970 when it ends, while running, if the device knew the
    /// time at the start: kept so that a restart can go on where it was.
    end_unix: Option<i64>,
    /// Milliseconds left while paused; 0 for a phase not begun, which has its
    /// whole length as the settings say now.
    pub left_ms: u64,
    /// Focus rounds finished in the current set.
    pub done: u32,
    pub log: Log,
}

impl Default for Timer {
    fn default() -> Self {
        Self::new()
    }
}

impl Timer {
    #[must_use]
    pub const fn new() -> Self {
        Timer {
            phase: Phase::Focus,
            running: false,
            end_ms: 0,
            end_unix: None,
            left_ms: 0,
            done: 0,
            log: Log::empty(),
        }
    }

    /// Milliseconds left of a phase `whole` milliseconds long.
    #[must_use]
    pub fn remaining(&self, now_ms: u64, whole: u64) -> u64 {
        if self.running {
            self.end_ms.saturating_sub(now_ms)
        } else if self.left_ms == 0 {
            whole
        } else {
            self.left_ms.min(whole)
        }
    }

    /// Whether a running phase has reached its end.
    #[must_use]
    pub const fn is_due(&self, now_ms: u64) -> bool {
        self.running && now_ms >= self.end_ms
    }

    /// The text of the state: `phase|running|end|left|done|day|counts`.
    pub fn text(&self, out: &mut Stack<FILE_MAX>) -> bool {
        let end = if self.running {
            self.end_unix.unwrap_or(0)
        } else {
            0
        };
        let counts = self.log.counts();
        let wrote = write!(
            out,
            "{}|{}|{}|{}|{}|{}|{},{},{},{},{},{},{}",
            self.phase.code(),
            u8::from(self.running),
            end,
            self.left_ms,
            self.done,
            self.log.day(),
            counts[0],
            counts[1],
            counts[2],
            counts[3],
            counts[4],
            counts[5],
            counts[6],
        );
        wrote.is_ok()
    }

    /// Starts the phase, or goes on after a pause. Returns the milliseconds
    /// until it ends, `None` if it ran already.
    pub fn start(&mut self, now_ms: u64, unix: Option<i64>, lengths: &Lengths) -> Option<u64> {
        if self.running {
            return None;
        }
        self.running = true;
        if self.left_ms == 0 {
            self.left_ms = lengths.ms(self.phase);
        }
        self.end_ms = now_ms.saturating_add(self.left_ms);
        self.end_unix =
            unix.map(|s| s.saturating_add(i64::try_from(self.left_ms / 1000).unwrap_or(0)));
        Some(self.left_ms)
    }

    /// Stops the clock where it is. Whether it ran.
    pub fn pause(&mut self, now_ms: u64, lengths: &Lengths) -> bool {
        if !self.running {
            return false;
        }
        self.left_ms = self.remaining(now_ms, lengths.ms(self.phase));
        self.running = false;
        true
    }

    /// Stands at the start of `phase`, not running.
    fn enter(&mut self, phase: Phase) {
        self.phase = phase;
        self.running = false;
        self.end_ms = 0;
        self.end_unix = None;
        self.left_ms = 0;
    }

    /// Finishes the phase (its time is up, or it was skipped) and stands at the
    /// next. A finished focus round counts for `today`, if the device knows it.
    pub fn advance(&mut self, finished: bool, today: Option<i32>, lengths: &Lengths) {
        if self.phase == Phase::Focus && finished {
            self.done = self.done.saturating_add(1);
            self.log.roll(today);
            self.log.add();
        }
        let next = lengths.next(self.phase, self.done);
        if self.phase == Phase::Long {
            self.done = 0;
        }
        self.enter(next);
    }

    /// Back to the first round of a set.
    pub fn reset(&mut self) {
        self.done = 0;
        self.enter(Phase::Focus);
    }

    /// Takes what the kept text said, as of `now_ms` and `unix`. A round that was
    /// running goes on by the wall clock; one that ended meanwhile is [`Restored::Over`].
    pub fn restore(
        &mut self,
        kept: &Kept,
        (now_ms, unix, today): (u64, Option<i64>, Option<i32>),
        lengths: &Lengths,
    ) -> Restored {
        self.phase = kept.phase;
        self.done = kept.done;
        self.log = Log::kept(kept.day, kept.counts);
        self.log.roll(today);
        self.running = false;
        self.end_unix = None;
        let whole = lengths.ms(self.phase);
        self.left_ms = kept.left.min(whole);
        if !kept.running || kept.end <= 0 {
            return Restored::Idle;
        }
        let Some(unix) = unix else {
            return Restored::Idle;
        };
        if kept.end > unix {
            let left = u64::try_from(kept.end - unix)
                .unwrap_or(0)
                .saturating_mul(1000);
            self.left_ms = left.min(whole);
            self.start(now_ms, Some(unix), lengths)
                .map_or(Restored::Idle, Restored::Running)
        } else {
            // Time ran out while the device was off.
            Restored::Over
        }
    }

    /// The device woke: its monotonic clock may have started again, so the end
    /// of a running phase is worked out anew from the wall clock (where the
    /// device knew it at the start). [`Restored::Over`] where the time ran out.
    pub fn wake(&mut self, now_ms: u64, unix: Option<i64>) -> Restored {
        if !self.running {
            return Restored::Idle;
        }
        let (Some(end), Some(now)) = (self.end_unix, unix) else {
            return if self.is_due(now_ms) {
                Restored::Over
            } else {
                Restored::Running(self.end_ms.saturating_sub(now_ms))
            };
        };
        if end <= now {
            return Restored::Over;
        }
        let left = u64::try_from(end - now).unwrap_or(0).saturating_mul(1000);
        self.end_ms = now_ms.saturating_add(left);
        Restored::Running(left)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLASSIC: Lengths = Lengths::CLASSIC;

    fn kept_text(timer: &Timer) -> Stack<FILE_MAX> {
        let mut out = Stack::new();
        assert!(timer.text(&mut out));
        out
    }

    #[test]
    fn kept_state_reads_back() {
        let mut timer = Timer::new();
        timer.phase = Phase::Short;
        timer.left_ms = 123_000;
        timer.done = 3;
        timer.log = Log::kept(20_000, [2, 1, 0, 4, 0, 0, 9]);
        let text = kept_text(&timer);
        let kept = Kept::parse(text.as_str()).unwrap();
        let mut back = Timer::new();
        assert_eq!(
            back.restore(&kept, (0, None, None), &CLASSIC),
            Restored::Idle
        );
        assert_eq!(back.phase, Phase::Short);
        assert_eq!((back.left_ms, back.done), (123_000, 3));
        assert_eq!(back.log.counts(), [2, 1, 0, 4, 0, 0, 9]);
        assert_eq!(back.log.day(), 20_000);
    }

    #[test]
    fn damaged_text_is_left_alone() {
        for text in [
            "",
            "garbage",
            "0|0|0|1000|0|5",
            "0|0|0|1000|0|5|1,2,3",
            "0|0|0|1000|0|5|a,b,c,d,e,f,g",
            "0|0|0|1000|0|99999999999|0,0,0,0,0,0,0",
            "0|0|0|x|0|5|0,0,0,0,0,0,0",
            "0|0|0|1000|0|5|70000,0,0,0,0,0,0",
            "0|0|0|1000|0|5|0,0,0,0,0,0,0|more",
        ] {
            assert!(Kept::parse(text).is_none(), "{text:?}");
        }
    }

    #[test]
    fn odd_numbers_are_made_sane() {
        let kept = Kept::parse("9|1|-5|-100|500|7|0,0,0,0,0,0,0").unwrap();
        assert_eq!(kept.phase, Phase::Focus);
        assert_eq!(kept.left, 0);
        assert_eq!(kept.done, 99);
    }

    #[test]
    fn a_round_runs_pauses_and_goes_on() {
        let mut timer = Timer::new();
        assert_eq!(timer.remaining(0, 25 * 60_000), 25 * 60_000);
        assert_eq!(timer.start(1_000, Some(5_000), &CLASSIC), Some(25 * 60_000));
        assert_eq!(
            timer.start(1_000, Some(5_000), &CLASSIC),
            None,
            "ran already"
        );
        assert_eq!(timer.remaining(61_000, 25 * 60_000), 24 * 60_000);
        assert!(!timer.is_due(61_000));
        assert!(timer.pause(61_000, &CLASSIC));
        assert!(!timer.pause(61_000, &CLASSIC));
        assert_eq!(timer.left_ms, 24 * 60_000);
        // Paused time does not pass.
        assert_eq!(timer.remaining(900_000, 25 * 60_000), 24 * 60_000);
        assert_eq!(timer.start(900_000, None, &CLASSIC), Some(24 * 60_000));
        assert!(timer.is_due(900_000 + 24 * 60_000));
    }

    #[test]
    fn a_finished_focus_round_counts_and_a_skipped_one_does_not() {
        let mut timer = Timer::new();
        timer.advance(true, Some(100), &CLASSIC);
        assert_eq!(
            (timer.phase, timer.done, timer.log.today()),
            (Phase::Short, 1, 1)
        );
        timer.advance(false, Some(100), &CLASSIC);
        assert_eq!(timer.phase, Phase::Focus);
        timer.advance(false, Some(100), &CLASSIC);
        assert_eq!(
            (timer.phase, timer.log.today()),
            (Phase::Short, 1),
            "skipped"
        );
        assert_eq!(timer.done, 1);
    }

    #[test]
    fn the_set_ends_with_the_long_break_and_starts_again() {
        let mut timer = Timer::new();
        for round in 1..=4 {
            timer.advance(true, Some(7), &CLASSIC);
            assert_eq!(timer.done, round);
            if round < 4 {
                assert_eq!(timer.phase, Phase::Short);
                timer.advance(true, Some(7), &CLASSIC);
                assert_eq!(timer.phase, Phase::Focus);
            }
        }
        assert_eq!(timer.phase, Phase::Long);
        assert_eq!(timer.log.today(), 4);
        timer.advance(true, Some(7), &CLASSIC);
        assert_eq!((timer.phase, timer.done), (Phase::Focus, 0));
        assert_eq!(timer.log.today(), 4, "a break counts nothing");
        timer.done = 2;
        timer.reset();
        assert_eq!(
            (timer.phase, timer.done, timer.running),
            (Phase::Focus, 0, false)
        );
    }

    #[test]
    fn a_running_round_goes_on_after_a_restart() {
        let mut timer = Timer::new();
        timer.start(0, Some(10_000), &CLASSIC);
        let text = kept_text(&timer);
        let kept = Kept::parse(text.as_str()).unwrap();
        // Ten minutes later, by the wall clock; the host's clock began again.
        let mut back = Timer::new();
        let restored = back.restore(&kept, (5_000, Some(10_600), None), &CLASSIC);
        assert_eq!(restored, Restored::Running(15 * 60_000));
        assert!(back.running);
        assert_eq!(back.remaining(5_000, 25 * 60_000), 15 * 60_000);
        // Past its end: to be finished.
        let mut late = Timer::new();
        assert_eq!(
            late.restore(&kept, (5_000, Some(10_000 + 25 * 60), None), &CLASSIC),
            Restored::Over
        );
        // Without the time the round cannot go on by the clock.
        let mut unknown = Timer::new();
        assert_eq!(
            unknown.restore(&kept, (5_000, None, None), &CLASSIC),
            Restored::Idle
        );
        assert!(!unknown.running);
    }

    #[test]
    fn waking_finds_the_end_by_the_wall_clock() {
        let mut timer = Timer::new();
        timer.start(1_000_000, Some(50_000), &CLASSIC);
        // Slept ten minutes; the monotonic clock began again at 3 s.
        assert_eq!(
            timer.wake(3_000, Some(50_600)),
            Restored::Running(15 * 60_000)
        );
        assert_eq!(timer.remaining(3_000, 25 * 60_000), 15 * 60_000);
        assert_eq!(timer.wake(3_000, Some(50_000 + 25 * 60)), Restored::Over);
        // No wall clock: the monotonic one decides.
        let mut plain = Timer::new();
        plain.start(0, None, &CLASSIC);
        assert_eq!(plain.wake(60_000, None), Restored::Running(24 * 60_000));
        assert_eq!(plain.wake(25 * 60_000, None), Restored::Over);
        assert_eq!(Timer::new().wake(0, Some(1)), Restored::Idle);
    }
}
