use super::*;
use crate::testing::heap::{fixture, peak};
use alloc::string::ToString;
use alloc::vec;
use paperpaw_sdk::Storage;

fn book(id: u16, finished: i64, last_open: i64) -> Book {
    Book {
        id,
        title: alloc::format!("Book {id}"),
        path: alloc::format!("books/{id}.epub"),
        finished,
        last_open,
        ..Book::default()
    }
}

/// A "year" of 1000 seconds, for short numbers (the tally reads the real
/// calendar, so the tests go through `Tally` with the real year of a moment).
fn moment(year: i32, second: i64) -> i64 {
    // The first day of `year`, in Unix seconds, plus `second`.
    let days = (0..60_000)
        .find(|&d| date_of(d).year == year)
        .map_or(0, i64::from);
    days * 86_400 + second
}

fn tally_of(year: i32, books: &[Book]) -> Tally {
    let mut tally = Tally::new(year);
    for book in books {
        tally.add(book).unwrap();
    }
    tally
}

#[test]
fn the_plan_spreads_the_goal_over_the_year() {
    // 12 books: one a month, so by mid-October nine.
    let (ordinal, length) = day_of_year(20_740);
    assert_eq!((ordinal, length), (287, 365));
    assert_eq!(plan(9, 12, ordinal, length), 0);
    assert_eq!(plan(3, 12, ordinal, length), -6);
    assert_eq!(plan(11, 12, ordinal, length), 2);
    // On the first day nothing is expected yet.
    assert_eq!(plan(0, 52, 1, 365), 0);
    // A year of no days and the largest numbers do not overflow.
    assert_eq!(plan(u32::MAX, u32::MAX, u32::MAX, 0), 0);
    assert!(plan(0, 52, 400, 365) < 0);
}

#[test]
fn the_day_of_the_year_counts_a_leap_day() {
    // 29 February counts in a leap year.
    let leap_day = (19_000..22_000)
        .find(|&d| {
            let t = date_of(d);
            (t.year, t.month, t.day) == (2028, 3, 1)
        })
        .unwrap_or(0);
    assert_eq!(day_of_year(leap_day), (61, 366));
    let first = (19_000..22_000)
        .find(|&d| {
            let t = date_of(d);
            (t.year, t.month, t.day) == (2026, 1, 1)
        })
        .unwrap_or(0);
    assert_eq!(day_of_year(first), (1, 365));
    assert_eq!(day_of_year(first + 364), (365, 365));
    assert!(leap(2000) && leap(2024) && !leap(1900) && !leap(2026));
}

#[test]
fn the_year_of_a_moment_is_clamped() {
    assert_eq!(year_of(-5), 1970);
    assert_eq!(year_of(0), 1970);
    assert_eq!(year_of(i64::MAX), year_of(i64::from(u32::MAX)));
    assert_eq!(year_of(moment(2026, 100)), 2026);
    assert_eq!(year_of(moment(2026, 0) - 1), 2025);
}

#[test]
fn books_count_in_the_year_they_were_finished() {
    let (this, before) = (2026, 2025);
    let books = [
        book(1, moment(this, 100), moment(this, 100)),
        book(2, moment(this, 900), moment(this, 950)),
        // Last year's.
        book(3, moment(before, 500), moment(this, 500)),
        // Without a date: the library has it read to the end.
        book(4, 0, moment(this, 950)),
        // Without a date, not read to the end.
        book(5, 0, moment(this, 990)),
        // Without a date, opened last year.
        book(6, 0, moment(before, 990)),
    ];
    let read = |path: &str| Ok(path == "books/4.epub" || path == "books/6.epub");
    let (count, last) = tally_of(this, &books).finish(read).unwrap();
    // Books 1, 2 and (from the library) 4; 6 was opened last year.
    assert_eq!(count, 3);
    // Book 4 was last opened after book 2 was finished.
    assert_eq!(last.as_deref(), Some("Book 4"));
    let (count, last) = tally_of(before, &books).finish(read).unwrap();
    assert_eq!((count, last.as_deref()), (2, Some("Book 6")));
    assert_eq!(tally_of(2030, &books).finish(read).unwrap().0, 0);
}

#[test]
fn the_library_is_asked_about_books_without_a_date_only() {
    let this = 2026;
    let books = [
        book(1, moment(this, 5), moment(this, 5)),
        book(2, 0, moment(this, 9)),
        // Never opened, or without a path: nothing to look up.
        book(3, 0, 0),
        Book {
            path: String::new(),
            ..book(4, 0, moment(this, 9))
        },
    ];
    let mut asked = Vec::new();
    let (count, _) = tally_of(this, &books)
        .finish(|path| {
            asked.push(path.to_string());
            Ok(false)
        })
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(asked, vec!["books/2.epub".to_string()]);
}

#[test]
fn the_last_book_of_a_tie_is_the_smaller_id_where_dated_and_the_first_asked_otherwise() {
    let this = 2026;
    let at = moment(this, 77);
    let dated = [book(9, at, at), book(3, at, at), book(5, at, at)];
    let (_, last) = tally_of(this, &dated).finish(|_| Ok(false)).unwrap();
    assert_eq!(last.as_deref(), Some("Book 3"));
    // An undated book that ties a dated one does not replace it.
    let mixed = [book(8, at, at), book(2, 0, at)];
    let (count, last) = tally_of(this, &mixed).finish(|_| Ok(true)).unwrap();
    assert_eq!((count, last.as_deref()), (2, Some("Book 8")));
}

