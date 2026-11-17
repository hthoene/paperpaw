//! The helpers that adapt a screen to its size (#88): the readable column,
//! the columns of a cover grid and the columns of a home grid in
//! landscape and a grid laid out anew for another one; the choice and the question in that column. On the X4 in
//! both orientations, on a tablet, and where the interface runs right to
//! left.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod support;

use paperpaw_sdk::Rect;
use paperpaw_sdk::widgets::grid::{self, MAX_COLUMNS};

/// The X4 upright, the X4 on its side, a small tablet and a large one, each
/// upright and on its side: width, height, dots per inch.
const X4: (u16, u16, u16) = (480, 800, 220);
const X4_SIDE: (u16, u16, u16) = (800, 480, 220);
const TABLET_SIDE: (u16, u16, u16) = (1920, 1200, 220);

fn whole((width, height, _): (u16, u16, u16)) -> Rect {
    Rect::new(0, 0, width, height)
}

#[test]
fn a_home_grid_gets_more_columns_on_its_side() {
    // Upright, or square: as many as were made for.
    assert_eq!(grid::columns_for(whole(X4), 4), 4);
    assert_eq!(grid::columns_for(Rect::new(0, 0, 600, 600), 4), 4);
    // The X4 on its side: 4 times 800/480 is 6.7, 7, at most 6.
    assert_eq!(grid::columns_for(whole(X4_SIDE), 4), 6);
    assert_eq!(grid::columns_for(whole(X4_SIDE), 3), 5);
    assert_eq!(grid::columns_for(whole(X4_SIDE), 2), 3);
    assert_eq!(grid::columns_for(whole(TABLET_SIDE), 4), 6);
    // A little wider than high: no fewer than were made for.
    assert_eq!(grid::columns_for(Rect::new(0, 0, 520, 480), 4), 4);
    // Nonsense in, something usable out.
    assert_eq!(grid::columns_for(Rect::new(0, 0, 100, 0), 0), MAX_COLUMNS);
    assert_eq!(grid::columns_for(Rect::default(), 9), MAX_COLUMNS);
    assert_eq!(grid::columns_for(whole(X4), 0), 1);
    for columns in 0..=255u8 {
        for area in [whole(X4), whole(X4_SIDE), Rect::new(0, 0, u16::MAX, 1)] {
            let n = grid::columns_for(area, columns);
            assert!((1..=MAX_COLUMNS).contains(&n));
        }
    }
}
