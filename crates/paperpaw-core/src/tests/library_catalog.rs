//! The kernel's catalog of the library: never walked at a start, walked
//! once a view asks after one (and when asked anew), nothing done while a
//! book is open, books moved, removed or changed caught up without a
//! walk, and a walk that fails told to the views.

use super::*;
use crate::Items;
use alloc::rc::Rc;
use core::cell::Cell;
use paperpaw_platform::{DirEntry, Storage, StorageError};
use paperpaw_sdk::builtin::{events, params};
use paperpaw_sdk::document::DocumentError;
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::library::{self, catalog};
use paperpaw_sdk::{FormatSpec, MemoryStorage, Params};

/// Reads text books (as names only) and keeps a rating in the catalog;
/// logs every new catalog.
struct LibraryPlugin {
    log: Log,
}

fn library_plugin(kept: LibraryPlugin) -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    move |r| {
        r.add_format(FormatSpec::new(
            Id::new_static("lib.txt"),
            "TXT",
            alloc::vec![Cow::Borrowed("txt")],
            |_, _, _| Err(DocumentError::UnsupportedFormat),
        ))?;
        // A reader, as the reader's slot shows it.
        r.add_fake(crate::testing::FakeSpec::new(
            Id::new_static("lib.reader"),
            "reader",
            Box::new(|_, _| Box::new(Page)),
        ))?;
        r.bind_slot(
            slots::READER,
            Id::new_static("lib.reader"),
            Priority(i16::MAX),
        )?;
        // A library's list, as the library's slot shows it.
        r.add_fake(crate::testing::FakeSpec::new(
            Id::new_static("lib.list"),
            "list",
            Box::new(|_, _| Box::new(Page)),
        ))?;
        r.bind_slot(
            slots::LIBRARY,
            Id::new_static("lib.list"),
            Priority(i16::MAX),
        )?;
        crate::tests::hear_into(&kept.log);
        r.subscribe(events::CATALOG_FAILED.id().clone(), |_, _| {
            crate::tests::heard("failed")
        })?;
        r.subscribe(events::CATALOG_CHANGED.id().clone(), |_, _| {
            crate::tests::heard("catalog")
        })
    }
}

/// What [`library_plugin`] registers under.
fn library_plugin_manifest() -> Manifest {
    Manifest::new(Id::new_static("lib"), "Library", Version::new(1, 0, 0))
}

/// A book's page.
struct Page;

