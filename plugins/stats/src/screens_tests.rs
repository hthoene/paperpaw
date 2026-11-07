//! The rows of the books and book screens, in both languages, made over the
//! plugin's own catalogs; and when the heap refuses.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_i18n::{Arg as Message, Catalog, Translator};
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_ui_kit::Item;

use crate::store::Book;
use crate::testing::heap::{armed, fixture};
use crate::texts::{self, Words};
use crate::totals::BookTotals;
use crate::{book_list, book_rows};

/// The plugin's catalog in one language, as the host would resolve it.
struct Catalogued(Translator);

fn words(language: &str) -> Catalogued {
    let source = match language {
        "de" => include_str!("../i18n/de.ini"),
        _ => include_str!("../i18n/en.ini"),
    };
    let mut translator = Translator::new();
    translator.add(Catalog::parse(source).unwrap());
    translator.set_language(language);
    Catalogued(translator)
}

impl Words for Catalogued {
    fn copy(&mut self, key: &str) -> Result<String, Error> {
        Ok(self.0.tr(key).to_string())
    }
    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error> {
        let mut named: Vec<(&str, Message<'_>)> = Vec::new();
        let mut count = None;
        for arg in args {
            match *arg {
                Arg::Text { name, value } => named.push((name, Message::Str(value))),
                Arg::Int { name, value } => named.push((name, Message::Int(value))),
                Arg::Count(n) => count = Some(n),
                _ => {}
            }
        }
        Ok(match count {
            Some(n) => self.0.plural(key, n, &named),
            None => self.0.format(key, &named),
        })
    }
    fn digits(&mut self, text: &str) -> Result<String, Error> {
        Ok(self.0.localize_digits(text).to_string())
    }
}

/// Days as in UTC.
fn utc(unix: u32) -> i32 {
    (unix / 86_400) as i32
}

fn book(id: u16, title: &str, authors: &str, last_open: i64, pages: u32) -> Book {
    Book {
        id,
        title: title.to_string(),
        authors: authors.to_string(),
        pages,
        last_open,
        ..Book::default()
    }
}

/// 2025-10-14 12:00 UTC, a Tuesday.
const NOON: i64 = 1_760_443_200;

fn shelf() -> (Vec<Book>, Vec<u64>) {
    (
        alloc::vec![
            book(1, "Old", "N/A", NOON - 86_400 * 40, 100),
            book(2, "Unread", "Someone", NOON, 100),
            book(3, "New", "A\nB", NOON, 0),
            book(4, "Same time", "", NOON, 10),
        ],
        alloc::vec![3_600 + 5 * 60, 0, 59, 42 * 60],
    )
}

fn texts_of(items: &[Item]) -> Vec<(String, String, String)> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Row(row) => Some((row.title.clone(), row.value.clone(), row.note.clone())),
            Item::Heading(_) => None,
        })
        .collect()
}

#[test]
fn durations_read_as_the_catalog_writes_them() {
    let mut en = words("en");
    let say = |w: &mut Catalogued, s| texts::duration(w, s).unwrap();
    assert_eq!(say(&mut en, 0), "0 min");
    assert_eq!(say(&mut en, 59), "< 1 min");
    assert_eq!(say(&mut en, 42 * 60), "42 min");
    assert_eq!(say(&mut en, 3_600 + 5 * 60), "1 h 05 min");
    assert_eq!(say(&mut words("de"), 3_600 + 5 * 60), "1 h 05 min");
}

#[test]
fn days_are_local() {
    // 23:30 UTC on 2025-10-14 is the 15th at +01:00.
    let unix = (NOON + 11 * 3_600 + 1_800) as u32;
    let east = |u: u32| ((i64::from(u) + 3_600).div_euclid(86_400)) as i32;
    let day = utc(unix);
    assert_eq!(east(unix), day + 1);
    assert_eq!(texts::date(&mut words("en"), day).unwrap(), "10/14/2025");
    assert_eq!(texts::date(&mut words("de"), day).unwrap(), "14.10.2025");
    // The books screen shows the day the caller's function names.
    let (mut books, seconds) = shelf();
    books[0].last_open = i64::from(unix);
    let places = [0];
    let rows = |day_of: &dyn Fn(u32) -> i32| {
        let items = book_list::items(&mut words("en"), &books, &seconds, &places, day_of);
        texts_of(&items.unwrap())[0].2.clone()
    };
    assert_eq!(rows(&utc), "last read 10/14/2025");
    assert_eq!(rows(&east), "last read 10/15/2025");
}

#[test]
fn the_books_come_last_read_first_and_only_those_read() {
    let (books, seconds) = shelf();
    let places = book_list::order(&books, &seconds).unwrap();
    // Id 3 and 4 opened at the same time: by id; the unread one is left out.
    assert_eq!(places, [2, 3, 0]);
}

