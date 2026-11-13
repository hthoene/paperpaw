//! Every linked plugin the app lets sleep (`paperpaw_app::SLEEPS`) behaves
//! the same parked as awake (#248, Decisions 165 and 166): one case for each,
//! named, and each the same flow twice on a card of its own, once with every
//! plugin awake and once with that plugin registered to sleep. The flow takes
//! the kernel's own routes to it first (its slots, the core actions it
//! serves), then opens each of its screens from home, moves and confirms on
//! it and leaves it, and runs each of its actions. After every step the screen, what it shows
//! (the described parts), the pixels drawn, the warnings and the effects are
//! recorded; at the end every file on the card. The two records must be
//! equal, and the sleeping run must really start with the plugin parked: a
//! flagged plugin that runs awake fails, so no case passes by doing nothing.
//!
//! The plugin sleeps from its admission and the flow's first input to it
//! wakes it. In the middle of the flow the device goes to standby and back:
//! the sleeping run must really park the plugin again there (D3, Decision
//! 166), and the next input makes it again. A plugin that kept something
//! only in its own memory then differs from the awake run (the counter
//! fixture in the kernel's `parked_tests.rs` shows the comparison catches it).
#![cfg(feature = "native-linked")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

#[path = "support/books.rs"]
mod books;
#[path = "support/card.rs"]
mod card;
mod native_activation;

use std::cell::Cell;
use std::fmt::Write as _;
use std::rc::Rc;

use card::Card;
use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation, Storage,
};
use paperpaw_sdk::builtin::{actions, slots};
use paperpaw_sdk::{Action, Id, Params};

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

thread_local! {
    /// The plugin this thread's case lets sleep.
    static SLEEPER: Cell<&'static str> = const { Cell::new("") };
}

fn only_the_sleeper(id: &str) -> bool {
    SLEEPER.with(|sleeper| sleeper.get() == id)
}

fn none_sleeps(_: &str) -> bool {
    false
}

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

/// One run of the flow and what it recorded.
struct Run {
    kernel: Kernel,
    card: Card,
    now: u64,
    steps: Vec<String>,
    /// Compared as well (CR on #1368): every pass of every step, unfolded,
    /// as `@instant effects`.
    raw: Vec<String>,
    /// Debug only, never compared: `(step, passes folded)` for each step
    /// that had a pass repeat the one before it at the same instant.
    repeats: Vec<(usize, u32)>,
    /// In standby (between `suspend` and `resume`): no timer is ticked.
    in_standby: bool,
    /// Whether the plugin was parked right after the standby in the flow.
    reparked: Option<bool>,
}

impl Run {
    fn start(sleep: fn(&str) -> bool, book: Option<&Book>) -> Self {
        native_activation::set_clock(0);
        let card = Card::new(true);
        if let Some(book) = book {
            card.write(book.path, &book.bytes).unwrap();
        }
        let builder = Kernel::builder(BUTTONS)
            .storage(card.clone())
            .user_config("[core]\nlanguage = en\n[help]\nshow = false\n");
        let mut kernel = native_activation::build_on_sleeping(
            paperpaw_app::install(builder, fonts()),
            Rc::new(card.clone()),
            sleep,
        );
        let effects = kernel.start(Instant(0));
        let mut run = Self {
            kernel,
            card,
            now: 0,
            steps: Vec::new(),
            raw: Vec::new(),
            repeats: Vec::new(),
            in_standby: false,
            reparked: None,
        };
        run.step("start", effects);
        run
    }

