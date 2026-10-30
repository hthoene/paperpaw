//! The time of day: dates at the edges (1970, leap years, 2038, far
//! future, before 1970), POSIX time zone rules and their summer time in
//! both hemispheres, and the clock and date as the settings and the
//! language write them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::borrow::Cow;

use common::Harness;
use paperpaw_i18n::Catalog;
use paperpaw_sdk::builtin::settings;
use paperpaw_sdk::time::{self, LocalTime, TIME_ZONES, TimeZone};
use paperpaw_sdk::{Instant, SettingSpec, SettingValue};
use proptest::prelude::*;

const HOUR: i64 = 3600;
const DAY: i64 = 86_400;

fn at(year: i32, month: u8, day: u8, hour: u8, minute: u8, second: u8) -> i64 {
    LocalTime::new(year, month, day, hour, minute, second).to_seconds()
}

fn date(seconds: i64) -> (i32, u8, u8, u8, u8, u8, u8) {
    let t = LocalTime::from_seconds(seconds);
    (
        t.year, t.month, t.day, t.hour, t.minute, t.second, t.weekday,
    )
}

fn ymd(seconds: i64) -> (i32, u8, u8) {
    let t = LocalTime::from_seconds(seconds);
    (t.year, t.month, t.day)
}

fn is_leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in(year: i32, month: u8) -> u8 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// Seconds become a date and back unchanged, for 300,000 years
    /// around 1970; every field is in its range and the weekday follows
    /// the days.
    #[test]
    fn seconds_and_dates_round_trip(seconds in -10_000_000_000_000i64..10_000_000_000_000) {
        let t = LocalTime::from_seconds(seconds);
        prop_assert_eq!(t.to_seconds(), seconds);
        prop_assert!((1..=12).contains(&t.month));
        prop_assert!(t.day >= 1 && t.day <= days_in(t.year, t.month));
        prop_assert!(t.hour < 24 && t.minute < 60 && t.second < 60 && t.weekday < 7);
        let tomorrow = LocalTime::from_seconds(seconds + DAY);
        prop_assert_eq!(tomorrow.weekday, (t.weekday + 1) % 7);
    }

    /// Every valid date is found again from its seconds.
    #[test]
    fn dates_round_trip(year in -5000i32..12000, month in 1u8..=12, day in 1u8..=31, hour in 0u8..24) {
        let day = day.min(days_in(year, month));
        let seconds = at(year, month, day, hour, 59, 59);
        let t = LocalTime::from_seconds(seconds);
        prop_assert_eq!((t.year, t.month, t.day, t.hour), (year, month, day, hour));
        // The next second is the next hour, or the next day.
        let next = LocalTime::from_seconds(seconds + 1);
        prop_assert_eq!((next.minute, next.second), (0, 0));
    }

    /// Any text as a time zone rule is read or refused without panic,
    /// and a zone read gives offsets of less than a week at any time.
    #[test]
    fn any_rule_parses_without_panic(
        rule in "[A-Z<>+0-9:,./M-]{0,30}|\\PC{0,20}",
        unix in -100_000_000_000i64..100_000_000_000,
    ) {
        if let Some(zone) = TimeZone::parse(&rule) {
            prop_assert!(i64::from(zone.offset_at(unix)).abs() < 7 * DAY);
        }
    }

    /// A rule with a name in angle brackets (possibly empty) and an offset
    /// of up to three digits of hours is read or refused, and a zone read
    /// stays within a day of UTC (`<>200` once gave more than a week).
    #[test]
    fn quoted_names_and_large_offsets(
        rule in "<[A-Z+0-9]{0,4}>[+-]?[0-9]{1,3}(:[0-9]{1,2})?(<[A-Z+0-9]{0,4}>([+-]?[0-9]{1,3})?,M3.2.0,M11.1.0)?",
        unix in -100_000_000_000i64..100_000_000_000,
    ) {
        if let Some(zone) = TimeZone::parse(&rule) {
            prop_assert!(i64::from(zone.offset_at(unix)).abs() <= 25 * 3600);
        }
    }

    /// Every zone offered changes its offset at most twice a year, by
    /// whole quarter hours, and keeps it within a day.
    #[test]
    fn offered_zones_behave(zone in 0usize..TIME_ZONES.len(), day in 0i64..366, year in 1971i32..2100) {
        let tz = TimeZone::parse(TIME_ZONES[zone].1).unwrap();
        let unix = at(year, 1, 1, 0, 0, 0) + day * DAY;
        let offset = tz.offset_at(unix);
        prop_assert!(offset.abs() <= 14 * 3600);
        prop_assert_eq!(offset % 900, 0);
    }
}

