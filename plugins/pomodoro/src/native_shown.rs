//! What the widgets and the screen show of the timer: one [`Shown`] made from
//! the state, the settings and the clock, compared with the one drawn last to
//! tell whether anything on screen changed, and the two lines that say the
//! day's rounds and the week's.

use alloc::string::String;
use paperpaw_sdk::native::{Error, formatting::Arg};

use crate::phase::{self, Lengths, Log, Phase};
use crate::state::Timer;

/// What is drawn of the timer: whole minutes left (counted up), the seconds
/// the full screen's clock shows, how much of the phase is gone, the dots of
/// the set and the day's counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shown {
    pub phase: Phase,
    pub running: bool,
    /// Whole minutes left, counted up.
    pub minutes: u64,
    /// The clock's seconds: whole minutes while more than one is left, steps of
    /// ten in the last, so that it never claims more than it redraws.
    pub secs: u64,
    /// Thousandths of the phase gone.
    pub gone: u16,
    pub done: u32,
    pub rounds: u32,
    pub today: u32,
    pub week: u32,
    pub goal: Option<u32>,
    /// Milliseconds left, rounded to the next tick: how soon to draw again.
    pub tick_ms: u64,
}

impl Shown {
    /// What the timer shows at `now_ms`; `today` is the day's number, if the
    /// device knows it (the count of a day gone by is not today's).
    #[must_use]
    pub fn of(
        timer: &Timer,
        lengths: &Lengths,
        goal: Option<u32>,
        (now_ms, today): (u64, Option<i32>),
    ) -> Self {
        let whole = lengths.ms(timer.phase);
        let left = timer.remaining(now_ms, whole).min(whole);
        let mut log: Log = timer.log;
        log.roll(today);
        Self {
            phase: timer.phase,
            running: timer.running,
            minutes: phase::minutes_left(left),
            secs: phase::shown_secs(left),
            gone: phase::gone(left, whole),
            done: timer.done,
            rounds: lengths.rounds.max(2),
            today: log.today(),
            week: log.week(),
            goal,
            tick_ms: phase::next_tick(left),
        }
    }

    /// A phase not begun: the full length, nothing gone.
    #[must_use]
    pub const fn fresh(&self) -> bool {
        !self.running && self.gone == 0
    }

    /// What a widget shows: all but the seconds of the full screen's clock
    /// and the time to the next tick, which only that screen uses.
    #[must_use]
    pub fn for_widget(&self) -> Self {
        Self {
            secs: 0,
            tick_ms: 0,
            ..*self
        }
    }
}

/// "3 rounds today", or "3 of 8 rounds today" with a goal.
///
/// # Errors
/// What `format` says.
pub fn count_line(
    shown: &Shown,
    mut format: impl FnMut(&str, &[Arg<'_>]) -> Result<String, Error>,
) -> Result<String, Error> {
    let today = i64::from(shown.today);
    match shown.goal {
        Some(goal) => format(
            "pomodoro.today.goal",
            &[
                Arg::Count(i64::from(goal)),
                Arg::Int {
                    name: "done",
                    value: today,
                },
            ],
        ),
        None => format("pomodoro.today", &[Arg::Count(today)]),
    }
}

/// "14 this week", when the week holds more than today.
///
/// # Errors
/// What `format` says.
pub fn week_line(
    shown: &Shown,
    mut format: impl FnMut(&str, &[Arg<'_>]) -> Result<String, Error>,
) -> Result<Option<String>, Error> {
    if shown.week <= shown.today {
        return Ok(None);
    }
    format("pomodoro.week", &[Arg::Count(i64::from(shown.week))]).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLASSIC: Lengths = Lengths::CLASSIC;

    #[test]
    fn a_fresh_timer_shows_the_whole_phase() {
        let timer = Timer::new();
        let shown = Shown::of(&timer, &CLASSIC, None, (0, Some(10)));
        assert!(shown.fresh());
        assert_eq!((shown.minutes, shown.secs, shown.gone), (25, 25 * 60, 0));
        assert_eq!((shown.done, shown.rounds, shown.today), (0, 4, 0));
    }

    #[test]
    fn a_running_timer_counts_minutes_up_and_the_bar_fills() {
        let mut timer = Timer::new();
        timer.start(0, None, &CLASSIC);
        let shown = Shown::of(&timer, &CLASSIC, None, (5 * 60_000, None));
        assert!(shown.running && !shown.fresh());
        assert_eq!(shown.minutes, 20);
        assert_eq!(shown.gone, 200);
        // The last minute reads "1 min", never "0".
        let last = Shown::of(&timer, &CLASSIC, None, (25 * 60_000 - 5_000, None));
        assert_eq!((last.minutes, last.secs, last.tick_ms), (1, 10, 5_000));
    }

    #[test]
    fn widgets_do_not_change_with_the_full_screens_seconds() {
        let mut timer = Timer::new();
        timer.start(0, None, &CLASSIC);
        let a = Shown::of(&timer, &CLASSIC, None, (25 * 60_000 - 50_000, None));
        let b = Shown::of(&timer, &CLASSIC, None, (25 * 60_000 - 40_000, None));
        assert_ne!(a, b);
        assert_eq!(a.minutes, b.minutes);
        assert_ne!(a.for_widget(), b.for_widget(), "the bar moved");
        let c = Shown::of(&timer, &CLASSIC, None, (25 * 60_000 - 50_000, None));
        assert_eq!(a.for_widget(), c.for_widget());
    }

    #[test]
    fn the_count_of_a_day_gone_by_is_not_todays() {
        let mut timer = Timer::new();
        timer.advance(true, Some(100), &CLASSIC);
        let same = Shown::of(&timer, &CLASSIC, Some(3), (0, Some(100)));
        assert_eq!((same.today, same.week, same.goal), (1, 1, Some(3)));
        let next = Shown::of(&timer, &CLASSIC, None, (0, Some(101)));
        assert_eq!((next.today, next.week), (0, 1));
    }

    fn words(key: &str, args: &[Arg<'_>]) -> Result<String, Error> {
        let mut out = String::from(key);
        for arg in args {
            match arg {
                Arg::Count(n) => out.push_str(&alloc::format!(" count={n}")),
                Arg::Int { name, value } => out.push_str(&alloc::format!(" {name}={value}")),
                _ => {}
            }
        }
        Ok(out)
    }

    #[test]
    fn the_lines_say_the_day_and_the_week() {
        let mut shown = Shown::of(&Timer::new(), &CLASSIC, None, (0, None));
        shown.today = 3;
        shown.week = 3;
        assert_eq!(count_line(&shown, words).unwrap(), "pomodoro.today count=3");
        assert_eq!(
            week_line(&shown, words).unwrap(),
            None,
            "no more than today"
        );
        shown.week = 14;
        shown.goal = Some(8);
        assert_eq!(
            count_line(&shown, words).unwrap(),
            "pomodoro.today.goal count=8 done=3"
        );
        assert_eq!(
            week_line(&shown, words).unwrap().as_deref(),
            Some("pomodoro.week count=14")
        );
    }
}
