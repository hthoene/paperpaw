//! Lists and grids draw again only the items a focus left and came to:
//! a step on the X4 then takes a fraction of a whole frame. This walks
//! every built-in screen with a list or a grid with the buttons and
//! checks two things at each step:
//!
//! - what the panel shows, kept as the panel keeps it (the rows of each
//!   frame put over the last), is what drawing the whole screen anew
//!   shows: nothing left of a focus moved away, nothing outside the rows
//!   changed;
//! - a step that keeps the page (the same parts shown, only the focus
//!   moved) draws no more than the rows from the item the focus left to
//!   the one it came to, and [`SLACK`]; where the screen says what has
//!   the focus (`Canvas::describe`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::{Effects, HttpResponse, Io, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, FrameView, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::builtin::{actions, params, slots};
use paperpaw_sdk::{Id, MemoryStorage, Params, Rect};

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

/// Rows a step may draw past the items the focus left and came to: those
/// a frame is aligned to, an item's frame drawn a little outside it, a
/// bar round a button.
const SLACK: u32 = 40;

const BOOK: &str = "books/alice-in-wonderland.txt";

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

/// The app, and the screen as the panel shows it: a byte a pixel.
struct App {
    kernel: Kernel,
    now: u64,
    panel: Vec<u8>,
    /// The rows the last frame drew.
    drawn: u32,
    /// The steps within a page in the walk going on.
    measured: usize,
}

/// Puts the rows of `view` at `y` over `screen`.
fn put(screen: &mut [u8], y: u16, view: &FrameView<'_>) {
    let width = usize::from(DEVICE.display.width);
    for row in 0..view.height {
        let at = (usize::from(y) + usize::from(row)) * width;
        for x in 0..view.width {
            screen[at + usize::from(x)] = view.pixel(x, row).r;
        }
    }
}

/// Writes the panel and the screen drawn anew as pictures where
/// `PAPERPAW_REDRAW_DUMP` names a folder, to see where they differ.
fn dump(panel: &[u8], whole: &[u8]) {
    let Ok(dir) = std::env::var("PAPERPAW_REDRAW_DUMP") else {
        return;
    };
    let (width, height) = (DEVICE.display.width, DEVICE.display.height);
    for (name, pixels) in [("panel", panel), ("whole", whole)] {
        let mut pgm = format!(
            "P5
{width} {height}
255
"
        )
        .into_bytes();
        pgm.extend_from_slice(pixels);
        std::fs::write(std::path::Path::new(&dir).join(format!("{name}.pgm")), pgm).unwrap();
    }
}

/// Whether `before` and `after` show the same page, the focus moved
/// within it: the same parts but those with the focus before or after,
/// which some screens describe only while they have it.
fn same_page(before: &[(String, bool)], after: &[(String, bool)]) -> bool {
    let focused: Vec<&String> = before
        .iter()
        .chain(after)
        .filter(|(_, focused)| *focused)
        .map(|(text, _)| text)
        .collect();
    let rest = |parts: &[(String, bool)]| -> Vec<String> {
        let mut rest: Vec<String> = parts
            .iter()
            .map(|(text, _)| text)
            .filter(|text| !focused.contains(text))
            .cloned()
            .collect();
        rest.sort();
        rest
    };
    rest(before) == rest(after)
}

/// A library of a few dozen books: lists of more than one page.
fn storage() -> MemoryStorage {
    let text = include_str!("../../../samples/library/books/alice-in-wonderland.txt");
    let mut storage = MemoryStorage::new()
        .with(BOOK, text)
        .with(".paperpaw/plugins/setup/done", "");
    for n in 1..=30 {
        storage = storage.with(&format!("books/book {n:02}.txt"), "A short book.\n");
    }
    storage
}

