//! Status items that do something when tapped: a tap on one in a status
//! line does it; a tap beside it shows the next line, as before.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;
use paperpaw_sdk::Point;

const ALICE: &[u8] = include_bytes!("../../../samples/library/books/alice-in-wonderland.epub");

/// The recorder's item alone at the bottom left.
const CONFIG: &str = "[reader]\nstatus.bottom-left = recorder.status\nstatus.bottom-center =\nstatus.bottom-right =\n";

#[test]
fn a_tapped_status_item_does_what_it_says() {
    let mut reading = Reading::new(TOUCH, Shared::default(), "books/alice.epub", ALICE, CONFIG);
    reading.open();
    let height = i32::from(TOUCH.display.height);
    let width = i32::from(TOUCH.display.width);
    // Beside the item: the next line, nothing opened.
    reading.tap(Point::new(width - 40, height - 4));
    assert!(reading.heard.borrow().slots.is_empty());
    // Back to the line of items, then on the item.
    reading.tap(Point::new(width - 40, height - 4));
    reading.tap(Point::new(width - 40, height - 4));
    reading.tap(Point::new(40, height - 4));
    let slots = reading.heard.borrow().slots.clone();
    assert_eq!(slots.len(), 1, "{slots:?}");
    assert_eq!(slots[0].0, "book-info");
    assert_eq!(slots[0].1.get("from"), Some("status"));
}

/// The reader's line of items drawn in another place shows that place's
/// items: the recorder's own, its item that does something there.
#[test]
fn the_line_of_items_shows_the_items_of_its_place() {
    // Not among the reader's own items: those of the place alone.
    let mut reading = Reading::new(TOUCH, Shared::default(), "books/alice.epub", ALICE, "");
    reading.now += 100;
    let effects = reading.kernel.open_screen(
        paperpaw_sdk::Instant(reading.now),
        paperpaw_sdk::Id::new_static("recorder.lines"),
        paperpaw_sdk::Params::new(),
    );
    reading.settle(effects);
    let slots = reading.heard.borrow().slots.clone();
    let lines = slots.iter().find(|(name, _)| *name == "lines").unwrap();
    assert_eq!(lines.1.get("targets"), Some("1"));
}