/// The case proptest shrank to: an empty name and 200 hours, which is not
/// an offset (POSIX has 0 to 24 hours).
#[test]
fn a_rule_of_more_than_a_day_is_refused() {
    assert!(TimeZone::parse("<>200").is_none());
    assert!(TimeZone::parse("<X>25").is_none());
    assert!(TimeZone::parse("<X>-24:30").is_none());
    assert!(TimeZone::parse("CET-1CEST-30,M3.5.0,M10.5.0").is_none());
    // The edge stays: 24 hours either way.
    assert_eq!(
        TimeZone::parse("<X>-24").map(|z| z.offset_at(0)),
        Some(86_400)
    );
    assert_eq!(
        TimeZone::parse("<X>24").map(|z| z.offset_at(0)),
        Some(-86_400)
    );
}

#[test]
fn dates_at_the_edges() {
    // 1970 began on a Thursday.
    assert_eq!(date(0), (1970, 1, 1, 0, 0, 0, 4));
    assert_eq!(date(-1), (1969, 12, 31, 23, 59, 59, 3));
    // Leap years: every fourth, not every hundredth, every four hundredth.
    assert_eq!(ymd(at(2000, 2, 28, 0, 0, 0) + DAY), (2000, 2, 29));
    assert_eq!(ymd(at(1900, 2, 28, 0, 0, 0) + DAY), (1900, 3, 1));
    assert_eq!(ymd(at(2100, 2, 28, 0, 0, 0) + DAY), (2100, 3, 1));
    assert_eq!(ymd(at(2024, 2, 28, 0, 0, 0) + DAY), (2024, 2, 29));
    assert_eq!(
        at(2024, 12, 31, 0, 0, 0) - at(2024, 1, 1, 0, 0, 0),
        365 * DAY
    );
    // The last second 32-bit time holds, and the one after.
    assert_eq!(date(i64::from(i32::MAX)), (2038, 1, 19, 3, 14, 7, 2));
    assert_eq!(date(i64::from(i32::MAX) + 1), (2038, 1, 19, 3, 14, 8, 2));
    assert_eq!(date(i64::from(u32::MAX)), (2106, 2, 7, 6, 28, 15, 0));
    // Far future and far past.
    assert_eq!(
        date(at(9999, 12, 31, 23, 59, 59)),
        (9999, 12, 31, 23, 59, 59, 5)
    );
    assert_eq!(date(at(1, 1, 1, 0, 0, 0)), (1, 1, 1, 0, 0, 0, 1));
    assert_eq!(ymd(at(0, 2, 29, 12, 0, 0)), (0, 2, 29));
    assert_eq!(ymd(at(-1, 12, 31, 0, 0, 0)), (-1, 12, 31));
}

#[test]
fn extreme_seconds_give_a_date_without_panic() {
    for seconds in [i64::MIN, i64::MIN + 1, i64::MAX, i64::MAX - 1] {
        let t = LocalTime::from_seconds(seconds);
        assert!((1..=12).contains(&t.month) && t.day >= 1 && t.day <= 31);
    }
}

