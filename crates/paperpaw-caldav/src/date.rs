//! Dates and times as iCalendar writes them (`20261120`,
//! `20261120T170000Z`), on the proleptic Gregorian calendar.
use alloc::string::String;

use crate::{Error, push_str};

/// The days since 1970-01-01 of a date.
#[must_use]
pub const fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    let y = year as i64 - (month <= 2) as i64;
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = month as i64;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date (year, month, day) of a day counted from 1970-01-01.
#[must_use]
pub const fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u8;
    let year = yoe + era * 400 + (month <= 2) as i64;
    (year as i32, month, day)
}

/// The days of a month.
#[must_use]
pub const fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

/// The day of the week of a day counted from 1970-01-01: 0 is Sunday.
#[must_use]
pub const fn weekday(days: i64) -> u8 {
    (days + 4).rem_euclid(7) as u8
}

/// A date, or a date with a time, as a property gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    /// Days since 1970-01-01.
    pub day: i32,
    /// Seconds into the day; `None` for a date alone.
    pub seconds: Option<u32>,
    /// In UTC (a `Z` at the end). Else it is the reader's own time: a time
    /// with a `TZID` is read as one without.
    pub utc: bool,
}

fn number(digits: &[u8]) -> Option<u32> {
    let mut value = 0u32;
    for digit in digits {
        if !digit.is_ascii_digit() {
            return None;
        }
        value = value * 10 + u32::from(digit - b'0');
    }
    Some(value)
}

impl DateTime {
    /// A date alone.
    #[must_use]
    pub const fn date(day: i32) -> Self {
        Self {
            day,
            seconds: None,
            utc: false,
        }
    }

    /// A time in UTC from seconds since 1970.
    #[must_use]
    pub fn from_unix(unix: i64) -> Self {
        Self {
            day: i32::try_from(unix.div_euclid(86_400)).unwrap_or(0),
            seconds: u32::try_from(unix.rem_euclid(86_400)).ok(),
            utc: true,
        }
    }

    /// Reads `20261120`, `20261120T170000` or `20261120T170000Z`.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let bytes = value.trim().as_bytes();
        let year = i32::try_from(number(bytes.get(..4)?)?).ok()?;
        let month = u8::try_from(number(bytes.get(4..6)?)?).ok()?;
        let day = u8::try_from(number(bytes.get(6..8)?)?).ok()?;
        if year == 0 || day == 0 || day > days_in_month(year, month) {
            return None;
        }
        let days = i32::try_from(days_from_civil(year, month, day)).ok()?;
        let Some(time) = bytes.get(8..).filter(|rest| !rest.is_empty()) else {
            return Some(Self::date(days));
        };
        let (time, utc) = match time.split_last() {
            Some((b'Z' | b'z', time)) => (time, true),
            _ => (time, false),
        };
        if time.len() != 7 || !matches!(time.first(), Some(b'T' | b't')) {
            return None;
        }
        let hour = number(time.get(1..3)?)?;
        let minute = number(time.get(3..5)?)?;
        let second = number(time.get(5..7)?)?.min(59);
        if hour > 23 || minute > 59 {
            return None;
        }
        Some(Self {
            day: days,
            seconds: Some(hour * 3600 + minute * 60 + second),
            utc,
        })
    }

    /// Seconds since 1970, the time read as UTC (a date alone: its start).
    #[must_use]
    pub fn unix(&self) -> i64 {
        i64::from(self.day) * 86_400 + i64::from(self.seconds.unwrap_or(0))
    }

    /// The day it falls on where the clock is `offset` seconds east of UTC:
    /// a time in UTC is moved there, anything else is that day already.
    #[must_use]
    pub fn local_day(&self, offset: i32) -> i32 {
        if self.utc && self.seconds.is_some() {
            let local = self.unix().saturating_add(i64::from(offset));
            i32::try_from(local.div_euclid(86_400)).unwrap_or(self.day)
        } else {
            self.day
        }
    }

    /// Appends it as iCalendar writes it.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn write(&self, out: &mut String) -> Result<(), Error> {
        let (year, month, day) = civil_from_days(i64::from(self.day));
        let mut text = [0u8; 16];
        let mut at = 0;
        let mut put = |value: u32, width: usize| {
            for n in (0..width).rev() {
                let digit = (value / 10u32.pow(u32::try_from(n).unwrap_or(0))) % 10;
                if let Some(slot) = text.get_mut(at) {
                    *slot = b'0' + u8::try_from(digit).unwrap_or(0);
                }
                at += 1;
            }
        };
        put(u32::try_from(year.clamp(0, 9999)).unwrap_or(0), 4);
        put(u32::from(month), 2);
        put(u32::from(day), 2);
        if let Some(seconds) = self.seconds {
            if let Some(slot) = text.get_mut(at) {
                *slot = b'T';
            }
            at += 1;
            let mut put = |value: u32| {
                for digit in [value / 10 % 10, value % 10] {
                    if let Some(slot) = text.get_mut(at) {
                        *slot = b'0' + u8::try_from(digit).unwrap_or(0);
                    }
                    at += 1;
                }
            };
            put(seconds / 3600);
            put(seconds / 60 % 60);
            put(seconds % 60);
            if self.utc {
                if let Some(slot) = text.get_mut(at) {
                    *slot = b'Z';
                }
                at += 1;
            }
        }
        let written = text.get(..at).unwrap_or(&[]);
        push_str(out, core::str::from_utf8(written).unwrap_or(""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_and_dates_agree() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(days_from_civil(2026, 11, 18), 20_775);
        assert_eq!(civil_from_days(20_775), (2026, 11, 18));
        assert_eq!(weekday(20_775), 3, "a Wednesday");
        for days in [-1, 59, 60, 11_016, 19_782, 47_540] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
    }

    #[test]
    fn dates_and_times_are_read_and_written() {
        let date = DateTime::parse("20261120").unwrap();
        assert_eq!(date, DateTime::date(20_777));
        let utc = DateTime::parse("20261120T233000Z").unwrap();
        assert_eq!((utc.seconds, utc.utc), (Some(84_600), true));
        // Half past eleven at night in UTC is the next day in Berlin.
        assert_eq!(utc.local_day(3600), 20_778);
        assert_eq!(utc.local_day(0), 20_777);
        let floating = DateTime::parse("20261120T233000").unwrap();
        assert_eq!(floating.local_day(3600), 20_777);
        for text in ["20261120", "20261120T233000Z", "20261120T233000"] {
            let mut out = String::new();
            DateTime::parse(text).unwrap().write(&mut out).unwrap();
            assert_eq!(out, text);
        }
        assert_eq!(DateTime::from_unix(1_795_217_400), utc);
    }

    #[test]
    fn what_is_no_date_is_none() {
        for text in [
            "",
            "2026",
            "20261320",
            "20260230",
            "2026112",
            "20261120T25",
            "20261120T250000",
            "20261120X120000",
            "2026-11-20",
            "00000101",
        ] {
            assert_eq!(DateTime::parse(text), None, "{text}");
        }
    }
}
