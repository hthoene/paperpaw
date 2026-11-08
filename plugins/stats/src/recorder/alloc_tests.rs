//! The recorder when the heap refuses: each allocation of a reading session
//! (a book opened, pages turned, the book closed) is refused in turn. An
//! allocation that would abort (an infallible `String::from`, `push`) ends the
//! whole test binary; a refused one leaves the books and the log readable, and
//! never records a stay twice or for the wrong book.

use alloc::string::String;
use alloc::vec::Vec;
use core::cell::Cell;

use paperpaw_sdk::{DirEntry, MemoryStorage, Storage, StorageError};

use super::*;
use crate::testing::heap::{armed, fixture};

/// A storage whose own allocations are not the plugin's: nothing is refused
/// or counted while it works.
#[derive(Default)]
struct Quiet(MemoryStorage);

impl Storage for Quiet {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        fixture(|| self.0.read_at(path, offset, buf))
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        fixture(|| self.0.size(path))
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        fixture(|| self.0.write(path, data))
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        fixture(|| self.0.append(path, data))
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        fixture(|| self.0.list(dir, visit))
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        fixture(|| self.0.remove(path))
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        fixture(|| self.0.rename(from, to))
    }
}

struct Quietly {
    data: Quiet,
    files: Quiet,
    now: Cell<u64>,
    wall: Cell<Option<i64>>,
}

impl Surroundings for Quietly {
    fn data(&self) -> &dyn Storage {
        &self.data
    }
    fn files(&self) -> &dyn Storage {
        &self.files
    }
    fn now_ms(&self) -> u64 {
        self.now.get()
    }
    fn unix_at(&self, at_ms: u64) -> Option<i64> {
        self.wall.get().map(|wall| wall + (at_ms / 1000) as i64)
    }
    fn day_of(&self, unix: u32) -> i32 {
        (unix / 86_400) as i32
    }
    fn limits(&self) -> Limits {
        Limits {
            enabled: true,
            min: 5,
            max: 120,
        }
    }
}

fn make(wall: Option<i64>) -> Quietly {
    let files = Quiet::default();
    fixture(|| {
        files.0.write("books/a.epub", b"the book a").unwrap();
        files.0.write("books/b.epub", b"the book b").unwrap();
    });
    Quietly {
        data: Quiet::default(),
        files,
        now: Cell::new(0),
        wall: Cell::new(wall),
    }
}

fn shown(path: &str, page: u32) -> Reading<'_> {
    Reading {
        path,
        title: "A book",
        progress_ppm: 0,
        page: Some(page),
        pages: Some(300),
        at: (0, page, 0),
    }
}

/// A session of two books, the clock known from the start or from the middle.
fn session(cx: &Quietly, recorder: &mut Recorder, learns_the_time: bool) {
    let step = |seconds: u64| cx.now.set(cx.now.get() + seconds * 1000);
    recorder.started(cx);
    recorder.book_opened(cx, &shown("books/a.epub", 1));
    step(10);
    recorder.page_turned(cx, &shown("books/a.epub", 2));
    step(10);
    recorder.suspended(cx);
    step(600);
    recorder.resumed(cx);
    recorder.book_opened(cx, &shown("books/b.epub", 1));
    step(20);
    if learns_the_time {
        cx.wall.set(Some(1_700_000_000));
        recorder.clock_changed(cx);
    }
    recorder.book_moved(cx, "books/a.epub", "books/c.epub");
    recorder.book_finished(cx, "books/b.epub");
    recorder.book_closed(cx, "books/b.epub");
}

fn seconds_and_books(cx: &Quietly) -> (u32, usize) {
    fixture(|| {
        let days = store::days(cx.data(), |unix| cx.day_of(unix)).unwrap();
        let books = store::load_books(cx.data()).unwrap();
        (days.iter().map(|d| d.seconds).sum(), books.len())
    })
}

#[test]
fn a_refused_allocation_never_aborts_and_leaves_the_statistics_readable() {
    for (wall, learns) in [(Some(1_700_000_000), false), (None, true)] {
        let whole = fixture(|| make(wall));
        let mut recorder = fixture(Recorder::default);
        let (_, count) = armed(usize::MAX, || session(&whole, &mut recorder, learns));
        let (seconds, books) = seconds_and_books(&whole);
        assert_eq!((seconds, books), (40, 2), "nothing refused");
        assert!(
            count > 10,
            "the session allocates nothing: it proves nothing"
        );
        for at in 1..=count {
            let cx = fixture(|| make(wall));
            let mut recorder = fixture(Recorder::default);
            armed(at, || session(&cx, &mut recorder, learns));
            // Whatever was refused: both files still read, and no more was
            // recorded than was read.
            let (seconds, books) = seconds_and_books(&cx);
            assert!(seconds <= 40 && books <= 2, "refusing {at}: {seconds} s");
        }
    }
}

#[test]
fn a_refused_allocation_while_a_book_is_added_adds_no_half_book() {
    let cx = fixture(|| make(Some(1_700_000_000)));
    let mut recorder = fixture(Recorder::default);
    let (_, count) = armed(usize::MAX, || {
        recorder.book_opened(&cx, &shown("books/a.epub", 1));
    });
    assert!(count > 0);
    for at in 1..=count {
        let cx = fixture(|| make(Some(1_700_000_000)));
        let mut recorder = fixture(Recorder::default);
        armed(at, || {
            recorder.book_opened(&cx, &shown("books/a.epub", 1));
        });
        let books: Vec<store::Book> = fixture(|| store::load_books(cx.data()).unwrap());
        for book in &books {
            assert!(
                !book.title.is_empty() && !book.md5.is_empty() && !book.path.is_empty(),
                "refusing {at}: a book with a field missing: {book:?}"
            );
            assert_eq!(book.authors, String::from("N/A"));
        }
    }
}
