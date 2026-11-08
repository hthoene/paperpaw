//! The statistics' files as they may be found on a card: `pages.bin`
//! cut off in a record, with stays at the edges of time, of no length or
//! of the longest; `days.bin` broken or stale; `books.ini` with odd
//! sections; and round trips of both files over arbitrary values.

#![allow(
    clippy::let_underscore_must_use,
    reason = "tests read what they check; what they leave out does not matter"
)]

extern crate std;

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{MemoryStorage, Storage};
use proptest::prelude::*;

use super::*;

/// `days` for tests: the memory of a test is not refused.
fn days(storage: &dyn Storage, day_of: impl Fn(u32) -> i32) -> Vec<Day> {
    super::days(storage, day_of).unwrap()
}

/// A day of 100 seconds, for short numbers.
fn short_days(unix: u32) -> i32 {
    (unix / 100) as i32
}

fn stay(book: u16, page: u32, start: u32, duration: u16) -> PageStat {
    PageStat {
        book,
        page,
        start,
        duration,
        pages: 300,
    }
}

fn logged(storage: &MemoryStorage) -> Vec<PageStat> {
    let mut out = Vec::new();
    let _ = scan(storage, |s| out.push(*s));
    out
}

fn recent(storage: &MemoryStorage, since: u32) -> Vec<PageStat> {
    let mut out = Vec::new();
    scan_recent(storage, since, |s| out.push(*s));
    out
}

/// A log of `stays` and `tail` bytes cut off after them.
fn log_with_tail(stays: &[PageStat], tail: &[u8]) -> MemoryStorage {
    let storage = MemoryStorage::new();
    let mut data = Vec::new();
    for s in stays {
        data.extend_from_slice(&s.encode());
    }
    data.extend_from_slice(tail);
    storage.write(PAGES, &data).unwrap();
    storage
}

#[test]
fn a_record_cut_off_at_the_end_is_left_out_everywhere() {
    let stays = [
        stay(1, 1, 1000, 30),
        stay(1, 2, 1030, 40),
        stay(2, 7, 1100, 20),
    ];
    for cut in 1..RECORD {
        let partial = stay(9, 9, 1150, 50).encode();
        let storage = log_with_tail(&stays, &partial[..cut]);
        assert_eq!(logged(&storage), stays, "cut after {cut} bytes");
        assert_eq!(
            recent(&storage, 0),
            stays.iter().rev().copied().collect::<Vec<_>>(),
            "cut after {cut} bytes"
        );
        let days = days(&storage, short_days);
        assert_eq!(
            days.iter()
                .map(|d| (d.day, d.seconds, d.stays))
                .collect::<Vec<_>>(),
            [(10, 70, 2), (11, 20, 1)],
            "cut after {cut} bytes"
        );
    }
}

#[test]
fn a_log_shorter_than_a_record_has_no_stays() {
    for len in 0..RECORD {
        let storage = log_with_tail(&[], &[0xAB; RECORD][..len]);
        assert!(logged(&storage).is_empty());
        assert!(recent(&storage, 0).is_empty());
        assert!(days(&storage, short_days).is_empty());
    }
}

#[test]
fn no_log_at_all_has_no_stays() {
    let storage = MemoryStorage::new();
    assert!(logged(&storage).is_empty());
    assert!(recent(&storage, 0).is_empty());
    assert!(days(&storage, short_days).is_empty());
    assert_eq!(sizes(&storage), (0, 0));
    assert!(books(&storage).is_empty());
}

/// A stay appended after a record that was cut off (e.g. by power lost
/// while writing, or a full card) is read as it was written: the cut
/// record goes first, as the log is read in records of 16 bytes from its
/// start and every stay after it would be read shifted.
#[test]
fn stays_after_a_record_cut_off_are_read_as_written() {
    let first = stay(1, 1, 1000, 30);
    let storage = log_with_tail(&[first], &stay(1, 2, 1030, 40).encode()[..7]);
    let later = stay(1, 3, 1100, 25);
    append(&storage, later).unwrap();
    assert_eq!(logged(&storage), [first, later]);
}

#[test]
fn stays_at_the_edges_of_time_are_counted_on_their_days() {
    let storage = MemoryStorage::new();
    // The first second of 1970, and the last a u32 holds (2106).
    append(&storage, stay(1, 1, 0, 30)).unwrap();
    append(&storage, stay(1, 2, u32::MAX, 40)).unwrap();
    let day_of = |unix: u32| (i64::from(unix) / 86_400) as i32;
    let days = days(&storage, day_of);
    assert_eq!(
        days.iter().map(|d| (d.day, d.seconds)).collect::<Vec<_>>(),
        [(0, 30), (49_710, 40)]
    );
    // From the end: only what started since.
    let since = u32::MAX - 10;
    assert_eq!(recent(&storage, since), [stay(1, 2, u32::MAX, 40)]);
    assert_eq!(recent(&storage, 0).len(), 2);
}

