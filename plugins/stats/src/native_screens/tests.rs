//! The books and book views, made over the plugin's own catalogs and files.

use alloc::string::ToString;
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::native::Error;
use paperpaw_sdk::{MemoryStorage, Storage};

use super::{BookView, BooksView};
use crate::screens_tests::{QuietWords, sweep, words};
use crate::store::{self, Book, PageStat};
use crate::testing::Quiet;
use crate::testing::heap::fixture;

const DAY: u32 = 86_400;
/// Day 20,000 (4 October 2024), UTC.
const FIRST: u32 = 20_000;

fn book(id: u16, title: &str, last_open: i64, pages: u32) -> Book {
    Book {
        id,
        title: title.to_string(),
        authors: "N/A".to_string(),
        pages,
        last_open,
        ..Book::default()
    }
}

fn read(storage: &MemoryStorage, day: u32, book: u16, page: u32, seconds: u16) {
    store::append(
        storage,
        PageStat {
            book,
            page,
            start: day * DAY + 8 * 3_600,
            duration: seconds,
            pages: 100,
        },
    )
    .unwrap();
}

/// Book 1 read on 12 days, book 2 once, book 3 never.
fn card() -> Quiet {
    let storage = MemoryStorage::new();
    store::save_books(
        &storage,
        &[
            book(1, "Anna Karenina", i64::from((FIRST + 11) * DAY), 300),
            book(2, "Effi Briest", i64::from((FIRST + 20) * DAY), 0),
            book(3, "Never read", i64::from(FIRST * DAY), 120),
        ],
    )
    .unwrap();
    for n in 0..12 {
        read(&storage, FIRST + n, 1, n + 1, 60 * (n as u16 + 1));
    }
    read(&storage, FIRST + 20, 2, 1, 41);
    Quiet(storage)
}

#[test]
fn the_books_come_last_read_first_and_open_by_their_ids() {
    let data = card();
    let view = BooksView::build(&mut words("en"), &data).unwrap();
    assert_eq!(view.ids, [2, 1], "book 3 was never read");
    assert_eq!(view.list.items().len(), 2);
}

#[test]
fn without_books_the_screen_says_so_in_both_languages() {
    for (language, said) in [
        ("en", "Nothing recorded yet"),
        ("de", "Noch nichts aufgezeichnet"),
    ] {
        let data = Quiet(MemoryStorage::new());
        let view = BooksView::build(&mut words(language), &data).unwrap();
        assert!(view.ids.is_empty());
        assert!(view.empty.starts_with(said), "{language}: {}", view.empty);
    }
}

#[test]
fn a_books_view_has_its_rows_and_the_last_ten_days() {
    let data = card();
    let view = BookView::build(&mut words("en"), &data, Some(1)).unwrap();
    assert_eq!(view.title, "Anna Karenina");
    let rows = view.rows.as_ref().unwrap();
    let names: Vec<&str> = rows.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names[0], "Time read");
    assert!(rows.iter().any(|(name, _)| name == "Pages read"));
    let chart = view.chart.as_ref().unwrap();
    assert_eq!(chart.values.len(), 10, "the last ten of twelve days");
    assert_eq!(chart.values[9], 720, "the last day: 12 minutes");
    assert_eq!(chart.values[0], 180, "the third day: 3 minutes");
    assert_eq!(chart.dates.len(), 10);
    assert_eq!(chart.top, "12 min");
    assert_eq!(
        chart.dates[9], "10/15",
        "month and day as the catalog writes them"
    );
}

#[test]
fn a_book_read_once_charts_one_day_and_a_book_never_read_none() {
    let data = card();
    let one = BookView::build(&mut words("de"), &data, Some(2)).unwrap();
    assert_eq!(one.chart.as_ref().unwrap().values, [41]);
    let none = BookView::build(&mut words("de"), &data, Some(3)).unwrap();
    assert!(none.chart.is_none());
    assert!(
        none.rows.is_some(),
        "the numbers of a book never read are zeros"
    );
}

#[test]
fn an_unknown_book_is_a_view_that_says_so() {
    let data = card();
    for id in [None, Some(9)] {
        let view = BookView::build(&mut words("en"), &data, id).unwrap();
        assert!(view.rows.is_none() && view.chart.is_none());
        assert!(view.none.starts_with("Nothing recorded yet"));
    }
}

#[test]
fn a_books_file_that_cannot_be_read_is_an_error_not_an_empty_list() {
    let storage = MemoryStorage::new();
    storage
        .write(".paperpaw/plugins/stats/books.ini", &[0xff, 0xfe, b'\n'])
        .unwrap();
    let data = Quiet(storage);
    assert!(matches!(
        BooksView::build(&mut words("en"), &data),
        Err(Error::Io | Error::NoMemory | Error::NotFound)
    ));
    assert!(BookView::build(&mut words("en"), &data, Some(1)).is_err());
}

#[test]
fn the_views_refuse_cleanly() {
    let data = card();
    let catalog = RefCell::new(fixture(|| words("en")));
    sweep(&|| BooksView::build(&mut QuietWords(&catalog), &data).map(|view| view.ids.len()));
    sweep(&|| {
        BookView::build(&mut QuietWords(&catalog), &data, Some(1))
            .map(|view| view.rows.map_or(0, |rows| rows.len()))
    });
    sweep(&|| {
        BookView::build(&mut QuietWords(&catalog), &data, Some(77)).map(|view| view.title.len())
    });
}

#[test]
fn building_the_books_screen_peaks_the_same_for_a_small_and_a_large_library() {
    use crate::testing::heap::peak;
    use crate::testing::library;
    let small = fixture(|| library(300));
    let large = fixture(|| library(3000));
    let mut en = fixture(|| words("en"));
    let (view, peak_small) = peak(|| BooksView::build(&mut en, &small).unwrap());
    assert_eq!(view.ids.len(), 200);
    drop(view);
    let (view, peak_large) = peak(|| BooksView::build(&mut en, &large).unwrap());
    assert_eq!(view.ids.len(), 200);
    assert_eq!(view.ids.first(), Some(&3000), "the last opened first");
    // The same 200 rows, the rows and their items, a few strings more for the book read.
    assert!(
        peak_large <= peak_small + 4096,
        "{peak_small} bytes for 300 books, {peak_large} for 3,000"
    );
    // 200 rows twice over (the books read, then the screen's items) and the set of ids.
    assert!(peak_large < 112 * 1024, "{peak_large}");
}

#[test]
fn a_damaged_books_file_shows_the_same_error_as_before_streaming() {
    let data = card();
    data.0
        .write(store::BOOKS, b"[book.1]\ntitle = \"One\"\n[book.2\n")
        .unwrap();
    for result in [
        BooksView::build(&mut words("en"), &data).map(|_| ()),
        BookView::build(&mut words("en"), &data, Some(1)).map(|_| ()),
    ] {
        assert_eq!(result, Err(Error::Io));
    }
}
