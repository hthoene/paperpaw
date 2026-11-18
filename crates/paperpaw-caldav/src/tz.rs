//! Time zones as calendars name them (`TZID=Europe/Berlin`): a small table of
//! the common ones with their daylight-saving rules, enough to move a time
//! to UTC and so to the reader's own zone. A name that is not in the table
//! is not known: the caller reads such a time as the reader's own.
use crate::date::{civil_from_days, days_from_civil, weekday};

/// How a zone moves its clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dst {
    None,
    /// Last Sunday of March to last Sunday of October, at 01:00 UTC.
    Eu,
    /// Second Sunday of March to first of November, at 02:00 local.
    Us,
    /// First Sunday of October to first of April, at 02:00 standard time.
    South,
    /// Last Sunday of September to first of April.
    NewZealand,
}

/// A zone of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Zone {
    /// Seconds east of UTC without daylight saving.
    std: i32,
    dst: Dst,
}

impl Zone {
    /// UTC itself.
    pub const UTC: Self = Self {
        std: 0,
        dst: Dst::None,
    };
}

const fn z(minutes: i32, dst: Dst) -> Zone {
    Zone {
        std: minutes * 60,
        dst,
    }
}

/// Names and the zone each is: the end of a `TZID` is matched, so that a
/// prefix like `/mozilla.org/20050126_1/` does no harm.
const TABLE: &[(&str, Zone)] = &[
    ("UTC", Zone::UTC),
    ("GMT", Zone::UTC),
    ("Etc/UTC", Zone::UTC),
    ("Etc/GMT", Zone::UTC),
    ("Europe/London", z(0, Dst::Eu)),
    ("Europe/Dublin", z(0, Dst::Eu)),
    ("Europe/Lisbon", z(0, Dst::Eu)),
    ("GMT Standard Time", z(0, Dst::Eu)),
    ("Europe/Paris", z(60, Dst::Eu)),
    ("Europe/Berlin", z(60, Dst::Eu)),
    ("Europe/Madrid", z(60, Dst::Eu)),
    ("Europe/Rome", z(60, Dst::Eu)),
    ("Europe/Amsterdam", z(60, Dst::Eu)),
    ("Europe/Brussels", z(60, Dst::Eu)),
    ("Europe/Vienna", z(60, Dst::Eu)),
    ("Europe/Zurich", z(60, Dst::Eu)),
    ("Europe/Stockholm", z(60, Dst::Eu)),
    ("Europe/Oslo", z(60, Dst::Eu)),
    ("Europe/Copenhagen", z(60, Dst::Eu)),
    ("Europe/Prague", z(60, Dst::Eu)),
    ("Europe/Warsaw", z(60, Dst::Eu)),
    ("Europe/Budapest", z(60, Dst::Eu)),
    ("W. Europe Standard Time", z(60, Dst::Eu)),
    ("Central Europe Standard Time", z(60, Dst::Eu)),
    ("Central European Standard Time", z(60, Dst::Eu)),
    ("Romance Standard Time", z(60, Dst::Eu)),
    ("Europe/Athens", z(120, Dst::Eu)),
    ("Europe/Helsinki", z(120, Dst::Eu)),
    ("Europe/Kiev", z(120, Dst::Eu)),
    ("Europe/Kyiv", z(120, Dst::Eu)),
    ("Europe/Bucharest", z(120, Dst::Eu)),
    ("Europe/Sofia", z(120, Dst::Eu)),
    ("E. Europe Standard Time", z(120, Dst::Eu)),
    ("FLE Standard Time", z(120, Dst::Eu)),
    ("Europe/Istanbul", z(180, Dst::None)),
    ("Europe/Moscow", z(180, Dst::None)),
    ("America/New_York", z(-300, Dst::Us)),
    ("America/Toronto", z(-300, Dst::Us)),
    ("Eastern Standard Time", z(-300, Dst::Us)),
    ("America/Chicago", z(-360, Dst::Us)),
    ("Central Standard Time", z(-360, Dst::Us)),
    ("America/Denver", z(-420, Dst::Us)),
    ("Mountain Standard Time", z(-420, Dst::Us)),
    ("America/Phoenix", z(-420, Dst::None)),
    ("America/Los_Angeles", z(-480, Dst::Us)),
    ("America/Vancouver", z(-480, Dst::Us)),
    ("Pacific Standard Time", z(-480, Dst::Us)),
    ("America/Anchorage", z(-540, Dst::Us)),
    ("Pacific/Honolulu", z(-600, Dst::None)),
    ("America/Mexico_City", z(-360, Dst::None)),
    ("America/Sao_Paulo", z(-180, Dst::None)),
    ("America/Argentina/Buenos_Aires", z(-180, Dst::None)),
    ("Africa/Lagos", z(60, Dst::None)),
    ("Africa/Johannesburg", z(120, Dst::None)),
    ("Africa/Cairo", z(120, Dst::None)),
    ("Africa/Nairobi", z(180, Dst::None)),
    ("Asia/Dubai", z(240, Dst::None)),
    ("Asia/Karachi", z(300, Dst::None)),
    ("Asia/Kolkata", z(330, Dst::None)),
    ("Asia/Calcutta", z(330, Dst::None)),
    ("Asia/Dhaka", z(360, Dst::None)),
    ("Asia/Bangkok", z(420, Dst::None)),
    ("Asia/Jakarta", z(420, Dst::None)),
    ("Asia/Shanghai", z(480, Dst::None)),
    ("Asia/Hong_Kong", z(480, Dst::None)),
    ("Asia/Singapore", z(480, Dst::None)),
    ("Australia/Perth", z(480, Dst::None)),
    ("Asia/Tokyo", z(540, Dst::None)),
    ("Asia/Seoul", z(540, Dst::None)),
    ("Australia/Adelaide", z(570, Dst::South)),
    ("Australia/Brisbane", z(600, Dst::None)),
    ("Australia/Sydney", z(600, Dst::South)),
    ("Australia/Melbourne", z(600, Dst::South)),
    ("Pacific/Auckland", z(720, Dst::NewZealand)),
];

