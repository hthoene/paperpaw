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
    /// More of what the shell measures, by name (`temperature-c`,
    /// `frontlight-percent`): values a later build adds without a new
    /// plugin interface. Unused places have an empty name.
    values: [(&'static str, i64); VALUES],
}

/// How many named values a [`SystemStatus`] holds.
const VALUES: usize = 4;

impl SystemStatus {
    /// Made of all its fields; the `with_` methods change one.
    pub const fn new(clock: Option<WallClock>, battery: Option<Battery>) -> Self {
        Self {
            clock,
            battery,
            values: [("", 0); VALUES],
        }
    }

    /// The value named `name`, if the shell gave it.
    pub fn value(&self, name: &str) -> Option<i64> {
        self.values
            .iter()
            .find(|(n, _)| !n.is_empty() && *n == name)
            .map(|(_, v)| *v)
    }

    /// With the value `name` (in place of one of that name, else in a free
    /// place; left out where none is free).
    #[must_use]
    pub fn with_value(mut self, name: &'static str, value: i64) -> Self {
        let place = self.values.iter().position(|(n, _)| *n == name);
        let place = place.or_else(|| self.values.iter().position(|(n, _)| n.is_empty()));
        if let (false, Some(slot)) = (name.is_empty(), place.and_then(|i| self.values.get_mut(i))) {
            *slot = (name, value);
        }
        self
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_by_name_in_four_places() {
        let status = SystemStatus::default()
            .with_value("temperature-c", 21)
            .with_value("frontlight-percent", 40)
            .with_value("temperature-c", 22);
        assert_eq!(status.value("temperature-c"), Some(22));
        assert_eq!(status.value("frontlight-percent"), Some(40));
        assert_eq!(status.value(""), None);
        let full = status
            .with_value("a", 1)
            .with_value("b", 2)
            .with_value("c", 3);
        assert_eq!((full.value("b"), full.value("c")), (Some(2), None));
    }
}
