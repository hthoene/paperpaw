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

const EPUB: &str = "books/alice-in-wonderland.epub";

/// A device on `profile` started, with the book read last or without one.
fn started(profile: &str, last_book: bool) -> (Device, Option<String>) {
    started_on(profile, last_book.then_some(BOOK))
}

/// A device on `profile` started, with `last` (a book of the sample library)
/// read last, if any.
fn started_on(profile: &str, last: Option<&str>) -> (Device, Option<String>) {
    let mut files = MemoryStorage::new()
        .with(
            BOOK,
            include_str!("../../../samples/library/books/alice-in-wonderland.txt"),
        )
        .with(
            EPUB,
            include_bytes!("../../../samples/library/books/alice-in-wonderland.epub").to_vec(),
        )
        .with(".paperpaw/plugins/setup/done", "")
        .with("paperpaw.ini", "[help]\nshow = false\n");
    if let Some(book) = last {
        files = files.with(
            ".paperpaw/plugins/reader/state.ini",
            format!("[reader]\nlast = {book}\n"),
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

/// Whether a frame is the whole screen of `profile`.
fn whole(profile: &str, region: paperpaw_platform::Rect) -> bool {
    let display = profiles::find(profile).unwrap().device.display;
    (region.x, region.y, region.width, region.height) == (0, 0, display.width, display.height)
}

#[test]
fn the_passes_after_the_start_draw_no_second_full_screen_frame() {
    for profile in ["xteink-x4", "xteink-x4-pro"] {
        let (mut device, _) = started(profile, true);
        for _ in 0..6 {
            device.advance(500);
            device.tick_if_due();
        }
        let full = device
            .shown
            .iter()
            .filter(|(r, _)| whole(profile, *r))
            .count();
        assert_eq!(full, 1, "{profile}: {:?}", device.shown);
    }
}

/// An EPUB read last: the book is the one full-screen frame; the cover and the
/// first pages may follow as frames of part of the screen, not a second whole one
/// (#252).
#[test]
fn an_epub_read_last_shows_one_full_frame_and_then_part_frames() {
    let profile = "xteink-x4";
    let (mut device, screen) = started_on(profile, Some(EPUB));
    assert_eq!(screen.as_deref(), Some("reader.screen"));
    for _ in 0..10 {
        device.advance(500);
        device.tick_if_due();
    }
    let full = device
        .shown
        .iter()
        .filter(|(r, _)| whole(profile, *r))
        .count();
    assert_eq!(full, 1, "{:?}", device.shown);
    assert!(
        device.shown.len() <= 2,
        "one full frame and at most the cover's: {:?}",
        device.shown
    );
}
