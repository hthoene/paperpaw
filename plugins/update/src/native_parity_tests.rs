//! The native decisions (`native_update`) against the interface-6 ones
//! (`look`, `count`, `online`): equal on the same input.
use crate::native_update as native;
use crate::{count, look, online};
use alloc::format;
use alloc::vec::Vec;

#[test]
fn the_weekly_look_waits_as_before() {
    let day = 24 * 3600;
    let times = [
        None,
        Some(0),
        Some(1000),
        Some(2 * day),
        Some(30 * day),
        Some(-day),
    ];
    for last in times {
        for now in times {
            assert_eq!(
                native::wait(last, now),
                look::wait(last, now),
                "{last:?} {now:?}"
            );
        }
    }
    assert_eq!(native::wait(Some(10 * day), Some(day)), 7 * day as u64);
}

#[test]
fn the_periods_and_the_age_are_those_of_interface_6() {
    // Every day over two years around the turn of 2020 to 2021 (a week of 53)
    // and of 2024 to 2025, and in each combination with the last one.
    let days: Vec<i64> = (18_250..18_250 + 800).collect();
    for &today in days.iter().step_by(3) {
        for last in days.iter().copied().step_by(17).map(Some).chain([None]) {
            let old = count::first(last, today);
            let new: Vec<&str> = native::first(last, today).names().collect();
            assert_eq!(old, new, "{last:?} {today}");
        }
        for first in [
            today - 40,
            today - 30,
            today - 7,
            today - 6,
            today,
            today + 3,
        ] {
            assert_eq!(native::age(first, today), count::age(first, today));
        }
    }
    assert_eq!(
        native::age(i64::MIN, i64::MAX),
        count::age(i64::MIN, i64::MAX)
    );
}

#[test]
fn versions_read_as_before() {
    for version in [
        "0.5.0",
        "0.5.0-beta.1",
        "0.5.0-dev",
        "0.5.0+abc",
        "0.5.0-dev+abc",
        "0.5",
        "0.5.0.1",
        "0..0",
        ".0.0",
        "a.b.c",
        "1.2.3-",
        "1.2.3-é",
        "1.2.3 4",
        "1.2.3/../x",
        "",
        "-",
        "+",
        "1.2.3-beta+build-1.x",
        &"1".repeat(64),
        &format!("1.2.3-{}", "x".repeat(70)),
        &format!("1.2.3-{}", "x".repeat(57)),
        &format!("1.2.3-{}", "x".repeat(58)),
    ] {
        assert_eq!(
            native::is_version(version),
            online::is_version(version),
            "{version}"
        );
        assert_eq!(
            native::is_pre_release(version),
            online::is_pre_release(version),
            "{version}"
        );
    }
}
