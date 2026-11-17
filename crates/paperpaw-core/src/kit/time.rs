//! The time of day: time zones with their summer time rules, and the local
//! time shown e.g. in a status line.
//!
//! Shells report the wall clock and, where the system has one, its time
//! zone (see [`SystemStatus`](paperpaw_sdk::SystemStatus)). The setting
//! `core.time-zone` uses the system's zone ("automatic", the default) or
//! one of [`TIME_ZONES`]; `core.time-zone-rule` takes any POSIX `TZ` rule
//! (e.g. `CET-1CEST,M3.5.0,M10.5.0/3`) for zones not in the list. Rules are
//! a few bytes each, so no time zone database is needed.

use alloc::format;
use alloc::string::String;

use paperpaw_sdk::builtin::settings;
use crate::Place;
pub use paperpaw_sdk::time::CLOCK_12H;
pub use paperpaw_sdk::time::LocalTime;
pub use paperpaw_sdk::time::TIME_ZONES;
pub use paperpaw_sdk::time::TimeZone;
pub use paperpaw_sdk::time::UnixTime;
pub use paperpaw_sdk::time::civil_from_days;
pub use paperpaw_sdk::time::day_of_year;
pub use paperpaw_sdk::time::days_from_civil;
pub use paperpaw_sdk::time::days_in_month;
pub use paperpaw_sdk::time::days_in_year;
pub use paperpaw_sdk::time::iso_week;
pub use paperpaw_sdk::time::weekday;
pub use paperpaw_sdk::time::weekday_of;

/// A monotonic clock in microseconds ([`Place::clock`](crate::Place::clock)),
/// for measuring how long work takes, e.g. reading a file: the shell's
/// (`KernelBuilder::clock`). Unlike [`Place::now`](crate::Place::now), it
/// goes on during a call. Where the shell gives none, nothing is measured.
#[derive(Clone, Copy, Default)]
pub struct Clock {
    micros: Option<fn() -> u64>,
}

impl Clock {
    /// No clock: nothing is measured.
    pub const NONE: Self = Self { micros: None };

    /// The clock `micros`, microseconds since some start (for the kernel).
    pub const fn new(micros: fn() -> u64) -> Self {
        Self {
            micros: Some(micros),
        }
    }
}

impl core::fmt::Debug for Clock {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Clock")
            .field("available", &self.micros.is_some())
            .finish()
    }
}

/// The automatic time zone: the system's, or UTC without one.
pub const AUTO: &str = "core.tz.auto";







#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Summer {
    offset: i32,
    start: Rule,
    end: Rule,
}

/// When summer time starts or ends: a day of the year, at `time` seconds
/// local time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rule {
    day: Day,
    time: i32,
}

/// A day of the year as POSIX `TZ` rules name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Day {
    /// `Mm.w.d`: weekday `d` (0 = Sunday) of week `w` (5 = the last) of
    /// month `m`.
    Weekday { month: u8, week: u8, weekday: u8 },
    /// `Jn`: day `n` from 1 to 365, 29 February not counted.
    Julian(u16),
    /// `n`: day `n` from 0 to 365, 29 February counted.
    Ordinal(u16),
}

/// The most a rule's offset from UTC may be: POSIX gives the hours as 0 to
/// 24 (a rule of `<>200` would be an offset of more than a week).
const MAX_OFFSET: i32 = 24 * 3600;


impl Rule {
    /// When the rule applies in `year`, in seconds since 1970 local time.
    fn local_seconds(&self, year: i32) -> i64 {
        let days = match self.day {
            Day::Weekday {
                month,
                week,
                weekday,
            } => {
                let first = days_from_civil(year, month, 1);
                let mut day = 1 + (weekday + 7 - weekday_of(first)) % 7 + (week - 1) * 7;
                while day > days_in_month(year, month) {
                    day -= 7;
                }
                first + i64::from(day) - 1
            }
            Day::Julian(n) => {
                let leap_day = is_leap(year) && n >= 60;
                days_from_civil(year, 1, 1) + i64::from(n) - 1 + i64::from(leap_day)
            }
            Day::Ordinal(n) => days_from_civil(year, 1, 1) + i64::from(n),
        };
        days * 86_400 + i64::from(self.time)
    }
}

