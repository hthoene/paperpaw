//! The layout of *Forecast* without a host.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::layout::*;
use crate::native_now::layout::Fonts;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;

/// A font whose every character is half its size wide and whose lines are a
/// fifth taller than the size.
struct Even;

impl Fonts for Even {
    fn width(&self, size: f32, text: &str) -> Result<f32, Error> {
        Ok(text.chars().count() as f32 * size * 0.5)
    }

    fn line(&self, size: f32) -> Result<u16, Error> {
        Ok((size * 1.2) as u16)
    }

    fn effective(&self, size: f32) -> Result<f32, Error> {
        Ok(size)
    }
}

fn setup() -> Setup {
    Setup {
        text: 16.0,
        small: 12.0,
        title: 24.0,
        gap: 8,
        rule: 1,
        mm6: 24,
        fixed: None,
    }
}

const PARTS: Parts = Parts {
    symbol: true,
    low: true,
    rain: false,
};

#[test]
fn a_wide_place_takes_columns_a_tall_one_a_list() {
    assert!(!as_list(None, 400, 150, 5));
    assert!(as_list(None, 150, 400, 5));
    // Too narrow for the columns.
    assert!(as_list(None, 150, 100, 5));
    // As set.
    assert!(as_list(Some(2), 400, 150, 5));
    assert!(!as_list(Some(1), 150, 400, 5));
}

#[test]
fn as_many_columns_as_have_room() {
    assert_eq!(row_days(&Even, &setup(), 1000, 5).unwrap(), 5);
    let few = row_days(&Even, &setup(), 100, 5).unwrap();
    assert!((1..5).contains(&few), "{few}");
    assert_eq!(row_days(&Even, &setup(), 0, 5).unwrap(), 1);
}

#[test]
fn as_many_rows_as_have_a_line() {
    assert_eq!(list_days(&Even, &setup(), 400, 5).unwrap(), 5);
    assert_eq!(list_days(&Even, &setup(), 30, 5).unwrap(), 2);
    assert_eq!(list_days(&Even, &setup(), 0, 5).unwrap(), 1);
}

#[test]
fn a_column_keeps_its_parts_inside() {
    let plan = row(&Even, &setup(), (400, 200), 5, PARTS).unwrap();
    assert_eq!(plan.column, 80);
    assert!(plan.with_low);
    assert!(plan.side > 0);
    assert!(plan.total <= 200);
    assert!(plan.top + plan.total <= 200);
}

#[test]
fn without_room_below_the_lowest_goes_beside_or_away() {
    let plan = row(&Even, &setup(), (400, 60), 5, PARTS).unwrap();
    assert!(!plan.with_low);
    assert!(plan.low_beside);
    assert_eq!(plan.side, 0);
    let none = row(
        &Even,
        &setup(),
        (400, 60),
        5,
        Parts {
            low: false,
            ..PARTS
        },
    )
    .unwrap();
    assert!(!none.with_low && !none.low_beside);
}

#[test]
fn a_set_size_is_the_ceiling() {
    let mut set = setup();
    set.fixed = Some(14.0);
    let plan = row(&Even, &set, (400, 300), 5, PARTS).unwrap();
    assert!(plan.high <= 14.0 + f32::EPSILON, "{}", plan.high);
}

fn texts(n: usize) -> (Vec<String>, Vec<String>, Vec<Option<String>>, Vec<String>) {
    let highs = (0..n).map(|i| (10 + i).to_string() + "°").collect();
    let lows = (0..n).map(|i| (i).to_string() + "°").collect();
    let rains = (0..n).map(|_| Some("80 %".to_string())).collect();
    let names = (0..n).map(|_| "Monday".to_string()).collect();
    (highs, lows, rains, names)
}

fn list_of(width: u16, height: u16, n: usize, wants: Wants) -> List {
    let (highs, lows, rains, long) = texts(n);
    let short: Vec<String> = long.iter().map(|_| "Mon".to_string()).collect();
    let initial: Vec<String> = long.iter().map(|_| "M".to_string()).collect();
    let texts = Texts {
        highs: &highs,
        lows: &lows,
        rains: &rains,
        names: [&long, &short, &initial],
    };
    list(&Even, &setup(), (width, height), &texts, wants).unwrap()
}

const WANTS: Wants = Wants {
    symbol: true,
    low: true,
    bars: true,
};

#[test]
fn a_list_with_room_shows_everything() {
    let plan = list_of(400, 300, 5, WANTS);
    assert_eq!(plan.height, 60);
    assert!(plan.side > 0 && plan.rain_w > 0 && plan.low_w > 0);
    assert!(plan.bars.is_some());
    assert_eq!(plan.names, 0);
    assert!(plan.high_x + plan.high_w <= 400);
}

#[test]
fn a_narrow_list_gives_up_the_symbol_then_the_rain_then_the_lowest() {
    let narrow = list_of(120, 300, 5, WANTS);
    assert_eq!(narrow.side, 0);
    let narrower = list_of(90, 300, 5, WANTS);
    assert_eq!(narrower.rain_w, 0);
    let narrowest = list_of(60, 300, 5, WANTS);
    assert_eq!(narrowest.low_w, 0);
    assert!(narrowest.bars.is_none());
}

#[test]
fn the_names_get_shorter_before_the_digits_do() {
    let plan = list_of(130, 300, 5, Wants { symbol: false, ..WANTS });
    assert!(plan.names >= 1, "{plan:?}");
}

#[test]
fn odd_places_never_panic() {
    for (w, h) in [(0, 0), (1, 1), (5, 400), (400, 5), (65535, 65535)] {
        let _ = row(&Even, &setup(), (w, h), 7, PARTS);
        let _ = row(&Even, &setup(), (w, h), 0, PARTS);
        let _ = list_of(w, h, 7, WANTS);
        let _ = list_of(w, h, 1, WANTS);
    }
}
