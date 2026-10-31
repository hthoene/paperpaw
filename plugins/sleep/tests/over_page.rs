//! *The page with a picture*: the page read stays while the device
//! sleeps, a picture over it whose clear parts show it, nothing else
//! changed; as see-through as asked; below on the right and clear of the
//! status lines unless asked otherwise; over the page, not a menu left
//! open; never over a lock; only where it is to be used, the cover or the
//! message otherwise; decoded once while it stays the same; a widget in
//! its place; pictures that other plugins offer as presets chosen in its
//! picker, which says why the last one did not show; and sleep modes of
//! other plugins.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::sync::atomic::{AtomicUsize, Ordering};

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    Color, DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_plugin_sleep::{Sleep, overlay};
use paperpaw_sdk::builtin::{actions, slots};
use paperpaw_sdk::presets::Preset;
use paperpaw_sdk::widgets::{Drawn, WidgetSpec};
use paperpaw_sdk::{
    Canvas, Cx, Env, Handled, Id, Manifest, MemoryStorage, Params, Plugin, Priority, Rect,
    RegisterError, Registrar, Screen, ScreenSpec, Version,
};

const X4: DeviceInfo = DeviceInfo {
    id: "test-x4",
    name: "Test X4",
    display: DisplayInfo {
        width: 480,
        height: 800,
        dpi: 220,
        format: PixelFormat::Mono1,
        rotation: Rotation::Deg0,
        color: None,
    },
    input: InputCaps {
        buttons: true,
        touch: false,
        pen: false,
        keys: &["back", "confirm", "left", "right", "up", "down", "power"],
    },
    memory: MemoryClass::Small,
};

const SCREEN: &str = "sleep.screen";
/// 64 × 64: clear around a black ring with a white middle.
const RING: &[u8] = include_bytes!("data/ring.png");
/// 480 × 800, all black: made for the whole X4 screen.
const FULL: &[u8] = include_bytes!("data/full-screen.png");
/// Where the ring goes in the middle at half the width: 240 × 240.
const BOX: (usize, usize, usize) = (120, 280, 240);
/// The picture in the middle, half the width large.
const MIDDLE: &str = "overlay-position = sleep.position.center\noverlay-size = 50";

/// A page of black lines, every eighth row, bound as the reader.
struct Page;

struct Lines;

impl Screen for Lines {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let display = env.device().display;
        canvas.clear(Color::WHITE);
        for y in (0..i32::from(display.height)).step_by(8) {
            canvas.fill_rect(0, y, u32::from(display.width), 1, Color::BLACK);
        }
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

/// A menu over the lower part of the page: black.
struct Menu;

impl Screen for Menu {
    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        Some(Rect::new(0, 600, 480, 200))
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Env<'_>) {
        canvas.clear(Color::BLACK);
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

/// A lock: it guards the device.
struct Guard;

impl Screen for Guard {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Env<'_>) {
        canvas.clear(Color::WHITE);
    }

    /// It lets the device sleep, as the lock does.
    fn on_action(&mut self, action: &Id, _: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::SLEEP {
            Handled::No
        } else {
            Handled::Yes
        }
    }

    fn guards(&self) -> bool {
        true
    }
}

impl Plugin for Page {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("page"), "Page", Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            Id::new_static("page.screen"),
            "Page",
            Box::new(|_, _| Box::new(Lines)),
        ))?;
        r.add_screen(ScreenSpec::new(
            Id::new_static("page.menu"),
            "Menu",
            Box::new(|_, _| Box::new(Menu)),
        ))?;
        r.add_screen(ScreenSpec::new(
            Id::new_static("page.lock"),
            "Lock",
            Box::new(|_, _| Box::new(Guard)),
        ))?;
        r.bind_slot(
            slots::READER,
            Id::new_static("page.screen"),
            Priority::DEFAULT,
        )
    }
}

/// Offers its owl as a picture over the page, in a corner; a moon widget
/// (a black square); and a sleep mode of its own, a night all black.
struct Owls;

struct Night;

