use super::*;
use crate::testing::Faulty;
use alloc::format;

/// `days` for tests: the memory of a test is not refused.
fn days(storage: &dyn Storage, day_of: impl Fn(u32) -> i32) -> Vec<Day> {
    super::days(storage, day_of).unwrap()
}
use paperpaw_sdk::MemoryStorage;

/// The files as version 1 wrote them stay readable (see
/// `docs/data-files.md`).
#[test]
fn files_of_version_1_are_read() {
    let storage = MemoryStorage::new()
        .with(BOOKS, include_str!("../../testdata/books.ini.v1"))
        .with(
            PAGES,
            include_bytes!("../../testdata/pages.bin.v1").to_vec(),
        );
    let books = load_books(&storage).unwrap();
    assert_eq!(books.len(), 2);
    assert_eq!(books[0].title, "Anna Karenina");
    assert_eq!(books[0].authors, "Leo Tolstoy\nRosamund Bartlett");
    assert_eq!(books[0].md5, "0f343b0931126a20f133d67c2b018a3b");
    assert_eq!((books[0].pages, books[0].last_open), (864, 1_760_000_000));
    assert_eq!(books[0].path, "books/Anna Karenina.epub");
    assert_eq!(books[1].title, "Short \"stories\"");
    let mut stays = Vec::new();
    scan(&storage, |s| stays.push(*s)).unwrap();
    assert_eq!(
        stays,
        [
            PageStat {
                book: 1,
                page: 5,
                start: 1_760_000_000,
                duration: 42,
                pages: 864
            },
            PageStat {
                book: 2,
                page: 1,
                start: 1_760_003_600,
                duration: 65000,
                pages: 12
            },
        ]
    );
}

fn stat(book: u16, page: u32, start: u32, duration: u16) -> PageStat {
    PageStat {
        book,
        page,
        start,
        duration,
        pages: 300,
    }
}

#[test]
fn books_survive_the_file() {
    let storage = MemoryStorage::new();
    let books_in = [
        Book {
            id: 1,
            title: String::from("Fjord \"1\""),
            authors: String::from("A. Author\nB. Author"),
            md5: String::from("abc"),
            pages: 614,
            language: String::from("no"),
            series: String::from("N/A"),
            last_open: 1_760_000_000,
            path: String::from("books/d.epub"),
            finished: 1_760_100_000,
        },
        Book {
            id: 2,
            title: String::from("Zwei"),
            ..Book::default()
        },
    ];
    save_books(&storage, &books_in).unwrap();
    assert_eq!(load_books(&storage).unwrap(), books_in);
    // Read in pieces: a long list of books.
    let many: Vec<Book> = (1..=300)
        .map(|id| Book {
            id,
            title: format!("Book {id} with a title of some length"),
            ..Book::default()
        })
        .collect();
    save_books(&storage, &many).unwrap();
    assert_eq!(load_books(&storage).unwrap(), many);
}

/// A `books.ini` that cannot be read whole is an error, and changes
/// leave it as it is; only a missing one is empty.
#[test]
fn unreadable_books_are_never_saved_over() {
    let one = "[book.1]\ntitle = Eins\nmd5 = aaa\n\n[book.2]\ntitle = Zwei\n";
    let add = |books: &mut Vec<Book>| {
        books.push(Book {
            id: 3,
            ..Book::default()
        });
        true
    };
    let cases: [(&str, &[u8]); 3] = [
        // Saved as Latin-1 on a computer.
        ("ANSI", b"[book.1]\ntitle = M\xfcller\n"),
        ("not an entry", b"[book.1]\ntitle = Eins\nsomething odd\n"),
        ("cut header", b"[book.1\ntitle = Eins\n"),
    ];
    for (what, data) in cases {
        let storage = MemoryStorage::new().with(BOOKS, data.to_vec());
        assert!(load_books(&storage).is_err(), "{what}");
        assert!(update_books(&storage, add).is_err(), "{what}");
        assert_eq!(storage.get(BOOKS).unwrap(), data, "{what}");
    }
    // A read error halfway.
    let storage = Faulty::new(MemoryStorage::new().with(BOOKS, one));
    storage.break_at(BOOKS, 20);
    assert_eq!(load_books(&storage), Err(StorageError::Io));
    assert_eq!(update_books(&storage, add), Err(StorageError::Io));
    assert_eq!(storage.inner.get(BOOKS).unwrap(), one.as_bytes());
    assert!(books(&storage).is_empty());
    // None yet.
    let storage = MemoryStorage::new();
    assert_eq!(update_books(&storage, add), Ok(true));
    assert_eq!(load_books(&storage).unwrap().len(), 1);
}

