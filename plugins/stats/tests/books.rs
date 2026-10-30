//! The books of the statistics as reading adds and finishes them, with
//! the real kernel and the reader's events: a new book joins the others
//! under an id of its own, and a `books.ini` that cannot be read is
//! never written over.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::Kernel;
use paperpaw_platform::{Rotation, SystemStatus, WallClock};
use paperpaw_sdk::builtin::events::{self, Reading};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::{
    Command, DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, MemoryStorage, PixelFormat,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Small,
);

/// 14 October 2026, 10:41 UTC.
const NOW: i64 = 1_791_967_260;
const DIR: &str = ".paperpaw/plugins/stats";
const NEW: &str = "books/new.epub";

/// Two books, and stays of a third whose entry was lost.
const BOOKS: &str = "[book.1]\ntitle = Anna Karenina\nmd5 = 0f343b0931126a20f133d67c2b018a3b\npath = books/anna.epub\n\n\
                     [book.2]\ntitle = Effi Briest\nmd5 = 1f343b0931126a20f133d67c2b018a3b\npath = books/effi.epub\n";

/// A stay of book 7.
const LOG: [u8; 16] = [
    7, 0, 1, 0, 0, 0, 0x00, 0x78, 0xe7, 0x68, 50, 0, 0x2c, 0x01, 0, 0,
];

struct Device {
    kernel: Kernel,
    now: u64,
}

impl Device {
    fn new(books: &[u8]) -> Self {
        Self::with(books, "books.ini", "pages.bin")
    }

    /// With the books and the log in the files named so.
    fn with(books: &[u8], books_file: &str, log_file: &str) -> Self {
        let storage = MemoryStorage::new()
            .with(NEW, "A new book")
            .with(&format!("{DIR}/{books_file}"), books.to_vec())
            .with(&format!("{DIR}/{log_file}"), LOG.to_vec());
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage)
            .user_config("[core]\nlanguage = en\n")
            .plugin(paperpaw_plugin_stats::Stats)
            .build()
            .unwrap();
        let clock = WallClock::new(Instant(0), NOW * 1000, Some(0));
        let status = SystemStatus::new(Some(clock), None);
        kernel.set_status(Instant(0), status);
        kernel.start(Instant(0));
        Self { kernel, now: 0 }
    }

    /// The reader says `event` about page `page` of the book at `path`,
    /// `seconds` after the start.
    fn reader(&mut self, seconds: u64, event: Event<Reading>, path: &str, page: u32) {
        let reading = Reading::new(
            path.into(),
            String::new(),
            page as f32 / 3.0,
            0,
            page * 1000,
            String::new(),
        )
        .page(page)
        .pages(300);
        let Command::Emit(id, params) = event.emit(reading) else {
            unreachable!("an event");
        };
        assert!(seconds * 1000 >= self.now, "time runs on");
        self.now = seconds * 1000;
        self.kernel.send_event(Instant(self.now), id, params);
    }

    fn books(&self) -> String {
        let storage = self.kernel.storage();
        let path = format!("{DIR}/books.ini");
        let size = storage.size(&path).unwrap() as usize;
        let mut data = vec![0u8; size];
        storage.read_at(&path, 0, &mut data).unwrap();
        String::from_utf8_lossy(&data).into_owned()
    }

    fn log_size(&self) -> u64 {
        self.kernel
            .storage()
            .size(&format!("{DIR}/pages.bin"))
            .unwrap()
    }
}

/// A new book joins the others, under an id no stay has yet.
#[test]
fn a_new_book_joins_the_others() {
    let mut device = Device::new(BOOKS.as_bytes());
    device.reader(1, events::BOOK_OPENED, NEW, 1);
    device.reader(31, events::PAGE_TURNED, NEW, 2);
    let books = device.books();
    assert!(books.contains("[book.1]\ntitle = Anna Karenina"), "{books}");
    assert!(books.contains("[book.2]\ntitle = Effi Briest"), "{books}");
    assert!(books.contains("[book.8]\ntitle = new\n"), "{books}");
    assert!(books.contains("path = books/new.epub"), "{books}");
    assert_eq!(device.log_size(), 32);
}

/// The books and the log left beside their places by a cut while they
/// were written anew are put there at the start, before a new book
/// would start a new list of books.
#[test]
fn files_left_beside_their_place_are_put_there() {
    let mut device = Device::with(BOOKS.as_bytes(), "books.new", "pages.new");
    device.reader(1, events::BOOK_OPENED, NEW, 1);
    device.reader(31, events::PAGE_TURNED, NEW, 2);
    let books = device.books();
    assert!(books.contains("[book.1]\ntitle = Anna Karenina"), "{books}");
    assert!(books.contains("[book.8]\ntitle = new\n"), "{books}");
    assert_eq!(device.log_size(), 32);
}

/// A `books.ini` that cannot be read (here saved as Latin-1 on a
/// computer) stays as it is when a new book is opened, read and
/// finished; the book is not recorded meanwhile.
#[test]
fn a_damaged_list_of_books_is_kept() {
    let damaged = b"[book.1]\ntitle = M\xfcller\nmd5 = 0f343b0931126a20f133d67c2b018a3b\n";
    let mut device = Device::new(damaged);
    device.reader(1, events::BOOK_OPENED, NEW, 1);
    device.reader(31, events::PAGE_TURNED, NEW, 2);
    device.reader(61, events::BOOK_FINISHED, NEW, 300);
    let storage = device.kernel.storage();
    let path = format!("{DIR}/books.ini");
    let mut data = vec![0u8; damaged.len() + 1];
    assert_eq!(storage.read_at(&path, 0, &mut data).unwrap(), damaged.len());
    assert_eq!(&data[..damaged.len()], damaged);
    assert_eq!(device.log_size(), 16);
}

/// A book read to the end is marked finished then, and not again later
/// the same year.
#[test]
fn finished_books_are_marked_once_a_year() {
    let mut device = Device::new(BOOKS.as_bytes());
    device.reader(1, events::BOOK_OPENED, NEW, 299);
    device.reader(10, events::BOOK_FINISHED, NEW, 300);
    let first = format!("finished = {}\n", NOW + 10);
    assert!(device.books().contains(&first), "{}", device.books());
    device.reader(100, events::BOOK_FINISHED, NEW, 300);
    assert!(device.books().contains(&first), "{}", device.books());
    // The other books are as they were.
    assert!(device.books().contains("[book.2]\ntitle = Effi Briest"));
}
