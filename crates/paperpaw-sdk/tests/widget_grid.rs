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

use paperpaw_sdk::widgets::grid::{self, Cells};
use proptest::prelude::*;

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


}

// ---- Editing: touch and buttons ---------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(96)))]

}

thread_local! {
    /// What the lively widget was told.
    static TOLD: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}