/// A card that is full: the error comes back, nothing is lost quietly.
#[test]
fn write_errors_are_returned() {
    let storage = Faulty::new(MemoryStorage::new());
    storage.fill_after(0);
    assert_eq!(
        append(&storage, stat(1, 1, 100, 10)),
        Err(StorageError::Full)
    );
    assert_eq!(save_books(&storage, &[]), Err(StorageError::Full));
    assert_eq!(update_books(&storage, |_| true), Err(StorageError::Full));
}

#[test]
fn stays_are_logged_and_summed_per_day() {
    let storage = MemoryStorage::new();
    // A day of 100 seconds, for short numbers.
    let day_of = |unix: u32| (unix / 100) as i32;
    append(&storage, stat(1, 1, 1000, 30)).unwrap();
    append(&storage, stat(1, 2, 1030, 40)).unwrap();
    append(&storage, stat(2, 7, 1100, 20)).unwrap();
    assert_eq!(
        days(&storage, day_of),
        [
            Day {
                day: 10,
                seconds: 70,
                stays: 2
            },
            Day {
                day: 11,
                seconds: 20,
                stays: 1
            },
        ]
    );
    // Later stays are added to what was summed, also earlier days.
    append(&storage, stat(1, 3, 950, 10)).unwrap();
    append(&storage, stat(1, 4, 1150, 5)).unwrap();
    assert_eq!(
        days(&storage, day_of),
        [
            Day {
                day: 9,
                seconds: 10,
                stays: 1
            },
            Day {
                day: 10,
                seconds: 70,
                stays: 2
            },
            Day {
                day: 11,
                seconds: 25,
                stays: 2
            },
        ]
    );
    // A replaced, shorter log is summed anew.
    storage.write(PAGES, &stat(3, 1, 1200, 9).encode()).unwrap();
    assert_eq!(
        days(&storage, day_of),
        [Day {
            day: 12,
            seconds: 9,
            stays: 1
        }]
    );
    let mut all = Vec::new();
    scan(&storage, |s| all.push(*s)).unwrap();
    assert_eq!(all, [stat(3, 1, 1200, 9)]);
}

/// The totals are summed anew in another time zone, and for a log
/// replaced by one as long or longer (e.g. when the device lost power
/// right after a merge swapped it in).
#[test]
fn day_totals_follow_the_zone_and_the_log() {
    let storage = MemoryStorage::new();
    let at = |hours: u32| 1_760_000_000 - 1_760_000_000 % 86_400 + hours * 3600;
    let in_zone =
        |offset: i64| move |unix: u32| (i64::from(unix) + offset).div_euclid(86_400) as i32;
    append(&storage, stat(1, 1, at(23), 60)).unwrap();
    let utc = days(&storage, in_zone(0));
    assert_eq!(utc.len(), 1);
    // Two hours ahead, 23:00 UTC is the next day.
    let berlin = days(&storage, in_zone(7200));
    assert_eq!(berlin[0].day, utc[0].day + 1);
    assert_eq!(days(&storage, in_zone(0)), utc);
    // Another log of the same length: its own days.
    storage
        .write(PAGES, &stat(2, 1, at(5), 30).encode())
        .unwrap();
    assert_eq!(
        days(&storage, in_zone(0)),
        [Day {
            day: utc[0].day,
            seconds: 30,
            stays: 1
        }]
    );
    // The totals of earlier versions are summed anew.
    let mut old = 16u64.to_le_bytes().to_vec();
    old.extend_from_slice(
        &Day {
            day: 1,
            seconds: 999,
            stays: 1,
        }
        .encode(),
    );
    storage.write(DAYS, &old).unwrap();
    assert_eq!(days(&storage, in_zone(0))[0].seconds, 30);
}

