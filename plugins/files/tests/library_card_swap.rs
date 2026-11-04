//! A library whose sorted books cannot be read, though the kernel thinks
//! them whole (#145: a card taken out while PaperPaw ran and put back
//! after books were copied onto it; its file system still holds what it
//! kept of the old card): the library must not say it is preparing the
//! books for ever. It says the folder cannot be read, and shows the books
//! once they can be.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::*;
use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

/// A card whose sorted books fail to read (their end, which the kernel
/// checks, reads) while `damaged`.
struct Swapped {
    card: MemoryStorage,
    damaged: Rc<Cell<bool>>,
}

impl Storage for Swapped {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        if self.damaged.get() && path.contains("sorted-") && offset == 0 {
            return Err(StorageError::Io);
        }
        self.card.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.card.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.card.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.card.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.card.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.card.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.card.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.card.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.card.create_dir(path)
    }
}

const TEXT: &[u8] = b"Some text of a book.\n\nAnd more of it.\n";

/// The app on `device` over a card with a few books and the first-run
/// setup done, its sorted books unreadable; the library opened.
fn app_with_damage(device: DeviceInfo) -> (App, Rc<Cell<bool>>) {
    let damaged = Rc::new(Cell::new(true));
    let mut card = MemoryStorage::new();
    for n in 1..=3 {
        card = card.with(&format!("books/Book {n:02}.txt"), TEXT);
    }
    card = card.with(SETUP_DONE, b"");
    card = card.with(paperpaw_sdk::builtin::CONFIG_FILE, BASE_CONFIG.as_bytes());
    let storage = Shared(Rc::new(Swapped {
        card,
        damaged: Rc::clone(&damaged),
    }));
    let mut app = App::new(device, storage);
    // The hints first.
    if app.screen() == "help.overlay" {
        app.run(actions::CONFIRM);
    }
    app.run(actions::LIBRARY);
    assert_eq!(app.screen(), "files.browser");
    (app, damaged)
}

/// Time passing, a few seconds a time.
fn pass(app: &mut App, seconds: u64) {
    for _ in 0..seconds.div_ceil(3) {
        app.wait(3000);
    }
}

#[test]
fn books_that_cannot_be_read_are_said_not_prepared_for_ever() {
    let _turn = turn();
    for device in DEVICES {
        let (mut app, _damaged) = app_with_damage(device);
        pass(&mut app, 300);
        assert!(!app.shows("Preparing"), "{}", app.described());
        assert!(!app.shows("Collecting"), "{}", app.described());
        assert!(app.shows("read the books folder"), "{}", app.described());
    }
}

#[test]
fn the_books_show_once_they_can_be_read_again() {
    let _turn = turn();
    for device in DEVICES {
        let (mut app, damaged) = app_with_damage(device);
        pass(&mut app, 300);
        assert!(app.shows("read the books folder"), "{}", app.described());
        damaged.set(false);
        // Out of the library and in again: it looks anew.
        app.run(actions::BACK);
        app.run(actions::LIBRARY);
        pass(&mut app, 60);
        assert!(!app.shows("read the books folder"), "{}", app.described());
        assert!(app.shows("Book 01"), "{}", app.described());
    }
}

/// How a card that can be taken out reads: there, taken out (reads and
/// writes failing, as the X4's storage does while it looks for the card
/// again) or lost until a restart.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    In,
    Out,
    Gone,
}

/// A card that can be taken out while PaperPaw runs (#145).
struct Pullable {
    card: MemoryStorage,
    slot: Rc<Cell<Slot>>,
}

impl Pullable {
    fn failing(&self) -> Option<StorageError> {
        match self.slot.get() {
            Slot::In => None,
            Slot::Out => Some(StorageError::Io),
            Slot::Gone => Some(StorageError::CARD_GONE),
        }
    }
}

impl Storage for Pullable {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.failing()
            .map_or_else(|| self.card.read_at(path, offset, buf), Err)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.failing().map_or_else(|| self.card.size(path), Err)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.failing()
            .map_or_else(|| self.card.write(path, data), Err)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.failing()
            .map_or_else(|| self.card.append(path, data), Err)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.failing()
            .map_or_else(|| self.card.list(dir, visit), Err)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.failing().map_or_else(|| self.card.remove(path), Err)
    }
    fn exists(&self, path: &str) -> bool {
        self.failing().is_none() && self.card.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.failing()
            .map_or_else(|| self.card.rename(from, to), Err)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.failing()
            .map_or_else(|| self.card.create_dir(path), Err)
    }
}