#[test]
fn stays_of_no_length_count_as_stays_without_time() {
    let storage = MemoryStorage::new();
    append(&storage, stay(1, 1, 1000, 0)).unwrap();
    append(&storage, stay(1, 2, 1010, 0)).unwrap();
    let days = days(&storage, short_days);
    assert_eq!(
        days,
        [Day {
            day: 10,
            seconds: 0,
            stays: 2
        }]
    );
}

#[test]
fn the_longest_stays_are_summed_without_loss() {
    let storage = MemoryStorage::new();
    // A thousand stays of the longest a record holds, on one day.
    for page in 0..1000 {
        append(&storage, stay(1, page + 1, 1000, u16::MAX)).unwrap();
    }
    let days = days(&storage, short_days);
    assert_eq!(days.len(), 1);
    assert_eq!(days[0].seconds, 1000 * u32::from(u16::MAX));
    assert_eq!(days[0].stays, 1000);
}

/// More than 65 537 stays of the longest a record holds on one day sum
/// to more than a u32 holds; a log merged from a broken or hostile
/// database can have them. Summing saturates instead of overflowing (a
/// panic in debug builds, a wrong total in release builds).
#[test]
fn a_day_of_very_many_long_stays_does_not_overflow() {
    let storage = MemoryStorage::new();
    let mut data = Vec::with_capacity(65_540 * RECORD);
    for page in 0..65_540u32 {
        data.extend_from_slice(&stay(1, page + 1, 1000, u16::MAX).encode());
    }
    storage.write(PAGES, &data).unwrap();
    let days = days(&storage, short_days);
    assert_eq!(days.len(), 1);
    assert!(days[0].seconds >= u32::MAX - u32::from(u16::MAX));
}

#[test]
fn a_broken_days_file_is_summed_anew_from_the_log() {
    let stays = [stay(1, 1, 1000, 30), stay(1, 2, 1100, 40)];
    let expected = [(10, 30, 1), (11, 40, 1)];
    let as_tuples = |days: Vec<Day>| {
        days.iter()
            .map(|d| (d.day, d.seconds, d.stays))
            .collect::<Vec<_>>()
    };
    // Shorter than its header.
    let storage = log_with_tail(&stays, &[]);
    storage.write(DAYS, &[1, 2, 3]).unwrap();
    assert_eq!(as_tuples(days(&storage, short_days)), expected);
    // Covering more than the log has (the log was replaced).
    let mut stale = Vec::new();
    stale.extend_from_slice(&10_000u64.to_le_bytes());
    stale.extend_from_slice(
        &Day {
            day: 3,
            seconds: 999,
            stays: 9,
        }
        .encode(),
    );
    storage.write(DAYS, &stale).unwrap();
    assert_eq!(as_tuples(days(&storage, short_days)), expected);
    // Its days cut off in a record: the whole ones kept.
    let mut cut = Vec::new();
    cut.extend_from_slice(&0u64.to_le_bytes());
    cut.extend_from_slice(&[7; 5]);
    storage.write(DAYS, &cut).unwrap();
    assert_eq!(as_tuples(days(&storage, short_days)), expected);
    // And brought up to date, it is read as written.
    assert_eq!(as_tuples(days(&storage, short_days)), expected);
}

#[test]
fn days_stay_in_order_whatever_order_the_stays_come_in() {
    let storage = MemoryStorage::new();
    for start in [5000u32, 100, 3000, 100, 9000, 0] {
        append(&storage, stay(1, 1, start, 10)).unwrap();
    }
    let days = days(&storage, short_days);
    let order: Vec<i32> = days.iter().map(|d| d.day).collect();
    assert_eq!(order, [0, 1, 30, 50, 90]);
    assert_eq!(days[1].stays, 2);
}

#[test]
fn odd_sections_of_books_ini_are_left_out() {
    let text = "[book.2]\ntitle = Two\n\
        [book.x]\ntitle = Not a number\n\
        [book.70000]\ntitle = Too large\n\
        [book.-1]\ntitle = Negative\n\
        [other]\ntitle = Not a book\n\
        [book.1]\ntitle = One\npages = many\nlast-open = soon\nfinished = -5\nunknown = kept out\n\
        [book.2]\ntitle = Two again\n";
    let storage = MemoryStorage::new().with(BOOKS, text);
    let books = books(&storage);
    assert_eq!(
        books
            .iter()
            .map(|b| (b.id, b.title.as_str()))
            .collect::<Vec<_>>(),
        [(1, "One"), (2, "Two")]
    );
    // Numbers that are none read as 0, negative ones as they are.
    assert_eq!((books[0].pages, books[0].last_open), (0, 0));
    assert_eq!(books[0].finished, -5);
}