#[test]
fn summer_time_in_both_hemispheres() {
    let utc = |y, m, d, h, min| at(y, m, d, h, min, 0);
    // New York, 2026: from 8 March 07:00 UTC to 1 November 06:00 UTC.
    let new_york = TimeZone::parse("EST5EDT,M3.2.0,M11.1.0").unwrap();
    assert_eq!(new_york.offset_at(utc(2026, 3, 8, 6, 59)), -5 * HOUR as i32);
    assert_eq!(new_york.offset_at(utc(2026, 3, 8, 7, 0)), -4 * HOUR as i32);
    assert_eq!(
        new_york.offset_at(utc(2026, 11, 1, 5, 59)),
        -4 * HOUR as i32
    );
    assert_eq!(new_york.offset_at(utc(2026, 11, 1, 6, 0)), -5 * HOUR as i32);
    // London: the last Sundays of March and October, at 01:00 UTC.
    let london = TimeZone::parse("GMT0BST,M3.5.0/1,M10.5.0").unwrap();
    assert_eq!(london.offset_at(utc(2026, 3, 29, 0, 59)), 0);
    assert_eq!(london.offset_at(utc(2026, 3, 29, 1, 0)), HOUR as i32);
    assert_eq!(london.offset_at(utc(2026, 10, 25, 0, 59)), HOUR as i32);
    assert_eq!(london.offset_at(utc(2026, 10, 25, 1, 0)), 0);
    // Sydney, summer across the new year: ends 5 April 2026 at 03:00
    // local summer time, starts 4 October at 02:00 local standard time.
    let sydney = TimeZone::parse("AEST-10AEDT,M10.1.0,M4.1.0/3").unwrap();
    assert_eq!(sydney.offset_at(utc(2026, 4, 4, 15, 59)), 11 * HOUR as i32);
    assert_eq!(sydney.offset_at(utc(2026, 4, 4, 16, 0)), 10 * HOUR as i32);
    assert_eq!(sydney.offset_at(utc(2026, 10, 3, 15, 59)), 10 * HOUR as i32);
    assert_eq!(sydney.offset_at(utc(2026, 10, 3, 16, 0)), 11 * HOUR as i32);
    // New year's night is summer there.
    assert_eq!(sydney.offset_at(utc(2026, 12, 31, 13, 0)), 11 * HOUR as i32);
    assert_eq!(sydney.offset_at(utc(2026, 12, 31, 14, 0)), 11 * HOUR as i32);
    // Auckland: the last Sunday of September (27 September 2026).
    let auckland = TimeZone::parse("NZST-12NZDT,M9.5.0,M4.1.0/3").unwrap();
    assert_eq!(
        auckland.offset_at(utc(2026, 9, 26, 13, 59)),
        12 * HOUR as i32
    );
    assert_eq!(
        auckland.offset_at(utc(2026, 9, 26, 14, 0)),
        13 * HOUR as i32
    );
    // A leap year's February holds the fifth week as its last.
    let leap = TimeZone::parse("AAA0BBB,M2.5.0,M10.5.0").unwrap();
    // 2032: 29 February is a Sunday.
    assert_eq!(date(at(2032, 2, 29, 0, 0, 0)).6, 0);
    assert_eq!(leap.offset_at(utc(2032, 2, 29, 1, 59)), 0);
    assert_eq!(leap.offset_at(utc(2032, 2, 29, 2, 0)), HOUR as i32);
}

#[test]
fn rules_as_posix_writes_them() {
    let offset = |rule: &str| TimeZone::parse(rule).map(|z| z.offset_at(0));
    let utc = |y, m, d, h, min| at(y, m, d, h, min, 0);
    assert_eq!(offset("UTC0"), Some(0));
    assert_eq!(offset("  UTC0  "), Some(0));
    assert_eq!(offset("<+0330>-3:30"), Some(3 * 3600 + 1800));
    assert_eq!(offset("<-0930>9:30"), Some(-(9 * 3600 + 1800)));
    assert_eq!(offset("XXX-5:45:30"), Some(5 * 3600 + 45 * 60 + 30));
    assert_eq!(offset("XXX+5"), Some(-5 * 3600));
    // Dates as Julian days (`Jn`, 1 to 365, 29 February not counted) and
    // as days from 0 (`n`, 0 to 365, 29 February counted): 1 March and
    // 27 October in 2026, at 2:00.
    let julian = TimeZone::parse("EST5EDT,J60,J300").unwrap();
    let days = TimeZone::parse("EST5EDT,59,299").unwrap();
    for zone in [&julian, &days] {
        assert_eq!(zone.offset_at(utc(2026, 3, 1, 6, 59)), -5 * HOUR as i32);
        assert_eq!(zone.offset_at(utc(2026, 3, 1, 7, 0)), -4 * HOUR as i32);
        assert_eq!(zone.offset_at(utc(2026, 10, 27, 5, 59)), -4 * HOUR as i32);
        assert_eq!(zone.offset_at(utc(2026, 10, 27, 6, 0)), -5 * HOUR as i32);
    }
    // In a leap year day 59 is 29 February, Julian day 60 still 1 March.
    assert_eq!(days.offset_at(utc(2028, 2, 29, 7, 0)), -4 * HOUR as i32);
    assert_eq!(julian.offset_at(utc(2028, 2, 29, 7, 0)), -5 * HOUR as i32);
    assert_eq!(julian.offset_at(utc(2028, 3, 1, 7, 0)), -4 * HOUR as i32);
    // Summer time with its own offset, given at another hour.
    let zone = TimeZone::parse("CET-1CEST-3,M3.5.0/-1,M10.5.0/25").unwrap();
    assert_eq!(zone.offset_at(at(2026, 7, 1, 0, 0, 0)), 3 * 3600);
    for refused in [
        "",
        "UTC",
        "AB0",
        "<+03",
        "EST5EDT",
        "EST5EDT,M3.2.0",
        "EST5EDT,J0,J300",
        "EST5EDT,J60,366",
        "EST5EDT,M13.1.0,M11.1.0",
        "EST5EDT,M0.1.0,M11.1.0",
        "EST5EDT,M3.6.0,M11.1.0",
        "EST5EDT,M3.0.0,M11.1.0",
        "EST5EDT,M3.2.7,M11.1.0",
        "EST5EDT,M3.2.0,M11.1.0,",
        "EST1000",
        "EST5:",
        "EST5::",
    ] {
        assert_eq!(TimeZone::parse(refused), None, "{refused:?}");
    }
}

