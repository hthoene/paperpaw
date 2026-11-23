//! The first open of a book says so before the wait (#510).
//!
//! Beta 3 showed "Opening the book for the first time. This takes a moment."
//! at once while an EPUB's first chapter was converted. Beta 4's EPUB format
//! converted inside the open, so the reader's first frame came only after the
//! conversion and the note was never seen. Now the open answers "preparing",
//! the reader draws the note, and the conversion follows between inputs; a
//! book opened before opens at once, without the note.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::sdk::{Id, MemoryStorage};
use paperpaw_emulator::{device::Device, headless::run_script, profiles};

const ALICE: &[u8] = include_bytes!("../../../samples/library/books/alice-in-wonderland.epub");
const NOTE: &str = "Opening the book for the first time. This takes a moment.";

/// Whether the screen shows the note (its lines as the screen wraps them).
fn shows_note(labels: &[String]) -> bool {
    labels.join(" ").contains(NOTE)
}

/// The labels of what the screen shows.
fn labels(device: &mut Device) -> Vec<String> {
    let mut labels = Vec::new();
    device.call(|kernel, _| {
        labels = kernel.describe().into_iter().map(|p| p.label).collect();
        paperpaw_core::Effects::default()
    });
    labels
}

/// Starts `profile` on `card` and opens its book from Files: the labels of
/// the frame the press of *Open* drew, before the work it leaves for later
/// runs (`Effects::proceed`: the device shows the frame, then goes on).
fn open_book(profile: &str, card: &Rc<MemoryStorage>) -> (Device, Vec<String>) {
    let profile = profiles::find(profile).unwrap();
    let mut device = Device::in_memory(profile, None, card.clone(), true, None).unwrap();
    device.start();
    // The walk puts the book into the catalog; then the folder `books`, then
    // its book, whose actions start at *Open*.
    run_script(
        &mut device,
        "wait:20000,open:file-manager.browser,wait:1000,run:core.confirm,wait:1000,\
         run:core.confirm,wait:1000",
    )
    .unwrap();
    let offered = labels(&mut device);
    assert!(offered.iter().any(|l| l == "Open"), "{offered:?}");
    let mut pressed = None;
    device.call(|kernel, now| {
        pressed = Some(kernel.run_action(now, Id::parse("core.confirm").unwrap()));
        paperpaw_core::Effects::default()
    });
    let pressed = pressed.unwrap();
    assert!(pressed.refresh().is_some(), "the press drew nothing");
    let shown = labels(&mut device);
    device.call(|_, _| pressed);
    (device, shown)
}

fn the_first_open_shows_the_note_and_a_later_one_does_not(profile: &str) {
    let card = Rc::new(
        MemoryStorage::new()
            .with(
                "paperpaw.ini",
                "[help]\nshow = false\n[reader]\nopen-last = false\n",
            )
            .with(".paperpaw/plugins/setup/done", "")
            .with("books/alice.epub", ALICE),
    );
    paperpaw_emulator::budget::override_limit(None);
    let (mut device, first) = open_book(profile, &card);
    assert!(
        shows_note(&first),
        "{profile}: no note on the first open: {first:?}"
    );
    // The book opens once its first chapter is converted.
    run_script(&mut device, "wait:5000").unwrap();
    let read = labels(&mut device);
    assert!(!shows_note(&read), "{profile}: still preparing: {read:?}");
    drop(device);
    // Opened before: at once, no note.
    let (_device, again) = open_book(profile, &card);
    assert!(
        !shows_note(&again),
        "{profile}: the note on a later open: {again:?}"
    );
}

#[test]
fn x4_says_so_on_the_first_open_only() {
    the_first_open_shows_the_note_and_a_later_one_does_not("xteink-x4");
}

#[test]
fn x4_classic_says_so_on_the_first_open_only() {
    the_first_open_shows_the_note_and_a_later_one_does_not("xteink-x4-classic");
}