    /// Ticks until nothing is due within a second, then records the step.
    fn step(&mut self, what: &str, mut effects: Effects) {
        // A pass that ends exactly as the pass before it, at the same
        // instant, is recorded once: a parked plugin is made at the next
        // safe point, which may take a pass more at that instant, with
        // nothing the user sees (#248). Nothing else is folded; the folds
        // are counted in `repeats` and every pass is kept in `raw`.
        let mut last = format!("{effects:?}");
        let mut last_at = self.now;
        let mut seen = last.clone();
        let mut raw = format!("{what}: @{} {last}", self.now);
        let mut folded = 0u32;
        for _ in 0..10_000 {
            let Some(at) = effects.wake_at() else {
                break;
            };
            // In standby the device ticks no timer: only the passes the
            // standby itself asks for at its instant run (#248).
            let horizon = if self.in_standby {
                self.now
            } else {
                self.now + 1000
            };
            if at.0 > horizon {
                break;
            }
            self.now = self.now.max(at.0);
            native_activation::set_clock(self.now);
            effects = self.kernel.tick(Instant(self.now));
            let now = format!("{effects:?}");
            let _ = write!(raw, " | @{} {now}", self.now);
            if now == last && self.now == last_at {
                folded += 1;
            } else {
                let _ = write!(seen, " {now}");
                last = now;
                last_at = self.now;
            }
        }
        if folded > 0 {
            self.repeats.push((self.steps.len(), folded));
        }
        self.raw.push(raw);
        let mut frame = Fnv::default();
        self.kernel.render(&mut |top, rows| {
            frame.bytes(&top.to_le_bytes());
            frame.bytes(rows.data);
        });
        let parts: Vec<String> = self
            .kernel
            .describe()
            .into_iter()
            .map(|part| {
                format!(
                    "{:?}|{}|{}|{:?}",
                    part.role, part.label, part.focused, part.area
                )
            })
            .collect();
        let warnings: Vec<String> = self
            .kernel
            .take_warnings()
            .into_iter()
            .map(|warning| warning.to_string())
            .collect();
        let screen = self
            .kernel
            .active_screen()
            .map(ToString::to_string)
            .unwrap_or_default();
        self.steps.push(format!(
            "{what}: screen {screen}, frame {:016x}, parts {parts:?}, warnings {warnings:?}, effects {seen}",
            frame.0
        ));
    }

    /// Records a step that is not a pass of the kernel (in `raw` too).
    fn note(&mut self, text: String) {
        self.raw.push(text.clone());
        self.steps.push(text);
    }

    /// With `PARITY_RAW_DIR` set, writes `raw` and `repeats` there, to read
    /// a difference of the unfolded runs (the case asserts them equal).
    fn dump(&self, plugin: &str, name: &str) {
        let Some(dir) = std::env::var_os("PARITY_RAW_DIR") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut text = self.raw.join("\n");
        let _ = write!(text, "\nrepeats {:?}\n", self.repeats);
        for ini in [
            ".paperpaw/plugins/widgets/reading.ini",
            ".paperpaw/library/status.log",
        ] {
            let body = self.card.0.get(ini).unwrap_or_default();
            let _ = write!(text, "FILE {ini}:\n{}\n", String::from_utf8_lossy(&body));
        }
        std::fs::write(dir.join(format!("{plugin}-{name}.txt")), text).unwrap();
    }

    fn run(&mut self, action: &Id) {
        self.now += 100;
        native_activation::set_clock(self.now);
        let effects = self.kernel.run_action(Instant(self.now), action.clone());
        self.step(&format!("run {action}"), effects);
    }

    /// Standby and back (D3): a plugin that may sleep and runs idle is
    /// parked again on standby, and made again by the next input.
    fn standby(&mut self, plugin: &str) {
        self.now += 100;
        native_activation::set_clock(self.now);
        let effects = self.kernel.suspend(Instant(self.now));
        self.reparked = self.kernel.native_is_parked(plugin);
        self.in_standby = true;
        self.step("suspend", effects);
        self.in_standby = false;
        self.now += 100;
        native_activation::set_clock(self.now);
        let effects = self.kernel.resume(Instant(self.now));
        self.step("resume", effects);
    }

    fn open_slot(&mut self, slot: &Id) {
        self.now += 100;
        native_activation::set_clock(self.now);
        let effects = self
            .kernel
            .open_slot(Instant(self.now), slot.clone(), Params::new());
        self.step(&format!("open slot {slot}"), effects);
    }

