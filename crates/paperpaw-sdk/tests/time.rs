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
use paperpaw_sdk::time::{self, LocalTime, TimeZone};
use paperpaw_sdk::{SettingSpec, SettingValue};
use proptest::prelude::*;

const HOUR: i64 = 3600;
const DAY: i64 = 86_400;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]



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
fn extreme_seconds_give_a_date_without_panic() {
    for seconds in [i64::MIN, i64::MIN + 1, i64::MAX, i64::MAX - 1] {
        let t = LocalTime::from_seconds(seconds);
        assert!((1..=12).contains(&t.month) && t.day >= 1 && t.day <= 31);
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
