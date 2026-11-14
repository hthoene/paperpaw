//! The book read last opens again at the start (`reader.open-last`, on by
//! default): with a book remembered and still on the card the start shows
//! the reader; with the book gone, the setting off or nothing remembered it
//! stays on the home screen, without an error screen.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod native_activation;

use std::fmt::Write as _;

use paperpaw_core::Effects;
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::MemoryStorage;

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

const BOOK: &str = "books/alice-in-wonderland.txt";
const STATE: &str = ".paperpaw/plugins/reader/state.ini";

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

/// The screen on top after the start, on a card with the book, `state` as the
/// reader's `state.ini` (none: no file) and `config` as the user's settings.
fn started(state: Option<&str>, config: &str) -> String {
    started_with_state(state, config).0
}

/// [`started`], and the reader's `state.ini` after the start (none: no file).
fn started_with_state(state: Option<&str>, config: &str) -> (String, Option<String>) {
    let mut storage = MemoryStorage::new()
        .with(
            BOOK,
            include_str!("../../../samples/library/books/alice-in-wonderland.txt"),
        )
        // The first-run setup was done.
        .with(".paperpaw/plugins/setup/done", "");
    if let Some(state) = state {
        storage = storage.with(STATE, state);
    }
    let (builder, card) = native_activation::quiet_on(DEVICE, storage, config);
    let mut kernel = native_activation::build_on(paperpaw_app::install(builder, fonts()), card);
    let mut now = 0;
    let mut effects: Effects = kernel.start(Instant(now));
    for _ in 0..10_000 {
        let Some(at) = effects.wake_at() else { break };
        if at.0 > now + 1000 {
            break;
        }
        now = now.max(at.0);
        effects = kernel.tick(Instant(now));
    }
    kernel.render(&mut |_, _| {});
    let mut text = String::new();
    let screen = kernel.active_screen().map_or("", |id| id.as_str());
    write!(text, "{screen}").unwrap();
    let mut kept = vec![0u8; 4096];
    let kept = kernel
        .storage()
        .read_at(STATE, 0, &mut kept)
        .ok()
        .map(|n| String::from_utf8_lossy(&kept[..n]).into_owned());
    (text, kept)
}

const LANGUAGE: &str = "[core]\nlanguage = en\n";

#[test]
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the native reader (`native-linked-reading`): the interface-6 reader is removed (#244)"
)]
fn the_last_book_opens_at_the_start() {
    let shown = started(
        Some("[reader]\nlast = books/alice-in-wonderland.txt\n"),
        LANGUAGE,
    );
    assert_eq!(shown, "reader.screen");
}

#[test]
fn a_book_gone_from_the_card_leaves_the_home_screen() {
    let shown = started(Some("[reader]\nlast = books/gone.txt\n"), LANGUAGE);
    assert_eq!(shown, "home.dashboard");
}

/// The book is forgotten once it is found gone: the next start has nothing to look for.
#[test]
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the native reader (`native-linked-reading`): the interface-6 reader keeps the path"
)]
fn a_book_gone_from_the_card_is_forgotten() {
    let (shown, kept) = started_with_state(Some("[reader]\nlast = books/gone.txt\n"), LANGUAGE);
    assert_eq!(shown, "home.dashboard");
    assert_eq!(kept, None);
    // One still there is kept.
    let (shown, kept) = started_with_state(
        Some("[reader]\nlast = books/alice-in-wonderland.txt\n"),
        LANGUAGE,
    );
    assert_eq!(shown, "reader.screen");
    assert!(kept.is_some_and(|text| text.contains("alice-in-wonderland")));
}

#[test]
fn nothing_remembered_leaves_the_home_screen() {
    assert_eq!(started(None, LANGUAGE), "home.dashboard");
    // A file that is not the reader's.
    assert_eq!(
        started(Some("\u{1}\u{2} not a state"), LANGUAGE),
        "home.dashboard"
    );
}

#[test]
fn the_setting_off_leaves_the_home_screen() {
    let shown = started(
        Some("[reader]\nlast = books/alice-in-wonderland.txt\n"),
        "[core]\nlanguage = en\n[reader]\nopen-last = false\n",
    );
    assert_eq!(shown, "home.dashboard");
}
