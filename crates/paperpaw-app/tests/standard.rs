//! Plugins reach the reader through what `paperpaw_sdk::builtin` defines
//! for every reader, never by the reader plugin's own ids: a probe plugin
//! that knows only those moves the shipped reader, hears where it is,
//! puts an entry in its menu, and marks words on its pages that it hears
//! taps on (ADR 0021).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_sdk::builtin::{actions, events, menus, params, slots};
use paperpaw_sdk::decorations::{self, Decorated, Decoration, DecoratorSpec, LineKind};
use paperpaw_sdk::document::Locator;
use paperpaw_sdk::{
    ActionSpec, Commands, Env, Id, Manifest, MemoryStorage, MenuEntry, Params, Plugin,
    RegisterError, Registrar, SDK_VERSION,
};

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

const BOOK: &str = "books/alice-in-wonderland.txt";
/// Where the probe asks the reader to go, in the book's text.
const TARGET: u32 = 20_000;
const GO: Id = Id::new_static("probe.go");
/// Where the reader's page began when it last turned (`u32::MAX`: not
/// yet).
static PAGE_AT: AtomicU32 = AtomicU32::new(u32::MAX);
/// The probe's role, and the words of its last mark tapped.
const WORD: Id = Id::new_static("probe.word");
static TAPPED: Mutex<String> = Mutex::new(String::new());

/// A plugin that knows no reader, only the standard ids.
struct Probe;

impl Plugin for Probe {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("probe"), "Probe", SDK_VERSION)
            .description("Knows only the standard ids.")
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_action(ActionSpec::new(GO, "probe.go").handler(
            |_: &Env<'_>, _, out: &mut Commands| {
                let goto = events::Goto::to(BOOK, Locator::new(0, TARGET));
                out.push(events::GOTO.emit(goto));
            },
        ))?;
        r.on(events::PAGE_TURNED, |_, reading, _| {
            PAGE_AT.store(reading.offset, Ordering::SeqCst);
        })?;
        r.add_menu_entry(MenuEntry::new(menus::READER_MORE, GO))?;
        // Every "Alice" on a page, wavy underlined; taps come back.
        decorations::add_role(
            r,
            &WORD,
            "probe.word",
            Decoration::NONE.line(LineKind::Wavy),
        )?;
        r.add_decorator(
            DecoratorSpec::new(Id::new_static("probe.alice"), "probe.alice", |_, page| {
                let alice = |w: &&decorations::PageWord<'_>| w.text.starts_with("Alice");
                page.words
                    .iter()
                    .filter(alice)
                    .filter_map(|w| Decorated::words(core::slice::from_ref(w), &WORD))
                    .collect()
            })
            .taps(),
        )?;
        r.on(events::DECORATION_TAPPED, |_, tapped, _| {
            *TAPPED.lock().unwrap() = tapped.text.clone();
        })?;
        Ok(())
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
    fn new() -> Self {
        let storage = MemoryStorage::new()
            .with(
                BOOK,
                include_str!("../../../samples/library/books/alice-in-wonderland.txt"),
            )
            .with(".paperpaw/plugins/setup/done", "");
        let builder = Kernel::builder(DEVICE)
            .storage(storage)
            .user_config("[help]\nshow = false\n[core]\nlanguage = en\n");
        let mut kernel = paperpaw_app::install(builder, fonts())
            .plugin(Probe)
            .build()
            .unwrap();
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

    fn run(&mut self, action: Id) {
        self.now += 100;
        let effects = self.kernel.run_action(Instant(self.now), action);
        self.settle(effects);
    }

    fn labels(&mut self) -> Vec<String> {
        self.kernel.render(&mut |_, _| {});
        self.kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect()
    }
}

/// The catalogs built in take their indexes from the program (made by
/// the build script): none is made on the heap, in English or German.
#[test]
fn translations_keep_no_index_on_the_heap() {
    let app = App::new();
    assert_eq!(app.kernel.translator().indexes_on_heap(), 0);
    let storage = MemoryStorage::new().with(".paperpaw/plugins/setup/done", "");
    let builder = Kernel::builder(DEVICE).storage(storage).user_config(
        "[core]
language = de
",
    );
    let mut kernel = paperpaw_app::install(builder, fonts()).build().unwrap();
    kernel.start(Instant(0));
    assert_eq!(kernel.translator().language(), "de");
    assert_eq!(kernel.translator().indexes_on_heap(), 0);
}

#[test]
fn a_plugin_reaches_the_reader_through_the_standard_ids() {
    let mut app = App::new();
    app.now += 100;
    let open = Params::new().with(params::PATH, BOOK);
    let effects = app.kernel.open_slot(Instant(app.now), slots::READER, open);
    app.settle(effects);
    // Shown where it was asked to go: the page that holds that place.
    app.run(GO);
    let at = PAGE_AT.load(Ordering::SeqCst);
    assert!(at <= TARGET && at > TARGET - 5_000, "page at {at}");
    // Its entry in the reader's "More" menu.
    app.run(actions::CONFIRM);
    app.run(menus::READER_MORE);
    let labels = app.labels();
    assert!(labels.iter().any(|l| l.contains("probe.go")), "{labels:?}");
}

#[test]
fn a_decorators_marks_are_chosen_with_the_buttons() {
    let mut app = App::new();
    app.now += 100;
    let open = Params::new().with(params::PATH, BOOK);
    let effects = app.kernel.open_slot(Instant(app.now), slots::READER, open);
    app.settle(effects);
    // The book's first page names Alice; once it is drawn (decorators are
    // asked for a page shown), the links mode stops at her, and
    // confirming tells the probe.
    app.kernel.render(&mut |_, _| {});
    app.run(Id::new_static("reader.links"));
    app.run(actions::CONFIRM);
    let tapped = TAPPED.lock().unwrap().clone();
    assert!(tapped.starts_with("Alice"), "tapped {tapped:?}");
}