impl crate::testing::Fake for Page {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

fn library(storage: MemoryStorage) -> (Kernel, Log, u64) {
    library_on(DEVICE, storage)
}

fn library_on(device: DeviceInfo, storage: MemoryStorage) -> (Kernel, Log, u64) {
    let log = log();
    let mut kernel = Kernel::builder(device)
        .items(
            library_plugin_manifest(),
            library_plugin(LibraryPlugin { log: log.clone() }),
        )
        .storage(storage)
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    let now = run(&mut kernel, 0, effects);
    (kernel, log, now)
}

/// Runs what is due within a minute and more, as time goes on.
fn run(kernel: &mut Kernel, mut now: u64, mut effects: crate::Effects) -> u64 {
    for _ in 0..2000 {
        match effects.wake_at() {
            Some(at) if at.0 <= now + 61_000 => {
                now = now.max(at.0);
                effects = kernel.tick(Instant(now));
            }
            _ => break,
        }
    }
    now
}

fn send(kernel: &mut Kernel, now: u64, event: &Id, params: Params) -> u64 {
    let effects = kernel.send_event(Instant(now), event.clone(), params);
    run(kernel, now, effects)
}

fn want(kernel: &mut Kernel, now: u64, anew: bool) -> u64 {
    let wanted = events::CatalogWanted::new(anew).to_params();
    send(kernel, now, events::CATALOG_WANTED.id(), wanted)
}

fn paths(kernel: &Kernel) -> Vec<String> {
    let mut paths = Vec::new();
    catalog::each(kernel.storage(), &mut |r| paths.push(r.path));
    paths.sort();
    paths
}

fn two_books() -> MemoryStorage {
    MemoryStorage::new()
        .with("books/a.txt", "one")
        .with("books/b.txt", "two")
}

#[test]
fn the_catalog_is_walked_when_a_view_asks_never_at_a_start() {
    let (mut kernel, log, now) = library(two_books());
    // A start (on the X4 every wake): nothing walked.
    assert!(!kernel.storage().exists(".paperpaw/library/catalog.txt"));
    // A view shows: walked, and said.
    let now = want(&mut kernel, now, false);
    assert_eq!(paths(&kernel), ["books/a.txt", "books/b.txt"]);
    assert_eq!(*log.borrow(), ["catalog"]);
    // Shown again: not again since the start; asked anew, it is.
    let now = want(&mut kernel, now, false);
    assert_eq!(log.borrow().len(), 1);
    want(&mut kernel, now, true);
    assert_eq!(log.borrow().len(), 2);
}

/// Woken from a light sleep (the X4's power button: no start), the first
/// view to show walks the library again, as after a start: books may
/// have been copied while it slept (the card in a computer). Without a
/// wake, a view showing again does not walk it.
#[test]
fn after_a_wake_the_first_view_walks_the_library_again() {
    let (mut kernel, log, now) = library(two_books());
    let now = want(&mut kernel, now, false);
    assert_eq!(log.borrow().len(), 1);
    kernel.storage().write("books/c.txt", b"three").unwrap();
    let now = want(&mut kernel, now, false);
    assert_eq!(paths(&kernel), ["books/a.txt", "books/b.txt"]);
    let effects = kernel.suspend(Instant(now + 1000));
    let now = run(&mut kernel, now + 1000, effects);
    let effects = kernel.resume(Instant(now + 60_000));
    let now = run(&mut kernel, now + 60_000, effects);
    // Woken, nothing walked until a view shows.
    assert_eq!(log.borrow().len(), 1);
    want(&mut kernel, now, false);
    assert_eq!(
        paths(&kernel),
        ["books/a.txt", "books/b.txt", "books/c.txt"]
    );
    assert_eq!(log.borrow().len(), 2);
}

#[test]
fn nothing_is_done_while_a_book_is_read() {
    let (mut kernel, log, now) = library(two_books());
    let path = Params::new().with(params::PATH, "books/a.txt");
    let effects = kernel.open_slot(Instant(now), slots::READER, path);
    let now = run(&mut kernel, now, effects);
    let now = want(&mut kernel, now, true);
    assert!(log.borrow().is_empty(), "walked while reading");
    assert!(!kernel.storage().exists(".paperpaw/library/catalog.txt"));
    // Another screen on top (the book left, or the library over it):
    // done then.
    let effects = kernel.run_action(Instant(now), actions::BACK);
    run(&mut kernel, now, effects);
    assert_eq!(*log.borrow(), ["catalog"]);
    assert_eq!(paths(&kernel), ["books/a.txt", "books/b.txt"]);
}

/// The catalog is made beside the library on every device, the X4 too:
/// the library holds a few kilobytes of the sorted books, not an index of
/// them all.
#[test]
fn the_catalog_is_made_beside_the_library() {
    for memory in [
        paperpaw_platform::MemoryClass::Small,
        paperpaw_platform::MemoryClass::Large,
    ] {
        let device = DEVICE.with_memory(memory);
        let (mut kernel, log, now) = library_on(device, two_books());
        let now = want(&mut kernel, now, false);
        log.borrow_mut().clear();
        let effects = kernel.open_slot(Instant(now), slots::LIBRARY, Params::new());
        let now = run(&mut kernel, now, effects);
        want(&mut kernel, now, true);
        assert_eq!(log.borrow().len(), 1, "{memory:?}");
    }
}

/// Woken with the library open (it asks no more: its view was made
/// before the sleep), the library is walked at once.
#[test]
fn woken_with_the_library_open_it_is_walked_at_once() {
    let (mut kernel, log, now) = library(two_books());
    let effects = kernel.run_action(Instant(now), actions::LIBRARY);
    let now = run(&mut kernel, now, effects);
    let now = want(&mut kernel, now, false);
    assert!(
        kernel
            .stack
            .iter()
            .any(|s| s.slot.as_ref() == Some(&slots::LIBRARY))
    );
    assert_eq!(log.borrow().len(), 1);
    let effects = kernel.suspend(Instant(now + 1000));
    let now = run(&mut kernel, now + 1000, effects);
    kernel.storage().write("books/c.txt", b"three").unwrap();
    let effects = kernel.resume(Instant(now + 60_000));
    run(&mut kernel, now + 60_000, effects);
    assert_eq!(
        paths(&kernel),
        ["books/a.txt", "books/b.txt", "books/c.txt"]
    );
    assert_eq!(log.borrow().len(), 2);
}

/// A start that ends a sleep (the X4's deep sleep) tells both the start
/// and the wake: the library is walked once, when a view first asks, not
/// twice, and that walk is not started again.
#[test]
fn a_start_after_a_sleep_walks_the_library_once() {
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            library_plugin_manifest(),
            library_plugin(LibraryPlugin { log: log.clone() }),
        )
        .storage(two_books())
        .after_sleep(true)
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    let now = run(&mut kernel, 0, effects);
    assert!(!kernel.storage().exists(".paperpaw/library/catalog.txt"));
    let now = want(&mut kernel, now, false);
    assert_eq!(paths(&kernel), ["books/a.txt", "books/b.txt"]);
    assert_eq!(log.borrow().len(), 1);
    // Asked again (another view): walked already since the wake.
    kernel.storage().write("books/c.txt", b"three").unwrap();
    want(&mut kernel, now + 8_000, false);
    assert_eq!(paths(&kernel), ["books/a.txt", "books/b.txt"]);
    assert_eq!(log.borrow().len(), 1);
}

