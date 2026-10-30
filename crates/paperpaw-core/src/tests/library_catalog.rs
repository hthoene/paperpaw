//! The kernel's catalog of the library: never walked at a start, walked
//! once a view asks after one (and when asked anew), nothing done while a
//! book is open, and books moved, removed or changed caught up without a
//! walk.

use super::*;
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

impl Plugin for LibraryPlugin {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("lib"), "Library", Version::new(1, 0, 0))
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_format(FormatSpec::new(
            Id::new_static("lib.txt"),
            "TXT",
            alloc::vec![Cow::Borrowed("txt")],
            Box::new(|_, _, _| Err(DocumentError::UnsupportedFormat)),
        ))?;
        r.add_book_field(
            library::BookField::new(Id::new_static("lib.rating"), "lib.rating").in_catalog(),
        )?;
        // A reader, as the reader's slot shows it.
        r.add_screen(ScreenSpec::new(
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
        r.add_screen(ScreenSpec::new(
            Id::new_static("lib.list"),
            "list",
            Box::new(|_, _| Box::new(Page)),
        ))?;
        r.bind_slot(
            slots::LIBRARY,
            Id::new_static("lib.list"),
            Priority(i16::MAX),
        )?;
        let log = self.log.clone();
        r.subscribe(
            events::CATALOG_CHANGED.id().clone(),
            Box::new(move |_, _, _| log.borrow_mut().push(String::from("catalog"))),
        )
    }
}

/// A book's page.
struct Page;

impl Screen for Page {
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
        .plugin(LibraryPlugin { log: log.clone() })
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

/// On a device of little memory (the X4) the catalog waits while the
/// library shows its books; on others it is made beside them.
#[test]
fn on_small_devices_nothing_is_done_while_the_library_shows() {
    for (memory, beside) in [
        (paperpaw_platform::MemoryClass::Small, false),
        (paperpaw_platform::MemoryClass::Large, true),
    ] {
        let device = DEVICE.with_memory(memory);
        let (mut kernel, log, now) = library_on(device, two_books());
        let now = want(&mut kernel, now, false);
        log.borrow_mut().clear();
        let effects = kernel.open_slot(Instant(now), slots::LIBRARY, Params::new());
        let now = run(&mut kernel, now, effects);
        let now = want(&mut kernel, now, true);
        assert_eq!(log.borrow().len(), usize::from(beside), "{memory:?}");
        let effects = kernel.run_action(Instant(now), actions::BACK);
        run(&mut kernel, now, effects);
        assert_eq!(log.borrow().len(), 1, "{memory:?}");
    }
}

#[test]
fn books_moved_removed_or_changed_are_caught_up() {
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
    let now = run(&mut kernel, now, effects);
    // A field kept in the catalog changed: caught up with it.
    library::set_field(kernel.storage(), "books/Kept/a.txt", "lib.rating", "5");
    let changed = events::BookChanged::new("books/Kept/a.txt".into(), library::what::FIELDS);
    send(
        &mut kernel,
        now,
        events::BOOK_CHANGED.id(),
        changed.to_params(),
    );
    let mut rating = None;
    catalog::each(kernel.storage(), &mut |r| {
        rating = r.field("lib.rating").map(String::from)
    });
    assert_eq!(rating.as_deref(), Some("5"));
}