/// A merged log replaces this one; the day totals go with it.
#[test]
fn the_log_is_replaced() {
    let storage = MemoryStorage::new();
    append(&storage, stat(1, 1, 100, 10)).unwrap();
    let day_of = |unix: u32| (unix / 100) as i32;
    assert_eq!(days(&storage, day_of).len(), 1);
    let merged: Vec<u8> = [[2u32, 3, 150, 70_000, 200], [1, 1, 300, 20, 300]]
        .iter()
        .flat_map(|r| r.iter().flat_map(|v| v.to_be_bytes()))
        .collect();
    storage.write("merged", &merged).unwrap();
    replace_log(&storage, "merged").unwrap();
    let mut all = Vec::new();
    scan(&storage, |s| all.push(*s)).unwrap();
    assert_eq!(
        all,
        [
            PageStat {
                book: 2,
                page: 3,
                start: 150,
                duration: u16::MAX,
                pages: 200
            },
            stat(1, 1, 300, 20)
        ]
    );
    assert!(!storage.exists(PAGES_NEW));
    assert_eq!(days(&storage, day_of).len(), 2);
    // A card that fills up halfway: the log stays as it was.
    let storage = Faulty::new(MemoryStorage::new().with("merged", merged));
    append(&storage, stat(1, 1, 100, 10)).unwrap();
    storage.fill_after(1);
    assert_eq!(replace_log(&storage, "merged"), Err(StorageError::Full));
    let mut all = Vec::new();
    scan(&storage, |s| all.push(*s)).unwrap();
    assert_eq!(all, [stat(1, 1, 100, 10)]);
    assert!(!storage.exists(PAGES_NEW));
}

#[test]
fn a_read_error_in_the_log_is_returned() {
    let storage = Faulty::new(MemoryStorage::new());
    for i in 0..100 {
        append(&storage, stat(1, i + 1, 1000 + i * 10, 9)).unwrap();
    }
    storage.break_at(PAGES, 16 * 70);
    let mut count = 0;
    assert_eq!(scan(&storage, |_| count += 1), Err(StorageError::Io));
    assert_eq!(count, 64);
    // No log yet: no stays.
    assert_eq!(scan(&MemoryStorage::new(), |_| count += 1), Ok(()));
}

#[test]
fn recent_stays_come_from_the_end() {
    let storage = MemoryStorage::new();
    for (i, start) in [100u32, 130, 90_000].into_iter().enumerate() {
        append(&storage, stat(1, i as u32 + 1, start, 10)).unwrap();
    }
    let mut recent = Vec::new();
    scan_recent(&storage, 120, |s| recent.push(s.start));
    assert_eq!(recent, [90_000, 130]);
}

/// A stay that began long before but ended after one that began
/// recently (e.g. KOReader's, of hours on one page) does not end the
/// search early.
#[test]
fn recent_stays_come_after_a_long_one() {
    let storage = MemoryStorage::new();
    append(&storage, stat(1, 1, 10_000, 10)).unwrap();
    for page in 2..70 {
        append(&storage, stat(1, page, 1000, 9_500)).unwrap();
    }
    let mut recent = Vec::new();
    scan_recent(&storage, 10_000, |s| recent.push(s.page));
    assert_eq!(recent, [1]);
}