/// A walk under way at the sleep (no library open) starts again after
/// the wake, rather than end as the walk after it: it had passed where a
/// book was copied while it slept.
#[test]
fn a_walk_under_way_at_the_sleep_starts_again_after_the_wake() {
    const BOOKS: usize = 600;
    const STEPS_BEFORE_SLEEP: u64 = 8;
    let mut storage = MemoryStorage::new();
    for n in 0..BOOKS {
        storage = storage.with(&alloc::format!("books/b{n:03}.txt"), "x");
    }
    let (mut kernel, log, now) = library(storage);
    let wanted = events::CatalogWanted::new(false).to_params();
    kernel.send_event(Instant(now), events::CATALOG_WANTED.id().clone(), wanted);
    // Under way, past the start of the folder: no catalog yet.
    for at in 1..=STEPS_BEFORE_SLEEP {
        kernel.tick(Instant(now + at * 10));
    }
    assert!(paths(&kernel).is_empty());
    kernel.suspend(Instant(now + 1000));
    kernel.storage().write("books/a.txt", b"new").unwrap();
    let effects = kernel.resume(Instant(now + 60_000));
    run(&mut kernel, now + 60_000, effects);
    let found = paths(&kernel);
    assert_eq!(found.len(), BOOKS + 1);
    assert_eq!(found.first().map(String::as_str), Some("books/a.txt"));
    // No catalog before: the walk after the wake showed the books it
    // found as it went (#108), none failed.
    let log = log.borrow();
    assert!(
        log.len() > 1 && log.iter().all(|e| e == "catalog"),
        "{log:?}"
    );
}

/// A book copied while PaperPaw runs, with no word to it (the X4's
/// debug link writing to the card): not walked for, however often a view
/// shows, until a start or a wake. One a plugin brings (transfer over
/// Wi-Fi or cable, calibre, OPDS) says so (`BOOK_ADDED`): caught up at a
/// calm moment, within a minute, without a walk.
#[test]
fn a_book_added_without_a_word_waits_one_added_by_a_plugin_comes() {
    let (mut kernel, log, now) = library(two_books());
    let now = want(&mut kernel, now, false);
    kernel.storage().write("books/c.txt", b"three").unwrap();
    let mut now = now;
    for _ in 0..3 {
        now = want(&mut kernel, now + 8_000, false);
    }
    assert_eq!(paths(&kernel), ["books/a.txt", "books/b.txt"]);
    assert_eq!(log.borrow().len(), 1);
    kernel.storage().write("books/d.txt", b"four").unwrap();
    let added = events::BookAdded::new("books/d.txt".into(), "transfer".into());
    send(&mut kernel, now, events::BOOK_ADDED.id(), added.to_params());
    assert_eq!(
        paths(&kernel),
        ["books/a.txt", "books/b.txt", "books/d.txt"]
    );
    assert_eq!(log.borrow().len(), 2);
}

#[test]
fn books_moved_or_removed_are_caught_up() {
    let (mut kernel, log, now) = library(two_books());
    let now = want(&mut kernel, now, false);
    log.borrow_mut().clear();
    // Moved: where it is now, not where it was.
    kernel
        .storage()
        .rename("books/a.txt", "books/Kept/a.txt")
        .unwrap();
    let moved = events::BookMoved::new("books/a.txt".into(), "books/Kept/a.txt".into());
    let now = send(&mut kernel, now, events::BOOK_MOVED.id(), moved.to_params());
    assert_eq!(paths(&kernel), ["books/Kept/a.txt", "books/b.txt"]);
    assert_eq!(*log.borrow(), ["catalog"]);
    // Removed: left out at once, before any catalog is made.
    kernel.storage().remove("books/b.txt").unwrap();
    let removed = events::BookRemoved::new("books/b.txt".into()).to_params();
    let effects = kernel.send_event(Instant(now), events::BOOK_REMOVED.id().clone(), removed);
    assert_eq!(paths(&kernel), ["books/Kept/a.txt"]);
    run(&mut kernel, now, effects);
}