/// The zone a `TZID` names, if the table has it.
#[must_use]
pub fn find(tzid: &str) -> Option<Zone> {
    let tzid = tzid.trim().trim_matches('"');
    TABLE
        .iter()
        .find(|(name, _)| {
            let (long, short) = (tzid.len(), name.len());
            long >= short
                && tzid
                    .get(long - short..)
                    .is_some_and(|end| end.eq_ignore_ascii_case(name))
                && (long == short || tzid.as_bytes().get(long - short - 1) == Some(&b'/'))
        })
        .map(|(_, zone)| *zone)
}

/// The day of the `n`th Sunday of a month (the last for `n` of 0).
fn sunday(year: i32, month: u8, n: u8) -> i64 {
    if n == 0 {
        let last = days_from_civil(year, month, crate::date::days_in_month(year, month));
        return last - i64::from(weekday(last));
    }
    let first = days_from_civil(year, month, 1);
    first + i64::from((7 - weekday(first)) % 7) + 7 * i64::from(n - 1)
}

impl Zone {
    /// Whether daylight saving is on at `utc` (seconds since 1970).
    fn saving(self, utc: i64) -> bool {
        let year = civil_from_days((utc + i64::from(self.std)).div_euclid(86_400)).0;
        let std = i64::from(self.std);
        let at = |day: i64, hour: i64, off: i64| day * 86_400 + hour * 3600 - off;
        match self.dst {
            Dst::None => false,
            Dst::Eu => (at(sunday(year, 3, 0), 1, 0)..at(sunday(year, 10, 0), 1, 0)).contains(&utc),
            Dst::Us => (at(sunday(year, 3, 2), 2, std)..at(sunday(year, 11, 1), 2, std + 3600))
                .contains(&utc),
            Dst::South => {
                utc >= at(sunday(year, 10, 1), 2, std) || utc < at(sunday(year, 4, 1), 2, std)
            }
            Dst::NewZealand => {
                utc >= at(sunday(year, 9, 0), 2, std) || utc < at(sunday(year, 4, 1), 2, std)
            }
        }
    }

