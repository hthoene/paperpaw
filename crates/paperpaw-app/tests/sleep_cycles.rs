//! Parking again leaves nothing behind (#248, D3): with every plugin the app
//! lets sleep (`paperpaw_app::SLEEPS`) flagged, 50 cycles of waking each of
//! them and going to standby. Each standby must really park each plugin
//! again, and each next use must make it again; the live heap and the
//! handles all native plugins hold must be the same after the 50th cycle as
//! after the first (a remade plugin opens its font and service handles
//! anew: the old ones must have gone with the old instance).
//!
//! The early cycles grow lasting state once: the storage recall cache
//! (`recall::Recall`, at most 64 paths) learns each path the first time it
//! is read, the catalog check after the first standby and, with books, the
//! cover and page caches made later in the quiet time; the same with every
//! plugin awake (#248). That growth from cycle 2 to [`SETTLED`] must
//! stay under [`ONCE_MAX`]; from [`SETTLED`] on the heap must not change.
//! The card's files are the test's storage, not the heap: what the card
//! takes while it changes is left out (`card::changing_card`). Its own
//! binary without the test harness (`harness = false`): the allocator counts
//! every allocation, and captured output would grow the heap it measures.
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

use std::alloc::{GlobalAlloc, Layout, System};
use std::rc::Rc;
use std::sync::atomic::{AtomicIsize, Ordering};

use card::Card;
use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation, Storage,
};
use paperpaw_sdk::builtin::{actions, slots};
use paperpaw_sdk::{Action, Id, Params};

/// Whether this allocation is left out: the card's own files (a test's
/// storage, not the kernel's heap).
fn uncounted() -> bool {
    card::changing_card()
}

/// The bytes allocated and not freed, all threads, but the card's files.
static LIVE: AtomicIsize = AtomicIsize::new(0);

struct Counting;

// SAFETY: forwards to `System` unchanged; only counts the sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's layout, as `GlobalAlloc::alloc` asks.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() && !uncounted() {
            LIVE.fetch_add(layout.size().cast_signed(), Ordering::Relaxed);
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if !uncounted() {
            LIVE.fetch_sub(layout.size().cast_signed(), Ordering::Relaxed);
        }
        // SAFETY: `ptr` came from `alloc` above with this layout.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: as `GlobalAlloc::realloc` asks, passed on unchanged.
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        if !new.is_null() && !uncounted() {
            LIVE.fetch_add(
                new_size.cast_signed() - layout.size().cast_signed(),
                Ordering::Relaxed,
            );
        }
        new
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

const CYCLES: usize = 50;
/// The cycle from which the heap must stay the same (every path seen).
const SETTLED: usize = 30;
/// The most the recall cache may grow once, from cycle 2 to [`SETTLED`].
const ONCE_MAX: isize = 4096;

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

fn fonts() -> paperpaw_app::Fonts {
    #[cfg(any(feature = "packs", not(feature = "outline")))]
    return paperpaw_app::Fonts::Packs;
    #[cfg(all(feature = "outline", not(feature = "packs")))]
    return paperpaw_app::Fonts::Outline;
}

