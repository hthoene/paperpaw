//! What the update decides without the host: how long to wait before the
//! weekly look, which periods a check is the first of (for the server's
//! counting of devices), how long a device has been in use, and which
//! texts read as versions. The rules are the ones interface 6 had, without
//! allocation. Asking the server, the files, the page and the install are the
//! native plugin's (see `PORTING-7.md`).
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

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{format, vec::Vec};
    use paperpaw_sdk::time::days_from_civil;

    fn day(year: i32, month: u8, day: u8) -> i64 {
        days_from_civil(year, month, day)
    }

    fn first_names(last: Option<i64>, today: i64) -> Vec<&'static str> {
        first(last, today).names().collect()
    }

    #[test]
    fn the_weekly_look_waits_the_rest_of_the_week() {
        let day = 24 * 3600;
        assert_eq!(wait(None, Some(1000)), 0);
        assert_eq!(wait(None, None), 0);
        assert_eq!(wait(Some(0), Some(2 * day)), WEEK - 2 * day as u64);
        assert_eq!(wait(Some(0), Some(30 * day)), 0);
        assert_eq!(wait(Some(0), None), WEEK);
        // A clock set back: a whole week, never more.
        assert_eq!(wait(Some(10 * day), Some(day)), WEEK);
        assert_eq!(wait(Some(-day), Some(0)), WEEK - day as u64);
        assert_eq!(wait(Some(i64::MIN), Some(i64::MAX)), 0);
        assert_eq!(wait(Some(i64::MAX), Some(i64::MIN)), WEEK);
    }

    #[test]
    fn the_first_check_of_a_device_is_the_first_of_everything() {
        assert_eq!(
            first_names(None, day(2026, 10, 25)),
            ["day", "week", "month"]
        );
    }

    #[test]
    fn a_second_check_the_same_day_is_the_first_of_nothing() {
        let today = day(2026, 10, 21);
        assert!(first_names(Some(today), today).is_empty());
    }

    #[test]
    fn weeks_begin_on_monday() {
        // Sunday 25 October 2026, then Monday 26.
        assert_eq!(
            first_names(Some(day(2026, 10, 24)), day(2026, 10, 25)),
            ["day"]
        );
        assert_eq!(
            first_names(Some(day(2026, 10, 25)), day(2026, 10, 26)),
            ["day", "week"]
        );
    }

    #[test]
    fn months_begin_on_the_first() {
        // Saturday 31 October 2026, Sunday 1 November: the same week.
        assert_eq!(
            first_names(Some(day(2026, 10, 31)), day(2026, 11, 1)),
            ["day", "month"]
        );
        // A month later, the same day of the month.
        assert_eq!(
            first_names(Some(day(2026, 10, 14)), day(2026, 11, 14)),
            ["day", "week", "month"]
        );
    }

    #[test]
    fn weeks_across_the_turn_of_the_year_are_iso_weeks() {
        // Monday 29 December 2025 starts week 1 of 2026: Thursday 1 January is
        // in it, a new month but not a new week.
        assert_eq!(
            first_names(Some(day(2025, 12, 29)), day(2026, 1, 1)),
            ["day", "month"]
        );
        // Week 53 of 2026 runs into January 2027; week 1 of 2027 begins on
        // Monday 4 January.
        assert_eq!(
            first_names(Some(day(2026, 12, 31)), day(2027, 1, 3)),
            ["day", "month"]
        );
        assert_eq!(
            first_names(Some(day(2027, 1, 3)), day(2027, 1, 4)),
            ["day", "week"]
        );
    }

    #[test]
    fn a_clock_gone_back_counts_nothing_again() {
        assert!(first_names(Some(day(2026, 10, 25)), day(2026, 9, 1)).is_empty());
    }

    /// The ISO week by its own rule (the week of its Thursday), apart from the
    /// SDK's: every day across the turns of 2020 to 2021 (a year of 53 weeks),
    /// 2025 to 2026 and 2026 to 2027, against every earlier check kept.
    #[test]
    fn the_periods_are_the_iso_weeks_and_the_months_over_years() {
        fn reference(days: i64) -> (i64, i64) {
            let thursday = days - (days + 3).rem_euclid(7) + 3;
            let (year, _, _) = civil_from_days(thursday);
            let new_year = days_from_civil(year, 1, 1);
            (i64::from(year), (thursday - new_year) / 7 + 1)
        }
        let days: Vec<i64> = (day(2020, 6, 1)..day(2027, 6, 1)).collect();
        for &today in days.iter().step_by(3) {
            for last in days
                .iter()
                .copied()
                .filter(|last| last % 11 == 0)
                .map(Some)
                .chain([None])
            {
                let got = first(last, today);
                let Some(last) = last else {
                    assert_eq!(first_names(None, today), ["day", "week", "month"]);
                    continue;
                };
                assert_eq!(got.day, today > last, "{last} {today}");
                assert_eq!(
                    got.week,
                    reference(today) > reference(last),
                    "{last} {today}"
                );
                let (ty, tm, _) = civil_from_days(today);
                let (ly, lm, _) = civil_from_days(last);
                assert_eq!(got.month, (ty, tm) > (ly, lm), "{last} {today}");
            }
        }
    }

    #[test]
    fn the_age_is_new_under_a_week_then_week_then_month() {
        let first = day(2026, 10, 1);
        assert_eq!(age(first, first), "new");
        assert_eq!(age(first, first + 6), "new");
        assert_eq!(age(first, first + 7), "week");
        assert_eq!(age(first, first + 30), "week");
        assert_eq!(age(first, first + 31), "month");
        assert_eq!(age(first, first + 400), "month");
        // A day before the first (the clock went back) is new.
        assert_eq!(age(first, first - 3), "new");
        assert_eq!(age(i64::MIN, i64::MAX), "month");
    }

    #[test]
    fn versions_are_read_as_semver() {
        for good in [
            "0.5.0",
            "10.20.30",
            "0.5.0-dev",
            "0.5.0-beta.1",
            "0.5.0+abc",
            "0.5.0-dev+abc",
            "1.0.0-rc.1+build-7",
            "1.2.3-beta+build-1.x",
            "1.2.3-",
            // The longest that names a file in `updates`: 64 bytes.
            &format!("1.2.3-{}", "x".repeat(58)),
        ] {
            assert!(is_version(good), "{good}");
        }
        for bad in [
            "",
            "-",
            "+",
            "0.5",
            "0.5.0.1",
            "0..0",
            ".0.0",
            "a.b.c",
            "0..5",
            "0.5.0/x",
            "1.2.3 4",
            "1.2.3/../x",
            "1.2.3-é",
            "0.5.0-a b",
            "0.5.0-a\\b",
            &"1".repeat(64),
            &format!("1.2.3-{}", "x".repeat(59)),
            &format!("1.2.3-{}", "x".repeat(70)),
        ] {
            assert!(!is_version(bad), "{bad}");
        }
    }

    #[test]
    fn pre_releases_are_told_from_builds() {
        for pre in ["0.5.0-beta.1", "0.5.0-dev", "0.5.0-dev+abc", "1.2.3-"] {
            assert!(is_pre_release(pre), "{pre}");
        }
        for release in ["0.5.0", "0.5.0+abc", "0.5.0+abc-def", ""] {
            assert!(!is_pre_release(release), "{release}");
        }
    }
}
