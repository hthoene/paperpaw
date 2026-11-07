//! The words of the screens: durations and dates as the catalog writes
//! them. Over [`Words`], so the screens' rows are made and tested without a
//! host; the host's catalog is behind [`HostWords`].
//!
//! Local days: the caller gives a `day_of` (Unix seconds to a local day, as
//! `store::days` takes it), so summer time and the zone the user chose plug in
//! with the shell (see PORTING-7.md).

use alloc::string::String;

use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::{Error, Host};
use paperpaw_sdk::time::LocalTime;
use paperpaw_ui_kit::Messages;

use crate::store::Day;

const DAY: i64 = 86_400;

/// Catalog texts of the plugin, made fallibly.
pub(crate) trait Words {
    /// The text of `key`.
    fn copy(&mut self, key: &str) -> Result<String, Error>;
    /// The text of `key` with its names filled in; `Arg::Count` selects the
    /// plural form.
    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error>;
    /// `text` (digits) in the language's digits.
    fn digits(&mut self, text: &str) -> Result<String, Error>;
}

/// The host's catalog.
pub(crate) struct HostWords<'h, 'a> {
    host: &'h Host<'a>,
    messages: Messages,
}

impl<'h, 'a> HostWords<'h, 'a> {
    pub(crate) fn new(host: &'h Host<'a>) -> Result<Self, Error> {
        Ok(Self {
            host,
            messages: Messages::new()?,
        })
    }
}

impl Words for HostWords<'_, '_> {
    fn copy(&mut self, key: &str) -> Result<String, Error> {
        self.messages.copy(self.host, key)
    }
    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error> {
        self.messages.format(self.host, key, args)
    }
    fn digits(&mut self, text: &str) -> Result<String, Error> {
        self.messages.digits(self.host, text)
    }
}

/// The date of a local day.
pub(crate) fn date_of(day: i32) -> LocalTime {
    LocalTime::from_seconds(i64::from(day) * DAY)
}

/// Seconds read on `day`, from the days read (in order).
pub(crate) fn seconds_on(days: &[Day], day: i32) -> u32 {
    days.binary_search_by_key(&day, |d| d.day)
        .ok()
        .and_then(|i| days.get(i))
        .map_or(0, |d| d.seconds)
}

/// `1 h 05 min`, `42 min` or `< 1 min`.
pub(crate) fn duration(words: &mut impl Words, seconds: u64) -> Result<String, Error> {
    let minutes = seconds / 60;
    if minutes == 0 {
        if seconds == 0 {
            return words.format(
                "stats.minutes",
                &[Arg::Int {
                    name: "m",
                    value: 0,
                }],
            );
        }
        return words.copy("stats.under-a-minute");
    }
    if minutes < 60 {
        return words.format(
            "stats.minutes",
            &[Arg::Int {
                name: "m",
                value: minutes as i64,
            }],
        );
    }
    // Two digits of the minutes, made without allocating.
    let m = (minutes % 60) as u8;
    let digits = [b'0' + m / 10, b'0' + m % 10];
    let rest = words.digits(core::str::from_utf8(&digits).unwrap_or("00"))?;
    words.format(
        "stats.hours",
        &[
            Arg::Int {
                name: "h",
                value: (minutes / 60) as i64,
            },
            Arg::Text {
                name: "m",
                value: &rest,
            },
        ],
    )
}

/// `14.10.2026` or `10/14/2026`, as the language writes dates.
pub(crate) fn date(words: &mut impl Words, day: i32) -> Result<String, Error> {
    let d = date_of(day);
    words.format(
        "stats.date",
        &[
            Arg::Int {
                name: "day",
                value: i64::from(d.day),
            },
            Arg::Int {
                name: "month",
                value: i64::from(d.month),
            },
            Arg::Int {
                name: "year",
                value: i64::from(d.year),
            },
        ],
    )
}

/// The name of a weekday, 0 being Sunday.
pub(crate) fn weekday(words: &mut impl Words, weekday: u8) -> Result<String, Error> {
    const KEYS: [&str; 7] = [
        "stats.weekday.0",
        "stats.weekday.1",
        "stats.weekday.2",
        "stats.weekday.3",
        "stats.weekday.4",
        "stats.weekday.5",
        "stats.weekday.6",
    ];
    words.copy(
        KEYS.get(usize::from(weekday % 7))
            .copied()
            .unwrap_or(KEYS[0]),
    )
}

/// The short name of a weekday (two letters), 0 being Sunday.
pub(crate) fn weekday_short(words: &mut impl Words, weekday: u8) -> Result<String, Error> {
    const KEYS: [&str; 7] = [
        "stats.weekday-short.0",
        "stats.weekday-short.1",
        "stats.weekday-short.2",
        "stats.weekday-short.3",
        "stats.weekday-short.4",
        "stats.weekday-short.5",
        "stats.weekday-short.6",
    ];
    words.copy(
        KEYS.get(usize::from(weekday % 7))
            .copied()
            .unwrap_or(KEYS[0]),
    )
}

/// The name of a month, 1 being January.
pub(crate) fn month(words: &mut impl Words, month: u8) -> Result<String, Error> {
    const KEYS: [&str; 12] = [
        "stats.month.1",
        "stats.month.2",
        "stats.month.3",
        "stats.month.4",
        "stats.month.5",
        "stats.month.6",
        "stats.month.7",
        "stats.month.8",
        "stats.month.9",
        "stats.month.10",
        "stats.month.11",
        "stats.month.12",
    ];
    let at = usize::from(month.clamp(1, 12) - 1);
    words.copy(KEYS.get(at).copied().unwrap_or(KEYS[0]))
}

/// `Dienstag, 14. Oktober`.
pub(crate) fn long_date(words: &mut impl Words, day: i32) -> Result<String, Error> {
    let d = date_of(day);
    let (weekday, month) = (weekday(words, d.weekday)?, month(words, d.month)?);
    date_with(words, d.day, &weekday, &month)
}

/// `Di, 14. Okt.`: [`long_date`] with the weekday short and the month as
/// given, for where the long one does not fit.
pub(crate) fn short_date(
    words: &mut impl Words,
    day: i32,
    short_month: &str,
) -> Result<String, Error> {
    let d = date_of(day);
    let weekday = weekday_short(words, d.weekday)?;
    date_with(words, d.day, &weekday, short_month)
}

fn date_with(words: &mut impl Words, day: u8, weekday: &str, month: &str) -> Result<String, Error> {
    words.format(
        "stats.long-date",
        &[
            Arg::Text {
                name: "weekday",
                value: weekday,
            },
            Arg::Int {
                name: "day",
                value: i64::from(day),
            },
            Arg::Text {
                name: "month",
                value: month,
            },
        ],
    )
}

/// The first day of the month of `day`.
pub(crate) fn month_start(day: i32) -> i32 {
    day - i32::from(date_of(day).day) + 1
}