thread_local! {
    /// Whether the moon shows in a place of widgets, as last asked.
    static MOON_SHOWN: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Asks, as it shows, whether the moon shows in any place of widgets.
struct Ask;

impl Screen for Ask {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        MOON_SHOWN.with(|m| m.set(Some(env.widgets().shown(&env, "owls.moon"))));
    }
}

/// How often the night was shown, and woken.
static NIGHT_SHOWN: AtomicUsize = AtomicUsize::new(0);
static NIGHT_WOKEN: AtomicUsize = AtomicUsize::new(0);

impl Screen for Night {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Env<'_>) {
        canvas.clear(Color::BLACK);
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }

    fn on_show(&mut self, _: &mut Cx<'_, '_>) {
        NIGHT_SHOWN.fetch_add(1, Ordering::Relaxed);
    }

    fn on_event(&mut self, event: &Id, _: &Params, _: &mut Cx<'_, '_>) {
        if *event == paperpaw_sdk::builtin::events::RESUME {
            NIGHT_WOKEN.fetch_add(1, Ordering::Relaxed);
        }
    }
}

static OWL: [Preset; 1] = [Preset::fixed(
    "owls.owl",
    overlay::KIND,
    "Owl",
    "",
    "sleep.overlay-picture = owls/owl.png\nsleep.overlay-position = sleep.position.top-left",
)];

impl Plugin for Owls {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("owls"), "Owls", Version::new(0, 1, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_presets(&OWL)?;
        r.add_widget(WidgetSpec::drawn(
            Id::new_static("owls.moon"),
            "Moon",
            Drawn::new(
                |_, _| 40,
                |canvas, _, area| canvas.fill(Rect::new(area.x, area.y, 40, 40), Color::BLACK),
            ),
        ))?;
        r.add_screen(ScreenSpec::new(
            Id::new_static("owls.ask"),
            "Ask",
            Box::new(|_, _| Box::new(Ask)),
        ))?;
        r.add_screen(ScreenSpec::new(
            Id::new_static("owls.night"),
            "owls.night",
            Box::new(|_, _| Box::new(Night)),
        ))?;
        r.add_choice(paperpaw_plugin_sleep::SHOW, "owls.night")?;
        // Not its own value: refused.
        r.add_choice(paperpaw_plugin_sleep::SHOW, "elsewhere.mode")
    }
}

struct Device {
    kernel: Kernel,
    now: u64,
    /// What the panel shows, a row after the other.
    shown: Vec<Color>,
}

impl Device {
    /// With `settings` in `[sleep]`, the overlay's pictures in `sleep/`,
    /// reading if `reading`.
    fn new(settings: &str, reading: bool) -> Self {
        Self::with_picture(settings, reading, RING)
    }

    /// The same, the pictures folder holding `picture`.
    fn with_picture(settings: &str, reading: bool, picture: &[u8]) -> Self {
        let storage = MemoryStorage::new()
            .with("sleep/ring.png", picture)
            .with("owls/owl.png", RING);
        let kernel = Kernel::builder(X4)
            .storage(storage)
            .user_config(format!(
                "[core]\nlanguage = en\n[sleep]\nshow = sleep.show.overlay\n{settings}\n"
            ))
            .sleep(true)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Sleep)
            .plugin(Page)
            .plugin(Owls)
            .build()
            .unwrap();
        let mut device = Self {
            kernel,
            now: 0,
            shown: vec![Color::WHITE; 480 * 800],
        };
        let effects = device.kernel.start(Instant(0));
        device.settle(effects);
        if reading {
            device.now += 100;
            let effects =
                device
                    .kernel
                    .open_slot(Instant(device.now), slots::READER, Params::new());
            device.settle(effects);
        }
        device.render();
        device
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