struct Parser<'a>(&'a [u8]);

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.0.first().copied()
    }

    fn done(&self) -> bool {
        self.0.is_empty()
    }

    fn expect(&mut self, byte: u8) -> Option<()> {
        (self.peek()? == byte).then(|| self.0 = &self.0[1..])
    }

    /// A zone name: letters, or anything in angle brackets.
    fn name(&mut self) -> Option<()> {
        if self.peek()? == b'<' {
            let end = self.0.iter().position(|&b| b == b'>')?;
            self.0 = &self.0[end + 1..];
            return Some(());
        }
        let len = self
            .0
            .iter()
            .take_while(|b| b.is_ascii_alphabetic())
            .count();
        (len >= 3).then(|| self.0 = &self.0[len..])
    }

    fn number(&mut self) -> Option<i32> {
        let len = self.0.iter().take_while(|b| b.is_ascii_digit()).count();
        if len == 0 || len > 3 {
            return None;
        }
        let value = self.0[..len]
            .iter()
            .fold(0, |n, &b| n * 10 + i32::from(b - b'0'));
        self.0 = &self.0[len..];
        Some(value)
    }

    /// `[+-]hh[:mm[:ss]]` in seconds.
    fn offset(&mut self) -> Option<i32> {
        let sign = match self.peek()? {
            b'-' => {
                self.0 = &self.0[1..];
                -1
            }
            b'+' => {
                self.0 = &self.0[1..];
                1
            }
            _ => 1,
        };
        let mut seconds = self.number()? * 3600;
        for unit in [60, 1] {
            if self.peek() != Some(b':') {
                break;
            }
            self.0 = &self.0[1..];
            seconds += self.number()? * unit;
        }
        Some(sign * seconds)
    }

    /// `Mm.w.d`, `Jn` or `n`, then `[/time]`.
    fn rule(&mut self) -> Option<Rule> {
        let day = match self.peek()? {
            b'M' => {
                self.0 = &self.0[1..];
                let month = self.number()?;
                self.expect(b'.')?;
                let week = self.number()?;
                self.expect(b'.')?;
                let weekday = self.number()?;
                let valid = (1..=12).contains(&month) && (1..=5).contains(&week) && weekday <= 6;
                valid.then_some(Day::Weekday {
                    month: month as u8,
                    week: week as u8,
                    weekday: weekday as u8,
                })?
            }
            b'J' => {
                self.0 = &self.0[1..];
                let n = self.number()?;
                (1..=365).contains(&n).then_some(Day::Julian(n as u16))?
            }
            _ => {
                let n = self.number()?;
                (0..=365).contains(&n).then_some(Day::Ordinal(n as u16))?
            }
        };
        let time = if self.peek() == Some(b'/') {
            self.0 = &self.0[1..];
            self.offset()?
        } else {
            7200
        };
        Some(Rule { day, time })
    }
}

// The calendar: days and weeks of the proleptic Gregorian calendar in
// whole numbers, for plugins that count days (statistics, calendars,
// forecasts).

/// Whether `year` has a 29 February.
pub fn is_leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}









/// The time zone chosen in the settings; `None` for the system's. A rule
/// that is not one ([`TimeZone::parse`]) counts as none: the zone chosen
/// from the list then.
fn chosen_zone(env: &Place<'_>) -> Option<TimeZone> {
    let rule = env.settings().read(&settings::TIME_ZONE_RULE).trim();
    if let Some(zone) = TimeZone::parse(rule) {
        return Some(zone);
    }
    let zone = env.settings().read(&settings::TIME_ZONE);
    TIME_ZONES
        .iter()
        .find(|(id, _)| *id == zone)
        .and_then(|(_, rule)| TimeZone::parse(rule))
}

/// The seconds the chosen time zone (or the system's) is ahead of UTC at
/// `unix` seconds since 1970.
fn zone_offset(env: &Place<'_>, unix: i64) -> i32 {
    match chosen_zone(env) {
        Some(zone) => zone.offset_at(unix),
        None => env
            .status
            .clock
            .and_then(|c| c.utc_offset_minutes)
            .map_or(0, |m| m * 60),
    }
}

