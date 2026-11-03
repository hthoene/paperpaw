//! The rules of the timer, with no clock and no screen: which phase
//! follows which, how long each lasts, what the clock shows, and the
//! count of finished rounds by day.

/// A stretch of the cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Focus,
    Short,
    Long,
}

impl Phase {
    /// The number it is kept as.
    pub const fn code(self) -> u8 {
        match self {
            Phase::Focus => 0,
            Phase::Short => 1,
            Phase::Long => 2,
        }
    }

    /// The phase kept as `code`; unknown numbers are a focus round.
    pub const fn from_code(code: i64) -> Phase {
        match code {
            1 => Phase::Short,
            2 => Phase::Long,
            _ => Phase::Focus,
        }
    }

    /// The translation key of its name.
    pub const fn key(self) -> &'static str {
        match self {
            Phase::Focus => "pomodoro.phase.focus",
            Phase::Short => "pomodoro.phase.short",
            Phase::Long => "pomodoro.phase.long",
        }
    }

    /// The translation key of its short name, for small places.
    pub const fn short_key(self) -> &'static str {
        match self {
            Phase::Focus => "pomodoro.phase.focus.short",
            Phase::Short => "pomodoro.phase.short.short",
            Phase::Long => "pomodoro.phase.long.short",
        }
    }
}

/// How long each phase lasts, in minutes, and how many focus rounds make
/// a set (the settings, read once).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lengths {
    pub focus: u32,
    pub short: u32,
    pub long: u32,
    pub rounds: u32,
}

impl Lengths {
    /// The classic: 25, 5 and 15 minutes, four rounds.
    pub const CLASSIC: Lengths = Lengths {
        focus: 25,
        short: 5,
        long: 15,
        rounds: 4,
    };

    /// Milliseconds `phase` lasts (at least a minute).
    pub fn ms(&self, phase: Phase) -> u64 {
        let minutes = match phase {
            Phase::Focus => self.focus,
            Phase::Short => self.short,
            Phase::Long => self.long,
        };
        u64::from(minutes.max(1)) * 60_000
    }

    /// The phase after `phase`, when `done` focus rounds of the set are
    /// finished (counting the one just ended).
    pub fn next(&self, phase: Phase, done: u32) -> Phase {
        match phase {
            Phase::Focus if done >= self.rounds.max(2) => Phase::Long,
            Phase::Focus => Phase::Short,
            Phase::Short | Phase::Long => Phase::Focus,
        }
    }
}

/// Whole minutes left, counted up: "1" until the last second is gone.
pub fn minutes_left(left_ms: u64) -> u64 {
    left_ms.div_ceil(60_000)
}

/// Thousandths of a phase `whole` milliseconds long that are gone, with
/// `left` milliseconds left.
pub fn gone(left: u64, whole: u64) -> u16 {
    (1000 - (left.saturating_mul(1000) / whole.max(1)).min(1000)) as u16
}

/// Milliseconds until the screen is drawn again: to the next full minute
/// of what is left (at least a second), every ten seconds in the last
/// minute.
pub fn next_tick(left_ms: u64) -> u64 {
    if left_ms <= 60_000 {
        10_000.min(left_ms.max(1000))
    } else {
        let into = left_ms % 60_000;
        if into == 0 { 60_000 } else { into }.max(1000)
    }
}

/// The seconds shown: whole minutes while more than a minute is left, in
/// steps of ten in the last one, so the clock never claims more than it
/// redraws.
pub fn shown_secs(left_ms: u64) -> u64 {
    let secs = left_ms.div_ceil(1000);
    if left_ms > 60_000 {
        secs.div_ceil(60) * 60
    } else {
        secs.div_ceil(10) * 10
    }
}

/// How many days of counts are kept: today and the six before.
pub const DAYS: usize = 7;

/// The focus rounds finished, by day: today's and the last six days'.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Log {
    /// The day of `counts[0]`, days since 1970 by the local time.
    day: i32,
    counts: [u16; DAYS],
}

impl Default for Log {
    fn default() -> Self {
        Log {
            day: i32::MIN,
            counts: [0; DAYS],
        }
    }
}

impl Log {
    /// A log kept as `day` and `counts` (newest first).
    pub fn kept(day: i32, counts: [u16; DAYS]) -> Log {
        Log { day, counts }
    }

    pub fn day(&self) -> i32 {
        self.day
    }

    pub fn counts(&self) -> [u16; DAYS] {
        self.counts
    }

    /// Moves to `today`, if the device knows it: the days between are
    /// empty, those older than a week forgotten. A clock set back only
    /// takes the new day.
    pub fn roll(&mut self, today: Option<i32>) {
        let Some(today) = today else {
            return;
        };
        if self.day == i32::MIN || today < self.day {
            self.day = today;
            return;
        }
        let gap = usize::try_from(today - self.day).unwrap_or(DAYS).min(DAYS);
        if gap > 0 {
            self.counts.rotate_right(gap);
            for count in &mut self.counts[..gap] {
                *count = 0;
            }
            self.day = today;
        }
    }

    /// One more round finished today.
    pub fn add(&mut self) {
        self.counts[0] = self.counts[0].saturating_add(1);
    }

    pub fn today(&self) -> u32 {
        u32::from(self.counts[0])
    }