/// The store's list: plugins of a few groups.
fn catalogue() -> String {
    let groups = ["reading", "library", "tools", "looks"];
    let entries: Vec<String> = (0..40)
        .map(|n| {
            let group = groups[n % groups.len()];
            format!(
                r#"{{"id": "plugin-{n:02}", "name": "Plugin {n:02}", "summary": "Does {n}.",
  "group": "{group}", "author": "Someone", "licence": "MIT", "kind": "data",
  "added": "2026-01-01", "latest": {{"version": "1.0.0", "sdk": "0.4", "formats": 1,
  "size": 1000, "date": "2026-01-01"}}}}"#
            )
        })
        .collect();
    format!(r#"{{"format": 2, "plugins": [{}]}}"#, entries.join(", "))
}

impl App {
    fn new() -> Self {
        let builder = Kernel::builder(DEVICE)
            .storage(storage())
            .network(true)
            .catalog_device("x4")
            .user_config("[core]\nlanguage = en\n[help]\nshow = false\n");
        let mut kernel = paperpaw_app::install(builder, fonts()).build().unwrap();
        let effects = kernel.start(Instant(0));
        let pixels = usize::from(DEVICE.display.width) * usize::from(DEVICE.display.height);
        let mut app = Self {
            kernel,
            now: 0,
            panel: vec![0; pixels],
            drawn: 0,
            measured: 0,
        };
        app.settle(effects);
        app
    }

