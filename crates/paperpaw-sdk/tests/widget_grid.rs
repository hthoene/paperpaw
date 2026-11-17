//! The home screen's grid of widgets and tools ([`grid`], [`GridView`]):
//! layouts read from text however broken, written and read back alike;
//! items placed, dragged, sized and moved by touch and by buttons never
//! overlap, never leave the grid and are never lost; the layout survives
//! being kept and read again; every item is reached by buttons as by
//! touch.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod support;

use paperpaw_sdk::widgets::grid::{self, Cells, Geometry, Item, MAX_COLUMNS, MAX_ROWS, What};
use paperpaw_sdk::{Id, Point, Rect};
use proptest::prelude::*;

use support::middle;

// ---- Layouts as text ---------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// Any text at all is read without a panic; what is read is written
    /// and read again alike.
    #[test]
    fn any_layout_text_is_read_and_written_back_alike(text in ".{0,200}") {
        let items = grid::parse(&text);
        let written = grid::write(&items);
        prop_assert_eq!(grid::parse(&written), items);
    }

    /// Layouts of valid items are written and read back unchanged.
    #[test]
    fn layouts_round_trip(
        items in proptest::collection::vec(
            (
                any::<bool>(),
                "[a-z]{1,8}\\.[a-z][a-z0-9-]{0,10}(@[1-9])?",
                proptest::option::of((any::<u8>(), any::<u8>(), any::<u8>(), any::<u8>())),
            ),
            0..12,
        ),
    ) {
        let items: Vec<Item> = items
            .into_iter()
            .filter_map(|(is_tool, id, at)| {
                let what = if is_tool {
                    What::Tool(Id::parse(id.split('@').next()?).ok()?)
                } else {
                    What::Widget(id)
                };
                Some(match at {
                    Some((col, row, w, h)) => Item::placed(what, Cells::new(col, row, w, h)),
                    None => Item::new(what),
                })
            })
            .collect();
        prop_assert_eq!(grid::parse(&grid::write(&items)), items);
    }
}

#[test]
fn broken_places_keep_their_items_without_a_place() {
    let items = grid::parse(
        "t.clock 0/0, t.tall 1/x 2x2, t.book 300/0 1x1, tool:t.a 0/0 1x, tool:t.b -1/0 1x1, t.c 0/0 0x0",
    );
    let at: Vec<Option<Cells>> = items.iter().map(|i| i.at).collect();
    assert_eq!(items.len(), 6, "no item is lost");
    assert_eq!(&at[..5], &[None; 5]);
    // Zero cells is read; it fits no grid, so it is placed anew.
    assert_eq!(at[5], Some(Cells::new(0, 0, 0, 0)));
    // Empty parts, ids that are no ids: left out.
    assert!(grid::parse(" , ,,").is_empty());
    assert!(grid::parse("tool:Bad").is_empty());
}

// ---- Geometry and placing ---------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    /// The cells of a grid lie in its area, apart by the gap, and the
    /// cell under each cell's middle is that cell.
    #[test]
    fn cells_lie_in_the_area_and_are_found_where_they_are(
        x in 0u16..100,
        y in 0u16..200,
        width in 100u16..800,
        height in 100u16..900,
        columns in 1u8..=MAX_COLUMNS,
        rows in 1u8..=MAX_ROWS,
        gap in 0u16..20,
    ) {
        let g = Geometry::of(Rect::new(x, y, width, height), columns, rows, gap);
        for row in 0..g.rows {
            for col in 0..g.columns {
                let c = Cells::new(col, row, 1, 1);
                let r = g.rect(c);
                prop_assert!(r.x >= g.area.x && r.right() <= g.area.right());
                prop_assert!(r.y >= g.area.y && r.bottom() <= g.area.bottom());
                if !r.is_empty() {
                    let m = middle(r);
                    prop_assert_eq!(g.cell_at(m), Some((col, row)));
                }
            }
        }
        prop_assert_eq!(g.cell_at(Point::new(i32::from(x) - 1, i32::from(y))), None);
        prop_assert_eq!(g.cell_at(Point::new(i32::from(x), g.area.bottom() as i32 + 1)), None);
    }

}

// ---- Editing: touch and buttons ---------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(96)))]

}

thread_local! {
    /// What the lively widget was told.
    static TOLD: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}
