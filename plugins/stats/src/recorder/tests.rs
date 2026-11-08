//! The recorder over fake surroundings: the monotonic clock is set by the
//! test, the wall clock is known from a moment on (or not at all), days are
//! UTC days.

use alloc::string::String;
use core::cell::Cell;

use paperpaw_sdk::{MemoryStorage, Storage};

use super::*;
use crate::testing::Faulty;

const BOOK: &str = "books/a.epub";
const WALL: i64 = 1_700_000_000;

struct Fake {
    data: Faulty,
    files: MemoryStorage,
    now: Cell<u64>,
    /// Unix seconds at monotonic 0, once the device knows the time.
    wall: Cell<Option<i64>>,
    limits: Cell<Limits>,
}

impl Fake {
    fn new() -> Self {
        let files = MemoryStorage::new();
        files.write(BOOK, b"the book a").unwrap();
        files.write("books/b.epub", b"the book b").unwrap();
        Self {
            data: Faulty::default(),
            files,
            now: Cell::new(0),
            wall: Cell::new(Some(WALL)),
            limits: Cell::new(Limits {
                enabled: true,
                min: 5,
                max: 120,
            }),
        }
    }

    fn after(&self, seconds: u64) {
        self.now.set(self.now.get() + seconds * 1000);
    }

    fn seconds_read(&self) -> u32 {
        store::days(&self.data, |unix| self.day_of(unix))
            .unwrap()
            .iter()
            .map(|d| d.seconds)
            .sum()
    }
}

impl Surroundings for Fake {
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
        self.limits.get()
    }
}

fn reading(path: &str, page: u32) -> Reading<'_> {
    Reading {
        path,
        title: "A book",
        progress_ppm: 0,
        page: Some(page),
        pages: Some(300),
        at: (0, page * 100, 0),
    }
}

#[test]
fn stays_count_as_koreader_counts_them() {
    assert_eq!(counted(3, 5, 120), 0);
    assert_eq!(counted(5, 5, 120), 5);
    assert_eq!(counted(45, 5, 120), 45);
    assert_eq!(counted(3600, 5, 120), 120);
}

#[test]
fn pages_come_from_the_reader_or_the_progress() {
    let counted = reading(BOOK, 12);
    assert_eq!(page_of(&counted), (12, 300));
    let estimated = Reading {
        page: None,
        pages: None,
        progress_ppm: 425_000,
        ..counted
    };
    assert_eq!(page_of(&estimated), (426, 1000));
    let end = Reading {
        progress_ppm: 1_000_000,
        ..estimated
    };
    assert_eq!(page_of(&end), (1000, 1000));
}

#[test]
fn a_book_read_is_recorded_page_by_page() {
    let cx = Fake::new();
    let mut recorder = Recorder::default();
    recorder.book_opened(&cx, &reading(BOOK, 1));
    cx.after(10);
    recorder.page_turned(&cx, &reading(BOOK, 2));
    cx.after(20);
    let told = recorder.book_closed(&cx, BOOK);
    assert_eq!(cx.seconds_read(), 30);
    let books = store::books(&cx.data);
    assert_eq!(books.len(), 1);
    assert_eq!(
        (
            books[0].title.as_str(),
            books[0].path.as_str(),
            books[0].pages
        ),
        ("A book", BOOK, 300)
    );
    assert_eq!(books[0].authors, "N/A");
    // Today's seconds are told when they changed, and only then.
    assert_eq!(told.today, Some(((WALL / 86_400) as i32, 30)));
    assert_eq!(recorder.tell(&cx).today, None);
    assert!(recorder.current_book().is_none());
}

#[test]
fn short_stays_and_other_books_pages_are_not_counted() {
    let cx = Fake::new();
    let mut recorder = Recorder::default();
    recorder.book_opened(&cx, &reading(BOOK, 1));
    cx.after(3);
    recorder.page_turned(&cx, &reading(BOOK, 2));
    cx.after(4);
    // A page of a book that is not the open one changes nothing.
    recorder.page_turned(&cx, &reading("books/b.epub", 9));
    cx.after(30);
    recorder.book_closed(&cx, BOOK);
    assert_eq!(cx.seconds_read(), 30 + 4);
}

#[test]
fn nothing_is_recorded_while_switched_off() {
    let cx = Fake::new();
    cx.limits.set(Limits {
        enabled: false,
        ..cx.limits.get()
    });
    let mut recorder = Recorder::default();
    recorder.book_opened(&cx, &reading(BOOK, 1));
    cx.after(60);
    recorder.book_closed(&cx, BOOK);
    assert_eq!(cx.seconds_read(), 0);
    assert!(store::books(&cx.data).is_empty());
}

#[test]
fn reading_before_the_time_is_known_waits_for_it() {
    let cx = Fake::new();
    cx.wall.set(None);
    let mut recorder = Recorder::default();
    recorder.book_opened(&cx, &reading(BOOK, 1));
    cx.after(10);
    recorder.page_turned(&cx, &reading(BOOK, 2));
    cx.after(20);
    recorder.book_closed(&cx, BOOK);
    assert_eq!(cx.seconds_read(), 0);
    assert!(store::books(&cx.data).is_empty());
    // The device learns the time: both stays go to the log, dated by when
    // they were read (the wall clock minus how long ago).
    cx.wall.set(Some(WALL - 30));
    let told = recorder.clock_changed(&cx);
    assert_eq!(cx.seconds_read(), 30);
    assert_eq!(store::books(&cx.data).len(), 1);
    assert_eq!(told.today.map(|(_, seconds)| seconds), Some(30));
}

