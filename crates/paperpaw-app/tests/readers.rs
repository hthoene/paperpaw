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

    /// `ms` of time going by, the kernel ticked as a shell does.
    fn wait(&mut self, ms: u64) {
        for _ in 0..ms / 500 {
            self.now += 500;
            let effects = self.kernel.tick(Instant(self.now));
            self.settle(effects);
        }
    }

    fn run(&mut self, action: paperpaw_sdk::Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
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

/// A book whose file is gone from the card says so, as Beta 3 did, instead of
/// calling the file damaged.
#[test]
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the native reader: the interface-6 reader is removed (#244); runs with `native-linked-reading` (the test-native step)"
)]
fn a_book_gone_from_the_card_says_its_file_is_no_longer_there() {
    let mut app = App::new(books(), "");
    assert_eq!(app.book("books/gone.txt"), SHIPPED);
    app.kernel.render(&mut |_, _| {});
    let labels: Vec<String> = app.kernel.describe().into_iter().map(|p| p.label).collect();
    assert!(
        labels.iter().any(|l| l.contains("no longer there")),
        "{labels:?}"
    );
}

/// A folder of pictures is one book, as in Beta 3: the opened picture and those
/// after it in name order are its pages. One page forward in a book of three
/// is a third of the way (a picture alone has no page to turn to). #277: the
/// format reads its book's folder through `core.files` root 3.
#[test]
#[cfg_attr(
    not(all(feature = "native-linked-reading", feature = "native-linked-store")),
    ignore = "needs the native reader and the picture format: they run with `native-linked-reading` and `native-linked-store` (the test-native step)"
)]
fn a_picture_opens_as_a_book_of_the_pictures_after_it() {
    let picture = &include_bytes!("../../../crates/paperpaw-image/tests/data/colors-rgb.png")[..];
    let card = MemoryStorage::new()
        .with("books/pics/01.png", picture)
        .with("books/pics/02.png", picture)
        .with("books/pics/03.png", picture)
        .with("books/pics/notes.txt", "not a picture");
    let mut app = App::new(card, "");
    assert_eq!(app.book("books/pics/01.png"), SHIPPED);
    app.wait(20_000);
    app.run(paperpaw_sdk::builtin::actions::NEXT_PAGE);
    app.wait(5_000);
    app.run(paperpaw_sdk::builtin::actions::HOME);
    // A screen with pictures is shown at once in black and white and its grays
    // follow after 3 s of calm (#267, Decision 196): the card is read after that.
    app.wait(5_000);
    app.kernel.render(&mut |_, _| {});
    let labels: Vec<String> = app.kernel.describe().into_iter().map(|p| p.label).collect();
    assert!(labels.iter().any(|l| l.contains("33 %")), "{labels:?}");
}
