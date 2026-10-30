//! The sleep screen with the real kernel: what shows when the device goes
//! to sleep with no cover, no pictures, broken pictures, several
//! pictures in turn (and the count of the next one kept in `next.txt`,
//! also when that file is damaged), the page kept, and the screen gone
//! when the device wakes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_plugin_sleep::Sleep;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::{Env, Id, MemoryStorage};

/// An X4 as the emulator's profile has it: buttons, no touch.
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

const SCREEN: &str = "sleep.screen";
/// A real picture: the logo of the about page.
const PNG: &[u8] = include_bytes!("../../about/logo.png");

fn next_file() -> String {
    format!("{}/next.txt", Env::plugin_dir("sleep"))
}

struct Device {
    kernel: Kernel,
    now: u64,
}

impl Device {
    fn new(storage: MemoryStorage, show: &str) -> Self {
        Self::with(storage, &format!("[sleep]\nshow = {show}\n"))
    }

    /// A device with the settings of `config` (sections and lines).
    fn with(storage: MemoryStorage, config: &str) -> Self {
        let kernel = Kernel::builder(X4)
            .storage(storage)
            .user_config(format!("[core]\nlanguage = en\n{config}"))
            .sleep(true)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(Sleep)
            .build()
            .unwrap();
        let mut device = Self { kernel, now: 0 };
        let effects = device.kernel.start(Instant(0));
        device.settle(effects);
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

    /// The power button: the sleep screen, then sleep.
    fn sleep(&mut self) -> Effects {
        self.now += 100;
        self.kernel.run_action(Instant(self.now), actions::SLEEP)
    }

    fn wake(&mut self) {
        self.now += 60_000;
        let effects = self.kernel.resume(Instant(self.now));
        self.settle(effects);
    }

    fn on_top(&self) -> Option<&str> {
        self.kernel.active_screen().map(Id::as_str)
    }

    /// The texts shown.
    fn texts(&mut self) -> Vec<String> {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect()
    }

    fn says_sleeping(&mut self) -> bool {
        self.texts().iter().any(|t| t == "Sleeping")
    }

    /// The sleeping cat shows: chosen, or where there is no cover or no
    /// picture.
    fn shows_cat(&mut self) -> bool {
        self.texts().iter().any(|t| t == "A sleeping cat")
    }

    fn next(&self) -> Option<String> {
        let data = paperpaw_sdk::read_file(self.kernel.storage(), &next_file(), 64).ok()?;
        Some(String::from_utf8(data).unwrap())
    }
}

#[test]
fn without_a_cover_the_cat_shows_and_waking_closes_it() {
    let mut device = Device::new(MemoryStorage::new(), "sleep.show.cover");
    let effects = device.sleep();
    assert!(effects.sleep());
    assert_eq!(device.on_top(), Some(SCREEN));
    assert!(device.shows_cat());
    device.wake();
    assert_ne!(device.on_top(), Some(SCREEN));
}

#[test]
fn the_message_shows_when_asked_for() {
    let storage = MemoryStorage::new().with("sleep/a.png", PNG);
    let mut device = Device::new(storage, "sleep.show.message");
    device.sleep();
    assert!(device.says_sleeping());
    // The pictures are not counted on.
    assert_eq!(device.next(), None);
}

#[test]
fn no_pictures_in_the_folder_shows_the_cat() {
    // Hidden files, folders and other files are no pictures; no folder
    // at all neither.
    let storage = MemoryStorage::new()
        .with("sleep/.hidden.png", PNG)
        .with("sleep/notes.txt", "not a picture")
        .with("sleep/folder.png/inside.png", PNG);
    let mut device = Device::new(storage, "sleep.show.picture");
    device.sleep();
    assert!(device.shows_cat());
    assert_eq!(device.next(), None);

    let mut device = Device::new(MemoryStorage::new(), "sleep.show.picture");
    device.sleep();
    assert!(device.shows_cat());
}

#[test]
fn a_picture_shows_instead_of_the_cat() {
    let storage = MemoryStorage::new().with("sleep/Cat.PNG", PNG);
    let mut device = Device::new(storage, "sleep.show.picture");
    device.sleep();
    assert_eq!(device.on_top(), Some(SCREEN));
    assert!(!device.shows_cat());
    assert_eq!(device.next().as_deref(), Some("1"));
}

#[test]
fn a_broken_picture_shows_the_cat() {
    let mut truncated = PNG.to_vec();
    truncated.truncate(PNG.len() / 2);
    for data in [b"not a picture at all".to_vec(), truncated, Vec::new()] {
        let storage = MemoryStorage::new().with("sleep/broken.png", data);
        let mut device = Device::new(storage, "sleep.show.picture");
        device.sleep();
        assert_eq!(device.on_top(), Some(SCREEN));
        assert!(device.shows_cat());
    }
}

#[test]
fn pictures_take_turns_in_name_order() {
    let storage = MemoryStorage::new()
        .with("sleep/b.png", PNG)
        .with("sleep/a.png", PNG)
        .with("sleep/c.jpg", b"broken jpeg".to_vec());
    let mut device = Device::new(storage, "sleep.show.picture");
    let mut shown = Vec::new();
    for _ in 0..4 {
        device.sleep();
        shown.push(!device.shows_cat());
        device.wake();
    }
    // a, b, then the broken c (the message), then a again.
    assert_eq!(shown, [true, true, false, true]);
    assert_eq!(device.next().as_deref(), Some("4"));
}

#[test]
fn the_folder_is_the_setting_with_slashes_around_it_or_not() {
    let config = |folder: &str| format!("sleep.show.picture\nfolder = {folder}");
    for folder in ["Pictures/Night", "/Pictures/Night/"] {
        let storage = MemoryStorage::new().with("Pictures/Night/a.png", PNG);
        let mut device = Device::new(storage, &config(folder));
        device.sleep();
        assert!(!device.shows_cat(), "{folder}");
    }
}

#[test]
fn a_damaged_count_starts_again_at_the_first_picture() {
    for count in ["", "seven", "-3", "   ", "\u{0}\u{0}"] {
        let storage = MemoryStorage::new()
            .with("sleep/a.png", PNG)
            .with(&next_file(), count);
        let mut device = Device::new(storage, "sleep.show.picture");
        device.sleep();
        assert!(!device.shows_cat(), "{count:?}");
        assert_eq!(device.next().as_deref(), Some("1"), "{count:?}");
    }
    // Version 1 of the count is read.
    let storage = MemoryStorage::new()
        .with("sleep/a.png", PNG)
        .with("sleep/b.png", PNG)
        .with(&next_file(), include_str!("../testdata/next.txt.v1"));
    let mut device = Device::new(storage, "sleep.show.picture");
    device.sleep();
    assert_eq!(device.next().as_deref(), Some("8"));
}

/// The count read from `next.txt` plus one overflows at the largest
/// number: a panic in debug builds (and a wrap in release ones).
#[test]
fn the_largest_count_does_not_overflow() {
    let storage = MemoryStorage::new()
        .with("sleep/a.png", PNG)
        .with(&next_file(), usize::MAX.to_string());
    let mut device = Device::new(storage, "sleep.show.picture");
    device.sleep();
    assert!(!device.shows_cat());
}

#[test]
fn the_page_stays_as_it_is() {
    let mut device = Device::new(MemoryStorage::new(), "sleep.show.page");
    device.sleep();
    assert_eq!(device.on_top(), Some(SCREEN));
    assert!(!device.says_sleeping());
    device.wake();
    assert_ne!(device.on_top(), Some(SCREEN));
}

#[test]
fn widgets_that_are_not_there_show_the_message() {
    let mut device = Device::new(
        MemoryStorage::new(),
        "sleep.show.widgets\nwidgets = no.such-widget, home.fill, other.gone",
    );
    device.sleep();
    assert!(device.says_sleeping());
}

/// Asleep, the screen takes every button but *Sleep* and *Home*: those
/// pass, so that the device sleeps again or goes home.
#[test]
fn buttons_do_nothing_while_asleep_but_home() {
    let mut device = Device::new(MemoryStorage::new(), "sleep.show.message");
    device.sleep();
    for action in [actions::BACK, actions::CONFIRM, actions::MENU] {
        device.now += 100;
        let effects = device
            .kernel
            .run_action(Instant(device.now), action.clone());
        device.settle(effects);
        assert_eq!(device.on_top(), Some(SCREEN), "{action}");
    }
    device.now += 100;
    let effects = device.kernel.run_action(Instant(device.now), actions::HOME);
    device.settle(effects);
    assert_ne!(device.on_top(), Some(SCREEN));
}

#[test]
fn an_unknown_choice_shows_the_cover_or_the_cat() {
    let mut device = Device::new(MemoryStorage::new(), "sleep.show.from-the-future");
    device.sleep();
    assert!(device.shows_cat());
    // Storage untouched by it.
    assert!(!device.kernel.storage().exists(&next_file()));
}

/// Nothing chosen: the sleeping cat, in the middle of the screen with
/// even margins, decoded once for the screen and drawn from that file
/// the next times.
#[test]
fn the_sleeping_cat_is_the_default() {
    let mut device = Device::with(MemoryStorage::new(), "");
    device.sleep();
    device.kernel.render(&mut |_, _| {});
    let cat = device
        .kernel
        .describe()
        .into_iter()
        .find(|p| p.label == "A sleeping cat")
        .expect("the cat");
    let a = cat.area;
    assert!(a.width >= 400 && a.width < 480, "{a:?}");
    assert_eq!(480 - (a.x + a.width), a.x, "even side margins");
    assert!(
        (i32::from(800 - (a.y + a.height)) - i32::from(a.y)).abs() <= 1,
        "{a:?}"
    );
    let decoded = format!("{}/cat-{}.gray", Env::plugin_dir("sleep"), 480 - 2 * a.x);
    assert!(device.kernel.storage().exists(&decoded));
    device.wake();
    // Asleep again: drawn from that file, which is not written anew (a
    // file that marks it as read stays as it was put there).
    let marked = paperpaw_sdk::read_file(device.kernel.storage(), &decoded, 1 << 20).unwrap();
    device.sleep();
    assert!(device.shows_cat());
    let again = paperpaw_sdk::read_file(device.kernel.storage(), &decoded, 1 << 20).unwrap();
    assert_eq!(marked, again);
}
