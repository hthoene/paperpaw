//! The shipped native reader opens the books: a text and an EPUB book open in
//! it, and a Beta 3 card whose configuration still names a reading app that is
//! gone raises no warning and opens its books there too. (The reading-app
//! resolver of interface 6, with its per-book, per-folder and per-format
//! choices, is not carried to the native host: ticket #260.)

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod native_activation;

use paperpaw_core::{Effects, Kernel};
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

const TEXT: &str = "books/notes.txt";
const EPUB: &str = "books/alice.epub";
const SHIPPED: &str = "reader.screen";

struct App {
    kernel: Kernel,
    now: u64,
}

impl App {
    fn new(storage: MemoryStorage, config: &str) -> Self {
        let storage = storage.with(".paperpaw/plugins/setup/done", "");
        // The linked plugins on the same card.
        let (builder, card) = native_activation::quiet_on(
            DEVICE,
            storage,
            &format!("[core]\nlanguage = en\n{config}"),
        );
        let builder = paperpaw_app::install(builder, paperpaw_app::Fonts::Packs);
        let mut kernel = native_activation::build_on(builder, card);
        let effects = kernel.start(Instant(0));
        let mut app = Self { kernel, now: 0 };
        app.settle(effects);
        app
    }

    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
            let Some(at) = effects.wake_at() else {
                return;
            };
            if at.0 > self.now + 1000 {
                return;
            }
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
        }
    }

    /// The screen the book at `path` opens in, as a shell opens it.
    fn book(&mut self, path: &str) -> String {
        self.now += 100;
        let effects = self.kernel.open_book(Instant(self.now), path);
        self.settle(effects);
        self.kernel
            .active_screen()
            .map(|id| id.to_string())
            .unwrap_or_default()
    }
}

fn books() -> MemoryStorage {
    MemoryStorage::new().with(TEXT, "Some notes.").with(
        EPUB,
        &include_bytes!("../../../samples/library/books/alice-in-wonderland.epub")[..],
    )
}

#[test]
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the native reader: the interface-6 reader is removed (#244); runs with `native-linked-reading` (the test-native step)"
)]
fn books_open_in_the_shipped_native_reader() {
    let mut app = App::new(books(), "");
    assert_eq!(app.book(TEXT), SHIPPED);
    assert_eq!(app.book(EPUB), SHIPPED);
}

/// A card of Beta 3 whose configuration still names reading apps that are
/// gone: nothing is warned and the book opens in the shipped reader.
#[test]
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the native reader: the interface-6 reader is removed (#244); runs with `native-linked-reading` (the test-native step)"
)]
fn a_reading_app_that_is_gone_is_passed_over_silently() {
    let config = "reading-app = gone.app\nreading-app.format-txt.txt = gone.app\n";
    let mut app = App::new(books(), config);
    let warnings = app.kernel.take_warnings();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(app.book(TEXT), SHIPPED);
}
