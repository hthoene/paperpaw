//! Moving rows of a [`ListView`] by dragging, as on phones: movable rows
//! carry a grip (≡) at their end; a finger that comes down on it takes the
//! row along, a bar shows where it will land, and lifting the finger puts
//! it there. Taps on the rest of the row work as before, and buttons keep
//! their own ways of moving rows (e.g. *Move up*, *Move down*).
//!
//! A screen keeps a [`Reorder`], draws rows with `Reorder::draw_grip`
//! in what [`Reorder::content`] leaves, the bar with [`Reorder::draw`],
//! and passes pointer samples to [`Reorder::on_pointer`] from
//! `on_pointer`.
//!
//! The row's end is where its line of text ends: at the left in a
//! right-to-left UI language ([`UiStyle::is_rtl`]). [`Reorder::draw`]
//! takes the direction along for the pointer samples that follow.

use alloc::vec::Vec;

/// `items` with the one at `from` moved to `to`, the others keeping their
/// order.
pub fn moved<T>(items: &mut Vec<T>, from: usize, to: usize) {
    if from < items.len() && to < items.len() && from != to {
        let item = items.remove(from);
        items.insert(to, item);
    }
}