/// The stays of a time range, from a log of many days in the order
/// the stays ended.
#[test]
fn stays_of_a_range() {
    let storage = MemoryStorage::new();
    let mut stays = Vec::new();
    let mut expected = Vec::new();
    for day in 0..30u32 {
        for minute in 0..20u32 {
            let start = 1_000_000 + day * 86_400 + minute * 60;
            stays.push(stat(1, minute + 1, start, 50));
            if day == 12 {
                expected.push(start);
            }
        }
    }
    // A stay of 18 hours that began the day before ends on day 12.
    let long = 1_000_000 + 11 * 86_400 + 20 * 3600;
    stays.push(stat(2, 1, long, u16::MAX));
    // One that began on day 12 and ended on day 13.
    let late = 1_000_000 + 12 * 86_400 + 86_000;
    stays.push(stat(2, 2, late, 600));
    expected.push(late);
    stays.sort_by_key(|s| s.start + u32::from(s.duration));
    for s in stays {
        append(&storage, s).unwrap();
    }
    let (from, to) = (1_000_000 + 12 * 86_400, 1_000_000 + 13 * 86_400);
    let mut found = Vec::new();
    scan_range(&storage, from, to, |s| found.push(s.start));
    found.sort_unstable();
    expected.sort_unstable();
    assert_eq!(found, expected);
    // Nothing before the log, all of it around it.
    let mut none = 0;
    scan_range(&storage, 0, 1000, |_| none += 1);
    assert_eq!(none, 0);
    let mut all = 0;
    scan_range(&storage, 0, u32::MAX, |_| all += 1);
    assert_eq!(all, 30 * 20 + 2);
}

#[test]
fn long_logs_are_read_in_chunks() {
    let storage = MemoryStorage::new();
    for i in 0..200u32 {
        append(&storage, stat(1, i + 1, 1000 + i * 10, 9)).unwrap();
    }
    let mut count = 0;
    scan(&storage, |_| count += 1).unwrap();
    assert_eq!(count, 200);
    let mut recent = 0;
    scan_recent(&storage, 1000 + 150 * 10, |_| recent += 1);
    assert_eq!(recent, 50);
}

/// A rename that takes the log away and then fails (as FAT may): the
/// new log stays, as it is all there is, and is put in place at the
/// next start; also where a cut record was dropped.
#[test]
fn a_failed_rename_leaves_the_new_log() {
    let merged: Vec<u8> = [[1u32, 1, 100, 10, 300], [1, 2, 200, 20, 300]]
        .iter()
        .flat_map(|r| r.iter().flat_map(|v| v.to_be_bytes()))
        .collect();
    let storage = Faulty::new(MemoryStorage::new().with("merged", merged));
    append(&storage, stat(1, 1, 100, 10)).unwrap();
    storage.drop_target_on_rename();
    assert!(replace_log(&storage, "merged").is_err());
    assert!(storage.exists(PAGES) || storage.exists(PAGES_NEW));
    recover(&storage.inner);
    let mut all = Vec::new();
    scan(&storage, |s| all.push(*s)).unwrap();
    assert_eq!(all, [stat(1, 1, 100, 10), stat(1, 2, 200, 20)]);
    // A cut record dropped.
    let storage = Faulty::new(MemoryStorage::new());
    append(&storage, stat(1, 1, 100, 10)).unwrap();
    storage.inner.append(PAGES, &[1, 2, 3]).unwrap();
    storage.drop_target_on_rename();
    assert!(append(&storage, stat(1, 2, 200, 20)).is_err());
    assert!(storage.exists(PAGES) || storage.exists(PAGES_NEW));
    recover(&storage.inner);
    let mut all = Vec::new();
    scan(&storage, |s| all.push(*s)).unwrap();
    assert_eq!(all, [stat(1, 1, 100, 10)]);
    // The books, too.
    let storage = Faulty::new(MemoryStorage::new());
    let one = [Book {
        id: 1,
        title: String::from("Eins"),
        ..Book::default()
    }];
    save_books(&storage, &one).unwrap();
    storage.drop_target_on_rename();
    let two = [
        one[0].clone(),
        Book {
            id: 2,
            ..Book::default()
        },
    ];
    assert!(save_books(&storage, &two).is_err());
    recover(&storage.inner);
    assert_eq!(load_books(&storage.inner).unwrap(), two);
}