#[test]
fn books_ini_that_is_no_text_has_no_books() {
    let storage = MemoryStorage::new().with(
        BOOKS,
        alloc::vec![0xFF, 0xFE, b'[', b'b', 0xC3, 0x28, b'\n'],
    );
    assert!(books(&storage).is_empty());
}

#[test]
fn books_of_version_1_are_written_back_as_read() {
    let storage = MemoryStorage::new().with(BOOKS, include_str!("../../testdata/books.ini.v1"));
    let v1 = books(&storage);
    assert_eq!(v1.len(), 2);
    // Without `finished`, which version 1 did not have.
    assert!(v1.iter().all(|b| b.finished == 0));
    save_books(&storage, &v1).unwrap();
    assert_eq!(books(&storage), v1);
}

#[test]
fn the_log_of_version_1_is_appended_to_as_written() {
    let storage = MemoryStorage::new().with(
        PAGES,
        include_bytes!("../../testdata/pages.bin.v1").to_vec(),
    );
    let new = stay(1, 6, 1_760_000_100, 12);
    append(&storage, new).unwrap();
    let all = logged(&storage);
    assert_eq!(all.len(), 3);
    assert_eq!(all[2], new);
    assert_eq!(recent(&storage, 1_760_000_050)[..2], [new, all[1]]);
}

/// Text as books.ini keeps it: anything but a carriage return (dropped
/// when quoted, as the config format does).
fn text() -> impl Strategy<Value = String> {
    proptest::string::string_regex("[^\r]{0,24}").unwrap()
}

fn any_book() -> impl Strategy<Value = Book> {
    (
        (text(), text(), "[0-9a-f]{0,32}", any::<u32>(), text()),
        (text(), any::<i64>(), text(), any::<i64>()),
    )
        .prop_map(
            |((title, authors, md5, pages, language), (series, last_open, path, finished))| Book {
                id: 0,
                title,
                authors,
                md5,
                pages,
                language,
                series,
                last_open,
                path,
                finished,
            },
        )
}

fn any_stay() -> impl Strategy<Value = PageStat> {
    (
        any::<u16>(),
        any::<u32>(),
        any::<u32>(),
        any::<u16>(),
        any::<u32>(),
    )
        .prop_map(|(book, page, start, duration, pages)| PageStat {
            book,
            page,
            start,
            duration,
            pages,
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]

    #[test]
    fn books_survive_books_ini(mut books_in in proptest::collection::vec(any_book(), 0..8)) {
        for (n, book) in books_in.iter_mut().enumerate() {
            book.id = n as u16 + 1;
        }
        let storage = MemoryStorage::new();
        save_books(&storage, &books_in).unwrap();
        prop_assert_eq!(books(&storage), books_in);
    }

    #[test]
    fn stays_survive_the_log(stays in proptest::collection::vec(any_stay(), 0..200)) {
        let storage = MemoryStorage::new();
        for s in &stays {
            append(&storage, *s).unwrap();
        }
        prop_assert_eq!(logged(&storage), stays.clone());
        let mut newest_first = stays.clone();
        newest_first.reverse();
        prop_assert_eq!(recent(&storage, 0), newest_first);
        // The days add up to the stays.
        let days = days(&storage, |unix| (unix / 86_400) as i32);
        let seconds: u64 = days.iter().map(|d| u64::from(d.seconds)).sum();
        let expected: u64 = stays.iter().map(|s| u64::from(s.duration)).sum();
        prop_assert_eq!(seconds, expected);
        prop_assert_eq!(days.iter().map(|d| d.stays as usize).sum::<usize>(), stays.len());
        prop_assert!(days.windows(2).all(|w| w[0].day < w[1].day));
    }

    #[test]
    fn any_bytes_as_files_do_not_panic(
        log in proptest::collection::vec(any::<u8>(), 0..400),
        cache in proptest::collection::vec(any::<u8>(), 0..64),
        ini in proptest::collection::vec(any::<u8>(), 0..300),
        since in any::<u32>(),
    ) {
        let storage = MemoryStorage::new()
            .with(PAGES, log.clone())
            .with(BOOKS, ini)
            .with(DAYS, cache);
        let mut count = 0usize;
        let _ = scan(&storage, |_| count += 1);
        prop_assert_eq!(count, log.len() / RECORD);
        scan_recent(&storage, since, |s| assert!(s.start >= since));
        let _ = days(&storage, |unix| (unix / 86_400) as i32);
        let _ = books(&storage);
    }
}
