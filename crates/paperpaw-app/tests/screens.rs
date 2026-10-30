//! What the standard app's screens show, compared with what they showed
//! when last looked at (`tests/snapshots`): each part a screen describes
//! (rows, buttons, headings) with its label and state, in order. A change
//! to what a screen shows (a row gone, another order, a label, a switch's
//! state) fails here; places on the screen are left out, so a change of
//! fonts does not. After a change that is meant, the files are written
//! anew with `PAPERPAW_SNAPSHOTS=update`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::fmt::Write as _;
use std::path::PathBuf;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass, PixelFormat, Point,
    PointerEvent, PointerPhase, PointerTool, Rotation,
};
use paperpaw_sdk::builtin::{actions, params, slots};
use paperpaw_sdk::{Id, MemoryStorage, Params};

/// An X4 as the emulator's profile has it: buttons, no touch.
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

/// A touch screen as large as the X4's.
const TOUCH: DeviceInfo = DEVICE
    .with_id("test-touch")
    .with_name("Test touch")
    .with_input(InputCaps::new(false, true, false, &[]));

const BOOK: &str = "books/alice-in-wonderland.txt";

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
    /// The app on a card with two books, past its first-run setup.
    fn new() -> Self {
        Self::on(DEVICE)
    }

    /// The same on `device`.
    fn on(device: DeviceInfo) -> Self {
        let storage = MemoryStorage::new()
            .with(
                BOOK,
                include_str!("../../../samples/library/books/alice-in-wonderland.txt"),
            )
            .with(
                "books/die-verwandlung.txt",
                include_str!("../../../samples/library/books/die-verwandlung.txt"),
            )
            // The first-run setup was done.
            .with(".paperpaw/plugins/setup/done", "");
        let builder = Kernel::builder(device)
            .storage(storage)
            // On touch screens without the hints over it all.
            .user_config("[core]\nlanguage = en\n[help]\nshow = false\n");
        let mut kernel = paperpaw_app::install(builder, fonts()).build().unwrap();
        let effects = kernel.start(Instant(0));
        let mut app = Self { kernel, now: 0 };
        app.settle(effects);
        app
    }

    /// Lets timers run until nothing is due within a second.
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

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    /// A tap on the part of the screen that says `label`.
    fn tap_on(&mut self, label: &str) {
        self.kernel.render(&mut |_, _| {});
        let parts = self.kernel.describe();
        let area = paperpaw_sdk::semantics::find(&parts, label)
            .unwrap_or_else(|| panic!("nothing says {label}"))
            .area;
        let at = Point::new(
            i32::from(area.x) + i32::from(area.width) / 2,
            i32::from(area.y) + i32::from(area.height) / 2,
        );
        self.finger(PointerPhase::Down, at);
        self.finger(PointerPhase::Up, at);
    }

    /// A finger at `point`, coming down, moving or lifted.
    fn finger(&mut self, phase: PointerPhase, point: Point) {
        self.now += 50;
        let event = InputEvent::Pointer(PointerEvent::new(
            0,
            phase,
            point,
            PointerTool::Finger,
            1000,
        ));
        let effects = self.kernel.handle(Instant(self.now), event);
        // Timers wait while the finger is down: a long press is not meant.
        if phase == PointerPhase::Up {
            self.settle(effects);
        }
    }

    fn open_book(&mut self) {
        self.now += 100;
        let params = Params::new().with(params::PATH, BOOK);
        let effects = self
            .kernel
            .open_slot(Instant(self.now), slots::READER, params);
        self.settle(effects);
    }

    /// What the screen shows: its id, then a line for each part.
    fn described(&mut self) -> String {
        // What is shown is drawn first.
        self.kernel.render(&mut |_, _| {});
        let mut text = String::new();
        let screen = self.kernel.active_screen().map_or("", |id| id.as_str());
        writeln!(text, "screen {screen}").unwrap();
        for part in self.kernel.describe() {
            let mut state = String::new();
            if part.focused {
                state.push_str(" (focused)");
            }
            match part.on {
                Some(true) => state.push_str(" (on)"),
                Some(false) => state.push_str(" (off)"),
                None => {}
            }
            let label = part.label.replace('\n', " / ");
            writeln!(text, "{:?} \"{label}\"{state}", part.role).unwrap();
        }
        text
    }

    /// Compares what the screen shows with `tests/snapshots/{name}.txt`.
    fn check(&mut self, name: &str) {
        let shown = self.described();
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots")
            .join(format!("{name}.txt"));
        if std::env::var("PAPERPAW_SNAPSHOTS").as_deref() == Ok("update") {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &shown).unwrap();
            return;
        }
        let kept = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("{} missing: PAPERPAW_SNAPSHOTS=update", path.display()))
            .replace("\r\n", "\n");
        assert_eq!(
            shown, kept,
            "{name} shows something else now (PAPERPAW_SNAPSHOTS=update if meant)"
        );
    }
}

#[test]
fn home() {
    let mut app = App::new();
    app.check("home");
}

#[test]
fn library() {
    let mut app = App::new();
    app.run(actions::LIBRARY);
    app.check("library");
}

#[test]
fn main_menu_and_tools() {
    let mut app = App::new();
    app.run(actions::MENU);
    app.check("main-menu");
    app.run(actions::BACK);
    app.run(actions::TOOLS);
    app.check("tools");
}