    /// Opens the book at `path` as a shell does (the library's row).
    fn book(&mut self, path: &str) {
        self.now += 100;
        native_activation::set_clock(self.now);
        let effects = self.kernel.open_book(Instant(self.now), path);
        self.step(&format!("open book {path}"), effects);
    }

    fn open(&mut self, screen: &Id) {
        self.now += 100;
        native_activation::set_clock(self.now);
        let effects = self
            .kernel
            .open_screen(Instant(self.now), screen.clone(), Params::new());
        self.step(&format!("open {screen}"), effects);
    }

    /// Every file on the card with its bytes, as one step.
    fn card_files(&mut self) {
        let mut files = Vec::new();
        list_files(&self.card, "", &mut files);
        files.sort();
        let mut text = String::from("card:");
        for path in files {
            let mut hash = Fnv::default();
            hash.bytes(&self.card.0.get(&path).unwrap_or_default());
            let _ = write!(text, " {path}={:016x}", hash.0);
        }
        self.note(text);
    }
}

fn list_files(card: &Card, dir: &str, out: &mut Vec<String>) {
    let mut entries = Vec::new();
    let _ = card.list(dir, &mut |entry| {
        entries.push((entry.name.to_string(), entry.is_dir));
    });
    for (name, is_dir) in entries {
        let path = if dir.is_empty() {
            name
        } else {
            format!("{dir}/{name}")
        };
        if is_dir {
            list_files(card, &path, out);
        } else {
            out.push(path);
        }
    }
}

/// FNV-1a, 64 bits.
struct Fnv(u64);
impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}
impl Fnv {
    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
}

/// The kernel's own routes that reach `plugin` (screens bound to it, core
/// actions it serves): taken first in the flow, while the sleeping plugin is
/// still parked, so that a route which fails to wake it differs from the
/// awake run.
fn kernel_routes(plugin: &str) -> (&'static [Id], &'static [Id]) {
    static MENU_SLOT: [Id; 1] = [slots::MENU];
    static MENU_ACTION: [Id; 1] = [actions::MENU];
    static KEYBOARD_SLOT: [Id; 1] = [slots::KEYBOARD];
    static SETTINGS_SLOT: [Id; 1] = [slots::SETTINGS];
    static SETTINGS_ACTION: [Id; 1] = [actions::SETTINGS];
    match plugin {
        "menu" => (&MENU_SLOT, &MENU_ACTION),
        "keyboard" => (&KEYBOARD_SLOT, &[]),
        "settings" => (&SETTINGS_SLOT, &SETTINGS_ACTION),
        _ => (&[], &[]),
    }
}

/// The flow for `plugin`: the kernel's routes to it first (each recording
/// whether the plugin is parked after it); then each of its screens opened
/// from home, moved on, confirmed and left; each of its actions run from
/// home; the card at the end.
fn flow(run: &mut Run, plugin: &str) {
    if let Some(book) = book_of(plugin) {
        book_flow(run, plugin, &book);
        return;
    }
    let (route_slots, route_actions) = kernel_routes(plugin);
    for screen in route_slots {
        run.run(&actions::HOME);
        run.open_slot(screen);
        let parked = run.kernel.native_is_parked(plugin);
        run.note(format!("{plugin} parked after {screen}: {parked:?}"));
        run.run(&actions::DOWN);
        run.run(&actions::CONFIRM);
        run.run(&actions::BACK);
    }
    for action in route_actions {
        run.run(&actions::HOME);
        run.run(action);
        let parked = run.kernel.native_is_parked(plugin);
        run.note(format!("{plugin} parked after {action}: {parked:?}"));
        run.run(&actions::BACK);
    }
    // Mid-flow: home, standby, and the next input wakes it (D3).
    run.run(&actions::HOME);
    run.standby(plugin);
    let mut screens: Vec<Id> = run
        .kernel
        .known_screens()
        .filter(|screen| screen.namespace() == plugin)
        .cloned()
        .collect();
    screens.sort();
    let mut own: Vec<Id> = run
        .kernel
        .actions()
        .iter()
        .filter(|action| matches!(action, Action::Native(_)))
        .map(Action::id)
        .filter(|id| id.namespace() == plugin)
        .cloned()
        .collect();
    own.sort();
    own.dedup();
    for screen in &screens {
        run.run(&actions::HOME);
        run.open(screen);
        run.run(&actions::DOWN);
        run.run(&actions::CONFIRM);
        run.run(&actions::BACK);
    }
    for action in &own {
        run.run(&actions::HOME);
        run.run(action);
        run.run(&actions::BACK);
    }
    run.run(&actions::HOME);
    run.card_files();
    run.note(format!(
        "{plugin}: {} screens, {} actions",
        screens.len(),
        own.len()
    ));
}