/// After a power cut between taking the old file away and putting the
/// new one in its place, the new one is put there; an unfinished new
/// one beside the old goes.
#[test]
fn files_left_beside_their_place_are_recovered() {
    let storage = MemoryStorage::new();
    let log = stat(1, 1, 100, 10).encode();
    storage.write(PAGES_NEW, &log).unwrap();
    storage
        .write(
            BOOKS_NEW,
            b"[book.1]
title = Eins
",
        )
        .unwrap();
    recover(&storage);
    assert_eq!(storage.get(PAGES).unwrap(), log);
    assert_eq!(load_books(&storage).unwrap()[0].title, "Eins");
    assert!(!storage.exists(PAGES_NEW));
    assert!(!storage.exists(BOOKS_NEW));
    // Unfinished, beside the old ones.
    storage.write(PAGES_NEW, &[1, 2]).unwrap();
    storage.write(BOOKS_NEW, b"[book.1").unwrap();
    recover(&storage);
    assert_eq!(storage.get(PAGES).unwrap(), log);
    assert_eq!(load_books(&storage).unwrap()[0].title, "Eins");
    assert!(!storage.exists(PAGES_NEW));
    assert!(!storage.exists(BOOKS_NEW));
    // A change to the books before the next start recovers them first,
    // rather than starting a new list.
    let storage = MemoryStorage::new();
    storage
        .write(
            BOOKS_NEW,
            b"[book.1]
title = Eins
",
        )
        .unwrap();
    let added = update_books(&storage, |books| {
        books.push(Book {
            id: 2,
            ..Book::default()
        });
        true
    });
    assert_eq!(added, Ok(true));
    assert_eq!(load_books(&storage).unwrap().len(), 2);
}

/// Reads that return less than asked for are no end of the log.
#[test]
fn short_reads_are_read_on() {
    let storage = Faulty::new(MemoryStorage::new());
    for i in 0..100u32 {
        append(&storage, stat(1, i + 1, 1000 + i * 10, 9)).unwrap();
    }
    storage.read_short(100);
    let mut count = 0;
    scan(&storage, |_| count += 1).unwrap();
    assert_eq!(count, 100);
    let merged: Vec<u8> = (0..100u32)
        .flat_map(|i| [1u32, i + 1, 2000 + i, 5, 300])
        .flat_map(u32::to_be_bytes)
        .collect();
    storage.inner.write("merged", &merged).unwrap();
    replace_log(&storage, "merged").unwrap();
    let mut count = 0;
    scan(&storage.inner, |_| count += 1).unwrap();
    assert_eq!(count, 100);
}

#[test]
fn kept_days_over_the_limit_are_summed_anew_not_refused() {
    let storage = MemoryStorage::new();
    append(&storage, stat(1, 1, 100, 10)).unwrap();
    let day_of = |unix: u32| (unix / 100) as i32;
    let first = {
        let mut first = [0u8; RECORD];
        storage.read_at(PAGES, 0, &mut first).unwrap();
        first
    };
    // A `days.bin` that claims the whole log is summed but holds too many days.
    let header = Summed {
        covered: RECORD as u64,
        zone: zone(&day_of),
        first,
    };
    let mut kept = header.encode().to_vec();
    for day in 0..=MOST_DAYS as i32 {
        kept.extend_from_slice(
            &Day {
                day,
                seconds: 99,
                stays: 1,
            }
            .encode(),
        );
    }
    storage.write(DAYS, &kept).unwrap();
    let summed = days(&storage, day_of);
    assert_eq!(
        summed,
        [Day {
            day: 1,
            seconds: 10,
            stays: 1
        }]
    );
}