    /// The power button: the sleep screen; what it brought.
    fn sleep(&mut self) -> Effects {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), actions::SLEEP);
        self.settle(effects);
        effects
    }

    fn wake(&mut self) {
        self.now += 60_000;
        let effects = self.kernel.resume(Instant(self.now));
        self.settle(effects);
    }

    fn set(&mut self, id: &str, value: &str) {
        self.now += 100;
        let effects = self
            .kernel
            .set_setting(Instant(self.now), id, value)
            .unwrap();
        self.settle(effects);
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    fn render(&mut self) -> &[Color] {
        let shown = &mut self.shown;
        self.kernel.render(&mut |top, view| {
            for y in 0..view.height {
                for x in 0..view.width {
                    let at = (usize::from(top) + usize::from(y)) * 480 + usize::from(x);
                    shown[at] = view.pixel(x, y);
                }
            }
        });
        &self.shown
    }

    fn texts(&mut self) -> Vec<String> {
        self.render();
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect()
    }

    /// The sleeping cat shows: chosen, or where there is no cover or no
    /// picture.
    fn shows_cat(&mut self) -> bool {
        self.texts().iter().any(|t| t == "A sleeping cat")
    }
}

/// The pixel at `(x, y)` of `shown`.
fn at(shown: &[Color], x: usize, y: usize) -> Color {
    shown[y * 480 + x]
}

#[test]
fn a_picture_goes_over_the_page_which_shows_around_it() {
    let mut device = Device::new(MIDDLE, true);
    let page = device.render().to_vec();
    assert_eq!(at(&page, 0, 8), Color::BLACK);
    assert_eq!(at(&page, 0, 9), Color::WHITE);
    device.run(actions::SLEEP);
    assert_eq!(device.kernel.active_screen().map(Id::as_str), Some(SCREEN));
    let shown = device.render().to_vec();
    let (left, top, side) = BOX;
    // Outside the picture nothing changed, nothing dimmed.
    for (i, (now, before)) in shown.iter().zip(&page).enumerate() {
        let (x, y) = (i % 480, i / 480);
        if !(left..left + side).contains(&x) || !(top..top + side).contains(&y) {
            assert_eq!(now, before, "{x},{y}");
        }
    }
    // Its clear corners show the page; the ring is black, the middle
    // white over the lines.
    for y in top..top + 20 {
        assert_eq!(
            at(&shown, left + 2, y),
            at(&page, left + 2, y),
            "corner {y}"
        );
    }
    let middle = left + side / 2;
    assert_eq!(at(&shown, left + 40, top + side / 2), Color::BLACK);
    let line = (top + side / 2) / 8 * 8;
    assert_eq!(at(&page, middle, line), Color::BLACK);
    assert_eq!(at(&shown, middle, line), Color::WHITE);
    assert!(!device.shows_cat());
    // Waking, the page is as it was.
    device.now += 60_000;
    let effects = device.kernel.resume(Instant(device.now));
    device.settle(effects);
    assert_ne!(device.kernel.active_screen().map(Id::as_str), Some(SCREEN));
    assert_eq!(device.render(), &page[..]);
}

#[test]
fn the_page_shows_through_the_picture_as_much_as_asked() {
    let dark_in_ring = |settings: &str| {
        let mut device = Device::new(&format!("{MIDDLE}\n{settings}"), true);
        device.run(actions::SLEEP);
        let shown = device.render().to_vec();
        let (left, top, _) = BOX;
        // A square of the ring between two lines of the page.
        (top + 113..top + 119)
            .flat_map(|y| (left + 30..left + 50).map(move |x| (x, y)))
            .filter(|&(x, y)| at(&shown, x, y) == Color::BLACK)
            .count()
    };
    assert_eq!(dark_in_ring(""), 120);
    let half = dark_in_ring("overlay-see-through = 50");
    assert!((40..=80).contains(&half), "{half}");
}

/// The corners of the black the picture made on the page, not counting
/// the rows of the page's own lines.
fn black_box(shown: &[Color]) -> Option<(usize, usize, usize, usize)> {
    let black = |i: &usize| shown[*i] == Color::BLACK && !(*i / 480).is_multiple_of(8);
    let xs = (0..shown.len()).filter(black).map(|i| i % 480);
    let ys = (0..shown.len()).filter(black).map(|i| i / 480);
    Some((xs.clone().min()?, ys.clone().min()?, xs.max()?, ys.max()?))
}