/// The app on `device` over a card with a few pages of books, the
/// library open and its books shown; the hints on where `hints`, those of
/// the screens on the way shown and closed.
fn app_with_card(device: DeviceInfo, hints: bool) -> (App, Rc<Cell<Slot>>) {
    let slot = Rc::new(Cell::new(Slot::In));
    let mut card = MemoryStorage::new();
    for n in 1..=30 {
        card = card.with(&format!("books/Book {n:02}.txt"), TEXT);
    }
    let config = if hints {
        "[core]\nlanguage = en\n"
    } else {
        BASE_CONFIG
    };
    card = card.with(SETUP_DONE, b"");
    card = card.with(paperpaw_sdk::builtin::CONFIG_FILE, config.as_bytes());
    let storage = Shared(Rc::new(Pullable {
        card,
        slot: Rc::clone(&slot),
    }));
    let mut app = App::new(device, storage);
    close_hints(&mut app);
    app.run(actions::LIBRARY);
    pass(&mut app, 30);
    app.let_catalog_work();
    pass(&mut app, 30);
    close_hints(&mut app);
    assert!(app.shows("Book 01"), "{}", app.described());
    (app, slot)
}

/// The hints closed where they show.
fn close_hints(app: &mut App) {
    for _ in 0..3 {
        if app.screen() == "help.overlay" {
            app.run(actions::CONFIRM);
        }
    }
}

/// Woken from a light sleep `ms` later.
fn wake(app: &mut App, ms: u64) {
    app.now += ms;
    let effects = app.kernel.resume(Instant(app.now));
    app.settle(effects);
    app.wait(3000);
}

/// The card put back and mounted anew: the shell tells the kernel as
/// after a USB drive.
fn put_back(app: &mut App, slot: &Cell<Slot>) {
    slot.set(Slot::In);
    app.now += 100;
    let effects = app.kernel.usb_drive_ended(Instant(app.now));
    app.settle(effects);
    pass(app, 30);
}

/// Whether the library says it is waiting for its books.
fn waiting(app: &mut App) -> bool {
    app.shows("Preparing") || app.shows("Collecting")
}

#[test]
fn a_card_taken_out_with_the_library_open_is_said_at_once() {
    let _turn = turn();
    for device in DEVICES {
        let (mut app, slot) = app_with_card(device, false);
        slot.set(Slot::Out);
        // The next page is read from the sorted books, which cannot be.
        app.run(actions::NEXT_PAGE);
        app.wait(3000);
        assert!(!waiting(&mut app), "{}", app.described());
        assert!(app.shows("read the books folder"), "{}", app.described());
        app.run(actions::DOWN);
        pass(&mut app, 120);
        assert!(!waiting(&mut app), "{}", app.described());
        assert!(app.shows("read the books folder"), "{}", app.described());
        put_back(&mut app, &slot);
        assert!(!app.shows("read the books folder"), "{}", app.described());
        assert!(app.shows("Book 01"), "{}", app.described());
    }
}

#[test]
fn a_card_lost_for_the_run_asks_for_a_restart() {
    let _turn = turn();
    for device in DEVICES {
        let (mut app, slot) = app_with_card(device, false);
        slot.set(Slot::Gone);
        app.run(actions::NEXT_PAGE);
        app.wait(3000);
        assert!(!waiting(&mut app), "{}", app.described());
        assert!(app.shows("Restart PaperPaw"), "{}", app.described());
        // Found lost on waking (another card put in while asleep).
        let (mut app, slot) = app_with_card(device, false);
        slot.set(Slot::Gone);
        wake(&mut app, 60_000);
        assert!(app.shows("Restart PaperPaw"), "{}", app.described());
        app.run(actions::BACK);
        app.run(actions::LIBRARY);
        app.wait(3000);
        assert!(!waiting(&mut app), "{}", app.described());
        assert!(app.shows("Restart PaperPaw"), "{}", app.described());
        assert!(!app.shows("tries again"), "{}", app.described());
    }
}

#[test]
fn a_card_lost_for_the_run_is_not_greeted_as_a_new_one() {
    let _turn = turn();
    for device in DEVICES {
        let (mut app, slot) = app_with_card(device, true);
        slot.set(Slot::Gone);
        wake(&mut app, 60_000);
        assert_ne!(app.screen(), "help.overlay", "{}", app.described());
        app.run(actions::BACK);
        app.wait(3000);
        assert_ne!(app.screen(), "help.overlay", "{}", app.described());
        app.run(actions::LIBRARY);
        app.wait(3000);
        assert_ne!(app.screen(), "help.overlay", "{}", app.described());
        assert!(app.shows("Restart PaperPaw"), "{}", app.described());
    }
}
