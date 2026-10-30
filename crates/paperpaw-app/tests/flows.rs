//! Whole-system flows through the standard app, as a user goes them: the
//! first-run setup to the home screen, a book opened from the library
//! and read, home again; settings changed on their pages, saved to
//! `paperpaw.ini` and read back by a kernel started anew on the same
//! card; every entry of the main menu and the tools, and every screen a
//! plugin adds, opened and closed again on a device with buttons only
//! (as the X4) and on a touch screen.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DirEntry, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass, PixelFormat,
    Point, PointerEvent, PointerPhase, PointerTool, Rotation, Storage, StorageError,
};
use paperpaw_sdk::builtin::{CONFIG_FILE, actions, settings as core_settings};
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::{Id, MemoryStorage, Params, library};

/// An X4 as the emulator's profile has it: buttons, no touch.
const BUTTONS: DeviceInfo = DeviceInfo::new(
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

/// A touch screen as large as the X4's, without buttons.
const TOUCH: DeviceInfo = BUTTONS
    .with_id("test-touch")
    .with_name("Test touch")
    .with_input(InputCaps::new(false, true, false, &[]));

const ALICE: &str = "books/alice-in-wonderland.txt";
const GREGOR: &str = "books/die-verwandlung.txt";
const SETUP_DONE: &str = ".paperpaw/plugins/setup/done";

/// The card, kept over restarts of the kernel.
#[derive(Clone)]
struct Card(Rc<MemoryStorage>);

impl Card {
    /// A card with two books; past the first-run setup if `set_up`.
    fn new(set_up: bool) -> Self {
        let mut storage = MemoryStorage::new()
            .with(
                ALICE,
                include_str!("../../../samples/library/books/alice-in-wonderland.txt"),
            )
            .with(
                GREGOR,
                include_str!("../../../samples/library/books/die-verwandlung.txt"),
            );
        if set_up {
            storage = storage.with(SETUP_DONE, "");
        }
        Self(Rc::new(storage))
    }

    fn text(&self, path: &str) -> Option<String> {
        self.0
            .get(path)
            .map(|data| String::from_utf8(data).expect("UTF-8"))
    }
}

impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }
}

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
    /// The standard app started on `device` with `card` in it.
    fn start(device: DeviceInfo, card: &Card) -> Self {
        let builder = Kernel::builder(device)
            .storage(card.clone())
            // Without the tips on gestures over everything on touch screens.
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

    fn open(&mut self, screen: Id) {
        self.now += 100;
        let effects = self
            .kernel
            .open_screen(Instant(self.now), screen, Params::new());
        self.settle(effects);
    }

    fn screen(&self) -> String {
        self.kernel
            .active_screen()
            .map(ToString::to_string)
            .unwrap_or_default()
    }

    /// What the screen shows, drawn first: each part's role and label.
    fn parts(&mut self) -> Vec<paperpaw_sdk::semantics::Part> {
        self.kernel.render(&mut |_, _| {});
        self.kernel.describe()
    }

    fn shows(&mut self, text: &str) -> bool {
        self.parts().iter().any(|p| p.label.contains(text))
    }

    /// The label of the part that has the focus.
    fn focused(&mut self) -> Option<String> {
        self.parts()
            .into_iter()
            .find(|p| p.focused)
            .map(|p| p.label)
    }

    /// A tap on the part of the screen that says `label`.
    fn tap_on(&mut self, label: &str) {
        let parts = self.parts();
        let area = paperpaw_sdk::semantics::find(&parts, label)
            .unwrap_or_else(|| panic!("nothing on {} says {label}", self.screen()))
            .area;
        self.tap(Point::new(
            i32::from(area.x) + i32::from(area.width) / 2,
            i32::from(area.y) + i32::from(area.height) / 2,
        ));
    }

    fn tap(&mut self, at: Point) {
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            self.now += 50;
            let event =
                InputEvent::Pointer(PointerEvent::new(0, phase, at, PointerTool::Finger, 1000));
            let effects = self.kernel.handle(Instant(self.now), event);
            if phase == PointerPhase::Up {
                self.settle(effects);
            }
        }
    }

    /// Moves the focus down with the buttons until it rests on a part
    /// saying `label`, at most `steps` times.
    fn focus_on(&mut self, label: &str, steps: usize) {
        for _ in 0..steps {
            if self.focused().is_some_and(|l| l.contains(label)) {
                return;
            }
            self.run(actions::DOWN);
        }
        panic!(
            "the focus never came to {label} on {}: {:?}",
            self.screen(),
            self.focused()
        );
    }

    /// The labels of the rows of the screen, in order.
    fn rows(&mut self) -> Vec<String> {
        self.parts()
            .into_iter()
            .filter(|p| p.role == Role::Row)
            .map(|p| p.label)
            .collect()
    }

    /// Warnings the kernel collected that say something went wrong.
    fn warnings(&mut self) -> Vec<String> {
        self.kernel
            .take_warnings()
            .into_iter()
            .map(|w| w.to_string())
            .collect()
    }
}