/// How the cycle reaches `plugin`: its slot where the kernel has one, else
/// its first screen, else its first own action.
enum Use {
    Slot(Id),
    Screen(Id),
    Action(Id),
    /// A format: a book of it opened.
    Book(&'static str),
}

/// A book of each format that may sleep, on the card of the cycles.
const BOOKS: [(&str, &str, &[u8]); 5] = [
    ("format-txt", card::ALICE, b""),
    (
        "format-epub",
        "books/alice.epub",
        include_bytes!("../../../samples/library/books/alice-in-wonderland.epub"),
    ),
    (
        "format-html",
        "books/rooms.html",
        b"<html><body><p>A room.</p><p>Another room.</p></body></html>",
    ),
    ("format-markdown", "books/rooms.md", b"# Rooms\n\nA room.\n"),
    (
        "format-fb2",
        "books/rooms.fb2",
        b"<?xml version=\"1.0\" encoding=\"utf-8\"?><FictionBook \
          xmlns=\"http://www.gribuser.ru/xml/fictionbook/2.0\"><description><title-info>\
          <book-title>Rooms</book-title></title-info></description><body><section>\
          <p>A room.</p></section></body></FictionBook>",
    ),
];

/// The books built in memory (comic, pre-rendered pages).
const BUILT: [(&str, &str, fn() -> Vec<u8>); 2] = [
    ("format-cbz", "books/rooms.cbz", books::cbz),
    ("format-xtc", "books/rooms.xtc", books::xtc),
];

struct Run {
    kernel: Kernel,
    now: u64,
}

impl Run {
    /// Ticks until nothing is due within a second; drops the warnings.
    fn settle(&mut self, mut effects: Effects) {
        for _ in 0..10_000 {
            let Some(at) = effects.wake_at() else {
                break;
            };
            if at.0 > self.now + 1000 {
                break;
            }
            self.now = self.now.max(at.0);
            native_activation::set_clock(self.now);
            effects = self.kernel.tick(Instant(self.now));
        }
        drop(self.kernel.take_warnings());
        // As a shell does after every kernel call: the radio commands of
        // each standby would else pile up in the queue (#248).
        drop(self.kernel.take_io());
    }

    fn run(&mut self, action: &Id) {
        self.now += 100;
        native_activation::set_clock(self.now);
        let effects = self.kernel.run_action(Instant(self.now), action.clone());
        self.settle(effects);
    }

    fn reach(&mut self, way: &Use) {
        self.run(&actions::HOME);
        self.now += 100;
        native_activation::set_clock(self.now);
        let effects = match way {
            Use::Slot(slot) => {
                self.kernel
                    .open_slot(Instant(self.now), slot.clone(), Params::new())
            }
            Use::Screen(screen) => {
                self.kernel
                    .open_screen(Instant(self.now), screen.clone(), Params::new())
            }
            Use::Action(action) => self.kernel.run_action(Instant(self.now), action.clone()),
            Use::Book(path) => self.kernel.open_book(Instant(self.now), path),
        };
        self.settle(effects);
        self.run(&actions::BACK);
    }

    fn standby(&mut self) {
        self.now += 100;
        native_activation::set_clock(self.now);
        let effects = self.kernel.suspend(Instant(self.now));
        self.settle(effects);
    }

    fn resume(&mut self) {
        self.now += 100;
        native_activation::set_clock(self.now);
        let effects = self.kernel.resume(Instant(self.now));
        self.settle(effects);
    }

