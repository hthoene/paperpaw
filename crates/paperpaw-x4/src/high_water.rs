//! The test image's high-water log (#2460, Senior's conditions on #2462): the
//! lowest the stack's room and the heap went, written to the serial log every
//! [`PERIOD_MS`] and kept in RTC memory (the firmware's slot) only when a
//! figure falls below the last one kept, and once before a sleep, so that the
//! owner reads the device's real numbers after a run (G7, S1–S8) without the
//! log running all the time.
//!
//! The reading is on plain numbers here, so that a computer tests when it
//! logs and when it keeps; the firmware takes the samples (a scan of the
//! painted stack and a search for the largest block, both too dear for every
//! turn of the loop: [`Tracker::due`] says when).

/// Milliseconds between two samples (and log lines).
pub const PERIOD_MS: u64 = 30_000;

/// What one sample reads: the lowest each has been is kept apart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sample {
    /// Bytes of the stack never used since start.
    pub stack_left: u32,
    /// The heap's free bytes.
    pub heap_free: u32,
    /// The largest block that could be had.
    pub largest: u32,
}

impl Sample {
    /// The lowest of each of two samples.
    #[must_use]
    pub fn lowest(self, other: Self) -> Self {
        Self {
            stack_left: self.stack_left.min(other.stack_left),
            heap_free: self.heap_free.min(other.heap_free),
            largest: self.largest.min(other.largest),
        }
    }

    /// Whether any figure is below the one of `kept`.
    #[must_use]
    pub fn below(self, kept: Self) -> bool {
        self.stack_left < kept.stack_left
            || self.heap_free < kept.heap_free
            || self.largest < kept.largest
    }
}

/// What a sample asks the firmware to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tick {
    /// Write the log line (always: the sample was due).
    pub log: bool,
    /// Keep the lows in RTC memory: a figure fell below the last one kept.
    pub keep: bool,
}

/// The lows since start and when the next sample is due.
#[derive(Clone, Copy, Debug)]
pub struct Tracker {
    next_ms: u64,
    low: Option<Sample>,
    kept: Option<Sample>,
}

impl Tracker {
    /// A tracker at start: the first sample is due one period on.
    #[must_use]
    pub const fn new(now_ms: u64) -> Self {
        Self {
            next_ms: now_ms.saturating_add(PERIOD_MS),
            low: None,
            kept: None,
        }
    }

    /// Whether a sample is due at `now_ms` (the loop asks; nothing is read
    /// before).
    #[must_use]
    pub fn due(&self, now_ms: u64) -> bool {
        now_ms >= self.next_ms
    }

    /// A sample taken at `now_ms`, when [`Self::due`] said yes: the next one
    /// is a period after this (not after the last due time: a long sleep
    /// gives one sample, not a burst).
    pub fn sampled(&mut self, now_ms: u64, sample: Sample) -> Tick {
        self.next_ms = now_ms.saturating_add(PERIOD_MS);
        let low = self.low.map_or(sample, |low| low.lowest(sample));
        self.low = Some(low);
        let keep = self.kept.is_none_or(|kept| low.below(kept));
        if keep {
            self.kept = Some(low);
        }
        Tick { log: true, keep }
    }

    /// A fresh sample before the device sleeps (the stack may have gone
    /// deeper since the last one): the lows to keep, when they fell below
    /// what is kept.
    pub fn before_sleep(&mut self, sample: Sample) -> Option<Sample> {
        let low = self.low.map_or(sample, |low| low.lowest(sample));
        self.low = Some(low);
        if self.kept.is_some_and(|kept| !low.below(kept)) {
            return None;
        }
        self.kept = Some(low);
        Some(low)
    }

    /// The lowest each figure has been since start, if sampled.
    #[must_use]
    pub fn low(&self) -> Option<Sample> {
        self.low
    }
}

/// The words of the lows as kept in RTC memory (which also holds noise after
/// a power-up and what the firmware before left): a magic, the three figures
/// and a check word.
pub const WORDS: usize = 5;
const MAGIC: u32 = 0x4857_4d4b;

