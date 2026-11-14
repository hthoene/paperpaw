//! A book open while the screen is turned or resized stays open: its pages
//! are laid out for the new size from the page shown, nothing is said to
//! have been closed, and turning goes on.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::*;
use paperpaw_plugin_reader::actions as reader;
use paperpaw_sdk::builtin::actions;

const ALICE: &[u8] = include_bytes!("../../../samples/library/books/alice-in-wonderland.epub");
const BOOK: &str = "books/numbered.txt";

fn open_book() -> Reading {
    let text = numbered_book(120, 12);
    let mut reading = Reading::new(BUTTONS, Shared::default(), BOOK, text.as_bytes(), "");
    reading.open();
    reading
}

#[test]
fn turned_the_book_stays_open_at_its_place() {
    let mut reading = open_book();
    for _ in 0..3 {
        reading.press(actions::NEXT_PAGE);
    }
    let place = reading.shown().at();
    reading.resize(800, 480);
    assert_eq!(reading.screen(), "reader.screen");
    let events = reading.events();
    assert!(!events.contains(&"closed"), "not closed: {events:?}");
    // Laid out for the new size: the page shows text, and turning goes on.
    reading.render();
    assert!(
        reading.image.iter().any(|row| row.iter().any(|&l| l < 128)),
        "the page is blank"
    );
    reading.press(actions::NEXT_PAGE);
    let ahead = reading.shown().at();
    assert!(ahead > place, "turned on from {place:?} to {ahead:?}");
    reading.resize(480, 800);
    reading.press(actions::PREV_PAGE);
    assert_eq!(reading.screen(), "reader.screen");
}

#[test]
fn turned_the_contents_and_the_marks_keep_their_entry() {
    let mut reading = Reading::new(BUTTONS, Shared::default(), "books/alice.epub", ALICE, "");
    reading.open();
    reading.press(reader::TOC);
    assert_eq!(reading.screen(), "reader.toc");
    reading.press(actions::DOWN);
    reading.press(actions::DOWN);
    let entry = focused(&mut reading);
    assert!(!entry.is_empty());
    reading.resize(800, 480);
    assert_eq!(reading.screen(), "reader.toc");
    assert_eq!(focused(&mut reading), entry, "the contents, landscape");
    reading.resize(480, 800);
    assert_eq!(focused(&mut reading), entry, "the contents, upright");
    reading.press(actions::BACK);
    reading.press(reader::ANNOTATIONS);
    assert_eq!(reading.screen(), "reader.annotations");
    reading.resize(800, 480);
    assert_eq!(reading.screen(), "reader.annotations");
}

/// The label of the part with the focus.
fn focused(reading: &mut Reading) -> String {
    reading.render();
    let parts = reading.kernel.describe();
    paperpaw_sdk::semantics::focused(&parts)
        .map(|p| p.label.clone())
        .unwrap_or_default()
}
