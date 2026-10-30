//! The shelves (by author, series, tag) with the real kernel, which makes
//! the library's catalog: walked once a view shows after a start,
//! caught up after single books came or went, and shelves shown follow
//! it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::Cell;
use std::rc::Rc;

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_sdk::builtin::{actions, events};
use paperpaw_sdk::{
    Command, DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage,
    PixelFormat, Storage, StorageError,
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

/// A card that counts how often the library folder is listed.
struct Counted {
    inner: Rc<MemoryStorage>,
    listings: Rc<Cell<usize>>,
}

impl Storage for Counted {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.inner.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.inner.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        if dir == "books" {
            self.listings.set(self.listings.get() + 1);
        }
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.inner.rename(from, to)
    }
}

/// Runs the timers asked for from `now`, `ticks` of them at most, and
/// the work due within a minute (the catalog made when it is cheap);
/// where it got to.
fn run(kernel: &mut Kernel, now: u64, mut effects: Effects, ticks: usize) -> u64 {
    let mut now = now;
    for _ in 0..ticks {
        match effects.wake_at() {
            Some(at) if at.0 <= now + 61_000 => {
                now = now.max(at.0);
                effects = kernel.tick(Instant(now));
            }
            _ => break,
        }
    }
    kernel.render(&mut |_, _| {});
    now
}

/// Sends the event `command` emits.
fn emit(kernel: &mut Kernel, now: u64, command: Command) -> Effects {
    let Command::Emit(event, params) = command else {
        panic!("not an event: {command:?}");
    };
    kernel.send_event(Instant(now), event, params)
}

fn shown(kernel: &mut Kernel) -> String {
    kernel.render(&mut |_, _| {});
    let labels: Vec<String> = kernel.describe().into_iter().map(|p| p.label).collect();
    labels.join("\n")
}

#[test]
fn the_kernel_makes_the_catalog_once_and_after_changes() {
    let storage = Rc::new((0..40).fold(MemoryStorage::new(), |card, i| {
        card.with(&format!("books/book {i:02}.txt"), "Some text.")
    }));
    let listings = Rc::new(Cell::new(0));
    let mut kernel = Kernel::builder(DEVICE)
        .storage(Counted {
            inner: Rc::clone(&storage),
            listings: Rc::clone(&listings),
        })
        .user_config("[core]\nlanguage = en\n")
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_plugin_format_txt::TxtFormat)
        .plugin(paperpaw_plugin_files::Files)
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    let mut now = run(&mut kernel, 0, effects, 1000);
    let by_author = paperpaw_plugin_files::BY_AUTHOR;

    // Made after the start; by author shows it.
    assert!(storage.exists(".paperpaw/library/catalog.txt"));
    let effects = kernel.run_action(Instant(now), by_author.clone());
    now = run(&mut kernel, now, effects, 1000);
    let text = shown(&mut kernel);
    assert!(text.contains("40 books"), "{text}");

    // Shown again: the catalog made is taken as it is.
    let effects = kernel.run_action(Instant(now), actions::BACK);
    now = run(&mut kernel, now, effects, 100);
    listings.set(0);
    let effects = kernel.run_action(Instant(now), by_author.clone());
    now = run(&mut kernel, now, effects, 1000);
    assert!(shown(&mut kernel).contains("40 books"));
    assert_eq!(listings.get(), 0, "made anew without a change");

    // A book came: caught up without walking the library (no folder
    // listed), and the shelf shown follows.
    storage.write("books/book 40.txt", b"More text.").unwrap();
    let book = paperpaw_sdk::library::added("books/book 40.txt", "test");
    let effects = emit(&mut kernel, now, book);
    now = run(&mut kernel, now, effects, 1000);
    assert_eq!(listings.get(), 0, "walked for one book");
    let text = shown(&mut kernel);
    assert!(text.contains("41 books"), "{text}");

    // One went: the same.
    storage.remove("books/book 00.txt").unwrap();
    let removed = events::BookRemoved::new(String::from("books/book 00.txt"));
    let effects = emit(&mut kernel, now, events::BOOK_REMOVED.emit(removed));
    now = run(&mut kernel, now, effects, 1000);
    let text = shown(&mut kernel);
    assert!(text.contains("40 books"), "{text}");

    // No catalog at all (removed on a computer): a shelf asks for it and
    // says the books are being collected until it is made.
    let effects = kernel.run_action(Instant(now), actions::BACK);
    now = run(&mut kernel, now, effects, 100);
    storage.remove(".paperpaw/library/catalog.txt").unwrap();
    let effects = kernel.run_action(Instant(now), Id::new_static("files.by-series"));
    kernel.render(&mut |_, _| {});
    let text = shown(&mut kernel);
    assert!(text.contains("Collecting"), "{text}");
    run(&mut kernel, now, effects, 1000);
    assert!(storage.exists(".paperpaw/library/catalog.txt"));
    let text = shown(&mut kernel);
    assert!(!text.contains("Collecting"), "{text}");
}
