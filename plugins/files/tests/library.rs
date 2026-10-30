//! The library with the real kernel: what it reads while a book (or any
//! full screen) covers it, where its focus is when it shows again, and
//! the folder chooser starting where the setting is.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::Cell;
use std::rc::Rc;

use paperpaw_core::Kernel;
use paperpaw_platform::Rotation;
use paperpaw_sdk::builtin::{events, params, slots};
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Params,
    PixelFormat, Storage, StorageError,
};

/// An X4: black and white, buttons.
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

/// A card that counts how often the library's catalog (or the log
/// beside it) is read.
struct Counted {
    inner: MemoryStorage,
    reads: Rc<Cell<usize>>,
}

impl Storage for Counted {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        if path.starts_with(".paperpaw/library/catalog") || path.ends_with("status.log") {
            self.reads.set(self.reads.get() + 1);
        }
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
        self.inner.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.inner.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.inner.create_dir(path)
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.inner.remove_dir_all(path)
    }
}

/// A card with `n` books, `book 00.txt` and on.
fn card(n: usize) -> MemoryStorage {
    (0..n).fold(MemoryStorage::new(), |card, i| {
        card.with(&format!("books/book {i:02}.txt"), "Some text.")
    })
}

/// The library on `storage`, shown, with nothing left to do; and the
/// count of reads of its catalog.
fn library(storage: MemoryStorage, config: &str) -> (Kernel, Rc<Cell<usize>>) {
    let reads = Rc::new(Cell::new(0));
    let user = format!("[core]\nlanguage = en\n{config}\n");
    let mut kernel = Kernel::builder(DEVICE)
        .storage(Counted {
            inner: storage,
            reads: Rc::clone(&reads),
        })
        .user_config(user)
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(paperpaw_plugin_format_txt::TxtFormat)
        .plugin(paperpaw_plugin_files::Files)
        .build()
        .unwrap();
    let effects = kernel.start(Instant(0));
    settle(&mut kernel, effects);
    let effects = kernel.open_slot(Instant(0), slots::LIBRARY, Params::new());
    settle(&mut kernel, effects);
    (kernel, reads)
}

/// Runs the timers asked for until there are none.
fn settle(kernel: &mut Kernel, mut effects: paperpaw_core::Effects) {
    let mut now = 0;
    for _ in 0..1000 {
        match effects.wake_at() {
            Some(at) if at.0 <= now + 10_000 => {
                now = now.max(at.0);
                effects = kernel.tick(Instant(now));
            }
            _ => break,
        }
    }
    kernel.render(&mut |_, _| {});
}

/// The label of the focused row.
fn focused(kernel: &mut Kernel) -> String {
    kernel.render(&mut |_, _| {});
    kernel
        .describe()
        .into_iter()
        .find(|part| part.focused)
        .map(|part| part.label)
        .unwrap_or_default()
}

fn action(kernel: &mut Kernel, id: &'static str) {
    let effects = kernel.run_action(Instant(0), Id::new_static(id));
    settle(kernel, effects);
}

/// Covered by a book (here its information, also a full screen), the
/// library reads nothing when the catalog or a book changes or settings
/// do: once, when it shows again.
#[test]
fn hidden_it_reads_nothing_until_it_shows() {
    let _turn = turn();
    let (mut kernel, reads) = library(card(5), "");
    let info = Params::new().with(params::PATH, "books/book 01.txt");
    let effects = kernel.open_screen(Instant(0), paperpaw_plugin_files::INFO_SCREEN, info);
    settle(&mut kernel, effects);
    reads.set(0);
    let changed = paperpaw_sdk::library::changed_book("books/book 02.txt", 2);
    let paperpaw_sdk::Command::Emit(book, book_params) = changed else {
        panic!("not an event");
    };
    for _ in 0..3 {
        for (event, params) in [
            (events::CATALOG_CHANGED.id().clone(), Params::new()),
            (book.clone(), book_params.clone()),
        ] {
            let effects = kernel.send_event(Instant(0), event, params);
            settle(&mut kernel, effects);
        }
        let effects = kernel
            .set_setting(Instant(0), "files.sort", "files.sort.name")
            .unwrap();
        settle(&mut kernel, effects);
    }
    assert_eq!(reads.get(), 0, "read while hidden");
    action(&mut kernel, "core.back");
    assert!(reads.get() > 0, "not read when shown again");
    assert!(focused(&mut kernel).contains("book 00"));
}

