//! "Now reading" on the real home screen, told where a book is read by
//! the standard reading events alone (as any reader sends them): the
//! page, the chapter and the pages left in it, and the time they take at
//! the speed read.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::Cell;
use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_platform::Rotation;
use paperpaw_plugin_widgets::Widgets;
use paperpaw_sdk::builtin::events;
use paperpaw_sdk::{
    ActionSpec, Commands, DeviceInfo, DisplayInfo, Env, Id, InputCaps, Instant, Manifest,
    MemoryClass, MemoryStorage, PixelFormat, Plugin, RegisterError, Registrar, SDK_VERSION,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);

const BOOK: &str = "books/alice.epub";
const OPEN: Id = Id::new_static("stand-in.open");
const TURN: Id = Id::new_static("stand-in.turn");
const CLOSE: Id = Id::new_static("stand-in.close");

/// A reader that only says where it is: page 40 of 200 at first, each
/// turn a page on, in a chapter that ends with page 60.
struct StandIn(Rc<Cell<u32>>);

impl StandIn {
    fn reading(page: u32) -> events::Reading {
        events::Reading::new(
            BOOK.into(),
            "Alice's Adventures in Wonderland".into(),
            page as f32 / 2.0,
            3,
            page * 100,
            "Down the Rabbit-Hole".into(),
        )
        .page(page)
        .pages(200)
        .chapter_left(60 - page)
    }
}

impl Plugin for StandIn {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("stand-in"), "Stand-in", SDK_VERSION)
            .description("Says where a book is read.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        for (id, event) in [
            (OPEN, events::BOOK_OPENED),
            (TURN, events::PAGE_TURNED),
            (CLOSE, events::BOOK_CLOSED),
        ] {
            let page = self.0.clone();
            r.add_action(ActionSpec::new(id.clone(), "stand-in.action").handler(
                move |_: &Env<'_>, _, out: &mut Commands| {
                    if id == TURN {
                        page.set(page.get() + 1);
                    }
                    out.push(event.emit(StandIn::reading(page.get())));
                },
            ))?;
        }
        Ok(())
    }
}

struct Home {
    kernel: Kernel,
    now: u64,
    /// Where the widget is: one row down or not.
    moved: bool,
}

impl Home {
    /// The home screen showing "Now reading", with `settings` set.
    fn new(settings: &[(&str, &str)]) -> Self {
        let storage = MemoryStorage::new()
            .with(BOOK, "a book")
            .with(".paperpaw/plugins/setup/done", "");
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_home::Home)
            .plugin(Widgets)
            .plugin(StandIn(Rc::new(Cell::new(40))))
            .build()
            .unwrap();
        kernel.start(Instant(0));
        let mut home = Self {
            kernel,
            now: 0,
            moved: false,
        };
        for (id, value) in settings {
            home.set(id, value);
        }
        home
    }

    fn set(&mut self, id: &str, value: &str) {
        self.now += 100;
        self.kernel
            .set_setting(Instant(self.now), id, value)
            .unwrap();
    }

    /// Reads `pages` pages, `millis` each.
    fn read(&mut self, pages: u32, millis: u64) {
        self.run(OPEN);
        for _ in 0..pages {
            self.now += millis;
            self.run(TURN);
        }
        self.run(CLOSE);
    }

    fn run(&mut self, action: Id) {
        self.now += 10;
        self.kernel.run_action(Instant(self.now), action);
    }

    /// The texts on the screen, in order, the widget made anew (as when
    /// the home screen shows again after reading).
    fn shown(&mut self) -> Vec<String> {
        self.moved = !self.moved;
        let row = if self.moved { 1 } else { 0 };
        self.set("home.page-1", &format!("widgets.reading 0/{row} 4x4"));
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect()
    }
}

fn has(shown: &[String], text: &str) -> bool {
    shown.iter().any(|l| l.contains(text))
}

#[test]
fn the_chapter_and_whats_left_of_it() {
    let mut home = Home::new(&[]);
    home.read(2, 1000);
    let shown = home.shown();
    assert!(has(&shown, "Page 42 of 200"), "{shown:?}");
    assert!(has(&shown, "Down the Rabbit-Hole"), "{shown:?}");
    assert!(has(&shown, "18 pages left in the chapter"), "{shown:?}");
    // Read at a page in 30 seconds: the time left in it too.
    home.read(10, 30_000);
    let shown = home.shown();
    assert!(has(&shown, "8 pages left"), "{shown:?}");
    assert!(has(&shown, "about 4 min"), "{shown:?}");
    assert!(has(&shown, "About 1 h 14 min left"), "{shown:?}");
}

#[test]
fn the_chapter_is_a_part_to_switch_off() {
    let mut home = Home::new(&[
        ("widgets.reading.chapter", "false"),
        ("widgets.reading.chapter-left", "false"),
    ]);
    home.read(2, 1000);
    let shown = home.shown();
    assert!(has(&shown, "Page 42 of 200"), "{shown:?}");
    assert!(!has(&shown, "Rabbit-Hole"), "{shown:?}");
    assert!(!has(&shown, "in the chapter"), "{shown:?}");
}

#[test]
fn the_chapter_in_german() {
    let mut home = Home::new(&[("core.language", "de")]);
    home.read(19, 30_000);
    let shown = home.shown();
    assert!(has(&shown, "Down the Rabbit-Hole"), "{shown:?}");
    assert!(has(&shown, "Noch 1 Seite"), "{shown:?}");
    assert!(has(&shown, "etwa 1 Min."), "{shown:?}");
    // Its last page.
    home.read(1, 30_000);
    let shown = home.shown();
    assert!(has(&shown, "Letzte Seite des Kapitels"), "{shown:?}");
}