#[test]
fn settings_and_a_group() {
    let mut app = App::new();
    app.run(actions::SETTINGS);
    app.check("settings");
    // Past the search, the first group.
    app.run(actions::DOWN);
    app.run(actions::CONFIRM);
    app.check("settings-reading");
}

#[test]
fn reading_and_its_menus() {
    let mut app = App::new();
    app.open_book();
    app.check("reader");
    app.run(actions::CONFIRM);
    app.check("reader-menu");
    app.run(actions::BACK);
    app.run(Id::new_static("reader.quick-settings"));
    app.check("reader-layout");
}

/// A number changed with the buttons shows its values at once, kept only
/// when chosen; *back* goes back to the one before.
#[test]
fn numbers_change_with_the_buttons_kept_when_chosen() {
    let mut app = App::new();
    app.run(actions::SETTINGS);
    app.run(actions::DOWN);
    app.run(actions::CONFIRM);
    // The reading settings, their page of the text: past the font, the
    // font size.
    app.run(actions::CONFIRM);
    app.run(actions::DOWN);
    let size = "core.font-size";
    let before = app.kernel.settings().int(size);
    app.run(actions::CONFIRM);
    app.run(actions::RIGHT);
    app.run(actions::RIGHT);
    let settings = app.kernel.settings();
    assert!(settings.previewing());
    assert_eq!(settings.int(size), before + 2);
    assert!(!settings.is_set(size));
    // Back: as before.
    app.run(actions::BACK);
    let settings = app.kernel.settings();
    assert!(!settings.previewing());
    assert_eq!(settings.int(size), before);
    assert!(app.described().contains("screen settings.group"));
    // Chosen: kept.
    app.run(actions::CONFIRM);
    app.run(actions::LEFT);
    app.run(actions::CONFIRM);
    let settings = app.kernel.settings();
    assert!(!settings.previewing());
    assert!(settings.is_set(size));
    // The X4's reading font draws 10 and 11 points alike: a step down
    // from 11 goes on to the next size that looks different.
    assert_eq!(settings.int(size), before - 2);
}

/// On a touch screen a slider's knob is taken along by the finger, as far
/// as it moves: what it is moved to shows at once and is kept where the
/// finger is lifted.
#[test]
fn sliders_follow_the_finger_kept_where_it_is_lifted() {
    let mut app = App::on(TOUCH);
    app.run(actions::SETTINGS);
    app.tap_on("Reading");
    app.tap_on("Text");
    app.kernel.render(&mut |_, _| {});
    let track = app
        .kernel
        .describe()
        .into_iter()
        .find(|p| p.role == paperpaw_sdk::semantics::Role::Slider)
        .expect("a slider on the reading page")
        .area;
    let y = i32::from(track.y) + i32::from(track.height) / 2;
    let size = "core.font-size";
    let before = app.kernel.settings().int(size);
    // As far as the reading font's sizes go.
    let Some(paperpaw_sdk::SettingKind::Int { max, .. }) =
        app.kernel.settings().spec(size).map(|s| s.kind.clone())
    else {
        panic!("a number");
    };
    // Down at the track's start, moved its whole length: as far, to the
    // end.
    app.finger(PointerPhase::Down, Point::new(i32::from(track.x), y));
    app.finger(PointerPhase::Move, Point::new(track.right() as i32, y));
    let settings = app.kernel.settings();
    assert!(settings.previewing());
    // The largest size the font draws, shown as the smallest drawn alike
    // (15 and 16 points look the same).
    assert_eq!(settings.int(size), max - 1, "the end of the track");
    assert!(!settings.is_set(size));
    // As far past the start: the start.
    let past = i32::from(track.x) - i32::from(track.width);
    app.finger(PointerPhase::Move, Point::new(past, y));
    app.finger(PointerPhase::Up, Point::new(past, y));
    let settings = app.kernel.settings();
    assert!(!settings.previewing());
    assert_eq!(settings.int(size), 6, "the start of the track");
    assert!(settings.is_set(size));
    assert_ne!(before, 6);
}

/// The home screen's widgets read what they show when it shows again:
/// after reading, the book read is the one to continue.
#[test]
fn home_shows_the_book_read_last() {
    let mut app = App::new();
    assert!(app.described().contains("Start reading"));
    app.open_book();
    app.run(actions::HOME);
    let shown = app.described();
    assert!(shown.starts_with("screen home.dashboard"), "{shown}");
    assert!(shown.contains("Continue reading"), "{shown}");
    assert!(shown.contains("Alice"), "{shown}");
}

/// A group's advanced settings, a page one level down, stay that page
/// when one of them changes.
#[test]
fn a_groups_advanced_page_stays_when_a_setting_changes() {
    let mut app = App::new();
    app.now += 100;
    let params = Params::new()
        .with(params::ID, "core.group.reading-layout")
        .with("more", "1");
    let effects =
        app.kernel
            .open_screen(Instant(app.now), Id::new_static("settings.group"), params);
    app.settle(effects);
    assert!(app.described().contains("Heading \"Layout · More\""));
    // A margin changed with the buttons and kept: the page is made anew,
    // as the advanced page still.
    app.run(actions::CONFIRM);
    app.run(actions::UP);
    app.run(actions::CONFIRM);
    let shown = app.described();
    assert!(shown.contains("Heading \"Layout · More\""), "{shown}");
    assert!(shown.contains("Top and bottom margins, 5 mm"), "{shown}");
}