    /// Seconds east of UTC at `utc`.
    #[must_use]
    pub fn offset_at_utc(self, utc: i64) -> i32 {
        self.std + if self.saving(utc) { 3600 } else { 0 }
    }

    /// Seconds east of UTC where the clock of the zone shows `local` (seconds
    /// since 1970 as if the clock's time were UTC). In the hour a change
    /// skips or repeats, one of the two readings.
    #[must_use]
    pub fn offset_at_local(self, local: i64) -> i32 {
        let first = self.offset_at_utc(local - i64::from(self.std));
        self.offset_at_utc(local - i64::from(first))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;

    fn unix(y: i32, m: u8, d: u8, h: i64) -> i64 {
        days_from_civil(y, m, d) * 86_400 + h * 3600
    }

    #[test]
    fn names_are_found_by_their_end() {
        assert!(find("Europe/Berlin").is_some());
        assert!(find("/mozilla.org/20050126_1/Europe/Berlin").is_some());
        assert!(find("europe/berlin").is_some());
        assert!(find("W. Europe Standard Time").is_some());
        assert!(find("Mars/Olympus").is_none());
        assert!(find("xEurope/Berlin").is_none());
    }

    #[test]
    fn europe_changes_on_the_last_sunday() {
        let berlin = find("Europe/Berlin").unwrap();
        // 2026: the clocks go forward on 29 March at 01:00 UTC.
        assert_eq!(berlin.offset_at_utc(unix(2026, 3, 29, 0)), 3600);
        assert_eq!(berlin.offset_at_utc(unix(2026, 3, 29, 1)), 7200);
        assert_eq!(berlin.offset_at_utc(unix(2026, 7, 1, 12)), 7200);
        // Back on 25 October.
        assert_eq!(berlin.offset_at_utc(unix(2026, 10, 25, 0)), 7200);
        assert_eq!(berlin.offset_at_utc(unix(2026, 10, 25, 1)), 3600);
        assert_eq!(berlin.offset_at_local(unix(2026, 7, 1, 10)), 7200);
        assert_eq!(berlin.offset_at_local(unix(2026, 1, 1, 10)), 3600);
    }

    #[test]
    fn america_and_the_south_follow_their_rules() {
        let ny = find("America/New_York").unwrap();
        // 2026: forward on 8 March, back on 1 November.
        assert_eq!(ny.offset_at_local(unix(2026, 3, 8, 1)), -18_000);
        assert_eq!(ny.offset_at_local(unix(2026, 3, 8, 3)), -14_400);
        assert_eq!(ny.offset_at_local(unix(2026, 11, 1, 3)), -18_000);
        assert_eq!(ny.offset_at_local(unix(2026, 7, 4, 12)), -14_400);
        let sydney = find("Australia/Sydney").unwrap();
        assert_eq!(sydney.offset_at_utc(unix(2026, 1, 15, 0)), 39_600);
        assert_eq!(sydney.offset_at_utc(unix(2026, 7, 15, 0)), 36_000);
        let tokyo = find("Asia/Tokyo").unwrap();
        assert_eq!(tokyo.offset_at_utc(unix(2026, 7, 15, 0)), 32_400);
        let auckland = find("Pacific/Auckland").unwrap();
        assert_eq!(auckland.offset_at_utc(unix(2026, 12, 25, 0)), 46_800);
        assert_eq!(auckland.offset_at_utc(unix(2026, 6, 25, 0)), 43_200);
    }
}