/// The lows sealed for RTC memory.
#[must_use]
pub fn seal(low: Sample) -> [u32; WORDS] {
    let check = MAGIC
        ^ low.stack_left.rotate_left(7)
        ^ low.heap_free.rotate_left(13)
        ^ low.largest.rotate_left(21);
    [MAGIC, low.stack_left, low.heap_free, low.largest, check]
}

/// The lows kept, if what the words hold is a sealed record.
#[must_use]
pub fn unseal(words: [u32; WORDS]) -> Option<Sample> {
    let low = Sample {
        stack_left: words[1],
        heap_free: words[2],
        largest: words[3],
    };
    (words[0] == MAGIC && seal(low) == words).then_some(low)
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn sample(stack: u32, heap: u32, largest: u32) -> Sample {
        Sample {
            stack_left: stack,
            heap_free: heap,
            largest,
        }
    }

    #[test]
    fn a_sample_is_due_every_period_and_nothing_is_read_before() {
        let mut tracker = Tracker::new(1_000);
        assert!(!tracker.due(1_000));
        assert!(!tracker.due(1_000 + PERIOD_MS - 1));
        assert!(tracker.due(1_000 + PERIOD_MS));
        let tick = tracker.sampled(1_000 + PERIOD_MS, sample(30_000, 90_000, 60_000));
        assert!(tick.log);
        assert!(!tracker.due(1_000 + PERIOD_MS + 1));
        assert!(tracker.due(1_000 + 2 * PERIOD_MS));
    }

    #[test]
    fn a_long_sleep_gives_one_sample_not_a_burst() {
        let mut tracker = Tracker::new(0);
        tracker.sampled(PERIOD_MS, sample(1, 1, 1));
        let later = 100 * PERIOD_MS;
        assert!(tracker.due(later));
        tracker.sampled(later, sample(1, 1, 1));
        assert!(!tracker.due(later + 1));
    }

    #[test]
    fn the_lows_are_kept_only_when_a_figure_falls() {
        let mut tracker = Tracker::new(0);
        // The first sample is the first low: kept.
        assert!(tracker.sampled(30_000, sample(30_000, 90_000, 60_000)).keep);
        // The same, or higher: logged, not kept.
        assert_eq!(
            tracker.sampled(60_000, sample(31_000, 95_000, 62_000)),
            Tick {
                log: true,
                keep: false
            }
        );
        // One figure lower: kept, with the lows of the others.
        assert!(tracker.sampled(90_000, sample(29_000, 99_000, 70_000)).keep);
        assert_eq!(tracker.low(), Some(sample(29_000, 90_000, 60_000)));
        assert!(
            !tracker
                .sampled(120_000, sample(29_000, 99_000, 70_000))
                .keep
        );
    }

    #[test]
    fn before_a_sleep_only_a_new_low_is_written() {
        let mut tracker = Tracker::new(0);
        // Nothing sampled yet: the sleep's own sample is the first low.
        assert_eq!(
            tracker.before_sleep(sample(30_000, 90_000, 60_000)),
            Some(sample(30_000, 90_000, 60_000))
        );
        // Not lower: nothing written.
        assert_eq!(tracker.before_sleep(sample(31_000, 91_000, 61_000)), None);
        // The stack went deeper since the last sample: written.
        assert_eq!(
            tracker.before_sleep(sample(25_000, 91_000, 61_000)),
            Some(sample(25_000, 90_000, 60_000))
        );
        // A sample already kept is not written twice.
        tracker.sampled(30_000, sample(25_000, 90_000, 60_000));
        assert_eq!(tracker.before_sleep(sample(25_000, 90_000, 60_000)), None);
    }

    #[test]
    fn the_kept_lows_come_back_and_noise_does_not() {
        let low = sample(25_000, 90_000, 60_000);
        assert_eq!(unseal(seal(low)), Some(low));
        assert_eq!(unseal([0; WORDS]), None);
        let mut damaged = seal(low);
        damaged[2] ^= 1;
        assert_eq!(unseal(damaged), None);
    }

    #[test]
    fn the_time_cannot_overflow() {
        let tracker = Tracker::new(u64::MAX);
        assert!(tracker.due(u64::MAX));
    }
}