/// On the X4 an empty catalog is walked beside the library: books
/// copied onto the card meanwhile show at once.
#[test]
fn an_empty_catalog_is_walked_beside_the_library_on_small_devices() {
    let device = DEVICE.with_memory(paperpaw_platform::MemoryClass::Small);
    let empty = two_books().with(".paperpaw/library/catalog.txt", "\tcatalog\t2\t1\n");
    let (mut kernel, log, now) = library_on(device, empty);
    let effects = kernel.open_slot(Instant(now), slots::LIBRARY, Params::new());
    let now = run(&mut kernel, now, effects);
    assert!(paths(&kernel).is_empty());
    want(&mut kernel, now, true);
    assert_eq!(*log.borrow(), ["catalog"]);
    assert_eq!(paths(&kernel), ["books/a.txt", "books/b.txt"]);
}

/// A library with nothing in it is walked once per start: asked again, the
/// view is told (so that it stops saying books are being collected), but
/// no new catalog is made.
#[test]
fn an_empty_library_is_not_walked_again_each_time_a_view_asks() {
    let (mut kernel, log, now) = library(MemoryStorage::new());
    let now = want(&mut kernel, now, false);
    assert_eq!(*log.borrow(), ["catalog"]);
    let generation = catalog::generation(kernel.storage());
    assert!(generation.is_some());
    // Shown again, twice: told, not walked (the generation stays).
    let now = want(&mut kernel, now, false);
    want(&mut kernel, now, false);
    assert_eq!(log.borrow().len(), 3);
    assert_eq!(catalog::generation(kernel.storage()), generation);
    // Books copied in meanwhile are found where one asks anew.
    kernel.storage().write("books/a.txt", b"one").unwrap();
    want(&mut kernel, now, true);
    assert_eq!(paths(&kernel), ["books/a.txt"]);
}

/// A card whose books folder cannot be listed while `broken` is set.
struct BrokenFolder {
    files: MemoryStorage,
    broken: Rc<core::cell::Cell<bool>>,
}

impl paperpaw_platform::Storage for BrokenFolder {
    fn read_at(
        &self,
        path: &str,
        offset: u64,
        buf: &mut [u8],
    ) -> Result<usize, paperpaw_platform::StorageError> {
        self.files.read_at(path, offset, buf)
    }

    fn size(&self, path: &str) -> Result<u64, paperpaw_platform::StorageError> {
        self.files.size(path)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.files.write(path, data)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        self.files.append(path, data)
    }

    fn list(
        &self,
        dir: &str,
        visit: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), paperpaw_platform::StorageError> {
        if self.broken.get() && dir.trim_matches('/').starts_with("books") {
            return Err(paperpaw_platform::StorageError::Io);
        }
        self.files.list(dir, visit)
    }

    fn remove(&self, path: &str) -> Result<(), paperpaw_platform::StorageError> {
        self.files.remove(path)
    }
}

#[test]
fn a_walk_that_fails_is_told_and_a_later_one_tells_the_catalog() {
    let broken = Rc::new(core::cell::Cell::new(true));
    let log = log();
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            library_plugin_manifest(),
            library_plugin(LibraryPlugin { log: log.clone() }),
        )
        .storage(BrokenFolder {
            files: two_books(),
            broken: Rc::clone(&broken),
        })
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    let now = run(&mut kernel, 0, effects);
    let now = want(&mut kernel, now, false);
    // Told that it failed, not that a catalog was made; none is there.
    assert_eq!(log.borrow().first().map(String::as_str), Some("failed"));
    assert!(!log.borrow().iter().any(|l| l == "catalog"));
    assert!(paths(&kernel).is_empty());
    // The card can be read again: asked anew, the books are there.
    broken.set(false);
    log.borrow_mut().clear();
    want(&mut kernel, now + 600_000, true);
    assert!(
        log.borrow().iter().any(|l| l == "catalog"),
        "{:?}",
        log.borrow()
    );
    assert_eq!(paths(&kernel), ["books/a.txt", "books/b.txt"]);
}

