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

#[path = "support/card.rs"]
mod card;
mod native_activation;

use card::{ALICE, Card, GREGOR};

use std::rc::Rc;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, InputEvent, Instant, MemoryClass, PixelFormat, Point,
    PointerEvent, PointerPhase, PointerTool, Rotation,
};
use paperpaw_sdk::builtin::{CONFIG_FILE, actions, settings as core_settings};
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::{Id, Params, library};

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
        Self::start_with(device, card, false)
    }

    /// [`Self::start`], with the native help's first-time hints where `hints`
    /// says so. Without them (the flows), as in the headless tests (#2411):
    /// a hint takes the next button over the first screen. `help.show` is a
    /// setting of the native help only.
    fn start_with(device: DeviceInfo, card: &Card, hints: bool) -> Self {
        let builder = Kernel::builder(device).storage(card.clone()).user_config(
            if cfg!(feature = "native-linked") && !hints {
                "[core]\nlanguage = en\n[help]\nshow = false\n"
            } else {
                "[core]\nlanguage = en\n"
            },
        );
        let mut kernel = native_activation::build_on(
            paperpaw_app::install(builder, fonts()),
            Rc::new(card.clone()),
        );
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

/// A book opened in the reader, read, and home again: the book is the one to
/// continue, now and after a restart. (The library that lists the books is the
/// native `files` plugin's and the first-run guide the native setup plugin's;
/// the app registers neither until the host activates native plugins: the
/// pending cases of `plugins/files` and `plugins/setup`.)
#[test]
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the native reader: the interface-6 reader is removed (#244); runs with `native-linked-reading` (the test-native step)"
)]
fn reading_and_home_keep_the_book_opened() {
    let card = Card::new(true);
    let mut app = App::start(BUTTONS, &card);
    assert_eq!(app.screen(), "home.dashboard");
    assert!(app.shows("Start reading"));

    // The book opened in the reader, as the library would.
    let open = Params::new().with(paperpaw_sdk::builtin::params::PATH, ALICE);
    let effects =
        app.kernel
            .open_slot(Instant(app.now), paperpaw_sdk::builtin::slots::READER, open);
    app.settle(effects);
    assert_eq!(app.screen(), "reader.screen");
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
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the native Settings, home and reader (`native-linked-reading`): the interface-6 ones are removed (#244)"
)]
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

/// With the help's first-time hints on, as on a new device: the start shows
/// home, a hint the first button closes takes nothing with it, and home and
/// the reader still open without a warning.
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the native home (`native-linked-reading`): the interface-6 home is removed (#244)"
)]
#[test]
fn the_first_start_with_the_help_hints_on_reaches_home_and_a_book() {
    let card = Card::new(true);
    let mut app = App::start_with(BUTTONS, &card, true);
    let _ = app.parts();
    // The first button may close a hint instead of acting.
    app.run(actions::BACK);
    app.run(actions::HOME);
    assert_eq!(app.screen(), "home.dashboard");
    app.now += 100;
    let open = Params::new().with(paperpaw_sdk::builtin::params::PATH, ALICE);
    let effects =
        app.kernel
            .open_slot(Instant(app.now), paperpaw_sdk::builtin::slots::READER, open);
    app.settle(effects);
    let _ = app.parts();
    app.run(actions::BACK);
    app.run(actions::HOME);
    assert_eq!(app.screen(), "home.dashboard");
    let warnings = app.warnings();
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// A bookmark keeps the words its page begins with as the book has them:
/// a word broken at the end of a line is whole in it, not
/// "restric- tions" with the hyphen and the line break of the page.
#[test]
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the native reader: the interface-6 reader is removed (#244); runs with `native-linked-reading` (the test-native step)"
)]
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