#[test]
fn the_books_screen_rows_in_english_and_german() {
    let (books, seconds) = shelf();
    let places = book_list::order(&books, &seconds).unwrap();
    let en = book_list::items(&mut words("en"), &books, &seconds, &places, utc).unwrap();
    assert_eq!(
        texts_of(&en),
        [
            (
                "New".into(),
                "< 1 min".into(),
                "A, B · last read 10/14/2025".into()
            ),
            (
                "Same time".into(),
                "42 min".into(),
                "last read 10/14/2025".into()
            ),
            (
                "Old".into(),
                "1 h 05 min".into(),
                "last read 9/4/2025".into()
            ),
        ]
    );
    let de = book_list::items(&mut words("de"), &books, &seconds, &places, utc).unwrap();
    let rows = texts_of(&de);
    assert_eq!(rows[0].2, "A, B · zuletzt 14.10.2025");
    assert_eq!(rows[2].1, "1 h 05 min");
    assert_eq!(book_list::authors("N/A").unwrap(), "");
}

fn totals() -> BookTotals {
    BookTotals {
        seconds: 3_000,
        pages_read: 30,
        days: alloc::vec![(20_000, 1_000), (20_002, 2_000)],
    }
}

#[test]
fn a_books_rows_in_english() {
    let mut en = words("en");
    let rows = book_rows::rows(&mut en, &book(1, "T", "A\nB", 0, 100), &totals()).unwrap();
    let rows = texts_of(&rows);
    let pairs: Vec<(&str, &str)> = rows
        .iter()
        .map(|(l, v, _)| (l.as_str(), v.as_str()))
        .collect();
    assert_eq!(
        pairs,
        [
            ("Author", "A, B"),
            ("Time read", "50 min"),
            ("Pages read", "30 of 100"),
            ("Pace", "100 s per page"),
            ("Time left (estimated)", "1 h 56 min"),
            ("Reading days", "2 days"),
            ("First read", "10/4/2024"),
            ("Last read", "10/6/2024"),
        ]
    );
}

#[test]
fn a_book_without_pages_known_has_no_time_left_and_a_single_day_no_last_day() {
    let mut t = totals();
    t.days.truncate(1);
    let rows = book_rows::rows(&mut words("en"), &book(1, "T", "N/A", 0, 0), &t).unwrap();
    let rows = texts_of(&rows);
    let labels: Vec<&str> = rows.iter().map(|(l, _, _)| l.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Time read",
            "Pages read",
            "Pace",
            "Reading days",
            "First read"
        ]
    );
    assert_eq!(rows[1].1, "30 pages");
    assert_eq!(rows[3].1, "1 day");
}

/// Refuses each allocation of `run` in turn: it must come back as
/// `NoMemory` (or be unaffected), never abort.
fn sweep(run: &dyn Fn() -> Result<usize, Error>) {
    let (whole, count) = armed(usize::MAX, run);
    assert!(whole.is_ok());
    assert!(count > 0, "allocates nothing: the sweep proves nothing");
    for at in 1..=count {
        let (result, _) = armed(at, run);
        match result {
            Ok(_) | Err(Error::NoMemory) => {}
            Err(e) => panic!("refusing allocation {at}: {e:?}"),
        }
    }
}

#[test]
fn the_order_refuses_cleanly() {
    let (books, seconds) = shelf();
    sweep(&|| book_list::order(&books, &seconds).map(|p| p.len()));
}

// The catalog behind `Words` is the test's: its own allocations are the
// host's and are not refused.
#[test]
fn the_books_rows_refuse_cleanly() {
    let (books, seconds) = shelf();
    let places = fixture(|| book_list::order(&books, &seconds).unwrap());
    let catalog = RefCell::new(fixture(|| words("en")));
    sweep(&|| {
        let mut w = QuietWords(&catalog);
        book_list::items(&mut w, &books, &seconds, &places, utc).map(|i| i.len())
    });
}

#[test]
fn a_books_numbers_refuse_cleanly() {
    let (books, _) = shelf();
    let catalog = RefCell::new(fixture(|| words("en")));
    let totals = fixture(totals);
    sweep(&|| {
        let mut w = QuietWords(&catalog);
        book_rows::rows(&mut w, &books[0], &totals).map(|i| i.len())
    });
}

/// The host's catalog allocates on the host's side: nothing of it is refused.
struct QuietWords<'a>(&'a RefCell<Catalogued>);

impl Words for QuietWords<'_> {
    fn copy(&mut self, key: &str) -> Result<String, Error> {
        fixture(|| self.0.borrow_mut().copy(key))
    }
    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error> {
        fixture(|| self.0.borrow_mut().format(key, args))
    }
    fn digits(&mut self, text: &str) -> Result<String, Error> {
        fixture(|| self.0.borrow_mut().digits(text))
    }
}
