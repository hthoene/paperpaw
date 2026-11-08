//! A refused allocation in the widgets' own logic ends in `Error::NoMemory`
//! and never aborts: each pure call is run once to count its allocations and
//! again with each of them refused. (The calls that need a host are covered
//! by the SDK's and the kit's sweeps of what they call, and by reading; the
//! SDK has no mock host yet.)

use crate::native_battery_shapes::{battery, bolt, ring};
use crate::native_moon::lit_spans_for_tests;
use crate::native_tiles::tiles;
use crate::native_word_clock::{capitalised, lit, upper};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::time::LocalTime;
use paperpaw_sdk::{Color, Point, Rect};

#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;

#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

fn time(hour: u8, minute: u8) -> LocalTime {
    // 2026-11-07 at the given time (UTC).
    let midnight = 1_794_009_600_i64;
    LocalTime::from_seconds(midnight + i64::from(hour) * 3600 + i64::from(minute) * 60)
}

#[test]
fn tiles_survive_refusals() {
    for (twelve, pairs) in [(true, false), (false, false), (true, true), (false, true)] {
        let total = fail_alloc::sweep(|| tiles(&time(13, 5), twelve, pairs).map(|_| ()));
        assert!(total > 0);
    }
}

#[test]
fn the_word_clock_text_helpers_survive_refusals() {
    fail_alloc::sweep(|| capitalised("viertel nach elf").map(|_| ()));
    fail_alloc::sweep(|| upper("fünf vor zwölf").map(|_| ()));
    let grid: Vec<Vec<char>> = ["ITLISASTIME", "ACQUARTERDC", "TWENTYFIVEX"]
        .iter()
        .map(|row| row.chars().collect())
        .collect();
    fail_alloc::sweep(|| lit(&grid, "it is quarter").map(|_| ()));
    fail_alloc::sweep(|| lit(&grid, "it is teatime").map(|_| ()));
}

#[test]
fn the_moon_rows_survive_refusals() {
    fail_alloc::sweep(|| lit_spans_for_tests(20, 90, false).map(|_| ()));
    fail_alloc::sweep(|| lit_spans_for_tests(20, 270, true).map(|_| ()));
}

#[test]
fn the_kit_note_helpers_survive_refusals() {
    let notes: Vec<String> = ["3 weeks", "7 Nov", "12 days left"]
        .iter()
        .map(|s| String::from(*s))
        .collect();
    fail_alloc::sweep(|| paperpaw_ui_kit::text::join_notes(&notes).map(|_| ()));
    fail_alloc::sweep(|| paperpaw_ui_kit::mem::strs(&notes).map(|_| ()));
    fail_alloc::sweep(|| paperpaw_ui_kit::mem::one(7_u8).map(|_| ()));
}

#[test]
fn the_battery_shapes_survive_refusals() {
    let ink = Color::BLACK;
    let colours = (ink, Color::WHITE);
    let total = fail_alloc::sweep(|| {
        let mut out = Vec::new();
        battery(
            &mut out,
            colours,
            Rect::new(0, 0, 106, 50),
            (60, true, false),
            1,
        )?;
        battery(
            &mut out,
            colours,
            Rect::new(0, 0, 50, 100),
            (60, false, true),
            1,
        )?;
        ring(&mut out, ink, (Point::new(40, 40), 30, 6), 60, 1)?;
        bolt(&mut out, Rect::new(0, 0, 20, 40), ink)?;
        Ok(out.len())
    });
    assert!(total > 0);
}
