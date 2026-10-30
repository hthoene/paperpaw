//! Every widget of the standard app, drawn on the home screen of an X4
//! (black and white, buttons) and of a Boox (grays, touch): in many sizes,
//! from a single cell to the whole page, in each frame, in English and
//! German, with a book read and without. None may fail to draw, and each
//! has settings of its own, its frame among them, so that the reader can
//! shape it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::widgets::{self, FRAMES};
use paperpaw_sdk::{MemoryStorage, Params};

const X4: DeviceInfo = DeviceInfo::new(
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

const BOOX: DeviceInfo = DeviceInfo::new(
    "test-boox",
    "Test Boox",
    DisplayInfo::new(1264, 1680, 300, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, true, false, &[]),
    MemoryClass::Large,
);

const BOOK: &str = "books/alice-in-wonderland.txt";

/// Sizes in cells (columns, rows), on a grid of four columns.
const SIZES: [(u8, u8); 7] = [(1, 1), (2, 1), (2, 2), (4, 1), (4, 2), (3, 3), (4, 6)];

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

struct App {
    kernel: Kernel,
    now: u64,
}

impl App {
    fn new(device: DeviceInfo, language: &str) -> Self {
        let storage = MemoryStorage::new()
            .with(
                BOOK,
                include_str!("../../../samples/library/books/alice-in-wonderland.txt"),
            )
            .with(".paperpaw/plugins/setup/done", "");
        let builder = Kernel::builder(device)
            .storage(storage)
            .user_config(format!(
                "[help]\nshow = false\n[core]\nlanguage = {language}\n"
            ));
        let mut kernel = paperpaw_app::install(builder, fonts()).build().unwrap();
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

    /// Sets `id` to `value` where it exists (not every widget has every
    /// setting).
    fn set(&mut self, id: &str, value: &str) {
        self.now += 10;
        if let Ok(effects) = self.kernel.set_setting(Instant(self.now), id, value) {
            self.settle(effects);
        }
    }

    fn read_a_book(&mut self) {
        self.now += 100;
        let open = Params::new().with(params::PATH, BOOK);
        let effects = self
            .kernel
            .open_slot(Instant(self.now), slots::READER, open);
        self.settle(effects);
        self.kernel.render(&mut |_, _| {});
        self.now += 100;
        let effects = self
            .kernel
            .open_slot(Instant(self.now), slots::HOME, Params::new());
        self.settle(effects);
    }

    /// Draws what changed; whether anything was.
    fn draw(&mut self) -> bool {
        let mut drawn = false;
        self.kernel.render(&mut |_, _| drawn = true);
        drawn
    }

    /// The widgets to draw: all but the room between others.
    fn widget_ids(&self) -> Vec<String> {
        self.kernel
            .widgets()
            .iter()
            .map(|(id, _)| id.as_str().to_string())
            .filter(|id| !widgets::repeatable(id) && id != "home.tools")
            .collect()
    }
}

/// Each widget alone on the first page, in every size and frame.
fn draw_all(device: DeviceInfo, language: &str, read: bool) {
    let mut app = App::new(device, language);
    if read {
        app.read_a_book();
    }
    for id in app.widget_ids() {
        for frame in FRAMES {
            app.set(&format!("{id}.frame"), frame);
            for (w, h) in SIZES {
                app.set("home.page-1", &format!("{id} 0/0 {w}x{h}"));
                assert!(app.draw(), "{id} {w}x{h} in {frame} not drawn");
            }
        }
        app.set(&format!("{id}.frame"), widgets::Frame::None.value());
    }
}

#[test]
fn every_widget_draws_on_the_x4() {
    draw_all(X4, "en", true);
}

#[test]
fn every_widget_draws_on_the_boox() {
    draw_all(BOOX, "de", false);
}

#[test]
fn every_widget_can_be_shaped() {
    let app = App::new(X4, "en");
    let settings = app.kernel.settings();
    let mut missing = Vec::new();
    for id in app.widget_ids() {
        let group = settings.groups().any(|g| g.id.as_str() == id);
        let frame = settings
            .specs()
            .any(|s| s.id.as_str() == format!("{id}.frame"));
        if !group || !frame {
            missing.push(id);
        }
    }
    assert!(
        missing.is_empty(),
        "widgets without settings or a frame: {missing:?}"
    );
}

#[test]
fn one_week_start_for_all() {
    use paperpaw_sdk::builtin::settings::WEEK_START;
    use paperpaw_sdk::time::WEEK_STARTS;
    let mut app = App::new(X4, "en");
    assert_eq!(app.kernel.settings().read(&WEEK_START), WEEK_STARTS[0]);
    app.set(WEEK_START.as_str(), "core.week.sunday");
    assert_eq!(app.kernel.settings().read(&WEEK_START), "core.week.sunday");
    // The calendars follow it, drawn as before.
    app.set(
        "home.page-1",
        "widgets.calendar 0/0 4x3, stats.widget.calendar 0/3 4x3",
    );
    assert!(app.draw());
}

#[test]
fn a_widget_placed_twice_has_two_looks() {
    let mut app = App::new(X4, "en");
    app.set(
        "home.page-1",
        "widgets.analogue-clock@1 0/0 2x2, widgets.analogue-clock@2 2/0 2x2",
    );
    app.set("widgets.analogue-clock@2.frame", "core.frame.dark");
    let settings = app.kernel.settings();
    assert_eq!(
        settings.text("widgets.analogue-clock@2.frame"),
        "core.frame.dark"
    );
    assert_eq!(
        settings.text("widgets.analogue-clock@1.frame"),
        "core.frame.none"
    );
    assert_eq!(
        settings.text("widgets.analogue-clock.frame"),
        "core.frame.none"
    );
    assert!(app.draw());
    // Kept in the configuration, as other settings are, and read back.
    let config = paperpaw_sdk::read_file(
        app.kernel.storage(),
        paperpaw_sdk::builtin::CONFIG_FILE,
        1 << 16,
    )
    .unwrap();
    let config = String::from_utf8(config).unwrap();
    assert!(config.contains("analogue-clock@2.frame"), "{config}");
    let storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
    let builder = Kernel::builder(X4).storage(storage).user_config(config);
    let kernel = paperpaw_app::install(builder, fonts()).build().unwrap();
    let settings = kernel.settings();
    assert_eq!(
        settings.text("widgets.analogue-clock@2.frame"),
        "core.frame.dark"
    );
    assert_eq!(
        settings.text("widgets.analogue-clock@1.frame"),
        "core.frame.none"
    );
    // While placement 2 is drawn, its widget reads its own.
    let frame = settings.with_placement(Some("widgets.analogue-clock@2"), || {
        settings.text("widgets.analogue-clock.frame").to_string()
    });
    assert_eq!(frame, "core.frame.dark");
}