/// The reader asked to open a book while it shows one (a book opened from
/// another screen while the reader is still on the stack): the same book
/// stays open where it was; another book opens, and the first one again
/// after it. Bookmarks land in the book on show,
/// nothing says the book could not be opened, nothing warns.
#[test]
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the native reader: the interface-6 reader is removed (#244); runs with `native-linked-reading` (the test-native step)"
)]
fn a_book_opened_while_the_reader_shows_one() {
    fn open(app: &mut App, path: &str) {
        app.now += 100;
        let params = Params::new().with(paperpaw_sdk::builtin::params::PATH, path);
        let effects = app.kernel.open_slot(
            Instant(app.now),
            paperpaw_sdk::builtin::slots::READER,
            params,
        );
        app.settle(effects);
        app.kernel.render(&mut |_, _| {});
    }
    fn mark(app: &mut App) {
        app.kernel.render(&mut |_, _| {});
        app.run(Id::new_static("reader.toggle-bookmark"));
    }
    let marks = |card: &Card, book: &str| paperpaw_sdk::annotations::load(card, book).len();
    let card = Card::new(true);
    let mut app = App::start(BUTTONS, &card);
    let cannot = "could not be opened";

    // The same book again: it stays open.
    open(&mut app, ALICE);
    app.run(actions::NEXT_PAGE);
    open(&mut app, ALICE);
    assert_eq!(app.screen(), "reader.screen");
    assert!(!app.shows(cannot));
    mark(&mut app);
    assert_eq!(marks(&card, ALICE), 1);

    // Another book, and the first one again.
    open(&mut app, GREGOR);
    assert_eq!(app.screen(), "reader.screen");
    assert!(!app.shows(cannot));
    mark(&mut app);
    assert_eq!(marks(&card, GREGOR), 1);
    open(&mut app, ALICE);
    assert_eq!(app.screen(), "reader.screen");
    assert!(!app.shows(cannot));
    app.run(actions::NEXT_PAGE);
    mark(&mut app);
    assert_eq!(marks(&card, ALICE), 2);
    assert_eq!(marks(&card, GREGOR), 1);
    let warnings = app.warnings();
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// The page's bookmark toggled on and off again: added, then removed, each in
/// its own committed transaction of the library (with the native reader, ops
/// 15, 17, 18 and 20 on the host); the other page's bookmark stays.
#[test]
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the native reader: the interface-6 reader is removed (#244); runs with `native-linked-reading` (the test-native step)"
)]
fn a_bookmark_toggled_twice_is_gone_and_the_others_stay() {
    let card = Card::new(true);
    let mut app = App::start(BUTTONS, &card);
    app.now += 100;
    let open = Params::new().with(paperpaw_sdk::builtin::params::PATH, ALICE);
    let effects =
        app.kernel
            .open_slot(Instant(app.now), paperpaw_sdk::builtin::slots::READER, open);
    app.settle(effects);
    let bookmark = Id::new_static("reader.toggle-bookmark");
    let marks = |card: &Card| paperpaw_sdk::annotations::load(card, ALICE).len();
    app.kernel.render(&mut |_, _| {});
    app.run(bookmark.clone());
    app.run(actions::NEXT_PAGE);
    app.kernel.render(&mut |_, _| {});
    app.run(bookmark.clone());
    assert_eq!(marks(&card), 2, "one on each page");
    app.kernel.render(&mut |_, _| {});
    app.run(bookmark);
    assert_eq!(
        marks(&card),
        1,
        "this page's is gone, the first page's stays"
    );
    let warnings = app.warnings();
    assert!(warnings.is_empty(), "{warnings:?}");
}

/// A row's label without the state a switch row tells at its end.
fn unswitched(row: &str) -> &str {
    row.strip_suffix(", On")
        .or_else(|| row.strip_suffix(", Off"))
        .unwrap_or(row)
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
        // The walk pressed the Night mode row, a switch, so it has switched
        // (as in interface 6); its row now tells the other state.
        assert_eq!(
            app.rows()
                .iter()
                .map(|row| unswitched(row))
                .collect::<Vec<_>>(),
            rows.iter().map(|row| unswitched(row)).collect::<Vec<_>>(),
            "{open} shows the same rows again"
        );
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
#[cfg_attr(
    not(all(feature = "native-linked", feature = "native-linked-reading")),
    ignore = "the menu and the Settings are native plugins: linked with `native-linked` and `native-linked-reading` only (#244)"
)]
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
#[cfg_attr(
    not(all(feature = "native-linked", feature = "native-linked-reading")),
    ignore = "the menu and the Settings are native plugins: linked with `native-linked` and `native-linked-reading` only (#244)"
)]
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
        // The folder editor is the slot `core.choose-folder`, filled by the native
        // `files` plugin, which the app does not register (see `NATIVE_PLUGINS`).
        if screen.as_str() == "core.folder-editor" {
            continue;
        }
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

#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the native home (`native-linked-reading`): the interface-6 home is removed (#244)"
)]
#[test]
fn every_screen_opens_and_closes_with_the_buttons() {
    open_every_screen(BUTTONS);
}

#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the native home (`native-linked-reading`): the interface-6 home is removed (#244)"
)]
#[test]
fn every_screen_opens_and_closes_by_touch() {
    open_every_screen(TOUCH);
}