#[test]
fn a_picture_made_for_the_screen_fills_it_at_full_size() {
    // 480 × 800 at 100 %: the whole panel, to the edges, no margin or
    // status line left uncovered (the page's lines are black, so the
    // white ones between them tell).
    let mut device = Device::with_picture("overlay-size = 100", true, FULL);
    device.run(actions::SLEEP);
    let shown = device.render().to_vec();
    assert!(shown.iter().all(|c| *c == Color::BLACK));
}

#[test]
fn a_picture_made_for_the_screen_is_a_part_of_it_at_a_part_size() {
    // Half the screen: 240 × 400, in the middle.
    let mut device = Device::with_picture(
        "overlay-size = 50\noverlay-position = sleep.position.center",
        true,
        FULL,
    );
    device.run(actions::SLEEP);
    let shown = device.render().to_vec();
    let (x0, y0, x1, y1) = black_box(&shown).expect("a picture");
    // Rows 200, 208, ... hold the page's black lines, not told apart.
    assert_eq!((x0, x1, y1), (120, 359, 599));
    assert!((200..=201).contains(&y0), "{y0}");
}

#[test]
fn size_and_place_are_settings() {
    let mut device = Device::new(
        "overlay-size = 20\noverlay-position = sleep.position.bottom-right",
        true,
    );
    let page = device.render().to_vec();
    device.run(actions::SLEEP);
    let shown = device.render().to_vec();
    // 96 pixels, inside the margin of the bottom right corner.
    let changed: Vec<(usize, usize)> = shown
        .iter()
        .zip(&page)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| (i % 480, i / 480))
        .collect();
    assert!(!changed.is_empty());
    assert!(
        changed
            .iter()
            .all(|&(x, y)| x >= 480 - 96 - 40 && y >= 800 - 96 - 100)
    );
    assert!(changed.iter().all(|&(x, y)| x < 480 - 8 && y < 800 - 30));
}

#[test]
fn only_where_it_is_used_else_the_cover_or_the_cat() {
    // Not reading: the cover, of which there is none.
    let mut device = Device::new("overlay-when = sleep.when.reading", false);
    device.run(actions::SLEEP);
    assert_eq!(device.kernel.active_screen().map(Id::as_str), Some(SCREEN));
    assert!(device.shows_cat());
    let mut device = Device::new("overlay-when = sleep.when.reading", true);
    device.run(actions::SLEEP);
    assert!(!device.shows_cat());
    let mut device = Device::new("overlay-when = sleep.when.elsewhere", true);
    device.run(actions::SLEEP);
    assert!(device.shows_cat());
    // No picture to show.
    let mut device = Device::new("folder = empty", true);
    device.run(actions::SLEEP);
    assert!(device.shows_cat());
}

#[test]
fn plugins_offer_pictures_in_the_picker() {
    let mut device = Device::new("", true);
    device.now += 100;
    let effects = device
        .kernel
        .open_screen(Instant(device.now), overlay::PICK, Params::new());
    device.settle(effects);
    let texts = device.texts();
    assert!(
        texts
            .iter()
            .any(|t| t.starts_with("From the pictures folder")),
        "{texts:?}"
    );
    assert!(texts.iter().any(|t| t == "Owl"), "{texts:?}");
    // The owl: its picture and its place.
    device.run(actions::DOWN);
    device.run(actions::CONFIRM);
    let settings = device.kernel.settings();
    assert_eq!(settings.text(overlay::PICTURE.as_str()), "owls/owl.png");
    assert_eq!(
        settings.text(overlay::POSITION.as_str()),
        "sleep.position.top-left"
    );
    assert_ne!(
        device.kernel.active_screen(),
        Some(&overlay::PICK),
        "closed once chosen"
    );
    // Asleep, it is in the top left corner.
    let page = device.render().to_vec();
    device.run(actions::SLEEP);
    let shown = device.render().to_vec();
    assert_ne!(at(&shown, 60, 140), at(&page, 60, 140));
    assert_eq!(&shown[600 * 480..], &page[600 * 480..]);
    // And back to the folder.
    device.now += 60_000;
    let effects = device.kernel.resume(Instant(device.now));
    device.settle(effects);
    let effects = device
        .kernel
        .open_screen(Instant(device.now), overlay::PICK, Params::new());
    device.settle(effects);
    device.run(actions::CONFIRM);
    assert_eq!(device.kernel.settings().text(overlay::PICTURE.as_str()), "");
}