/// A card whose writes to the library's folder fail while `full` is set,
/// as a full card's do.
struct Card {
    inner: MemoryStorage,
    full: Rc<Cell<bool>>,
    /// The old folder of the books' details cannot be listed (damaged).
    damaged: bool,
}

impl Card {
    fn check(&self, path: &str) -> Result<(), StorageError> {
        if self.full.get() && path.starts_with(".paperpaw/library") {
            Err(StorageError::Io)
        } else {
            Ok(())
        }
    }
}

impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.inner.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.inner.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.check(path)?;
        self.inner.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.check(path)?;
        self.inner.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        if self.damaged && dir == ".paperpaw/library/books" {
            return Err(StorageError::Io);
        }
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.check(to)?;
        self.inner.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.check(path)?;
        self.inner.create_dir(path)
    }
}

/// A walk that fails (the card is full) tells the views that it failed, so
/// that none keeps saying the books are being collected; it is tried again
/// after a while, and tells once more when it works.
#[test]
fn a_walk_that_cannot_write_is_told_and_tried_again() {
    let full = Rc::new(Cell::new(true));
    let log = log();
    let card = Card {
        inner: two_books(),
        full: full.clone(),
        damaged: false,
    };
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            library_plugin_manifest(),
            library_plugin(LibraryPlugin { log: log.clone() }),
        )
        .storage(card)
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    let now = run(&mut kernel, 0, effects);
    let now = want(&mut kernel, now, false);
    assert!(paths(&kernel).is_empty());
    assert_eq!(log.borrow().first().map(String::as_str), Some("failed"));
    // The card has room again: the next attempt (after the wait) makes
    // the catalog, and tells once more.
    full.set(false);
    let told = log.borrow().len();
    let mut now = now;
    for _ in 0..40 {
        if !paths(&kernel).is_empty() {
            break;
        }
        now += 60_000;
        let effects = kernel.tick(Instant(now));
        now = run(&mut kernel, now, effects);
    }
    assert_eq!(paths(&kernel), ["books/a.txt", "books/b.txt"]);
    assert!(log.borrow().len() > told);
}

/// Details kept where earlier versions kept them, in a folder that cannot
/// be read: moving them fails once, and the catalog is made all the same.
#[test]
fn an_old_folder_that_cannot_be_read_does_not_keep_the_catalog_from_being_made() {
    let card = Card {
        inner: two_books().with(".paperpaw/library/books/0123456789abcdef.ini", "x"),
        full: Rc::new(Cell::new(false)),
        damaged: true,
    };
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            library_plugin_manifest(),
            library_plugin(LibraryPlugin { log: log() }),
        )
        .storage(card)
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    let mut now = run(&mut kernel, 0, effects);
    now = want(&mut kernel, now, false);
    for _ in 0..10 {
        if !paths(&kernel).is_empty() {
            break;
        }
        now += 60_000;
        let effects = kernel.tick(Instant(now));
        now = run(&mut kernel, now, effects);
    }
    assert_eq!(paths(&kernel), ["books/a.txt", "books/b.txt"]);
    assert!(
        kernel
            .storage()
            .exists(".paperpaw/library/books/0123456789abcdef.ini")
    );
}

/// The file of the sorted books of the view leaving `archive` out.
fn sorted_of(archive: &str) -> String {
    alloc::format!(
        ".paperpaw/library/sorted-{:08x}.idx",
        library::index::hash(archive)
    )
}

fn want_sorted(kernel: &mut Kernel, now: u64, archive: &str) -> u64 {
    let wanted = events::SortedWanted::new(String::from(archive)).to_params();
    send(kernel, now, events::SORTED_WANTED.id(), wanted)
}

/// The books sorted ([`library::sorted`]) are made once a view asks, and
/// anew (before views hear of it) whenever the catalog is made anew; not
/// again while they are of the catalog there is.
#[test]
fn the_sorted_books_are_made_when_asked_and_after_the_catalog_changed() {
    let (mut kernel, log, now) = library(two_books());
    let now = want(&mut kernel, now, false);
    let sorted = sorted_of("books/Archive");
    assert!(!kernel.storage().exists(&sorted), "made unasked");
    log.borrow_mut().clear();
    let now = want_sorted(&mut kernel, now, "books/Archive");
    assert!(kernel.storage().exists(&sorted));
    assert_eq!(*log.borrow(), ["catalog"]);
    let made = kernel.storage().size(&sorted).unwrap();
    // Asked again: as it is.
    let now = want_sorted(&mut kernel, now, "books/Archive");
    assert_eq!(log.borrow().len(), 1);
    // A book came: the catalog anew, then the books sorted anew, then
    // views hear of it, once.
    kernel.storage().write("books/c.txt", b"three").unwrap();
    want(&mut kernel, now, true);
    assert_eq!(log.borrow().len(), 2);
    assert!(kernel.storage().size(&sorted).unwrap() > made);
    assert!(!kernel.storage().exists(".paperpaw/library/sorted.runs"));
}