    /// The last seven days, today included.
    pub fn week(&self) -> u32 {
        self.counts.iter().map(|&c| u32::from(c)).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_set_ends_with_the_long_break() {
        let l = Lengths::CLASSIC;
        assert_eq!(l.next(Phase::Focus, 1), Phase::Short);
        assert_eq!(l.next(Phase::Focus, 3), Phase::Short);
        assert_eq!(l.next(Phase::Focus, 4), Phase::Long);
        assert_eq!(l.next(Phase::Short, 1), Phase::Focus);
        assert_eq!(l.next(Phase::Long, 4), Phase::Focus);
    }

    #[test]
    fn rounds_are_at_least_two() {
        let l = Lengths {
            rounds: 0,
            ..Lengths::CLASSIC
        };
        assert_eq!(l.next(Phase::Focus, 1), Phase::Short);
        assert_eq!(l.next(Phase::Focus, 2), Phase::Long);
    }

    #[test]
    fn lengths_in_milliseconds() {
        let l = Lengths::CLASSIC;
        assert_eq!(l.ms(Phase::Focus), 25 * 60_000);
        assert_eq!(l.ms(Phase::Short), 5 * 60_000);
        assert_eq!(l.ms(Phase::Long), 15 * 60_000);
        let zero = Lengths { focus: 0, ..l };
        assert_eq!(zero.ms(Phase::Focus), 60_000);
    }

    #[test]
    fn phases_survive_being_kept() {
        for p in [Phase::Focus, Phase::Short, Phase::Long] {
            assert_eq!(Phase::from_code(i64::from(p.code())), p);
        }
        assert_eq!(Phase::from_code(-5), Phase::Focus);
        assert_eq!(Phase::from_code(99), Phase::Focus);
    }

    #[test]
    fn minutes_count_up() {
        assert_eq!(minutes_left(25 * 60_000), 25);
        assert_eq!(minutes_left(24 * 60_000 + 1), 25);
        assert_eq!(minutes_left(1), 1);
        assert_eq!(minutes_left(0), 0);
    }

    #[test]
    fn progress_in_thousandths() {
        assert_eq!(gone(100, 100), 0);
        assert_eq!(gone(50, 100), 500);
        assert_eq!(gone(0, 100), 1000);
        assert_eq!(gone(500, 100), 0);
        assert_eq!(gone(0, 0), 1000);
    }

    #[test]
    fn the_screen_wakes_rarely() {
        // Once a minute, on the minute of what is left.
        assert_eq!(next_tick(25 * 60_000), 60_000);
        assert_eq!(next_tick(24 * 60_000 + 20_000), 20_000);
        // Every ten seconds in the last minute, never faster than a second.
        assert_eq!(next_tick(60_000), 10_000);
        assert_eq!(next_tick(5_000), 5_000);
        assert_eq!(next_tick(0), 1_000);
        // Never more often than that over a whole phase.
        let mut left: u64 = 25 * 60_000;
        let mut wakes = 0;
        while left > 0 {
            left = left.saturating_sub(next_tick(left));
            wakes += 1;
        }
        assert!(wakes <= 25 + 6, "{wakes}");
    }

    #[test]
    fn the_clock_never_claims_more_than_it_redraws() {
        assert_eq!(shown_secs(25 * 60_000), 25 * 60);
        assert_eq!(shown_secs(24 * 60_000 + 1), 25 * 60);
        assert_eq!(shown_secs(60_000), 60);
        assert_eq!(shown_secs(59_000), 60);
        assert_eq!(shown_secs(51_000), 60);
        assert_eq!(shown_secs(50_000), 50);
        assert_eq!(shown_secs(1), 10);
        assert_eq!(shown_secs(0), 0);
    }

    #[test]
    fn days_roll_and_forget() {
        let mut log = Log::default();
        log.roll(None);
        assert_eq!(log.day(), i32::MIN);
        log.roll(Some(100));
        log.add();
        log.add();
        assert_eq!((log.today(), log.week()), (2, 2));
        log.roll(Some(100));
        assert_eq!(log.today(), 2);
        log.roll(Some(101));
        assert_eq!((log.today(), log.week()), (0, 2));
        log.add();
        assert_eq!(log.counts(), [1, 2, 0, 0, 0, 0, 0]);
        log.roll(Some(103));
        assert_eq!(log.counts(), [0, 0, 1, 2, 0, 0, 0]);
        // A week later it is forgotten.
        log.roll(Some(110));
        assert_eq!(log.week(), 0);
        log.roll(Some(200));
        assert_eq!(log.week(), 0);
    }

    #[test]
    fn a_clock_set_back_does_not_lose_counts() {
        let mut log = Log::kept(100, [3, 1, 0, 0, 0, 0, 0]);
        log.roll(Some(50));
        assert_eq!(log.day(), 50);
        assert_eq!(log.today(), 3);
        log.roll(Some(51));
        assert_eq!(log.counts(), [0, 3, 1, 0, 0, 0, 0]);
    }

    #[test]
    fn counts_do_not_overflow() {
        let mut log = Log::kept(1, [u16::MAX, 0, 0, 0, 0, 0, 0]);
        log.add();
        assert_eq!(log.today(), u32::from(u16::MAX));
    }
}