/// A book a format reads, for its case.
struct Book {
    path: &'static str,
    bytes: Vec<u8>,
}

/// The book of the format `plugin` (#248: a format sleeps until a book of
/// it opens); `None` for a plugin that is no format.
fn book_of(plugin: &str) -> Option<Book> {
    let story = "Gregor woke one morning from uneasy dreams and found himself \
                 changed. His room, a proper room for a person, lay quiet. ";
    let long = |open: &str, close: &str| -> Vec<u8> {
        let mut text = String::new();
        for at in 0..40 {
            let _ = writeln!(text, "{open}{at}. {story}{story}{close}");
        }
        text.into_bytes()
    };
    let (path, bytes) = match plugin {
        "format-txt" => ("books/notes.txt", long("", "")),
        "format-epub" => (
            "books/alice.epub",
            include_bytes!("../../../samples/library/books/alice-in-wonderland.epub").to_vec(),
        ),
        "format-html" => {
            let mut page = b"<html><head><title>Rooms</title></head><body>".to_vec();
            page.extend(long("<p>", "</p>"));
            page.extend(b"</body></html>");
            ("books/rooms.html", page)
        }
        "format-markdown" => {
            let mut text = b"# Rooms\n\n".to_vec();
            text.extend(long("", "\n"));
            ("books/rooms.md", text)
        }
        "format-fb2" => {
            let mut text = b"<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
                <FictionBook xmlns=\"http://www.gribuser.ru/xml/fictionbook/2.0\">\
                <description><title-info><book-title>Rooms</book-title></title-info>\
                </description><body><section>"
                .to_vec();
            text.extend(long("<p>", "</p>"));
            text.extend(b"</section></body></FictionBook>");
            ("books/rooms.fb2", text)
        }
        "format-cbz" => ("books/rooms.cbz", books::cbz()),
        "format-xtc" => ("books/rooms.xtc", books::xtc()),
        _ => return None,
    };
    Some(Book { path, bytes })
}

/// The flow for a format: its book opened while the format sleeps (first
/// open from parked), paged and left; standby with the book closed (parked
/// again); the book opened again (made again: the place kept) and paged.
fn book_flow(run: &mut Run, plugin: &str, book: &Book) {
    run.run(&actions::HOME);
    run.book(book.path);
    let parked = run.kernel.native_is_parked(plugin);
    run.note(format!("{plugin} parked after the first open: {parked:?}"));
    run.run(&actions::NEXT_PAGE);
    run.run(&actions::NEXT_PAGE);
    run.run(&actions::BACK);
    run.run(&actions::HOME);
    run.standby(plugin);
    run.book(book.path);
    let parked = run.kernel.native_is_parked(plugin);
    run.note(format!("{plugin} parked after the second open: {parked:?}"));
    run.run(&actions::NEXT_PAGE);
    run.run(&actions::BACK);
    run.run(&actions::HOME);
    run.card_files();
}