/// Seconds since 1970 (UTC) now, if the device knows the time:
/// `UnixTime::now(env)` as a number, for what keeps seconds.
pub fn unix_seconds(env: &Place<'_>) -> Option<i64> {
    UnixTime::now(env).map(UnixTime::secs)
}

/// The local date and time now, in the chosen zone (its offset in
/// [`LocalTime::offset`]), if the device knows the time.
pub fn local_time(env: &Place<'_>) -> Option<LocalTime> {
    let now = UnixTime::now(env)?;
    Some(now.local(zone_offset(env, now.secs())))
}

/// The time now as the user wants to see it: `14:05` or `2:05 PM`
/// (setting `core.clock-format`), if the device knows the time.
pub fn clock_text(env: &Place<'_>) -> Option<String> {
    let t = local_time(env)?;
    Some(clock_of(env, t.hour, t.minute))
}

/// Whether the user wants times on a twelve-hour clock (setting
/// `core.clock-format`).
pub(crate) fn twelve_hours(env: &Place<'_>) -> bool {
    env.settings().read(&settings::CLOCK_FORMAT) == CLOCK_12H
}

/// A time of day (`hour` 0 to 23) as the user wants to see it: `14:05`
/// or `2:05 PM` ([`twelve_hours`]), in the digits of the language shown.
pub(crate) fn clock_of(env: &Place<'_>, hour: u8, minute: u8) -> String {
    if !twelve_hours(env) {
        return env
            .i18n()
            .localize_digits(&format!("{hour:02}:{minute:02}"))
            .into_owned();
    }
    let key = if hour < 12 {
        "core.clock.am"
    } else {
        "core.clock.pm"
    };
    let shown = match hour % 12 {
        0 => 12,
        h => h,
    };
    let time = format!("{shown}:{minute:02}");
    let time = env.i18n().localize_digits(&time);
    env.i18n()
        .format(key, &[("time", paperpaw_i18n::Arg::Str(&time))])
}

/// Today in words, e.g. "Thursday, 15 October", if the device knows the
/// date.
pub fn date_text(env: &Place<'_>) -> Option<String> {
    dated(env, "core.date", "")
}

/// Today in short words, e.g. "Thu 15 Oct", if the device knows the date.
pub fn short_date_text(env: &Place<'_>) -> Option<String> {
    dated(env, "core.date.short", ".short")
}

/// Today as the translation `key` writes it, with the names of the
/// weekday and the month (`core.weekday<kind>.<n>`, `core.month<kind>.<n>`).
fn dated(env: &Place<'_>, key: &str, kind: &str) -> Option<String> {
    let now = local_time(env)?;
    let weekday = format!("core.weekday{kind}.{}", now.weekday);
    let month = format!("core.month{kind}.{}", now.month);
    Some(env.i18n().format(
        key,
        &[
            ("weekday", paperpaw_i18n::Arg::Str(env.i18n().tr(&weekday))),
            ("day", paperpaw_i18n::Arg::Int(i64::from(now.day))),
            ("month", paperpaw_i18n::Arg::Str(env.i18n().tr(&month))),
        ],
    ))
}

/// The values of the first day of the week
/// ([`WEEK_START`](paperpaw_sdk::builtin::settings::WEEK_START)): as the
/// language has it, Monday, Sunday.
pub const WEEK_STARTS: &[&str] = &["core.week.auto", "core.week.monday", "core.week.sunday"];

/// The values of `core.clock-format`
/// ([`CLOCK_FORMAT`](paperpaw_sdk::builtin::settings::CLOCK_FORMAT)): 24 hours,
/// 12 hours.
pub const CLOCK_24H: &str = "core.clock.24h";

