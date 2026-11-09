//! The layout of *Weather now* without a host.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::layout::*;
use alloc::vec::Vec;
use paperpaw_sdk::{Rect, native::Error};

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
        small: 14.0,
        title: 24.0,
        gap: 8,
        fixed: None,
        symbol: 0,
        placing: 0,
    }
}

fn lines() -> Vec<Line> {
    alloc::vec![
        Line::new("Berlin", 14.0, true, 3).unwrap(),
        Line::new("Partly cloudy", 18.0, false, 5).unwrap(),
        Line::new("H 12° · L 3°", 14.0, false, 4).unwrap(),
        Line::new("Feels like 9°", 14.0, true, 2).unwrap(),
        Line::new("Humidity 80 %", 14.0, true, 1).unwrap(),
    ]
}

#[test]
fn a_wide_place_puts_the_symbol_beside() {
    let plan = plan(&Even, &setup(), Rect::new(0, 0, 400, 150), "12°C", &lines()).unwrap();
    assert_eq!(plan.placing, Placing::Beside);
    assert!(plan.symbol.width > 0);
    assert!(plan.size > 0.0);
    assert!(plan.caption);
}

#[test]
fn a_tall_place_puts_the_symbol_above() {
    let plan = plan(&Even, &setup(), Rect::new(0, 0, 150, 400), "12°C", &lines()).unwrap();
    assert_eq!(plan.placing, Placing::Above);
    assert_eq!(plan.symbol.y, 0);
}

#[test]
fn no_symbol_is_set_means_none() {
    let mut set = setup();
    set.symbol = 2;
    let plan = plan(&Even, &set, Rect::new(0, 0, 400, 150), "12°C", &lines()).unwrap();
    assert_eq!(plan.placing, Placing::None);
    assert_eq!(plan.symbol.width, 0);
}

#[test]
fn a_small_symbol_goes_before_the_temperature() {
    let mut set = setup();
    set.symbol = 1;
    let plan = plan(&Even, &set, Rect::new(0, 0, 400, 150), "12°C", &lines()).unwrap();
    assert_eq!(plan.placing, Placing::Inline);
}

#[test]
fn the_symbol_is_placed_as_set() {
    let mut set = setup();
    set.placing = 2;
    let plan = plan(&Even, &set, Rect::new(0, 0, 400, 150), "12°C", &lines()).unwrap();
    assert_ne!(plan.placing, Placing::Beside);
}

#[test]
fn little_room_drops_the_least_important_lines_first() {
    let all = lines();
    let plan = plan(&Even, &setup(), Rect::new(0, 0, 160, 70), "12°C", &all).unwrap();
    assert!(plan.lines.len() < all.len());
    // What is kept is no lower in rank than what was left out.
    let least = plan.lines.iter().map(|l| l.rank).min().unwrap();
    assert!(least >= 2, "{:?}", plan.lines);
    assert!(plan.lines.iter().all(|l| l.rank != 1));
}

#[test]
fn what_fits_is_all_kept() {
    let all = lines();
    let plan = plan(&Even, &setup(), Rect::new(0, 0, 600, 600), "12°C", &all).unwrap();
    assert_eq!(plan.lines.len(), 5);
}

#[test]
fn details_share_a_line_where_they_fit() {
    let all = alloc::vec![
        Line::new("Feels like 9°", 14.0, true, 2).unwrap(),
        Line::new("Humidity 80 %", 14.0, true, 1).unwrap(),
    ];
    let wide = packed(&Even, &all, 0, 400.0).unwrap();
    assert_eq!(wide.len(), 1);
    assert_eq!(wide[0].text, "Feels like 9° · Humidity 80 %");
    let narrow = packed(&Even, &all, 0, 100.0).unwrap();
    assert_eq!(narrow.len(), 1);
    assert_eq!(narrow[0].text, "Feels like 9°");
}

#[test]
fn the_condition_takes_the_small_size_before_it_is_cut() {
    let all = alloc::vec![
        Line::new("Thunderstorm with heavy hail", 18.0, false, 5).unwrap(),
        Line::new("H 12°", 14.0, false, 4).unwrap(),
    ];
    // 28 characters: 252 wide at 18, 196 at 14.
    let packed = packed(&Even, &all, 0, 200.0).unwrap();
    assert_eq!(packed[0].size, 14.0);
}

#[test]
fn a_fixed_size_is_kept() {
    let mut set = setup();
    set.fixed = Some(40.0);
    let plan = plan(&Even, &set, Rect::new(0, 0, 300, 200), "12°C", &lines()).unwrap();
    assert_eq!(plan.size, 40.0);
}

#[test]
fn odd_places_never_panic() {
    for (w, h) in [(0, 0), (1, 1), (5, 400), (400, 5), (65535, 65535), (30, 30)] {
        let _ = plan(&Even, &setup(), Rect::new(0, 0, w, h), "-12°C", &lines());
    }
}

#[test]
fn refused_memory_is_an_error() {
    use crate::fail_alloc::sweep;
    sweep(|| {
        plan(&Even, &setup(), Rect::new(0, 0, 400, 150), "12°C", &lines()).map(|_| ())
    });
}