    fn way_to(&self, plugin: &str) -> Use {
        match plugin {
            "menu" => return Use::Slot(slots::MENU),
            "keyboard" => return Use::Slot(slots::KEYBOARD),
            "settings" => return Use::Slot(slots::SETTINGS),
            _ => {}
        }
        let built = BUILT.iter().map(|(format, path, _)| (*format, *path));
        let mut known = BOOKS
            .iter()
            .map(|(format, path, _)| (*format, *path))
            .chain(built);
        if let Some((_, path)) = known.find(|(format, _)| *format == plugin) {
            return Use::Book(path);
        }
        let mut screens: Vec<Id> = self
            .kernel
            .known_screens()
            .filter(|screen| screen.namespace() == plugin)
            .cloned()
            .collect();
        screens.sort();
        if let Some(screen) = screens.into_iter().next() {
            return Use::Screen(screen);
        }
        let mut own: Vec<Id> = self
            .kernel
            .actions()
            .iter()
            .filter(|action| matches!(action, Action::Native(_)))
            .map(Action::id)
            .filter(|id| id.namespace() == plugin)
            .cloned()
            .collect();
        own.sort();
        Use::Action(own.into_iter().next().expect("a way to reach the plugin"))
    }
}

/// The bytes of the card's files under `dir` (left out of the heap).
fn card_bytes(card: &Card, dir: &str) -> u64 {
    let mut found = Vec::new();
    let _ = card.list(dir, &mut |entry| {
        found.push((entry.name.to_string(), entry.is_dir, entry.size));
    });
    found
        .into_iter()
        .map(|(name, is_dir, size)| {
            if is_dir {
                let path = if dir.is_empty() {
                    name
                } else {
                    format!("{dir}/{name}")
                };
                card_bytes(card, &path)
            } else {
                size
            }
        })
        .sum()
}

/// Something must be compared after the settled cycle.
const _: () = assert!(CYCLES > SETTLED);

/// Keeps cycle 2's heap and handles, checks the once-growth at [`SETTLED`]
/// and that nothing changed from there to the last cycle.
fn compare(first: &mut Option<(isize, usize)>, cycle: usize, now: (isize, usize)) {
    match *first {
        None if cycle == 2 => *first = Some(now),
        Some((heap, handles)) if cycle == SETTLED => {
            assert_eq!(now.1, handles, "handles after cycle {cycle} vs cycle 2");
            let once = now.0 - heap;
            assert!(
                (0..=ONCE_MAX).contains(&once),
                "the heap grew {once} B from cycle 2 to {SETTLED}, more than the recall \
                 cache's once"
            );
            *first = Some(now);
        }
        Some((heap, handles)) if cycle == CYCLES => {
            assert_eq!(
                now.1, handles,
                "handles after cycle {cycle} vs cycle {SETTLED}"
            );
            assert_eq!(
                now.0, heap,
                "live heap (bytes) after cycle {cycle} vs cycle {SETTLED}"
            );
        }
        _ => {}
    }
}

fn main() {
    native_activation::set_clock(0);
    let card = Card::new(true);
    for (_, path, bytes) in BOOKS {
        if !bytes.is_empty() {
            card.write(path, bytes).unwrap();
        }
    }
    for (_, path, make) in BUILT {
        card.write(path, &make()).unwrap();
    }
    let builder = Kernel::builder(BUTTONS)
        .storage(card.clone())
        .user_config("[core]\nlanguage = en\n[help]\nshow = false\n");
    let mut kernel = native_activation::build_on_sleeping(
        paperpaw_app::install(builder, fonts()),
        Rc::new(card.clone()),
        paperpaw_app::sleeps,
    );
    let effects = kernel.start(Instant(0));
    let files_card = card;
    let mut run = Run { kernel, now: 0 };
    run.settle(effects);
    // A format is a native plugin only where the build activates them (the
    // flip, #243): one not registered is left out and counted, every other
    // plugin that may sleep must be there.
    let (registered, absent): (Vec<&str>, Vec<&str>) = paperpaw_app::SLEEPS
        .iter()
        .copied()
        .partition(|plugin| run.kernel.native_is_parked(plugin).is_some());
    assert!(
        absent.iter().all(|plugin| plugin.starts_with("format-")),
        "plugins that may sleep but are not registered: {absent:?}"
    );
    let ways: Vec<(&str, Use)> = registered
        .into_iter()
        .map(|plugin| (plugin, run.way_to(plugin)))
        .collect();
    // An empty list would pass with nothing tested (CR on #1365).
    assert!(!ways.is_empty(), "no plugin may sleep: nothing to cycle");
    let mut first = None;
    for cycle in 1..=CYCLES {
        for (plugin, way) in &ways {
            run.reach(way);
            assert_eq!(
                run.kernel.native_is_parked(plugin),
                Some(false),
                "{plugin}: not made in cycle {cycle}, the cycle tests nothing"
            );
        }
        run.run(&actions::HOME);
        run.standby();
        for (plugin, _) in &ways {
            assert_eq!(
                run.kernel.native_is_parked(plugin),
                Some(true),
                "{plugin}: not parked again on standby in cycle {cycle}"
            );
        }
        run.resume();
        let now = (
            LIVE.load(Ordering::Relaxed),
            run.kernel.native_handle_count().expect("the handle table"),
        );
        compare(&mut first, cycle, now);
    }
    let (heap, handles) = first.expect("cycle 2 measured");
    println!(
        "sleep_cycles: {CYCLES} cycles of {} plugins: live heap {heap} B and {handles} handles \
         after cycle {SETTLED} and cycle {CYCLES}, the card's files left out ({} B at the end) ({} \
         formats not registered: {absent:?})",
        ways.len(),
        card_bytes(&files_card, ""),
        absent.len()
    );
}
