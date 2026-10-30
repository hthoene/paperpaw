//! The size of book text on a touch screen: its slider's track is the
//! sizes the reading font draws differently, evenly spaced, so that a
//! finger's width moves the knob as far everywhere on it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;
use paperpaw_platform::Point;

const SIZE: &str = "core.font-size";

fn slider_area(app: &mut App) -> paperpaw_sdk::Rect {
    app.parts()
        .into_iter()
        .find(|p| p.role == Role::Slider)
        .expect("the size's slider")
        .area
}

/// The track's width the knob moves over, in pixels.
fn travel() -> i32 {
    let mut app = settings_page();
    let track = slider_area(&mut app);
    i32::from(track.width) - i32::from(track.height)
}

fn settings_page() -> App {
    let mut app = App::new(TOUCH, Shared::memory("", &[]));
    app.run(actions::SETTINGS);
    app.open_screen(
        Id::new_static("settings.group"),
        Params::new().with(params::ID, "core.group.reading-text"),
    );
    app.set(SIZE, "6");
    app
}

/// The size a finger leaves where it is lifted: put down on the knob at
/// the smallest size, moved `along` pixels along the track.
fn dragged(along: i32) -> i64 {
    let mut app = settings_page();
    let track = slider_area(&mut app);
    let knob = i32::from(track.height);
    let (left, y) = (i32::from(track.x) + knob / 2, i32::from(track.y) + knob / 2);
    app.drag(Point::new(left, y), Point::new(left + along, y));
    app.kernel.settings().int(SIZE)
}

#[test]
fn the_sizes_are_spread_evenly_along_the_track() {
    let sizes: Vec<i64> = (0..=travel()).map(dragged).collect();
    // How many pixels of the track each size has.
    let mut runs: Vec<(i64, i32)> = Vec::new();
    for size in &sizes {
        match runs.last_mut() {
            Some((last, n)) if last == size => *n += 1,
            _ => runs.push((*size, 1)),
        }
    }
    assert!(sizes.windows(2).all(|w| w[0] <= w[1]), "{runs:?}");
    // Every size the font draws differently is there, once.
    assert!(runs.len() >= 5, "{runs:?}");
    // Equal widths but for the two ends (half a step each, and the first
    // size where the finger lands on the knob).
    let inner: Vec<i32> = runs[2..runs.len() - 2].iter().map(|r| r.1).collect();
    let (least, most) = (inner.iter().min().unwrap(), inner.iter().max().unwrap());
    assert!(most - least <= 3, "uneven steps: {runs:?}");
}