/// On the X4 the books are sorted while the library shows (the view says
/// they are being prepared until then); never while a book is read.
#[test]
fn on_small_devices_the_books_are_sorted_beside_the_library() {
    let device = DEVICE.with_memory(paperpaw_platform::MemoryClass::Small);
    let (mut kernel, log, now) = library_on(device, two_books());
    let now = want(&mut kernel, now, false);
    let effects = kernel.open_slot(Instant(now), slots::LIBRARY, Params::new());
    let now = run(&mut kernel, now, effects);
    log.borrow_mut().clear();
    want_sorted(&mut kernel, now, "");
    assert!(kernel.storage().exists(&sorted_of("")));
    assert_eq!(*log.borrow(), ["catalog"]);
}

/// Every file of `storage` from `dir` on, copied into `to`.
fn copied(storage: &dyn Storage, dir: &str, mut to: MemoryStorage) -> MemoryStorage {
    let mut entries = Vec::new();
    storage
        .list(dir, &mut |e| entries.push((String::from(e.name), e.is_dir)))
        .unwrap();
    for (name, is_dir) in entries {
        let path = paperpaw_sdk::files::join(dir, &name);
        to = if is_dir {
            copied(storage, &path, to)
        } else {
            let mut data = alloc::vec![0; storage.size(&path).unwrap() as usize];
            storage.read_at(&path, 0, &mut data).unwrap();
            to.with(&path, data)
        };
    }
    to
}

/// Two views of two archive folders each have their sorted books: asked
/// one after the other, both are made, and both anew (before views hear
/// of it, once) when the catalog is made anew, also after a restart.
#[test]
fn the_sorted_books_of_two_archives_are_both_kept() {
    let (mut kernel, log, now) = library(two_books());
    let now = want(&mut kernel, now, false);
    log.borrow_mut().clear();
    let now = want_sorted(&mut kernel, now, "books/Archive");
    let now = want_sorted(&mut kernel, now, "");
    assert_eq!(log.borrow().len(), 2);
    let (archived, all) = (sorted_of("books/Archive"), sorted_of(""));
    let sizes = |kernel: &Kernel| {
        let storage = kernel.storage();
        (
            storage.size(&archived).unwrap(),
            storage.size(&all).unwrap(),
        )
    };
    let made = sizes(&kernel);
    // Asked again, each: as they are.
    let now = want_sorted(&mut kernel, now, "books/Archive/");
    let now = want_sorted(&mut kernel, now, "");
    assert_eq!(log.borrow().len(), 2);
    kernel.storage().write("books/c.txt", b"three").unwrap();
    want(&mut kernel, now, true);
    assert_eq!(log.borrow().len(), 3);
    let grown = sizes(&kernel);
    assert!(grown.0 > made.0 && grown.1 > made.1, "{made:?} {grown:?}");
    // After a restart the kernel makes both anew, unasked.
    let (mut kernel, log, now) = library(copied(kernel.storage(), "", MemoryStorage::new()));
    kernel.storage().write("books/d.txt", b"four").unwrap();
    log.borrow_mut().clear();
    want(&mut kernel, now, true);
    assert_eq!(*log.borrow(), ["catalog"]);
    let again = sizes(&kernel);
    assert!(
        again.0 > grown.0 && again.1 > grown.1,
        "{grown:?} {again:?}"
    );
}

#[test]
fn what_a_sorting_cut_short_left_goes_at_a_start() {
    let storage = two_books()
        .with(".paperpaw/library/sorted.runs", "runs")
        .with(".paperpaw/library/sorted.tmp", "half");
    let (kernel, _, _) = library(storage);
    assert!(!kernel.storage().exists(".paperpaw/library/sorted.runs"));
    assert!(!kernel.storage().exists(".paperpaw/library/sorted.tmp"));
}