/// The case for `plugin`: the flow awake and asleep, the same.
fn same_awake_and_asleep(plugin: &'static str) {
    assert!(
        paperpaw_app::sleeps(plugin),
        "{plugin} is not in paperpaw_app::SLEEPS"
    );
    let book = book_of(plugin);
    let mut awake = Run::start(none_sleeps, book.as_ref());
    assert_eq!(
        awake.kernel.native_is_parked(plugin),
        Some(false),
        "{plugin}: registered and awake in the awake run"
    );
    SLEEPER.with(|sleeper| sleeper.set(plugin));
    let mut asleep = Run::start(only_the_sleeper, book.as_ref());
    assert_eq!(
        asleep.kernel.native_is_parked(plugin),
        Some(true),
        "{plugin} is flagged to sleep but runs awake at the start: the case would test nothing"
    );
    flow(&mut awake, plugin);
    flow(&mut asleep, plugin);
    awake.dump(plugin, "awake");
    asleep.dump(plugin, "asleep");
    assert!(
        awake.steps.len() > 2,
        "{plugin}: the flow did nothing: {:?}",
        awake.steps
    );
    for (index, (a, b)) in awake.steps.iter().zip(&asleep.steps).enumerate() {
        assert_eq!(a, b, "{plugin} differs asleep at step {index}");
    }
    assert_eq!(
        awake.steps.len(),
        asleep.steps.len(),
        "{plugin}: the runs took a different number of steps"
    );
    // The unfolded passes too: asleep, no pass may be added or differ, not
    // even one the fold above would take (CR on #1368).
    for (index, (a, b)) in awake.raw.iter().zip(&asleep.raw).enumerate() {
        assert_eq!(
            a, b,
            "{plugin} differs asleep in its unfolded passes at step {index}"
        );
    }
    assert_eq!(
        awake.raw.len(),
        asleep.raw.len(),
        "{plugin}: the unfolded runs took a different number of steps"
    );
    assert_eq!(
        asleep.reparked,
        Some(true),
        "{plugin} did not sleep again on standby: the re-park was not tested"
    );
}

/// Every flagged plugin has a case here (and no case is left without one).
#[test]
fn every_plugin_that_may_sleep_has_a_case() {
    let cases = [
        "about",
        "file-manager",
        "files",
        "format-cbz",
        "format-epub",
        "format-fb2",
        "format-html",
        "format-markdown",
        "format-txt",
        "format-xtc",
        "keyboard",
        "menu",
        "opds",
        "settings",
    ];
    assert_eq!(paperpaw_app::SLEEPS, cases);
}

#[test]
fn about_is_the_same_asleep() {
    same_awake_and_asleep("about");
}

#[test]
fn file_manager_is_the_same_asleep() {
    same_awake_and_asleep("file-manager");
}

#[test]
fn files_is_the_same_asleep() {
    same_awake_and_asleep("files");
}

#[test]
fn keyboard_is_the_same_asleep() {
    same_awake_and_asleep("keyboard");
}

#[test]
fn menu_is_the_same_asleep() {
    same_awake_and_asleep("menu");
}

#[test]
fn opds_is_the_same_asleep() {
    same_awake_and_asleep("opds");
}

#[test]
fn settings_is_the_same_asleep() {
    same_awake_and_asleep("settings");
}

#[test]
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the linked formats (`native-linked-reading`) (formats are native plugins with the flip, #243)"
)]
fn format_cbz_is_the_same_asleep() {
    same_awake_and_asleep("format-cbz");
}

#[test]
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the linked formats (`native-linked-reading`) (formats are native plugins with the flip, #243)"
)]
fn format_epub_is_the_same_asleep() {
    same_awake_and_asleep("format-epub");
}

#[test]
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the linked formats (`native-linked-reading`) (formats are native plugins with the flip, #243)"
)]
fn format_fb2_is_the_same_asleep() {
    same_awake_and_asleep("format-fb2");
}

#[test]
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the linked formats (`native-linked-reading`) (formats are native plugins with the flip, #243)"
)]
fn format_html_is_the_same_asleep() {
    same_awake_and_asleep("format-html");
}

#[test]
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the linked formats (`native-linked-reading`) (formats are native plugins with the flip, #243)"
)]
fn format_markdown_is_the_same_asleep() {
    same_awake_and_asleep("format-markdown");
}