#[test]
fn by_default_below_on_the_right_clear_of_the_status_lines() {
    let mut device = Device::new("", true);
    let page = device.render().to_vec();
    let effects = device.sleep();
    // A picture over all of the screen: in grays where the panel has them.
    assert!(effects.grays());
    let shown = device.render().to_vec();
    let changed = changes(&shown, &page);
    assert!(!changed.is_empty());
    // A third of the width, more or less.
    let (left, right) = (
        changed.iter().map(|c| c.0).min().unwrap(),
        changed.iter().map(|c| c.0).max().unwrap(),
    );
    assert!((120..=200).contains(&(right - left)), "{left}..{right}");
    assert!(
        changed.iter().all(|&(x, y)| x > 240 && y > 400),
        "on the right below"
    );
    assert!(
        changed.iter().all(|&(_, y)| y < 800 - 30),
        "clear of the status line"
    );
}

#[test]
fn over_the_page_not_a_menu_left_open() {
    let mut device = Device::new(
        &format!("{MIDDLE}\noverlay-when = sleep.when.reading"),
        true,
    );
    device.now += 100;
    let effects = device.kernel.open_screen(
        Instant(device.now),
        Id::new_static("page.menu"),
        Params::new(),
    );
    device.settle(effects);
    let with_menu = device.render().to_vec();
    assert_eq!(at(&with_menu, 5, 701), Color::BLACK);
    device.run(actions::SLEEP);
    assert_eq!(device.kernel.active_screen().map(Id::as_str), Some(SCREEN));
    // Read below the menu: the picture shows, over the page alone.
    assert!(!device.shows_cat());
    let shown = device.render().to_vec();
    assert_eq!(at(&shown, 5, 701), Color::WHITE, "the page, not the menu");
    assert_eq!(at(&shown, 5, 704), Color::BLACK, "a line of the page");
}

#[test]
fn never_over_a_lock() {
    let mut device = Device::new("", true);
    device.now += 100;
    let effects = device.kernel.open_screen(
        Instant(device.now),
        Id::new_static("page.lock"),
        Params::new(),
    );
    device.settle(effects);
    device.run(actions::SLEEP);
    assert_eq!(device.kernel.active_screen().map(Id::as_str), Some(SCREEN));
    // The cover, of which there is none: the message, nothing of the page.
    assert!(device.shows_cat());
}

#[test]
fn the_picture_is_decoded_once_while_it_stays_the_same() {
    let mut device = Device::new(MIDDLE, true);
    device.run(actions::SLEEP);
    let decoded = format!("{}/overlay.gray", Env::plugin_dir("sleep"));
    let kept = device.kernel.storage().size(&decoded).unwrap();
    assert!(kept > 0);
    device.wake();
    // A picture of the same size (64 x 64, never made larger), all
    // black: kept, not decoded again.
    let mut black = b"PPGA".to_vec();
    black.extend_from_slice(&64u32.to_le_bytes());
    black.extend_from_slice(&64u32.to_le_bytes());
    black.extend(std::iter::repeat_n([0u8, 255], 64 * 64).flatten());
    assert_eq!(black.len() as u64, kept);
    device.kernel.storage().write(&decoded, &black).unwrap();
    device.run(actions::SLEEP);
    let shown = device.render().to_vec();
    let (left, top, _) = BOX;
    assert_eq!(at(&shown, left + 2, top + 2), Color::BLACK, "the kept one");
    // A picture that cannot be read: the key of the one before goes.
    device.wake();
    let key = format!("{}/overlay.key", Env::plugin_dir("sleep"));
    assert!(device.kernel.storage().exists(&key));
    device
        .kernel
        .storage()
        .write("owls/broken.png", b"not a picture")
        .unwrap();
    device.set("sleep.overlay-picture", "owls/broken.png");
    device.run(actions::SLEEP);
    assert!(!device.kernel.storage().exists(&key));
    device.set("sleep.overlay-picture", "");
    // Larger: decoded anew, the ring's clear corner showing the page.
    device.wake();
    device.set("sleep.overlay-size", "60");
    device.run(actions::SLEEP);
    let shown = device.render().to_vec();
    assert_ne!(at(&shown, 99, 241), Color::BLACK);
}