/// A library of `books` books, "Sleep after" one minute, and the walk
/// wanted at once: each tick takes a step of it.
fn walking(books: usize, battery: Option<u8>) -> Kernel {
    let mut storage = MemoryStorage::new();
    for n in 0..books {
        storage = storage.with(&alloc::format!("books/b{n:04}.txt"), "x");
    }
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            library_plugin_manifest(),
            library_plugin(LibraryPlugin { log: log() }),
        )
        .storage(storage)
        .user_config("[core]\nsleep-after = 1\n")
        .sleep(true)
        .speech(true)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    if let Some(percent) = battery {
        let status = paperpaw_platform::SystemStatus::default()
            .with_battery(Some(paperpaw_platform::Battery::new(percent, false, None)));
        kernel.set_status(Instant(0), status);
    }
    let wanted = events::CatalogWanted::new(false).to_params();
    kernel.send_event(Instant(0), events::CATALOG_WANTED.id().clone(), wanted);
    // Ten seconds without input: the walk begins.
    for at in [11_000, 12_000] {
        kernel.tick(Instant(at));
    }
    kernel
}

/// Whether the library's walk is over: its catalog in place, nothing
/// building. Not that the catalog lists books: a first walk lists those
/// found so far at checkpoints while it goes on (#108).
fn walked(kernel: &Kernel) -> bool {
    let storage = kernel.storage();
    storage.exists(".paperpaw/library/catalog.txt")
        && !storage.exists(".paperpaw/library/catalog.tmp")
}

/// The minute at which the device is first told to sleep, ticking each
/// minute for at most `minutes`; `None` if it never is.
fn first_sleep(kernel: &mut Kernel, minutes: u64) -> Option<u64> {
    (1..=minutes).find(|minute| kernel.tick(Instant(minute * 60_000)).sleep())
}

/// "Sleep after" does not stop a library walk that outlasts it: the sleep
/// is put off while the walk goes on, and comes with the minute after it
/// ends.
#[test]
fn the_sleep_waits_for_the_library_walk_and_comes_after_it() {
    let mut kernel = walking(30, None);
    let mut done = None;
    for minute in 1..=59u64 {
        let effects = kernel.tick(Instant(minute * 60_000));
        if walked(&kernel) {
            done = Some(minute);
            break;
        }
        assert!(!effects.sleep(), "slept while walking, minute {minute}");
    }
    let done = done.expect("the walk ended within the hour");
    assert!(done > 5, "the walk was too short to test: {done} minutes");
    // The walk is over: the sleep comes at the next one due.
    let slept = (done + 1..=done + 12).find(|minute| kernel.tick(Instant(minute * 60_000)).sleep());
    assert!(slept.is_some(), "no sleep after the walk (ended at {done})");
}

/// A walk that outlasts an hour without a press lets the device sleep
/// anyway (and goes on after the wake).
#[test]
fn a_walk_keeps_the_device_awake_for_an_hour_at_most() {
    // A step each minute, a book a step: the walk takes over 100 minutes.
    let mut kernel = walking(100, None);
    let minute = first_sleep(&mut kernel, 150).expect("slept at the cap");
    assert!((60..=62).contains(&minute), "slept at minute {minute}");
    assert!(!walked(&kernel), "the walk had ended");
}

/// A first walk lists the books found so far at checkpoints (#108),
/// each a few steps: appending to the catalog, then sorting the books
/// for the view that asked. The sleep that falls due during them is put
/// off as during any other step of the walk, and comes once it is over.
#[test]
fn the_sleep_waits_through_the_checkpoints_of_a_first_walk() {
    let mut kernel = walking(50, None);
    let wanted = events::SortedWanted::new(String::new()).to_params();
    kernel.send_event(Instant(12_500), events::SORTED_WANTED.id().clone(), wanted);
    let storage = |kernel: &Kernel, path: &str| kernel.storage().size(path).ok();
    let (catalog, sorted) = (".paperpaw/library/catalog.txt", sorted_of(""));
    let (mut appended, mut sorting, mut done) = (0, 0, None);
    for minute in 1..=59u64 {
        let before = (
            storage(&kernel, catalog),
            kernel.storage().exists(".paperpaw/library/sorted.tmp"),
        );
        // "Sleep after" is a minute: the sleep falls due at each tick.
        let effects = kernel.tick(Instant(minute * 60_000));
        if walked(&kernel) {
            done = Some(minute);
            break;
        }
        assert!(!effects.sleep(), "slept during the walk, minute {minute}");
        appended += usize::from(storage(&kernel, catalog) > before.0);
        let sorts = before.1 || kernel.storage().exists(".paperpaw/library/sorted.tmp");
        sorting += usize::from(sorts);
    }
    let done = done.expect("the walk ended within the hour");
    assert!(
        appended >= 2,
        "checkpoint appends at a sleep due: {appended}"
    );
    assert!(sorting >= 2, "checkpoint sorts at a sleep due: {sorting}");
    assert!(storage(&kernel, &sorted).is_some());
    assert_eq!(paths(&kernel).len(), 50);
    let slept = (done + 1..=done + 12).find(|minute| kernel.tick(Instant(minute * 60_000)).sleep());
    assert!(slept.is_some(), "no sleep after the walk (ended at {done})");
}