#[test]
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the linked formats (`native-linked-reading`) (formats are native plugins with the flip, #243)"
)]
fn format_txt_is_the_same_asleep() {
    same_awake_and_asleep("format-txt");
}

#[test]
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the linked formats (`native-linked-reading`) (formats are native plugins with the flip, #243)"
)]
fn format_xtc_is_the_same_asleep() {
    same_awake_and_asleep("format-xtc");
}

/// A book whose document is ready at once (every plugin awake): the reader
/// counts its pages, so the page index is on the card and the reading record
/// has the pages. The count used to start only for a book prepared first
/// (#248: the open path asked before the screen was on top).
#[test]
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the linked formats (`native-linked-reading`) (the book's format is a native plugin with the flip, #243)"
)]
fn a_book_ready_at_once_has_its_pages_counted() {
    let book = book_of("format-txt").expect("a txt book");
    let mut run = Run::start(none_sleeps, Some(&book));
    run.run(&actions::HOME);
    run.book(book.path);
    run.run(&actions::NEXT_PAGE);
    run.run(&actions::BACK);
    let mut files = Vec::new();
    list_files(&run.card, "", &mut files);
    assert!(
        files
            .iter()
            .any(|path| path.starts_with(".paperpaw/plugins/reader/pages-")),
        "no page index: {files:?}"
    );
    let record = run
        .card
        .0
        .get(".paperpaw/plugins/widgets/reading.ini")
        .unwrap_or_default();
    let record = String::from_utf8_lossy(&record);
    assert!(
        record.contains("pages = "),
        "no pages in the record: {record}"
    );
}

/// Whether the reader's page index of a book is on the card.
fn page_index(run: &Run) -> bool {
    let mut files = Vec::new();
    list_files(&run.card, "", &mut files);
    files
        .iter()
        .any(|path| path.starts_with(".paperpaw/plugins/reader/pages-"))
}

/// The book opens and, the moment its reading screen is on top, the
/// settings cover it: the count waits while it is covered (its layouts would
/// add to the settings' peak) and goes on when the book shows again (#248,
/// Manager's decision 1).
#[test]
#[cfg_attr(
    not(feature = "native-linked-reading"),
    ignore = "needs the linked formats (`native-linked-reading`) (the book's format is a native plugin with the flip, #243)"
)]
fn the_page_count_waits_while_another_screen_covers_the_book() {
    let book = book_of("format-txt").expect("a txt book");
    let mut run = Run::start(none_sleeps, Some(&book));
    run.run(&actions::HOME);
    run.now += 100;
    native_activation::set_clock(run.now);
    let mut effects = run.kernel.open_book(Instant(run.now), book.path);
    // Only the passes that bring the reading screen up: the count's first
    // step is due later.
    for _ in 0..1000 {
        if run
            .kernel
            .active_screen()
            .is_some_and(|screen| screen.as_str() == "reader.screen")
        {
            break;
        }
        let Some(at) = effects.wake_at() else {
            break;
        };
        run.now = run.now.max(at.0);
        native_activation::set_clock(run.now);
        effects = run.kernel.tick(Instant(run.now));
    }
    assert_eq!(
        run.kernel.active_screen().map(Id::as_str),
        Some("reader.screen"),
        "the book did not open"
    );
    let effects = run
        .kernel
        .open_slot(Instant(run.now), slots::SETTINGS, Params::new());
    run.step("settings over the book", effects);
    // Each step ticks whatever is due within a second, so a count going on
    // would run to its end here.
    for _ in 0..3 {
        run.run(&actions::DOWN);
        run.run(&actions::UP);
    }
    assert!(
        !page_index(&run),
        "the pages were counted under the settings"
    );
    run.run(&actions::BACK);
    assert_eq!(
        run.kernel.active_screen().map(Id::as_str),
        Some("reader.screen")
    );
    assert!(
        page_index(&run),
        "the count did not go on with the book shown"
    );
}
