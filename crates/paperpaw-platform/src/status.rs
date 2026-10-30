//! What a device knows about the time of day and its battery, reported by
//! the shell (see `Kernel::set_status`).

use crate::input::Instant;

/// The shell's view of time and power. Unknown parts are `None`: a desktop
/// has no battery, a device without a clock chip no time of day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct SystemStatus {
    pub clock: Option<WallClock>,
    pub battery: Option<Battery>,
}

impl SystemStatus {
    /// Made of all its fields; the `with_` methods change one.
    pub const fn new(clock: Option<WallClock>, battery: Option<Battery>) -> Self {
        Self { clock, battery }
    }

    #[must_use]
    pub const fn with_clock(mut self, clock: Option<WallClock>) -> Self {
        self.clock = clock;
        self
    }

    #[must_use]
    pub const fn with_battery(mut self, battery: Option<Battery>) -> Self {
        self.battery = battery;
        self
    }
}

/// Wall-clock time: at the monotonic time `at`, it was `unix_millis`
/// milliseconds since 1970 (UTC). Later times follow from the monotonic
/// clock, so shells report it once and again only when it is corrected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct WallClock {
    pub at: Instant,
    pub unix_millis: i64,
    /// The offset of the system's time zone from UTC at `at`, in minutes
    /// (e.g. 120 for summer time in Berlin), if the system has one.
    pub utc_offset_minutes: Option<i32>,
}

impl WallClock {
    /// Made of all its fields; the `with_` methods change one.
    pub const fn new(at: Instant, unix_millis: i64, utc_offset_minutes: Option<i32>) -> Self {
        Self {
            at,
            unix_millis,
            utc_offset_minutes,
        }
    }

    #[must_use]
    pub const fn with_at(mut self, at: Instant) -> Self {
        self.at = at;
        self
    }

    #[must_use]
    pub const fn with_unix_millis(mut self, unix_millis: i64) -> Self {
        self.unix_millis = unix_millis;
        self
    }

    #[must_use]
    pub const fn with_utc_offset_minutes(mut self, utc_offset_minutes: Option<i32>) -> Self {
        self.utc_offset_minutes = utc_offset_minutes;
        self
    }
}

impl WallClock {
    /// Milliseconds since 1970 (UTC) at the monotonic time `now`.
    pub fn unix_millis_at(&self, now: Instant) -> i64 {
        self.unix_millis + (now.millis() as i64 - self.at.millis() as i64)
    }
}

/// The battery's charge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Battery {
    /// Charge from 0 to 100.
    pub percent: u8,
    pub charging: bool,
    /// Its voltage, where the device measures it: finer than the percent,
    /// for measuring what uses the battery.
    pub millivolts: Option<u16>,
}

impl Battery {
    /// Made of all its fields; the `with_` methods change one.
    pub const fn new(percent: u8, charging: bool, millivolts: Option<u16>) -> Self {
        Self {
            percent,
            charging,
            millivolts,
        }
    }

    #[must_use]
    pub const fn with_percent(mut self, percent: u8) -> Self {
        self.percent = percent;
        self
    }

    #[must_use]
    pub const fn with_charging(mut self, charging: bool) -> Self {
        self.charging = charging;
        self
    }

    #[must_use]
    pub const fn with_millivolts(mut self, millivolts: Option<u16>) -> Self {
        self.millivolts = millivolts;
        self
    }
}