    /// Lets timers run until nothing is due within a second, then shows
    /// what changed.
    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
            let Some(at) = effects.wake_at() else {
                break;
            };
            if at.0 > self.now + 1000 {
                break;
            }
            self.now = self.now.max(at.0);
            effects = self.kernel.tick(Instant(self.now));
        }
        let panel = &mut self.panel;
        let mut drawn = 0;
        self.kernel.render(&mut |y, view| {
            drawn += u32::from(view.height);
            put(panel, y, &view);
        });
        self.drawn = drawn;
    }

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    fn open_slot(&mut self, slot: Id, params: Params) {
        self.now += 100;
        let effects = self.kernel.open_slot(Instant(self.now), slot, params);
        self.settle(effects);
    }

    /// Answers every web request asked for now: the store's list with
    /// [`catalogue`], anything else not found.
    fn answer(&mut self) {
        let calls: Vec<_> = self
            .kernel
            .take_io()
            .into_iter()
            .filter_map(|io| match io {
                Io::Http(call) => Some(call),
                _ => None,
            })
            .collect();
        for call in calls {
            let list = call.url.contains("/api/v1/plugins.json");
            let response = HttpResponse {
                id: call.id,
                status: if list { 200 } else { 404 },
                body: if list {
                    catalogue().into_bytes()
                } else {
                    Vec::new()
                },
                ..HttpResponse::default()
            };
            self.now += 100;
            let effects = self.kernel.http_response(Instant(self.now), response);
            self.settle(effects);
        }
    }

    /// The parts the screen shows, and whether each has the focus.
    fn shown(&mut self) -> Vec<(String, bool)> {
        self.kernel
            .describe()
            .into_iter()
            .map(|part| {
                let text = format!("{:?} {} {:?}", part.role, part.label, part.on);
                (text, part.focused)
            })
            .collect()
    }

    /// Where the part with the focus is, if the screen says: the last
    /// one drawn, of the screen on top.
    fn focused(&mut self) -> Option<Rect> {
        self.kernel
            .describe()
            .into_iter()
            .rfind(|part| part.focused)
            .map(|part| part.area)
    }

    /// Where the panel shows something else than the whole screen drawn
    /// anew, if anywhere.
    fn differs(&mut self, after: &str) -> Result<(), String> {
        let mut whole = vec![0; self.panel.len()];
        if !self
            .kernel
            .capture(&mut |y, view| put(&mut whole, y, &view))
        {
            return Err(format!("after {after}: changes left undrawn"));
        }
        let width = usize::from(DEVICE.display.width);
        match self.panel.iter().zip(&whole).position(|(a, b)| a != b) {
            Some(at) => {
                let screen = self.kernel.active_screen().map(|id| id.to_string());
                dump(&self.panel, &whole);
                Err(format!(
                    "after {after} on {screen:?}: the panel differs from the screen drawn anew \
                     at x {}, y {}",
                    at % width,
                    at / width
                ))
            }
            None => Ok(()),
        }
    }

    /// Fails where the panel shows something else than the whole screen
    /// drawn anew.
    fn check(&mut self, after: &str) {
        if let Err(why) = self.differs(after) {
            panic!("{why}");
        }
    }

    /// One step with `action`, checked; `name` says where.
    fn try_step(&mut self, action: Id, name: &str) -> Result<(), String> {
        let (before, left) = (self.shown(), self.focused());
        self.run(action.clone());
        self.differs(name)?;
        if !same_page(&before, &self.shown()) {
            return Ok(());
        }
        // Where the screen does not say what has the focus, what a step
        // may draw is not known: the panel alone is checked.
        let (Some(left), Some(reached)) = (left, self.focused()) else {
            return Ok(());
        };
        let top = left.y.min(reached.y);
        let most = left.bottom().max(reached.bottom()) - u32::from(top) + SLACK;
        if self.drawn > most {
            let screen = self
                .kernel
                .active_screen()
                .map_or_else(String::new, ToString::to_string);
            return Err(format!(
                "{name} on {screen}: a step within the page drew {} rows, not at most {most} \
                 (from {left:?} to {reached:?})",
                self.drawn,
            ));
        }
        if self.drawn > 0 {
            self.measured += 1;
        }
        Ok(())
    }

    fn step(&mut self, action: Id, name: &str) {
        if let Err(why) = self.try_step(action, name) {
            panic!("{why}");
        }
    }

    /// Steps `count` times with `on`, then back as many times with
    /// `back`, checking each step.
    fn walk_with(&mut self, on: Id, back: Id, count: usize, name: &str) {
        let screen = self.kernel.active_screen().cloned();
        self.measured = 0;
        for n in 0..count {
            self.step(on.clone(), &format!("{name}: {on} {}", n + 1));
        }
        for n in 0..count {
            self.step(back.clone(), &format!("{name}: {back} {}", n + 1));
        }
        assert_eq!(
            self.kernel.active_screen(),
            screen.as_ref(),
            "{name}: the steps left the screen"
        );
        assert!(self.measured > 0, "{name}: no step stayed on its page");
    }

    /// Steps down `count` times, then back up.
    fn walk(&mut self, count: usize, name: &str) {
        self.walk_with(actions::DOWN, actions::UP, count, name);
    }

    /// Down and right, then back: for grids.
    fn walk_grid(&mut self, count: usize, name: &str) {
        self.walk_with(actions::RIGHT, actions::LEFT, count, name);
        self.walk(count, name);
    }

    fn opened(&mut self, name: &str) {
        self.check(&format!("opening {name}"));
        assert!(self.kernel.active_screen().is_some(), "{name}: no screen");
    }
}

#[test]
fn settings_lists_drawn_in_part_show_what_a_whole_frame_does() {
    let mut app = App::new();
    app.run(actions::SETTINGS);
    app.opened("the settings");
    app.walk(12, "settings");
    // Reading, then its first part (Text), with numbers, switches and
    // choices.
    app.run(actions::DOWN);
    app.run(actions::CONFIRM);
    app.opened("Reading");
    app.walk(8, "Reading");
    app.run(actions::CONFIRM);
    app.opened("a part of Reading");
    app.walk(14, "a part of Reading");
}

#[test]
fn home_grid() {
    let mut app = App::new();
    // A book read: home shows it.
    app.open_slot(slots::READER, Params::new().with(params::PATH, BOOK));
    app.run(actions::HOME);
    app.opened("home");
    app.walk_grid(6, "home");
}

