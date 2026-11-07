//! The words of the screens: durations and dates as the catalog writes
//! them. Over [`Words`], so the screens' rows are made and tested without a
//! host; the host's catalog is behind [`HostWords`].
//!
//! Local days: the time zone's offset in seconds is given; reading the zone
//! the user chose is the shell's (see PORTING-7.md).

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

/// The local day (days since 1970) of a moment in Unix seconds, `offset`
/// seconds east of UTC.
pub(crate) fn day_of(unix: u32, offset: i32) -> i32 {
    (i64::from(unix) + i64::from(offset)).div_euclid(DAY) as i32
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