/// The tests' turns with the library: when its catalog was made is kept
/// for the whole program, as on a device, so tests that make or read it
/// take turns rather than run beside each other.
static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Waits for this test's turn with the library.
fn turn() -> std::sync::MutexGuard<'static, ()> {
    TURN.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Coming back, the focus is on the book it was on, on its page, and
/// marking a book finished on its sheet keeps the focus on it.
#[test]
fn the_focus_stays_on_its_book() {
    let _turn = turn();
    // The order as it comes: by recent reading.
    let (mut kernel, _) = library(card(30), "");
    for _ in 0..13 {
        action(&mut kernel, "core.down");
    }
    assert!(
        focused(&mut kernel).contains("book 13"),
        "{}",
        focused(&mut kernel)
    );
    let info = Params::new().with(params::PATH, "books/book 13.txt");
    let effects = kernel.open_screen(Instant(0), paperpaw_plugin_files::INFO_SCREEN, info);
    settle(&mut kernel, effects);
    action(&mut kernel, "core.back");
    assert!(
        focused(&mut kernel).contains("book 13"),
        "{}",
        focused(&mut kernel)
    );

    // Its sheet: Open, then Mark as finished.
    action(&mut kernel, "core.menu");
    action(&mut kernel, "core.down");
    action(&mut kernel, "core.confirm");
    let shown = focused(&mut kernel);
    assert!(
        shown.contains("book 13") && shown.contains("Finished"),
        "{shown}"
    );
}

/// Buttons work as in every list: left and right move a book, page
/// buttons turn a page, and before the first book is the row that
/// chooses what is shown, which confirm opens.
#[test]
fn buttons_work_as_in_every_list() {
    let _turn = turn();
    let (mut kernel, _) = library(
        card(30),
        "[files]
sort = files.sort.name
",
    );
    assert!(focused(&mut kernel).contains("book 00"));
    action(&mut kernel, "core.right");
    assert!(focused(&mut kernel).contains("book 01"));
    // A page on: its first book.
    action(&mut kernel, "core.left");
    action(&mut kernel, "core.next-page");
    let shown = focused(&mut kernel);
    assert!(
        !shown.contains("book 00") && shown.contains("book 0"),
        "{shown}"
    );
    action(&mut kernel, "core.prev-page");
    assert!(focused(&mut kernel).contains("book 00"));
    // Before the first book: what is shown, opening the choice of it.
    action(&mut kernel, "core.left");
    assert_eq!(focused(&mut kernel), "All books");
    action(&mut kernel, "core.confirm");
    assert_eq!(
        kernel.active_screen().map(|id| id.as_str()),
        Some("core.choice-dialog")
    );
    // Unread: no book was opened.
    action(&mut kernel, "core.down");
    action(&mut kernel, "core.down");
    action(&mut kernel, "core.confirm");
    let shown = focused(&mut kernel);
    assert!(shown.contains("book 00"), "{shown}");
    kernel.render(&mut |_, _| {});
    let parts = kernel.describe();
    assert!(parts.iter().any(|p| p.label == "Unread"), "{parts:?}");
}

/// The folder chooser opens in the folder the setting names (a folder,
/// not a file).
#[test]
fn the_folder_chooser_starts_in_the_folder_set() {
    let _turn = turn();
    let storage = card(2)
        .with("books/Fantasy/a.txt", "A")
        .with("books/Fantasy/Old/b.txt", "B");
    let (mut kernel, _) = library(storage, "[files]\narchive = books/Fantasy\n");
    let params = Params::new().with("id", "files.archive");
    let effects = kernel.open_screen(Instant(0), paperpaw_plugin_files::CHOOSE_FOLDER, params);
    settle(&mut kernel, effects);
    let shown: Vec<String> = kernel.describe().into_iter().map(|p| p.label).collect();
    assert!(
        shown.iter().any(|l| l.contains("books/Fantasy")),
        "{shown:?}"
    );
    assert!(shown.iter().any(|l| l.contains("Old/")), "{shown:?}");
}
