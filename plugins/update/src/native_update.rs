//! What the update decides without the host: how long to wait before the
//! weekly look, which periods a check is the first of (for the server's
//! counting of devices), how long a device has been in use, and which
//! texts read as versions. The same rules as the interface-6 plugin
//! (`look.rs`, `count.rs`, `online.rs`), without allocation. Asking the
//! server, the files, the page and the install are the native plugin's (see
//! `PORTING-7.md`).
use paperpaw_sdk::time::{civil_from_days, iso_week};

const WEEK: u64 = 7 * 24 * 3600;
/// The longest version that names a file in `updates`.
pub const MAX_VERSION: usize = 64;

/// How long to wait before looking, with `last` the last look and `now` the
/// time, where known: the rest of the week; a week where the clock is not
/// known now but a look was (not again at every start).
pub fn wait(last: Option<i64>, now: Option<i64>) -> u64 {
    match (last, now) {
        (Some(last), Some(now)) => WEEK.saturating_sub(now.saturating_sub(last).max(0) as u64),
        (Some(_), None) => WEEK,
        (None, _) => 0,
    }
}

/// The periods of which a check is the first: the day, the ISO week and the
/// month.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Periods {
    pub day: bool,
    pub week: bool,
    pub month: bool,
}

impl Periods {
    /// The names the server counts by, in order.
    pub fn names(self) -> impl Iterator<Item = &'static str> {
        [
            (self.day, "day"),
            (self.week, "week"),
            (self.month, "month"),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
    }
}

/// The ISO week of the day `days` after 1970-01-01, with its year (which is
/// the next or the last near the turn of the year).
fn week(days: i64) -> (i32, u8) {
    let (year, month, day) = civil_from_days(days);
    let week = iso_week(year, month, day);
    let year = match (month, week) {
        (1, 52..) => year - 1,
        (12, 1) => year + 1,
        _ => year,
    };
    (year, week)
}

fn month(days: i64) -> (i32, u8) {
    let (year, month, _) = civil_from_days(days);
    (year, month)
}

/// The periods of which a check on `today` is the first after one on `last`
/// (days since 1970). A day before `last` (the clock went back) is the first
/// of none.
pub fn first(last: Option<i64>, today: i64) -> Periods {
    let Some(last) = last else {
        return Periods {
            day: true,
            week: true,
            month: true,
        };
    };
    Periods {
        day: today > last,
        week: week(today) > week(last),
        month: month(today) > month(last),
    }
}

/// How long a device has been in use, as the server counts it: under seven
/// days `new`, up to thirty `week`, after that `month`.
pub fn age(first: i64, today: i64) -> &'static str {
    match today.saturating_sub(first) {
        ..7 => "new",
        7..=30 => "week",
        _ => "month",
    }
}

/// Whether `version` is a pre-release (`0.5.0-beta.1`, `0.5.0-dev`).
pub fn is_pre_release(version: &str) -> bool {
    version.split('+').next().is_some_and(|v| v.contains('-'))
}

/// Whether `version` reads as a version: three numbers, then a pre-release
/// (`-dev`) or build (`+abc`) of letters, digits, `.` and `-` where it has
/// one. Nothing else may name a file in `updates`.
pub fn is_version(version: &str) -> bool {
    let (numbers, rest) = version.split_once(['-', '+']).unwrap_or((version, ""));
    let mut count = 0;
    let numbers_read = numbers.split('.').all(|number| {
        count += 1;
        !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())
    });
    numbers_read
        && count == 3
        && version.len() <= MAX_VERSION
        && rest
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-+.".contains(&b))
}