#[test]
fn sleeping_pauses_reading_and_waking_goes_on() {
    let cx = Fake::new();
    let mut recorder = Recorder::default();
    recorder.book_opened(&cx, &reading(BOOK, 1));
    cx.after(10);
    recorder.suspended(&cx);
    // Asleep for an hour: not read.
    cx.after(3600);
    recorder.resumed(&cx);
    cx.after(15);
    recorder.page_turned(&cx, &reading(BOOK, 2));
    assert_eq!(cx.seconds_read(), 10 + 15);
    // Woken again with nothing open: nothing starts.
    recorder.book_closed(&cx, BOOK);
    recorder.resumed(&cx);
    cx.after(60);
    recorder.suspended(&cx);
    assert_eq!(cx.seconds_read(), 25);
}

#[test]
fn the_same_page_again_is_not_a_new_stay() {
    let cx = Fake::new();
    let mut recorder = Recorder::default();
    recorder.book_opened(&cx, &reading(BOOK, 4));
    cx.after(10);
    recorder.page_turned(&cx, &reading(BOOK, 4));
    cx.after(10);
    recorder.book_opened(&cx, &reading(BOOK, 4));
    cx.after(10);
    recorder.book_closed(&cx, BOOK);
    // One stay of 30 s, limited to the longest page (120 s) as a whole.
    assert_eq!(cx.seconds_read(), 30);
}

#[test]
fn another_book_ends_the_page_of_the_first() {
    let cx = Fake::new();
    let mut recorder = Recorder::default();
    recorder.book_opened(&cx, &reading(BOOK, 1));
    cx.after(12);
    recorder.book_opened(&cx, &reading("books/b.epub", 1));
    cx.after(8);
    recorder.book_closed(&cx, "books/b.epub");
    assert_eq!(cx.seconds_read(), 20);
    assert_eq!(store::books(&cx.data).len(), 2);
}

#[test]
fn a_failed_write_is_told_once_and_the_stay_is_kept() {
    let cx = Fake::new();
    let mut recorder = Recorder::default();
    recorder.book_opened(&cx, &reading(BOOK, 1));
    // The card is full from now on.
    cx.data.fill_after(0);
    cx.after(20);
    let told = recorder.page_turned(&cx, &reading(BOOK, 2));
    assert!(told.not_saved);
    cx.after(20);
    let told = recorder.book_closed(&cx, BOOK);
    assert!(!told.not_saved, "said once");
    // Room again: the next event records what waited.
    cx.data.fill_after(u32::MAX);
    recorder.clock_changed(&cx);
    assert!(cx.seconds_read() >= 20);
}

#[test]
fn a_book_finished_is_marked_once_a_year() {
    let cx = Fake::new();
    let mut recorder = Recorder::default();
    recorder.book_opened(&cx, &reading(BOOK, 1));
    cx.after(10);
    recorder.page_turned(&cx, &reading(BOOK, 2));
    recorder.book_finished(&cx, BOOK);
    let first = store::books(&cx.data)[0].finished;
    assert_eq!(first, WALL + 10);
    cx.after(100);
    recorder.book_finished(&cx, BOOK);
    assert_eq!(store::books(&cx.data)[0].finished, first, "the same year");
    // A year later it counts again.
    cx.after(366 * 86_400);
    recorder.book_finished(&cx, BOOK);
    assert!(store::books(&cx.data)[0].finished > first);
}

#[test]
fn a_book_moved_keeps_its_numbers() {
    let cx = Fake::new();
    let mut recorder = Recorder::default();
    recorder.book_opened(&cx, &reading(BOOK, 1));
    cx.after(10);
    recorder.book_closed(&cx, BOOK);
    recorder.book_moved(&cx, BOOK, "books/new/a.epub");
    let books = store::books(&cx.data);
    assert_eq!(books.len(), 1);
    assert_eq!(books[0].path, "books/new/a.epub");
}

#[test]
fn the_view_includes_the_page_being_read() {
    let cx = Fake::new();
    let mut recorder = Recorder::default();
    recorder.book_opened(&cx, &reading(BOOK, 1));
    cx.after(10);
    recorder.settle(&cx);
    assert_eq!(cx.seconds_read(), 10);
    assert!(recorder.current_book().is_some());
    // Reading goes on from now, not from the page's start.
    cx.after(10);
    recorder.book_closed(&cx, BOOK);
    assert_eq!(cx.seconds_read(), 20);
}

#[test]
fn a_book_without_a_title_is_named_by_its_file() {
    let cx = Fake::new();
    let mut recorder = Recorder::default();
    let mut shown = reading(BOOK, 1);
    shown.title = "";
    recorder.book_opened(&cx, &shown);
    assert_eq!(store::books(&cx.data)[0].title, String::from("a"));
}

#[test]
fn a_moment_on_the_monotonic_clock_is_dated_from_a_snapshot() {
    // The snapshot: Unix 1_000_000 at 50 s on the monotonic clock.
    assert_eq!(unix_at(1_000_000, 50_000, 50_000), Some(1_000_000));
    assert_eq!(unix_at(1_000_000, 50_000, 20_000), Some(999_970));
    // Rounded down, also before the snapshot.
    assert_eq!(unix_at(1_000_000, 50_000, 49_001), Some(999_999));
    assert_eq!(unix_at(1_000_000, 50_000, 50_999), Some(1_000_000));
    // A moment after it (a stay's end is the snapshot's own moment or later).
    assert_eq!(unix_at(1_000_000, 50_000, 61_000), Some(1_000_011));
    assert_eq!(unix_at(i64::MAX, 0, 5_000), None);
}
