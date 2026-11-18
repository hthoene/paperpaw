//! The X4's time of day across a restart (#280). The X4 has no clock
//! chip, but the chip's RTC timer is not reset by a restart or a deep
//! sleep (only by a power-up), and the two RTC store registers the HAL
//! keeps its "boot time" in survive them as well. The shell stores the
//! time there whenever it is set (the network, the remote tool), and
//! reads it back at start: the stored time plus what the RTC timer counted
//! since.
//!
//! What is read back is *approximate*: the RTC's slow clock is an RC
//! oscillator (a few per cent off), and the time is only as new as the
//! last time it was set. The clock tells the two apart ([`Clock::exact`]);
//! the network sets it exactly at its next chance.
//!
//! The rule for showing it: after a restart or a deep sleep the clock
//! goes on from the kept time, and Home shows it (the RTC counted on);
//! after a power-up from nothing (battery out, the switch-off with no USB:
//! the latch lets go of the power) nothing is kept, the register reads
//! under the minimum below, and the clock stays unknown (`--:--`) until
//! the network or the remote tool sets it.

/// The oldest time taken for one that was set: 2024-01-01 (UTC), in
/// microseconds. A register that was never set holds the RTC timer's
/// count since power-up (a few seconds or hours), which is far under it.
pub const MIN_US: u64 = 1_704_067_200_000_000;
/// The latest: 2100-01-01.
pub const MAX_US: u64 = 4_102_444_800_000_000;

/// What the RTC's time is to be set to for `unix_ms`; `None` for a time
/// that cannot be one that was set (before 2024, after 2100), so it is
/// not kept and not taken for one later.
pub fn keep(unix_ms: i64) -> Option<u64> {
    let us = u64::try_from(unix_ms).ok()?.checked_mul(1000)?;
    (MIN_US..MAX_US).contains(&us).then_some(us)
}

/// The Unix time in milliseconds of what the RTC says (`us`), if it was
/// set; `None` after a power-up, which left only the count since then.
pub fn restore(rtc_us: u64) -> Option<i64> {
    if !(MIN_US..MAX_US).contains(&rtc_us) {
        return None;
    }
    i64::try_from(rtc_us / 1000).ok()
}

/// The time of day on the monotonic timer: Unix milliseconds minus the
/// timer's, once known.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Clock {
    base: Option<i64>,
    approximate: bool,
    changed: bool,
}

impl Clock {
    pub const fn new() -> Self {
        Self {
            base: None,
            approximate: false,
            changed: false,
        }
    }

    /// Sets the time: `unix_ms` at the timer's `now_ms`. Exact.
    pub fn set(&mut self, now_ms: u64, unix_ms: i64) {
        *self = Self {
            base: Some(unix_ms.saturating_sub(now_ms as i64)),
            approximate: false,
            changed: true,
        };
    }

    /// Takes the time read back from the RTC after a restart (see
    /// [`restore`]) at the timer's `now_ms`: approximate. Never replaces
    /// a time that was set; returns whether it was taken.
    pub fn restore(&mut self, now_ms: u64, rtc_us: u64) -> bool {
        if self.base.is_some() {
            return false;
        }
        let Some(unix_ms) = restore(rtc_us) else {
            return false;
        };
        *self = Self {
            base: Some(unix_ms.saturating_sub(now_ms as i64)),
            approximate: true,
            changed: true,
        };
        true
    }

    /// Whether the time was set (or restored) since this was last asked.
    pub fn take_changed(&mut self) -> bool {
        core::mem::take(&mut self.changed)
    }

    /// Milliseconds since 1970 (UTC) at the timer's `now_ms`, once known
    /// (approximate after a restore).
    pub fn unix_ms(&self, now_ms: u64) -> Option<i64> {
        self.base.map(|base| base.saturating_add(now_ms as i64))
    }

    /// Whether the time is known and was set in this run, not taken from
    /// the RTC.
    pub fn exact(&self) -> bool {
        self.base.is_some() && !self.approximate
    }

    /// Whether the time was taken from the RTC and not set since.
    pub fn approximate(&self) -> bool {
        self.base.is_some() && self.approximate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-11-17 12:00:00 UTC.
    const NOV: i64 = 1_794_916_800_000;

    #[test]
    fn a_time_that_was_set_is_kept_and_read_back_after_the_time_that_passed() {
        let us = keep(NOV).expect("kept");
        // A restart 90 s later: the RTC counted on.
        let later = us + 90_000_000;
        assert_eq!(restore(later), Some(NOV + 90_000));
    }

    #[test]
    fn a_power_up_leaves_the_count_since_then_which_is_no_time() {
        // Never set: the RTC's own count since the power-up.
        for since_power_up in [0, 1_000_000, 3_600_000_000, 30 * 86_400_000_000] {
            assert_eq!(restore(since_power_up), None);
        }
        assert_eq!(restore(u64::MAX), None);
    }

    #[test]
    fn a_time_that_cannot_have_been_set_is_not_kept() {
        assert_eq!(keep(0), None);
        assert_eq!(keep(-5), None);
        // 2023 and 2101.
        assert_eq!(keep(1_700_000_000_000), None);
        assert_eq!(keep(4_133_980_800_000), None);
        assert_eq!(keep(i64::MAX), None);
    }

    #[test]
    fn a_restored_clock_goes_on_from_the_kept_time_and_is_approximate() {
        let mut clock = Clock::new();
        assert_eq!(clock.unix_ms(5), None);
        // Started: the timer is at 700 ms; the RTC says NOV.
        assert!(clock.restore(700, keep(NOV).expect("kept")));
        assert_eq!(clock.unix_ms(700), Some(NOV));
        assert_eq!(clock.unix_ms(1700), Some(NOV + 1000));
        assert!(clock.approximate());
        assert!(!clock.exact());
        assert!(clock.take_changed(), "the status is passed on once");
        assert!(!clock.take_changed());
    }

    #[test]
    fn after_a_power_up_the_clock_stays_unknown() {
        let mut clock = Clock::new();
        assert!(!clock.restore(700, 12_345_678));
        assert_eq!(clock.unix_ms(700), None);
        assert!(!clock.approximate() && !clock.exact());
        assert!(!clock.take_changed());
    }

    #[test]
    fn setting_the_time_makes_it_exact_and_a_restore_never_replaces_it() {
        let mut clock = Clock::new();
        clock.restore(700, keep(NOV).expect("kept"));
        clock.take_changed();
        // The network sets it, a few minutes off the kept one.
        clock.set(5_000, NOV + 300_000);
        assert!(clock.exact() && !clock.approximate());
        assert_eq!(clock.unix_ms(5_000), Some(NOV + 300_000));
        assert!(clock.take_changed());
        assert!(!clock.restore(6_000, keep(NOV).expect("kept")));
        assert_eq!(clock.unix_ms(6_000), Some(NOV + 301_000));
    }
}