#[test]
fn only_the_books_opened_last_are_looked_up_and_at_most_the_most() {
    let this = 2026;
    // 40 books without a date, opened in the order of their ids; fed in a
    // scrambled order.
    let mut books: Vec<Book> = (1..=40u16)
        .map(|id| book(id, 0, moment(this, i64::from(id) * 10)))
        .collect();
    books.reverse();
    let half = books.len() / 2;
    books.rotate_left(half);
    let mut asked = Vec::new();
    let (count, last) = tally_of(this, &books)
        .finish(|path| {
            asked.push(path.to_string());
            Ok(true)
        })
        .unwrap();
    assert_eq!(asked.len(), LIBRARY_LOOKUPS);
    // The newest first: ids 40 down to 17.
    assert_eq!(asked.first().map(String::as_str), Some("books/40.epub"));
    assert_eq!(asked.last().map(String::as_str), Some("books/17.epub"));
    assert_eq!(count, 24);
    assert_eq!(last.as_deref(), Some("Book 40"));
}

#[test]
fn a_title_is_cut_to_80_bytes_at_a_character() {
    let this = 2026;
    let long = Book {
        title: "é".repeat(60),
        ..book(1, moment(this, 1), moment(this, 1))
    };
    let (_, last) = tally_of(this, &[long]).finish(|_| Ok(false)).unwrap();
    let last = last.unwrap();
    assert!(last.ends_with('…'));
    assert!(last.len() <= 80 + '…'.len_utf8());
    assert_eq!(last.chars().filter(|&c| c == 'é').count(), 40);
}

#[test]
fn a_book_the_library_cannot_answer_for_is_not_counted_but_a_refused_allocation_is_an_error() {
    let this = 2026;
    let books = [book(1, 0, moment(this, 1))];
    let (count, _) = tally_of(this, &books).finish(|_| Ok(false)).unwrap();
    assert_eq!(count, 0);
    assert_eq!(
        tally_of(this, &books)
            .finish(|_| Err(Error::NoMemory))
            .err(),
        Some(Error::NoMemory)
    );
}

#[test]
fn the_dots_take_the_rows_that_make_them_largest() {
    // 12 dots in 120 by 20: one row of 10 each.
    assert_eq!(dot_rows(12, (120, 20)), (1, 10));
    // In 40 by 60 the dots go in rows.
    let (rows, size) = dot_rows(12, (40, 60));
    assert!(rows > 1 && size >= 10, "{rows} {size}");
    // No room at all.
    assert_eq!(dot_rows(12, (0, 0)).1, 0);
    // More than the most are drawn as the most.
    assert_eq!(dot_rows(500, (600, 20)), dot_rows(MOST_DOTS, (600, 20)));
}

/// A library file of `n` books, read as the widget reads it.
fn file_of(n: u16, this: i32) -> paperpaw_sdk::MemoryStorage {
    let storage = paperpaw_sdk::MemoryStorage::new();
    let books: Vec<Book> = (1..=n)
        .map(|id| {
            // Every third finished this year, every third without a date.
            let (finished, open) = match id % 3 {
                0 => (moment(this, i64::from(id)), moment(this, i64::from(id))),
                1 => (0, moment(this, i64::from(id))),
                _ => (moment(this - 1, i64::from(id)), moment(this - 1, 1)),
            };
            Book {
                title: alloc::format!("{id:0>100}"),
                ..book(id, finished, open)
            }
        })
        .collect();
    store::save_books(&storage, &books).unwrap();
    storage
}

#[test]
fn the_file_is_streamed_and_what_is_held_does_not_grow_with_the_library() {
    let this = 2026;
    let read = |storage: &paperpaw_sdk::MemoryStorage| {
        let mut tally = Tally::new(this);
        store::each_book(storage, |book| {
            tally.add(&book).map_err(|_| StorageError::TooLarge)
        })
        .unwrap();
        tally.finish(|_| Ok(false)).unwrap()
    };
    let small = fixture(|| file_of(300, this));
    let large = fixture(|| file_of(3000, this));
    let ((few, last_few), at_300) = peak(|| read(&small));
    let ((many, last_many), at_3000) = peak(|| read(&large));
    assert_eq!((few, many), (100, 1000));
    // The last finished: the highest multiple of 3.
    assert_eq!(
        last_few,
        Some(cut(&alloc::format!("{:0>100}", 300)).unwrap())
    );
    assert_eq!(
        last_many,
        Some(cut(&alloc::format!("{:0>100}", 3000)).unwrap())
    );
    // The tally keeps one title and at most 24 books to look up, whatever the
    // size of the file (the file's 8 KiB set of ids is the same).
    assert!(at_3000 < at_300 + 4 * 1024, "{at_300} {at_3000}");
    assert!(at_3000 < 40 * 1024, "{at_3000}");
}

#[test]
fn a_file_that_cannot_be_read_whole_is_no_books() {
    let this = 2026;
    let storage = paperpaw_sdk::MemoryStorage::new();
    // A section header that is not closed.
    storage
        .write(store::BOOKS, b"[book.1\ntitle = \"x\"\nfinished = 5\n")
        .unwrap();
    let mut tally = Tally::new(this);
    let read = store::each_book(&storage, |book| {
        tally.add(&book).map_err(|_| StorageError::TooLarge)
    });
    assert!(matches!(read, Err(StorageError::InvalidData)));
}