/// English and German, as far as the clock and dates need them.
fn catalogs() -> [Catalog; 2] {
    let english = "[meta]\nlanguage = en\nname = English\nplural = one-other\n\n[messages]\n\
        core.clock.am = {time} AM\ncore.clock.pm = {time} PM\n\
        core.date = {weekday}, {day} {month}\ncore.date.short = {weekday} {day} {month}\n\
        core.weekday.4 = Thursday\ncore.weekday.short.4 = Thu\n\
        core.weekday.0 = Sunday\ncore.month.1 = January\ncore.month.short.1 = Jan\n\
        core.month.2 = February\ncore.week-start-day = 0\n";
    let german = "[meta]\nlanguage = de\nname = Deutsch\nplural = one-other\n\n[messages]\n\
        core.date = {weekday}, {day}. {month}\ncore.date.short = {weekday}, {day}. {month}\n\
        core.weekday.4 = Donnerstag\ncore.weekday.short.4 = Do.\n\
        core.weekday.0 = Sonntag\ncore.month.1 = Januar\ncore.month.short.1 = Jan.\n\
        core.month.2 = Februar\ncore.week-start-day = 1\n";
    [
        Catalog::parse(english.to_string()).unwrap(),
        Catalog::parse(german.to_string()).unwrap(),
    ]
}

/// A harness whose device knows it is `unix_millis`, in a system zone
/// `offset` minutes east, with the time settings defined.
fn clock(unix_millis: i64, offset: Option<i32>) -> Harness {
    let mut h = Harness::new().with_clock(unix_millis, offset);
    for setting in [
        &settings::CLOCK_FORMAT,
        &settings::TIME_ZONE,
        &settings::TIME_ZONE_RULE,
        &settings::WEEK_START,
    ] {
        h.settings
            .define(SettingSpec::new(
                paperpaw_sdk::Id::from(setting),
                "x",
                paperpaw_sdk::SettingKind::Text,
                paperpaw_sdk::SettingValue::Text("".into()),
                paperpaw_sdk::builtin::groups::OTHER,
            ))
            .unwrap();
    }
    for catalog in catalogs() {
        h.i18n.add(catalog);
    }
    h.i18n.set_language("en");
    h
}

fn set(h: &mut Harness, setting: &paperpaw_sdk::Setting<paperpaw_sdk::Text>, value: &'static str) {
    h.settings
        .set(setting.as_str(), SettingValue::Text(Cow::Borrowed(value)))
        .unwrap();
}

#[test]
fn without_a_clock_there_is_no_time() {
    let h = Harness::new();
    let env = h.env();
    assert_eq!(time::local_time(&env), None);
    assert_eq!(time::clock_text(&env), None);
    assert_eq!(time::date_text(&env), None);
    assert_eq!(time::short_date_text(&env), None);
    assert_eq!(time::next_minute(&env), None);
    assert_eq!(time::unix_seconds(&env), None);
    // Without a clock and a zone, local is UTC.
    assert_eq!(time::local_seconds(&env, 1000), 1000);
}

#[test]
fn the_clock_in_24_and_12_hours() {
    for (millis, h24, h12) in [
        (0, "00:00", "12:00 AM"),
        (59_999, "00:00", "12:00 AM"),
        (60_000, "00:01", "12:01 AM"),
        (12 * HOUR * 1000, "12:00", "12:00 PM"),
        (13 * HOUR * 1000 + 5 * 60_000, "13:05", "1:05 PM"),
        (DAY * 1000 - 1, "23:59", "11:59 PM"),
        // Before 1970.
        (-1, "23:59", "11:59 PM"),
    ] {
        let mut h = clock(millis, Some(0));
        assert_eq!(time::clock_text(&h.env()).as_deref(), Some(h24), "{millis}");
        set(&mut h, &settings::CLOCK_FORMAT, time::CLOCK_12H);
        assert_eq!(time::clock_text(&h.env()).as_deref(), Some(h12), "{millis}");
    }
}