/// The first start: the setup guide, gone through with *Next* alone, then
/// the home screen; a book chosen in the library with the buttons, read,
/// and home again: the book is the one to continue, now and after a
/// restart, and the guide does not come again.
#[test]
fn first_run_to_reading_and_home_keeps_the_book_opened() {
    let card = Card::new(false);
    let mut app = App::start(BUTTONS, &card);
    assert_eq!(app.screen(), "setup.guide");
    for _ in 0..30 {
        if app.screen() != "setup.guide" {
            break;
        }
        app.run(actions::NEXT_PAGE);
    }
    assert_eq!(app.screen(), "home.dashboard", "past the guide's last page");
    assert!(card.0.exists(SETUP_DONE), "the guide noted it was done");
    assert!(app.shows("Start reading"));

    // The library: the book chosen with the buttons.
    app.run(actions::LIBRARY);
    let library = app.screen();
    app.focus_on("Alice", 20);
    app.run(actions::CONFIRM);
    assert_eq!(app.screen(), "reader.screen", "from {library}");
    for _ in 0..3 {
        app.run(actions::NEXT_PAGE);
    }
    app.run(actions::HOME);
    assert_eq!(app.screen(), "home.dashboard");
    let opened = library::opened_books(&card);
    assert_eq!(
        opened.first().map(String::as_str),
        Some(ALICE),
        "{opened:?}"
    );
    assert!(app.shows("Continue reading"));
    assert!(app.shows("Alice"));
    assert!(app.warnings().is_empty());

    // Started again on the same card: no guide, the book still opened.
    drop(app);
    let mut again = App::start(BUTTONS, &card);
    assert_ne!(again.screen(), "setup.guide");
    assert_eq!(
        library::opened_books(&card).first().map(String::as_str),
        Some(ALICE)
    );
    again.run(actions::HOME);
    assert_eq!(again.screen(), "home.dashboard");
    assert!(again.shows("Continue reading"));
    assert!(again.shows("Alice"));
}

/// A switch tapped on a settings page on a touch screen and a number
/// changed with the buttons are written to `paperpaw.ini`; a kernel
/// started anew on the same card reads them back.
#[test]
fn settings_changed_in_the_ui_are_saved_and_read_back_after_a_restart() {
    let card = Card::new(true);

    // On a touch screen: night mode switched on, on the page of the
    // display, a part of the reading settings.
    let mut touch = App::start(TOUCH, &card);
    let night = core_settings::NIGHT_MODE.static_str();
    assert!(!touch.kernel.settings().bool(night));
    touch.run(actions::SETTINGS);
    touch.tap_on("Reading");
    touch.tap_on("Display");
    assert_eq!(touch.screen(), "settings.group");
    touch.tap_on("Night mode");
    assert!(touch.kernel.settings().bool(night));
    let ini = card.text(CONFIG_FILE).expect("paperpaw.ini written");
    assert!(ini.contains("[core]"), "{ini}");
    assert!(ini.contains("night-mode = on"), "{ini}");
    drop(touch);

    // With the buttons: the font size, one larger, chosen.
    let mut buttons = App::start(BUTTONS, &card);
    assert!(
        buttons.kernel.settings().bool(night),
        "read back after a restart"
    );
    let size = "core.font-size";
    let before = buttons.kernel.settings().int(size);
    buttons.run(actions::SETTINGS);
    buttons.focus_on("Reading", 10);
    buttons.run(actions::CONFIRM);
    // On the page of the text, a part of the reading settings.
    buttons.focus_on("Text", 10);
    buttons.run(actions::CONFIRM);
    buttons.focus_on("Font size", 10);
    buttons.run(actions::CONFIRM);
    buttons.run(actions::RIGHT);
    buttons.run(actions::CONFIRM);
    assert_eq!(buttons.kernel.settings().int(size), before + 1);
    let ini = card.text(CONFIG_FILE).unwrap();
    assert!(
        ini.contains(&format!("font-size = {}", before + 1)),
        "{ini}"
    );
    // Both in the one file, the night mode kept beside the new value.
    assert!(ini.contains("night-mode = on"), "{ini}");
    drop(buttons);

    let again = App::start(BUTTONS, &card);
    let settings = again.kernel.settings();
    assert!(settings.bool(night));
    assert_eq!(settings.int(size), before + 1);
    assert!(settings.is_set(size));
}

/// A bookmark keeps the words its page begins with as the book has them:
/// a word broken at the end of a line is whole in it, not
/// "restric- tions" with the hyphen and the line break of the page.
#[test]
fn bookmarks_keep_words_broken_across_lines_whole() {
    let card = Card::new(true);
    let mut app = App::start(BUTTONS, &card);
    app.now += 100;
    let open = Params::new().with(paperpaw_sdk::builtin::params::PATH, ALICE);
    let effects =
        app.kernel
            .open_slot(Instant(app.now), paperpaw_sdk::builtin::slots::READER, open);
    app.settle(effects);
    let bookmark = Id::new_static("reader.toggle-bookmark");
    for _ in 0..40 {
        app.kernel.render(&mut |_, _| {});
        app.run(bookmark.clone());
        app.run(actions::NEXT_PAGE);
    }
    let marks = paperpaw_sdk::annotations::load(&card, ALICE);
    assert_eq!(marks.len(), 40, "a bookmark on each page");
    let broken: Vec<&str> = marks
        .iter()
        .map(|m| m.text.as_str())
        .filter(|text| {
            text.as_bytes()
                .windows(3)
                .any(|w| w[0].is_ascii_lowercase() && w[1] == b'-' && w[2] == b' ')
        })
        .collect();
    assert!(broken.is_empty(), "words broken in bookmarks: {broken:#?}");
}

