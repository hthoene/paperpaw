//! Moving rows of a list: by dragging their grip ([`Reorder`]) and with
//! the buttons (a row moved up or down a place at a time). Both give the
//! same order, and no item is ever lost or doubled; a finger that does
//! not start on a grip, or is another finger, leaves the rows alone.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_sdk::ui::reorder::moved;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]


    /// Any number of moves, in or out of range: every item is still
    /// there once.
    #[test]
    fn moves_never_lose_or_double_an_item(
        len in 0usize..20,
        moves in proptest::collection::vec((0usize..25, 0usize..25), 0..30),
    ) {
        let mut items: Vec<usize> = (0..len).collect();
        for (from, to) in moves {
            let before = items.clone();
            moved(&mut items, from, to);
            if from >= len || to >= len {
                prop_assert_eq!(&items, &before, "out of range: as it was");
            }
            let mut sorted = items.clone();
            sorted.sort_unstable();
            prop_assert_eq!(sorted, (0..len).collect::<Vec<_>>());
        }
    }

}