/// [`UnixTime::parse_iso8601`], in milliseconds.
fn iso_millis(text: &str) -> Option<i64> {
    let text = text.trim();
    let (date, clock) = match text.split_once(['T', ' ']) {
        Some((date, clock)) => (date, Some(clock)),
        None => (text, None),
    };
    // Digits alone: no "+1".
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let mut d = date.splitn(3, '-');
    let (year, month, day) = (d.next()?, d.next()?, d.next()?);
    if !(digits(year) && digits(month) && digits(day)) {
        return None;
    }
    let year: i32 = year.parse().ok()?;
    let month: u8 = month.parse().ok()?;
    let day: u8 = day.parse().ok()?;
    if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
        return None;
    }
    let days = days_from_civil(year, month, day);
    let Some(clock) = clock else {
        return days.checked_mul(86_400_000);
    };
    // The offset: `Z`, `+hh:mm`, `-hh:mm` (or `+hhmm`), or none.
    let (clock, offset) = if let Some(clock) = clock.strip_suffix('Z') {
        (clock, 0)
    } else if let Some(at) = clock.rfind(['+', '-']) {
        let (clock, zone) = clock.split_at(at);
        let sign = if zone.starts_with('-') { -1 } else { 1 };
        let digits: alloc::string::String = zone[1..].chars().filter(|c| *c != ':').collect();
        if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let hours: i64 = digits[..2].parse().ok()?;
        let minutes: i64 = digits[2..].parse().ok()?;
        if hours > 23 || minutes > 59 {
            return None;
        }
        (clock, sign * (hours * 60 + minutes) * 60_000)
    } else {
        (clock, 0)
    };
    let (hms, fraction) = clock.split_once(['.', ',']).unwrap_or((clock, ""));
    let mut parts = hms.splitn(3, ':');
    // Digits alone: no hour of `-1` or `+5`.
    let number = |s: &str| {
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse::<u8>().ok())
            .flatten()
            .map(i64::from)
    };
    let hour = number(parts.next()?)?;
    let minute = number(parts.next()?)?;
    let second = parts.next().map_or(Some(0), number)?;
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    // Milliseconds from the first three digits of the fraction.
    let mut millis = 0i64;
    for (n, c) in fraction.chars().enumerate() {
        let digit = i64::from(c.to_digit(10)?);
        if n < 3 {
            millis += digit * [100, 10, 1][n];
        }
    }
    // Years far out overflow the milliseconds: none then.
    let ms = days
        .checked_mul(86_400_000)?
        .checked_add(((hour * 60 + minute) * 60 + second) * 1000 + millis)?;
    ms.checked_sub(offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_dates_are_moments() {
        let at = UnixTime::parse_http_date("Tue, 20 Oct 2026 08:00:00 GMT").unwrap();
        assert_eq!(at.iso8601(), "2026-10-20T08:00:00.000Z");
        assert_eq!(UnixTime::parse_http_date("20 Oct 2026"), None);
    }

    #[test]
    fn iso8601_times_are_written_and_read_back() {
        let iso8601 = |ms| UnixTime::from_millis(ms).iso8601();
        let parse_iso8601 = |text: &str| UnixTime::parse_iso8601(text).map(UnixTime::millis);
        let ms = 1_792_448_650_090; // 2026-10-19T22:24:10.090Z
        assert_eq!(iso8601(ms), "2026-10-19T22:24:10.090Z");
        assert_eq!(parse_iso8601(&iso8601(ms)), Some(ms));
        assert_eq!(iso8601(-1), "1969-12-31T23:59:59.999Z");
        // Offsets, no seconds, no fraction, a date alone.
        assert_eq!(parse_iso8601("2026-10-20T00:24:10.090+02:00"), Some(ms));
        assert_eq!(parse_iso8601("2026-10-19T22:24Z"), Some(ms - 10_090));
        assert_eq!(parse_iso8601("2026-10-19 22:24:10"), Some(ms - 90));
        assert_eq!(
            parse_iso8601("2026-10-19"),
            Some(days_from_civil(2026, 10, 19) * 86_400_000)
        );
        // More digits of the fraction than milliseconds hold.
        assert_eq!(parse_iso8601("2026-10-19T22:24:10.0909Z"), Some(ms));
        for bad in [
            "",
            "2026",
            "2026-13-01",
            "2026-02-30",
            "2026-10-19T25:00Z",
            "2026-10-19T10:00+2",
            // An offset past a day, an hour below zero, a year far out.
            "2026-10-19T10:00+25:00",
            "2026-10-19T-1:00Z",
            "2026-10-19T+5:00Z",
            "2026-+1-19",
            "+2026-10-19",
            "2147483647-12-31T23:59:59Z",
            "x",
        ] {
            assert_eq!(parse_iso8601(bad), None, "{bad}");
        }
    }

    fn utc(year: i32, month: u8, day: u8, hour: i64) -> i64 {
        days_from_civil(year, month, day) * 86_400 + hour * 3600
    }

    #[test]
    fn dates_round_trip() {
        for days in [-1, 0, 59, 365, 11_016, 20_000, 60_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
        let t = LocalTime::from_seconds(utc(2026, 10, 14, 13) + 5 * 60);
        assert_eq!(
            (t.year, t.month, t.day, t.hour, t.minute),
            (2026, 10, 14, 13, 5)
        );
        assert_eq!(t.weekday, 3);
        for seconds in [-86_401, 0, 951_782_400, 1_792_448_650] {
            assert_eq!(LocalTime::from_seconds(seconds).to_seconds(), seconds);
        }
    }

    #[test]
    fn days_and_weeks_of_the_calendar() {
        // Wednesday, 14 October 2026.
        assert_eq!(weekday(2026, 10, 14), 3);
        assert_eq!(weekday_of(0), 4);
        assert_eq!(day_of_year(2026, 10, 14), 287);
        assert_eq!(day_of_year(2024, 12, 31), 366);
        assert_eq!(days_in_year(2024), 366);
        assert_eq!(days_in_year(2100), 365);
        assert_eq!(days_in_year(2000), 366);
        assert_eq!(days_in_month(2028, 2), 29);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(iso_week(2026, 10, 14), 42);
        // 1 January 2027 is a Friday: still week 53 of 2026.
        assert_eq!(iso_week(2027, 1, 1), 53);
        assert_eq!(iso_week(2027, 1, 4), 1);
        // 29 December 2025 (a Monday) is in week 1 of 2026.
        assert_eq!(iso_week(2025, 12, 29), 1);
        assert_eq!(iso_week(2024, 12, 30), 1);
        assert_eq!(iso_week(2021, 1, 3), 53);
    }

    #[test]
    fn summer_time_in_berlin() {
        let berlin = TimeZone::parse("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
        // 2026: summer time from 29 March 01:00 UTC to 25 October 01:00 UTC.
        assert_eq!(berlin.offset_at(utc(2026, 1, 10, 12)), 3600);
        assert_eq!(berlin.offset_at(utc(2026, 3, 29, 0)), 3600);
        assert_eq!(berlin.offset_at(utc(2026, 3, 29, 1)), 7200);
        assert_eq!(berlin.offset_at(utc(2026, 10, 25, 0)), 7200);
        assert_eq!(berlin.offset_at(utc(2026, 10, 25, 1)), 3600);
    }

    #[test]
    fn southern_and_fixed_zones() {
        let sydney = TimeZone::parse("AEST-10AEDT,M10.1.0,M4.1.0/3").unwrap();
        assert_eq!(sydney.offset_at(utc(2026, 1, 10, 0)), 11 * 3600);
        assert_eq!(sydney.offset_at(utc(2026, 7, 10, 0)), 10 * 3600);
        // Days of the year: J60 is 1 March in every year, 59 is 1 March
        // in a leap year only.
        let julian = TimeZone::parse("XST0XDT,J60/0,J300/0").unwrap();
        assert_eq!(julian.offset_at(utc(2024, 2, 29, 12)), 0);
        assert_eq!(julian.offset_at(utc(2024, 3, 1, 1)), 3600);
        let ordinal = TimeZone::parse("XST0XDT,59/0,300/0").unwrap();
        assert_eq!(ordinal.offset_at(utc(2024, 2, 29, 12)), 3600);
        assert_eq!(ordinal.offset_at(utc(2026, 2, 28, 12)), 0);
        assert_eq!(ordinal.offset_at(utc(2026, 3, 1, 1)), 3600);
        assert!(TimeZone::parse("XST0XDT,J0,J300").is_none());
        assert!(TimeZone::parse("XST0XDT,366,J300").is_none());
        let india = TimeZone::parse("IST-5:30").unwrap();
        assert_eq!(india.offset_at(0), 5 * 3600 + 1800);
        let sao_paulo = TimeZone::parse("<-03>3").unwrap();
        assert_eq!(sao_paulo.offset_at(0), -3 * 3600);
        assert!(TimeZone::parse("nonsense").is_none());
        for (id, rule) in TIME_ZONES {
            assert!(TimeZone::parse(rule).is_some(), "{id}");
        }
    }
}