/// Opens each row of the menu that `open` shows, one after the other,
/// with the buttons (`buttons`) or by touch, and goes home after each:
/// every entry leads somewhere, and nothing it opens panics or warns.
fn walk_menu(device: DeviceInfo, open: Id, buttons: bool) -> Vec<(String, String)> {
    let card = Card::new(true);
    let mut app = App::start(device, &card);
    app.run(open.clone());
    let rows = app.rows();
    assert!(rows.len() >= 4, "{open} shows rows: {rows:?}");
    let mut reached = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        app.run(actions::HOME);
        app.run(open.clone());
        assert_eq!(app.rows(), rows, "{open} shows the same rows again");
        if buttons {
            for _ in 0..index {
                app.run(actions::DOWN);
            }
            assert_eq!(app.focused().as_deref(), Some(row.as_str()));
            app.run(actions::CONFIRM);
        } else {
            app.tap_on(row);
        }
        // Drawn and described without a panic.
        let _ = app.parts();
        reached.push((row.clone(), app.screen()));
        let warnings = app.warnings();
        assert!(warnings.is_empty(), "{row}: {warnings:?}");
        // And left again.
        app.run(actions::BACK);
        let _ = app.parts();
    }
    app.run(actions::HOME);
    assert_eq!(app.screen(), "home.dashboard");
    reached
}

#[test]
fn every_entry_of_the_main_menu_and_the_tools_opens_with_the_buttons() {
    let menu = walk_menu(BUTTONS, actions::MENU, true);
    let library = menu.iter().find(|(row, _)| row == "Library").unwrap();
    assert_ne!(library.1, "menu.list", "the library opened: {menu:?}");
    let settings = menu.iter().find(|(row, _)| row == "Settings").unwrap();
    assert_eq!(settings.1, "settings.list", "{menu:?}");
    let tools = walk_menu(BUTTONS, actions::TOOLS, true);
    // Each tool opens a screen of its own, none leaves the menu shown.
    let stayed: Vec<_> = tools.iter().filter(|(_, s)| s == "menu.list").collect();
    assert!(stayed.is_empty(), "tools that opened nothing: {stayed:?}");
}

#[test]
fn every_entry_of_the_main_menu_and_the_tools_opens_by_touch() {
    let menu = walk_menu(TOUCH, actions::MENU, false);
    let settings = menu.iter().find(|(row, _)| row == "Settings").unwrap();
    assert_eq!(settings.1, "settings.list", "{menu:?}");
    let tools = walk_menu(TOUCH, actions::TOOLS, false);
    let stayed: Vec<_> = tools.iter().filter(|(_, s)| s == "menu.list").collect();
    assert!(stayed.is_empty(), "tools that opened nothing: {stayed:?}");
}

/// Every screen a plugin adds, opened as another plugin could, without
/// the parameters it may expect: it shows (itself, or a screen it opens
/// at once, as a place asked for opens the keyboard), and neither
/// drawing it, pressing on it nor leaving it panics.
fn open_every_screen(device: DeviceInfo) {
    let card = Card::new(true);
    let mut app = App::start(device, &card);
    let mut screens: Vec<Id> = app.kernel.known_screens().cloned().collect();
    screens.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    assert!(screens.len() > 20, "{screens:?}");
    let mut elsewhere = Vec::new();
    for screen in screens {
        app.run(actions::HOME);
        app.open(screen.clone());
        if app.screen() != screen.as_str() {
            elsewhere.push(format!("{screen} -> {}", app.screen()));
        }
        assert!(
            app.screen() != "home.dashboard" || screen.as_str() == "home.dashboard",
            "{screen} did not open"
        );
        let _ = app.parts();
        if device.input.touch {
            app.tap(Point::new(240, 400));
        } else {
            app.run(actions::DOWN);
            app.run(actions::CONFIRM);
        }
        let _ = app.parts();
        app.run(actions::BACK);
        let _ = app.parts();
        let _ = app.warnings();
    }
    // Only a screen that asks for something at once shows another.
    assert!(elsewhere.len() <= 3, "{elsewhere:?}");
    app.run(actions::HOME);
    assert_eq!(app.screen(), "home.dashboard");
}

#[test]
fn every_screen_opens_and_closes_with_the_buttons() {
    open_every_screen(BUTTONS);
}

#[test]
fn every_screen_opens_and_closes_by_touch() {
    open_every_screen(TOUCH);
}
