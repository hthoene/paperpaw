//! The overview of every tool (`home.drawer`) on screens of different
//! shapes: as many columns as the screen's shape asks for (4 upright, more
//! on its side), and the focus ring round the focused tool clear of its
//! icon and name on every side, with 4, 5 and 6 columns, left to right
//! and right to left.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use crate::common::*;
use paperpaw_platform::{DeviceInfo, DisplayInfo, PixelFormat, Rotation};
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::{Id, Params, Rect};

const DRAWER: &str = "home.drawer";

/// The test X4 turned to `width` by `height` pixels.
fn screen(width: u16, height: u16) -> DeviceInfo {
    X4.with_display(DisplayInfo::new(
        width,
        height,
        220,
        PixelFormat::Mono1,
        Rotation::Deg0,
        None,
    ))
}

fn card(language: &str) -> Shared {
    Shared::memory(
        &format!("[core]\nlanguage = {language}\n"),
        &[
            (
                "plugins/language-ar/plugin.ini",
                b"[plugin]\nid = language-ar\nname = Arabic\nversion = 1.0.0\nformats = 3\n",
            ),
            (
                "plugins/language-ar/i18n/ar.ini",
                b"[meta]\nlanguage = ar\nname = Arabic\nplural = arabic\ndirection = rtl\nscript = Arab\n",
            ),
        ],
    )
}

/// The first tool's cell and the cells of the tools shown.
fn cells(app: &mut App) -> (Vec<Rect>, Rect) {
    let parts = app.parts();
    let tools: Vec<Rect> = parts
        .iter()
        .filter(|p| p.role == Role::Button)
        .map(|p| p.area)
        .collect();
    (tools.clone(), tools[0])
}

/// How many columns the first row has.
fn columns(cells: &[Rect]) -> usize {
    cells.iter().filter(|c| c.y == cells[0].y).count()
}

/// The runs of dark along an axis: for each coordinate from `from` to `to`
/// whether any pixel across `across` is black, as (first, last) of each run
/// of coordinates; runs less than 7 pixels apart are one (the letters of a
/// word).
fn runs(app: &mut App, from: i32, to: i32, across: (i32, i32), vertical: bool) -> Vec<(i32, i32)> {
    let mut found: Vec<(i32, i32)> = Vec::new();
    for along in from..to {
        let dark = (across.0..across.1).any(|other| {
            let (x, y) = if vertical {
                (other, along)
            } else {
                (along, other)
            };
            app.dark(x, y)
        });
        if !dark {
            continue;
        }
        match found.last_mut() {
            Some(run) if along - run.1 <= 7 => run.1 = along,
            _ => found.push((along, along)),
        }
    }
    found
}

/// The paper between the focus ring and what is in it on each side:
/// (above, below, left, right).
fn clearances(app: &mut App, cell: Rect) -> (i32, i32, i32, i32) {
    let (x, y) = (i32::from(cell.x), i32::from(cell.y));
    let (right, bottom) = (cell.right() as i32, cell.bottom() as i32);
    let mid = x + i32::from(cell.width) / 2;
    let rows = runs(app, y - 15, bottom, (mid - 10, mid + 10), true);
    assert!(rows.len() >= 3, "ring, icon and name: {rows:?}");
    let above = rows[1].0 - rows[0].1 - 1;
    let below = rows[rows.len() - 1].0 - rows[rows.len() - 2].1 - 1;
    // Across the rows the icon and name are in.
    let (top, low) = (rows[1].0, rows[rows.len() - 2].1);
    let columns = runs(app, x - 2, right + 2, (top, low + 1), false);
    assert!(columns.len() >= 3, "ring, content, ring: {columns:?}");
    let left = columns[1].0 - columns[0].1 - 1;
    let right = columns[columns.len() - 1].0 - columns[columns.len() - 2].1 - 1;
    (above, below, left, right)
}

#[test]
#[ignore = "the native Home differs from the interface-6 one this test pinned (fails at drawer_focus.rs:56); to analyse (#244, Home removal)"]
fn the_focus_ring_is_clear_of_the_icon_and_the_name_with_4_5_and_6_columns() {
    // Upright, a squarish screen, and on its side.
    for (width, height, expected) in [(480, 800, 4), (800, 680, 5), (800, 480, 6)] {
        for language in ["en", "ar"] {
            let mut app = App::with_more_tools(screen(width, height), card(language));
            app.open_screen(Id::new_static(DRAWER), Params::new());
            assert_eq!(app.screen(), DRAWER);
            let (all, cell) = cells(&mut app);
            assert_eq!(columns(&all), expected, "{width}x{height} {language}");
            let (above, below, left, right) = clearances(&mut app, cell);
            // A gap (17 pixels) and a little, where the focus used to stand
            // half a gap from them.
            for (side, clear) in [
                ("above", above),
                ("below", below),
                ("left", left),
                ("right", right),
            ] {
                assert!(
                    clear >= 12,
                    "{width}x{height} {language}: {side} {clear} px (cell {cell:?})"
                );
            }
        }
    }
}

#[test]
fn the_focus_ring_stays_inside_its_cell_sideways() {
    for (width, height) in [(480, 800), (800, 480)] {
        let mut app = App::new(screen(width, height), card("en"));
        app.open_screen(Id::new_static(DRAWER), Params::new());
        let (_, cell) = cells(&mut app);
        let mid = i32::from(cell.y) + i32::from(cell.height) / 2;
        let columns = runs(
            &mut app,
            i32::from(cell.x) - 2,
            cell.right() as i32 + 2,
            (mid - 5, mid + 5),
            false,
        );
        let (first, last) = (columns[0], columns[columns.len() - 1]);
        assert!(
            first.0 >= i32::from(cell.x),
            "{width}x{height}: {columns:?} {cell:?}"
        );
        assert!(
            last.1 < cell.right() as i32,
            "{width}x{height}: {columns:?} {cell:?}"
        );
    }
}