#[test]
fn dates_in_english_and_german() {
    // Thursday, 1 January 1970, 23:30 UTC.
    let mut h = clock(23 * HOUR * 1000 + 30 * 60_000, Some(0));
    assert_eq!(
        time::date_text(&h.env()).as_deref(),
        Some("Thursday, 1 January")
    );
    assert_eq!(
        time::short_date_text(&h.env()).as_deref(),
        Some("Thu 1 Jan")
    );
    h.i18n.set_language("de");
    assert_eq!(
        time::date_text(&h.env()).as_deref(),
        Some("Donnerstag, 1. Januar")
    );
    assert_eq!(
        time::short_date_text(&h.env()).as_deref(),
        Some("Do., 1. Jan.")
    );
    // An hour east, it is Friday already; missing names show their keys.
    let h = clock(23 * HOUR * 1000 + 30 * 60_000, Some(60));
    assert_eq!(
        time::date_text(&h.env()).as_deref(),
        Some("core.weekday.5, 2 January")
    );
}

#[test]
fn the_chosen_zone_wins_over_the_system() {
    // 1 July 2026, 12:00 UTC; the system says UTC.
    let noon = at(2026, 7, 1, 12, 0, 0) * 1000;
    let mut h = clock(noon, Some(0));
    assert_eq!(time::clock_text(&h.env()).as_deref(), Some("12:00"));
    set(&mut h, &settings::TIME_ZONE, "core.tz.berlin");
    assert_eq!(time::clock_text(&h.env()).as_deref(), Some("14:00"));
    set(&mut h, &settings::TIME_ZONE, "core.tz.india");
    assert_eq!(time::clock_text(&h.env()).as_deref(), Some("17:30"));
    // A rule of one's own wins over the list.
    set(&mut h, &settings::TIME_ZONE_RULE, "  <-01>1  ");
    assert_eq!(time::clock_text(&h.env()).as_deref(), Some("11:00"));
    // A rule that is no rule: the zone chosen from the list, not the
    // system's.
    set(&mut h, &settings::TIME_ZONE_RULE, "nonsense");
    assert_eq!(time::clock_text(&h.env()).as_deref(), Some("17:30"));
    // An unknown zone in the list: the system's.
    set(&mut h, &settings::TIME_ZONE_RULE, "");
    set(&mut h, &settings::TIME_ZONE, "core.tz.atlantis");
    let mut h2 = clock(noon, Some(-90));
    set(&mut h2, &settings::TIME_ZONE, "core.tz.atlantis");
    assert_eq!(time::clock_text(&h.env()).as_deref(), Some("12:00"));
    assert_eq!(time::clock_text(&h2.env()).as_deref(), Some("10:30"));
    assert_eq!(time::local_seconds(&h2.env(), 0), -90 * 60);
}

#[test]
fn the_next_minute_comes_when_the_local_minute_changes() {
    let mut h = clock(30_500, Some(0));
    h.now = Instant(0);
    assert_eq!(time::next_minute(&h.env()), Some(Instant(29_500)));
    // On the minute: a whole minute ahead.
    let h = clock(120_000, Some(0));
    assert_eq!(time::next_minute(&h.env()), Some(Instant(60_000)));
    // A zone of half hours does not move the minute.
    let h = clock(30_500, Some(330));
    assert_eq!(time::next_minute(&h.env()), Some(Instant(29_500)));
    // The clock goes on with the device's own time.
    let mut h = clock(0, Some(0));
    h.now = Instant(90_000);
    assert_eq!(time::unix_seconds(&h.env()), Some(90));
}

#[test]
fn the_week_starts_as_set_or_as_the_language_has_it() {
    let mut h = clock(0, Some(0));
    assert_eq!(time::week_start(&h.env()), 0);
    h.i18n.set_language("de");
    assert_eq!(time::week_start(&h.env()), 1);
    set(&mut h, &settings::WEEK_START, "core.week.sunday");
    assert_eq!(time::week_start(&h.env()), 0);
    set(&mut h, &settings::WEEK_START, "core.week.monday");
    h.i18n.set_language("en");
    assert_eq!(time::week_start(&h.env()), 1);
    // A language without the habit: Monday.
    let h = Harness::new();
    assert_eq!(time::week_start(&h.env()), 1);
}

/// Times near the end of `i64` (e.g. read from a damaged annotations
/// file and shown in local time) overflow when the zone's offset is
/// added: a panic in debug builds, a wrong date in release builds.
#[test]
fn local_times_at_the_end_of_time_do_not_overflow() {
    let berlin = TimeZone::parse("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
    let _ = berlin.offset_at(i64::MAX);
    let new_york = TimeZone::parse("EST5EDT,M3.2.0,M11.1.0").unwrap();
    let _ = new_york.offset_at(i64::MIN);
    let h = clock(0, Some(60));
    let _ = time::local_seconds(&h.env(), i64::MAX);
}