#[test]
fn main_menu_and_tools() {
    let mut app = App::new();
    app.run(actions::MENU);
    app.opened("the main menu");
    app.walk(14, "main menu");
    app.run(actions::BACK);
    app.run(actions::TOOLS);
    app.opened("tools");
    app.walk(14, "tools");
}

#[test]
fn library() {
    let mut app = App::new();
    app.run(actions::LIBRARY);
    app.opened("the library");
    app.walk_grid(12, "library");
}

#[test]
fn table_of_contents() {
    let mut app = App::new();
    app.open_slot(slots::READER, Params::new().with(params::PATH, BOOK));
    app.run(Id::new_static("reader.show-toc"));
    app.opened("the table of contents");
    app.walk(14, "table of contents");
}

#[test]
fn store() {
    let mut app = App::new();
    app.run(Id::new_static("store.open"));
    app.answer();
    app.opened("the store");
    app.walk(10, "store");
    // A group of it, past the search and the new ones.
    app.run(actions::DOWN);
    app.run(actions::DOWN);
    app.run(actions::CONFIRM);
    app.answer();
    app.opened("a part of the store");
    app.walk(10, "a part of the store");
}

#[test]
fn file_manager() {
    let mut app = App::new();
    app.run(Id::new_static("file-manager.open"));
    app.opened("the file manager");
    app.walk(6, "file manager");
    // Into the books.
    app.run(actions::CONFIRM);
    app.opened("a folder");
    app.walk(14, "a folder");
}

#[test]
fn quick_panel() {
    let mut app = App::new();
    app.run(actions::QUICK_PANEL);
    app.opened("the quick panel");
    app.walk_grid(6, "quick panel");
}

#[test]
fn widget_editors() {
    let mut app = App::new();
    app.run(Id::new_static("home.edit-pages"));
    app.opened("home, edited");
    app.walk_grid(6, "home, edited");
    app.run(actions::BACK);
    app.open_slot(
        slots::EDIT_WIDGETS,
        Params::new().with(params::ID, "sleep.widgets"),
    );
    app.opened("the sleep screen's widgets");
    app.walk(6, "sleep screen's widgets");
}

#[test]
fn keyboard() {
    let mut app = App::new();
    app.run(actions::SETTINGS);
    // The search, which asks for text.
    app.run(actions::CONFIRM);
    app.opened("the keyboard");
    // From the field's button to the keys.
    app.run(actions::DOWN);
    app.walk_grid(8, "keyboard");
}

/// Screens the sweep leaves out: those whose steps do more than move a
/// focus (a book's pages, a game), and those that need what a test has
/// not (a book open, a device found).
const NOT_SWEPT: &[&str] = &["reader.screen", "reader.app"];

/// Every screen the app's plugins add, opened alone and stepped through
/// with the buttons: what the panel shows stays what a whole frame does,
/// and a step within a page draws little.
#[test]
fn every_screen() {
    let screens: Vec<Id> = App::new().kernel.known_screens().cloned().collect();
    let mut failed = Vec::new();
    for screen in screens {
        if NOT_SWEPT.contains(&screen.as_str()) {
            continue;
        }
        let mut app = App::new();
        app.now += 100;
        let effects = app
            .kernel
            .open_screen(Instant(app.now), screen.clone(), Params::new());
        app.settle(effects);
        if app.kernel.active_screen() != Some(&screen) {
            continue;
        }
        if let Err(why) = app.differs(&format!("opening {screen}")) {
            failed.push(why);
            continue;
        }
        let steps = [actions::DOWN; 5].into_iter().chain([actions::UP; 5]);
        for (n, action) in steps.enumerate() {
            if app.kernel.active_screen() != Some(&screen) {
                break;
            }
            if let Err(why) = app.try_step(action, &format!("{screen}: step {}", n + 1)) {
                failed.push(why);
                break;
            }
        }
    }
    assert!(
        failed.is_empty(),
        "{}",
        failed.join(
            "
"
        )
    );
}
