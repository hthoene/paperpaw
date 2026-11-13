//! What the start shows: with a book read last, the book is the first and only
//! frame (the reader opens it inside the start, as Beta 3 did; home is not drawn
//! first, which would flash the panel and count as a page turn); without one,
//! the home screen as before (#243).

#![allow(
    clippy::unwrap_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::sdk::MemoryStorage;
use paperpaw_emulator::{device::Device, profiles};

const BOOK: &str = "books/alice-in-wonderland.txt";

/// A device on `profile` started, with the book read last or without one.
fn started(profile: &str, last_book: bool) -> (Device, Option<String>) {
    let mut files = MemoryStorage::new()
        .with(
            BOOK,
            include_str!("../../../samples/library/books/alice-in-wonderland.txt"),
        )
        .with(".paperpaw/plugins/setup/done", "")
        .with("paperpaw.ini", "[help]\nshow = false\n");
    if last_book {
        files = files.with(
            ".paperpaw/plugins/reader/state.ini",
            format!("[reader]\nlast = {BOOK}\n"),
        );
    }
    paperpaw_emulator::budget::override_limit(None);
    let profile = profiles::find(profile).unwrap();
    let mut device = Device::in_memory(profile, None, Rc::new(files), true, None).unwrap();
    // No pass of its own: what `start` shows is the first frame, as on a device.
    device.start();
    let mut screen = None;
    device.call(|kernel, _| {
        screen = kernel.active_screen().map(ToString::to_string);
        paperpaw_core::Effects::default()
    });
    (device, screen)
}

#[test]
fn with_a_last_book_the_start_shows_the_book_and_no_home() {
    for profile in ["xteink-x4", "xteink-x4-pro"] {
        let (device, screen) = started(profile, true);
        assert_eq!(screen.as_deref(), Some("reader.screen"), "{profile}");
        assert_eq!(device.shown.len(), 1, "{profile}: {:?}", device.shown);
        let (region, _) = device.shown[0];
        let screen = profiles::find(profile).unwrap().device.display;
        assert_eq!(
            (region.x, region.y, region.width, region.height),
            (0, 0, screen.width, screen.height),
            "{profile}: the whole screen"
        );
    }
}

#[test]
fn without_a_last_book_the_start_shows_home_once() {
    let (device, screen) = started("xteink-x4", false);
    assert_eq!(screen.as_deref(), Some("home.dashboard"));
    assert_eq!(device.shown.len(), 1, "{:?}", device.shown);
}