#[test]
fn a_widget_in_place_of_a_picture() {
    let mut device = Device::new("overlay-widget = owls.moon", true);
    let page = device.render().to_vec();
    device.run(actions::SLEEP);
    assert!(!device.shows_cat());
    let shown = device.render().to_vec();
    let changed = changes(&shown, &page);
    // The moon, black, on its panel of paper below on the right.
    let black = changed
        .iter()
        .filter(|&&(x, y)| at(&shown, x, y) == Color::BLACK)
        .count();
    assert!(black >= 40 * 30, "{black}");
    assert!(changed.iter().all(|&(x, y)| x > 240 && y > 400));
}

#[test]
fn the_picker_says_why_the_last_picture_did_not_show() {
    let mut device = Device::new("overlay-picture = owls/missing.png", true);
    device.run(actions::SLEEP);
    // Not shown: the cover, of which there is none.
    assert!(device.shows_cat());
    device.wake();
    device.now += 100;
    let effects = device
        .kernel
        .open_screen(Instant(device.now), overlay::PICK, Params::new());
    device.settle(effects);
    let texts = device.texts().join(" | ");
    assert!(texts.contains("Not shown at the last sleep"), "{texts}");
    assert!(texts.contains("owls/missing.png"), "{texts}");
    assert!(texts.contains("Widget: Moon"), "{texts}");
}

#[test]
fn other_plugins_add_sleep_modes() {
    let mut device = Device::new("show = owls.night", true);
    let choices: Vec<String> = match &device.kernel.settings().spec("sleep.show").unwrap().kind {
        paperpaw_sdk::SettingKind::Choice(choices) => choices.iter().map(String::from).collect(),
        _ => Vec::new(),
    };
    assert_eq!(choices.last().map(String::as_str), Some("owls.night"));
    assert!(!choices.iter().any(|c| c == "elsewhere.mode"));
    let warnings: Vec<String> = device
        .kernel
        .take_warnings()
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(
        warnings.iter().any(|w| w.contains("elsewhere.mode")),
        "{warnings:?}"
    );
    device.run(actions::SLEEP);
    assert_eq!(device.kernel.active_screen().map(Id::as_str), Some(SCREEN));
    let shown = device.render().to_vec();
    assert_eq!(at(&shown, 240, 400), Color::BLACK);
    assert_eq!(at(&shown, 5, 9), Color::BLACK);
    assert!(NIGHT_SHOWN.load(Ordering::Relaxed) >= 1, "told it shows");
    // Asleep as the sleep screen is: it closes as the device wakes.
    device.wake();
    assert!(NIGHT_WOKEN.load(Ordering::Relaxed) >= 1, "told of waking");
    assert_ne!(device.kernel.active_screen().map(Id::as_str), Some(SCREEN));
}

/// The pixels that differ, as `(x, y)`.
fn changes(shown: &[Color], page: &[Color]) -> Vec<(usize, usize)> {
    shown
        .iter()
        .zip(page)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(i, _)| (i % 480, i / 480))
        .collect()
}

/// A widget over the page shows in a place of widgets: a plugin asking
/// whether its widget shows (to fetch what it shows) is told so.
#[test]
fn a_widget_over_the_page_counts_as_shown() {
    // Shown only while the sleep screen shows the page with it: another
    // mode set later wins (`show` comes first in the settings).
    for (settings, shown) in [
        ("overlay-widget = owls.moon", true),
        ("", false),
        (
            "overlay-widget = owls.moon
show = sleep.show.cover",
            false,
        ),
    ] {
        let mut device = Device::new(settings, false);
        device.now += 100;
        let effects = device.kernel.open_screen(
            Instant(device.now),
            Id::new_static("owls.ask"),
            Params::new(),
        );
        device.settle(effects);
        assert_eq!(
            MOON_SHOWN.with(std::cell::Cell::get),
            Some(shown),
            "{settings:?}"
        );
    }
}