/// The hour counts from the last press: pressing something starts it
/// again.
#[test]
fn a_press_starts_the_walks_hour_again() {
    let mut kernel = walking(170, None);
    for minute in 1..=50u64 {
        assert!(!kernel.tick(Instant(minute * 60_000)).sleep(), "{minute}");
    }
    kernel.handle(
        Instant(50 * 60_000 + 500),
        paperpaw_platform::InputEvent::Button {
            name: "right",
            state: paperpaw_platform::ButtonState::Pressed,
        },
    );
    kernel.handle(
        Instant(50 * 60_000 + 600),
        paperpaw_platform::InputEvent::Button {
            name: "right",
            state: paperpaw_platform::ButtonState::Released,
        },
    );
    let minute = (51..=170u64)
        .find(|minute| kernel.tick(Instant(minute * 60_000)).sleep())
        .expect("slept");
    assert!(
        (110..=112).contains(&minute),
        "an hour from the press, not from the start: minute {minute}"
    );
}

/// A nearly empty battery (not charging) sleeps as it would.
#[test]
fn a_nearly_empty_battery_does_not_wait_for_the_walk() {
    let mut kernel = walking(30, Some(8));
    assert_eq!(first_sleep(&mut kernel, 5), Some(1));
    // Charging, it waits.
    let mut kernel = walking(30, None);
    assert_eq!(first_sleep(&mut kernel, 3), None);
}

/// A walk paused for a book that is open (a reader on top) holds nothing:
/// the device sleeps when it is due.
#[test]
fn a_walk_paused_for_a_book_does_not_hold_the_sleep() {
    let mut kernel = walking(30, None);
    let path = Params::new().with(params::PATH, "books/b0001.txt");
    kernel.open_slot(Instant(1000), slots::READER, path);
    assert_eq!(first_sleep(&mut kernel, 5), Some(2));
}

/// The hour counts from the reader's last input, not from a sleep put off
/// for something else: a text read aloud over the hour's end does not give
/// the walk another hour once it is over.
#[test]
fn the_walks_hour_is_not_restarted_by_a_sleep_put_off_for_speech() {
    let mut kernel = walking(170, None);
    for minute in 1..=39u64 {
        assert!(!kernel.tick(Instant(minute * 60_000)).sleep(), "{minute}");
    }
    let hello = paperpaw_sdk::Speech::new("Hello", Id::new_static("lib.spoken"));
    kernel.output.commands.push(Command::speak(hello));
    kernel.run_commands();
    let Some(SpeechCall::Say { id, .. }) = kernel.take_speech().pop() else {
        panic!("not spoken");
    };
    // Spoken through the hour's end: no sleep.
    for minute in 40..=69u64 {
        assert!(!kernel.tick(Instant(minute * 60_000)).sleep(), "{minute}");
    }
    assert!(kernel.speaking());
    kernel.speech_done(Instant(69 * 60_000 + 30_000), id, true);
    // Over: the hour had ended long before, the sleep comes.
    let minute = (70..=75u64)
        .find(|minute| kernel.tick(Instant(minute * 60_000)).sleep())
        .expect("slept after the speech, not an hour later");
    assert!(minute <= 72, "slept at minute {minute}");
}

/// *Look for new books* (#277): the kernel's action walks the library
/// again, though it was walked since the start, and finds a book copied
/// meanwhile.
#[test]
fn looking_for_new_books_walks_the_library_again() {
    let (mut kernel, log, now) = library(two_books());
    let now = want(&mut kernel, now, false);
    assert_eq!(log.borrow().len(), 1);
    kernel.storage().write("books/c.txt", b"three").unwrap();
    let look = Id::new_static("core.library.look-again");
    let effects = kernel.run_action(Instant(now), look);
    run(&mut kernel, now, effects);
    assert_eq!(
        paths(&kernel),
        ["books/a.txt", "books/b.txt", "books/c.txt"]
    );
    assert_eq!(log.borrow().len(), 2);
}