/// Odd books: quotes, newlines, spaces, empty fields, negative times.
fn odd_books(count: u16) -> Vec<Book> {
    (1..=count)
        .map(|id| Book {
            id,
            title: alloc::format!("  \"Quoted\" title\nline two\ttab \\ {id} \u{e9}\u{1f600}"),
            authors: if id % 2 == 0 {
                "".into()
            } else {
                "A. Author\nB. Author".into()
            },
            md5: "0123456789abcdef0123456789abcdef".into(),
            pages: id.into(),
            language: "de".into(),
            series: " lead".into(),
            last_open: -5 * i64::from(id),
            path: alloc::format!("Books/{id}.epub"),
            finished: i64::from(id) * 1_000_000_000_000,
        })
        .collect()
}

/// A storage that takes at most 4 KiB a call, as the host does.
struct Limited(MemoryStorage);
impl Storage for Limited {
    fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(p, o, b)
    }
    fn size(&self, p: &str) -> Result<u64, StorageError> {
        self.0.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        assert!(d.len() <= 4096, "a write of {} bytes", d.len());
        self.0.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
        assert!(d.len() <= 4096, "an append of {} bytes", d.len());
        self.0.append(p, d)
    }
    fn list(
        &self,
        d: &str,
        v: &mut dyn FnMut(paperpaw_sdk::DirEntry<'_>),
    ) -> Result<(), StorageError> {
        self.0.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), StorageError> {
        self.0.remove(p)
    }
    fn rename(&self, a: &str, b: &str) -> Result<(), StorageError> {
        self.0.rename(a, b)
    }
}

#[test]
fn books_are_written_in_pieces_as_config_would_write_them() {
    let storage = Limited(MemoryStorage::new());
    for books in [odd_books(0), odd_books(1), odd_books(120)] {
        save_books(&storage, &books).unwrap();
        assert_eq!(load_books(&storage).unwrap(), books);
        // The text is what the whole-string writer made.
        let mut expected = String::new();
        for b in &books {
            expected.push_str(&alloc::format!(
                "[book.{}]\ntitle = {}\nauthors = {}\nmd5 = {}\npages = {}\nlanguage = {}\nseries = {}\nlast-open = {}\npath = {}\nfinished = {}\n\n",
                b.id,
                paperpaw_sdk::config::quote(&b.title),
                paperpaw_sdk::config::quote(&b.authors),
                b.md5,
                b.pages,
                paperpaw_sdk::config::quote(&b.language),
                paperpaw_sdk::config::quote(&b.series),
                b.last_open,
                paperpaw_sdk::config::quote(&b.path),
                b.finished,
            ));
        }
        let size = storage.size(BOOKS).unwrap() as usize;
        let mut got = alloc::vec![0u8; size];
        storage.read_at(BOOKS, 0, &mut got).unwrap();
        assert_eq!(core::str::from_utf8(&got).unwrap(), expected);
        assert!(!storage.exists(BOOKS_NEW));
    }
}

#[test]
fn sections_in_a_row_merge_a_later_one_of_an_id_read_before_is_ignored() {
    let storage = MemoryStorage::new().with(
        BOOKS,
        b"[book.7]\ntitle = First\n\n[book.3]\n\n[book.7]\npages = 9\n\n[book.2]\ntitle = Two\n\n[book.7]\ntitle = Late\npages = 1\n".to_vec(),
    );
    let books = load_books(&storage).unwrap();
    let seen: Vec<_> = books
        .iter()
        .map(|b| (b.id, b.title.as_str(), b.pages))
        .collect();
    // 7 and the empty 3 and 7 again merge; the last 7 comes after 2: ignored.
    assert_eq!(seen, [(2, "Two", 0), (7, "First", 9)]);
}
