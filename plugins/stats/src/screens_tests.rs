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

// ---- The overview and the calendar ---------------------------------------

mod overview_and_calendar {
    use super::*;
    use crate::calendar_model::{self as cal, Step};
    use crate::overview_rows::{self as over, Facts, Way};
    use crate::store::{self, Day, PageStat};
    use crate::testing::Quiet;
    use paperpaw_sdk::MemoryStorage;

    const TODAY: i32 = 20_000;

    fn day(seconds: u32) -> impl Fn(i32) -> Day {
        move |day| Day {
            day,
            seconds,
            stays: 1,
        }
    }

    /// Reading on today (25 min), yesterday (10 min) and ten days ago (1 h).
    fn days() -> Vec<Day> {
        alloc::vec![
            day(3_600)(TODAY - 10),
            day(600)(TODAY - 1),
            day(1_500)(TODAY),
        ]
    }

    fn facts(days: &[Day], today: Option<i32>) -> Facts<'_> {
        Facts {
            today,
            days,
            books: 3,
            today_pages: 12,
            current_book: Some(7),
        }
    }

    fn day_of(year: i32, month: u8, d: u8) -> i32 {
        (0..60_000)
            .find(|&n| {
                let t = texts::date_of(n);
                (t.year, t.month, t.day) == (year, month, d)
            })
            .unwrap()
    }

    #[test]
    fn the_overview_in_english() {
        let days = days();
        let mut en = words("en");
        let tiles = over::tiles(&mut en, &facts(&days, Some(TODAY)))
            .unwrap()
            .unwrap();
        let shown: Vec<(&str, &str, &str)> = tiles
            .iter()
            .map(|t| (t.label.as_str(), t.value.as_str(), t.note.as_str()))
            .collect();
        assert_eq!(
            shown,
            [
                ("Today", "25 min", "12 pages"),
                ("Week", "35 min", "7 days"),
                ("Streak", "2 days", "in a row"),
            ]
        );
        let chart = over::chart(&mut en, &facts(&days, Some(TODAY)))
            .unwrap()
            .unwrap();
        assert_eq!(chart.title, "The last 14 days");
        assert_eq!(chart.values.len(), 14);
        assert_eq!(
            &chart.values[3..],
            [3_600, 0, 0, 0, 0, 0, 0, 0, 0, 600, 1_500]
        );
        assert_eq!(chart.names.len(), 14);
        assert_eq!(chart.top, "1 h 00 min");
        assert_eq!(
            over::totals_text(&mut en, &facts(&days, Some(TODAY)))
                .unwrap()
                .as_deref(),
            Some("1 h 35 min in all · 3 books · 3 reading days")
        );
    }

    #[test]
    fn the_overview_in_german_and_without_a_clock() {
        let days = days();
        let totals = over::totals_text(&mut words("de"), &facts(&days, Some(TODAY))).unwrap();
        assert_eq!(
            totals.as_deref(),
            Some("1 h 35 min insgesamt · 3 Bücher · 3 Lesetage")
        );
        // No clock: no tiles, no chart; the totals where anything was read.
        let mut en = words("en");
        assert!(over::tiles(&mut en, &facts(&days, None)).unwrap().is_none());
        assert!(over::chart(&mut en, &facts(&days, None)).unwrap().is_none());
        assert!(
            over::totals_text(&mut en, &facts(&days, None))
                .unwrap()
                .is_some()
        );
        assert!(
            over::totals_text(&mut en, &facts(&[], None))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn the_ways_on() {
        assert_eq!(
            over::ways(Some(7)).unwrap(),
            [Way::ThisBook(7), Way::Calendar, Way::Books, Way::Sync]
        );
        assert_eq!(
            over::ways(None).unwrap(),
            [Way::Calendar, Way::Books, Way::Sync]
        );
        let Item::Row(row) = over::way_row(&mut words("de"), Way::ThisBook(7)).unwrap() else {
            panic!("a row");
        };
        assert_eq!(
            (row.title.as_str(), row.value.as_str()),
            ("Dieses Buch", "›")
        );
    }

    fn log_of(storage: &MemoryStorage, day: i32, stays: &[(u16, u32, u32, u16)]) {
        for (book, page, second, duration) in stays {
            store::append(
                storage,
                PageStat {
                    book: *book,
                    page: *page,
                    start: (day as u32) * 86_400 + 8 * 3_600 + second,
                    duration: *duration,
                    pages: 100,
                },
            )
            .unwrap();
        }
    }

    fn utc_day(unix: u32) -> i32 {
        (unix / 86_400) as i32
    }

    #[test]
    fn pages_and_books_of_a_day() {
        let storage = MemoryStorage::new();
        let books = alloc::vec![book(1, "First", "", 0, 100), book(2, "Second", "", 0, 100)];
        let other = TODAY - 1;
        log_of(&storage, other, &[(1, 5, 0, 30)]);
        // Page 5 twice and page 6; book 2 longer than book 1.
        log_of(
            &storage,
            TODAY,
            &[
                (1, 5, 0, 30),
                (1, 5, 100, 20),
                (1, 6, 200, 10),
                (2, 1, 300, 90),
            ],
        );
        assert_eq!(over::distinct_pages(&storage, TODAY, utc_day).unwrap(), 3);
        let days = alloc::vec![day(30)(other), day(150)(TODAY)];
        let detail = cal::detail(&storage, &books, &days, TODAY, utc_day).unwrap();
        assert_eq!(
            detail,
            cal::Detail {
                seconds: 150,
                pages: 3,
                books: alloc::vec![("Second".into(), 90), ("First".into(), 60)],
            }
        );
        let text = cal::day_text(&mut words("en"), TODAY, &detail).unwrap();
        assert_eq!(text.summary, "2 min · 3 pages");
        assert_eq!(
            text.books,
            [
                ("Second".into(), "1 min".into()),
                ("First".into(), "1 min".into())
            ]
        );
        // A day with no reading says so; a book that has no entry is `?`.
        let none = cal::detail(&storage, &books, &days, TODAY - 5, utc_day).unwrap();
        assert_eq!(none, cal::Detail::default());
        assert_eq!(
            cal::day_text(&mut words("de"), TODAY - 5, &none)
                .unwrap()
                .summary,
            "Nicht gelesen"
        );
        let unknown = cal::detail(&storage, &[], &days, TODAY, utc_day).unwrap();
        assert_eq!(unknown.books[0].0, "?");
    }

    #[test]
    fn the_chosen_day_in_words() {
        // 2026-10-14 is a Wednesday.
        let d = day_of(2026, 10, 14);
        let none = cal::Detail::default();
        let en = cal::day_text(&mut words("en"), d, &none).unwrap();
        assert_eq!(
            (en.long.as_str(), en.short.as_str()),
            ("Wednesday, October 14", "We, Oct. 14")
        );
        let de = cal::day_text(&mut words("de"), d, &none).unwrap();
        assert_eq!(de.long, "Mittwoch, 14. Oktober");
        assert_eq!(
            cal::title(&mut words("en"), d).unwrap(),
            ["October 2026", "Oct. 2026"]
        );
        assert_eq!(
            cal::title(&mut words("de"), d).unwrap(),
            ["Oktober 2026", "Okt. 2026"]
        );
        assert_eq!(cal::short_month("May").unwrap(), "May");
        assert_eq!(cal::short_month("März").unwrap(), "März");
        assert_eq!(cal::short_month("Février").unwrap(), "Fév.");
    }

    #[test]
    fn months_turn_and_the_grid_starts_on_the_week() {
        let d = day_of(2026, 1, 31);
        assert_eq!(texts::month_start(d), day_of(2026, 1, 1));
        assert_eq!(cal::next_month(d), day_of(2026, 2, 1));
        assert_eq!(cal::next_month(day_of(2026, 12, 15)), day_of(2027, 1, 1));
        assert_eq!(cal::previous_month(day_of(2026, 3, 31)), day_of(2026, 2, 1));
        assert_eq!(cal::previous_month(day_of(2026, 1, 5)), day_of(2025, 12, 1));
        // 2026-10-01 is a Thursday.
        let oct = day_of(2026, 10, 14);
        assert_eq!(cal::first_shown(oct, 1), day_of(2026, 9, 28));
        assert_eq!(cal::first_shown(oct, 0), day_of(2026, 9, 27));
        assert_eq!(cal::month_of(oct), (2026, 10));
        assert_eq!(cal::step(oct, Step::DayBack), oct - 1);
        assert_eq!(cal::step(oct, Step::WeekForward), oct + 7);
        assert_eq!(cal::step(oct, Step::MonthForward), day_of(2026, 11, 1));
        assert_eq!(cal::step(oct, Step::MonthBack), day_of(2026, 9, 1));
        assert_eq!(cal::shade(0), None);
        assert_eq!(
            [1, 14, 15, 29, 30, 59, 60, 600].map(cal::shade),
            [220, 220, 180, 180, 120, 120, 40, 40].map(Some)
        );
    }

    #[test]
    fn the_streak_note_is_for_more_than_a_day() {
        let days = days();
        let note = |today| cal::streak_note(&mut words("en"), &days, today).unwrap();
        assert_eq!(note(Some(TODAY)).as_deref(), Some("2 days in a row"));
        assert_eq!(note(Some(TODAY - 5)), None);
        assert_eq!(note(None), None);
    }

    #[test]
    fn keys_swipes_and_taps_move_the_chosen_day_as_in_interface_6() {
        use cal::{Key, Swipe};
        let d = day_of(2026, 10, 14);
        let to = |key| cal::step(d, cal::key_step(key));
        assert_eq!(to(Key::Left), d - 1);
        assert_eq!(to(Key::Previous), d - 1);
        assert_eq!(to(Key::Right), d + 1);
        assert_eq!(to(Key::Next), d + 1);
        assert_eq!(to(Key::Up), d - 7);
        assert_eq!(to(Key::Down), d + 7);
        assert_eq!(to(Key::PreviousPage), day_of(2026, 9, 1));
        assert_eq!(to(Key::NextPage), day_of(2026, 11, 1));
        assert_eq!(
            cal::step(d, cal::swipe_step(Swipe::Left)),
            day_of(2026, 11, 1)
        );
        assert_eq!(
            cal::step(d, cal::swipe_step(Swipe::Right)),
            day_of(2026, 9, 1)
        );
        // A tap on a day of the month chooses it; on another month's day
        // (shown in the weeks around) it does nothing.
        assert_eq!(
            cal::tap_day(d, day_of(2026, 10, 20)),
            Some(day_of(2026, 10, 20))
        );
        assert_eq!(cal::tap_day(d, d), Some(d));
        assert_eq!(cal::tap_day(d, day_of(2026, 9, 28)), None);
        assert_eq!(cal::tap_day(d, day_of(2026, 11, 2)), None);
        // The same month of another year is another month.
        assert_eq!(cal::tap_day(d, day_of(2025, 10, 14)), None);
    }

    #[test]
    fn leap_years_have_a_february_29() {
        let feb29 = day_of(2024, 2, 29);
        assert_eq!(texts::date_of(feb29).day, 29);
        assert_eq!(cal::next_month(feb29), day_of(2024, 3, 1));
        assert_eq!(cal::previous_month(feb29), day_of(2024, 1, 1));
        assert_eq!(cal::next_month(day_of(2024, 1, 31)), day_of(2024, 2, 1));
        assert_eq!(cal::previous_month(day_of(2024, 3, 31)), day_of(2024, 2, 1));
        // 2023 has none: February ends on the 28th.
        assert_eq!(cal::next_month(day_of(2023, 2, 28)), day_of(2023, 3, 1));
        assert_eq!(texts::date_of(day_of(2023, 3, 1) - 1).day, 28);
        // 2024-02-01 is a Thursday: the grid starts on a Monday 4 days before.
        assert_eq!(cal::first_shown(feb29, 1), day_of(2024, 1, 29));
        assert_eq!(cal::month_of(feb29), (2024, 2));
        // 1900 and 2100 are not leap years, 2000 is.
        assert_eq!(texts::date_of(day_of(2000, 3, 1) - 1).day, 29);
        assert_eq!(texts::date_of(day_of(2100, 3, 1) - 1).day, 28);
    }

    /// The day of a stay at `second` of local day `day` in a zone.
    fn log_at(storage: &MemoryStorage, unix: u32, book: u16, page: u32, duration: u16) {
        store::append(
            storage,
            PageStat {
                book,
                page,
                start: unix,
                duration,
                pages: 100,
            },
        )
        .unwrap();
    }

    #[test]
    fn a_zone_and_summer_time_decide_which_day_a_stay_is_on() {
        // Berlin: +1 h, and +2 h from 2026-03-29 01:00 UTC.
        let change = 1_774_746_000u32;
        let berlin = move |unix: u32| {
            let offset = if unix >= change { 7_200 } else { 3_600 };
            ((i64::from(unix) + offset).div_euclid(86_400)) as i32
        };
        let day = berlin(change);
        // 23:30 UTC on the 28th is 00:30 on the 29th in winter time; 22:30 UTC
        // on the 29th is 00:30 on the 30th in summer time.
        let (a, b) = (change - 5_400, change + 77_400);
        assert_eq!((utc_day(a), utc_day(b)), (day - 1, day));
        assert_eq!((berlin(a), berlin(b)), (day, day + 1));
        let storage = MemoryStorage::new();
        log_at(&storage, a, 1, 1, 30);
        log_at(&storage, b, 1, 2, 50);
        assert_eq!(over::distinct_pages(&storage, day - 1, berlin).unwrap(), 0);
        assert_eq!(over::distinct_pages(&storage, day, berlin).unwrap(), 1);
        assert_eq!(over::distinct_pages(&storage, day + 1, berlin).unwrap(), 1);
        let books = alloc::vec![book(1, "First", "", 0, 100)];
        let days = alloc::vec![day_row(day, 30), day_row(day + 1, 50)];
        let first = cal::detail(&storage, &books, &days, day, berlin).unwrap();
        assert_eq!((first.seconds, first.pages), (30, 1));
        let second = cal::detail(&storage, &books, &days, day + 1, berlin).unwrap();
        assert_eq!((second.seconds, second.pages), (50, 1));
        assert_eq!(
            cal::detail(&storage, &books, &days, day - 1, berlin).unwrap(),
            cal::Detail::default()
        );
        // Read as UTC the stays are on the days before.
        let utc_days = alloc::vec![day_row(day - 1, 30), day_row(day, 50)];
        let utc = cal::detail(&storage, &books, &utc_days, day, utc_day).unwrap();
        assert_eq!(utc.seconds, 50);
    }

    fn day_row(day: i32, seconds: u32) -> Day {
        Day {
            day,
            seconds,
            stays: 1,
        }
    }

    #[test]
    fn equal_times_and_equal_pages_keep_a_stable_answer() {
        let storage = MemoryStorage::new();
        let start = (TODAY as u32) * 86_400 + 8 * 3_600;
        // Three books with the same time, met in the order 3, 1, 2; and the
        // same page of a book logged twice at the very same second.
        log_at(&storage, start, 3, 1, 60);
        log_at(&storage, start + 100, 1, 1, 60);
        log_at(&storage, start + 200, 2, 1, 60);
        log_at(&storage, start + 200, 2, 1, 60);
        log_at(&storage, start + 300, 4, 1, 5);
        let books = alloc::vec![
            book(1, "A", "", 0, 100),
            book(2, "B", "", 0, 100),
            book(3, "C", "", 0, 100),
            book(4, "D", "", 0, 100),
        ];
        let days = alloc::vec![day_row(TODAY, 245)];
        let detail = cal::detail(&storage, &books, &days, TODAY, utc_day).unwrap();
        // B has two stays (120 s) and leads; the others tie at 60 s and come
        // in the order they were first read; D (5 s) is cut at three books.
        let titles: Vec<&str> = detail.books.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(titles, ["B", "C", "A"]);
        // Four distinct pages (book, page) in all, the doubled one once.
        assert_eq!(detail.pages, 4);
        assert_eq!(over::distinct_pages(&storage, TODAY, utc_day).unwrap(), 4);
    }

    #[test]
    fn a_day_of_very_long_reading_does_not_overflow() {
        let storage = MemoryStorage::new();
        let start = (TODAY as u32) * 86_400;
        for n in 0..3 {
            log_at(&storage, start + n * 70_000, 1, n + 1, u16::MAX);
        }
        let books = alloc::vec![book(1, "A", "", 0, 100)];
        let days = alloc::vec![day_row(TODAY, 3 * 65_535)];
        let mut detail = cal::detail(&storage, &books, &days, TODAY, utc_day).unwrap();
        assert!(detail.books.len() <= 1);
        // The words say it too.
        detail.seconds = u32::MAX;
        assert!(cal::day_text(&mut words("en"), TODAY, &detail).is_ok());
    }

    #[test]
    fn overview_and_calendar_refuse_cleanly() {
        let days = days();
        let catalog = RefCell::new(fixture(|| words("en")));
        let storage = Quiet(MemoryStorage::new());
        let books =
            fixture(|| alloc::vec![book(1, "First", "", 0, 100), book(2, "Second", "", 0, 100)]);
        fixture(|| {
            log_of(
                &storage.0,
                TODAY,
                &[(1, 5, 0, 30), (1, 6, 200, 10), (2, 1, 300, 90)],
            );
        });
        let detail = fixture(|| cal::detail(&storage, &books, &days, TODAY, utc_day).unwrap());
        sweep(&|| {
            let mut w = QuietWords(&catalog);
            let f = facts(&days, Some(TODAY));
            let tiles = over::tiles(&mut w, &f)?.map_or(0, |t| t.len());
            let chart = over::chart(&mut w, &f)?.map_or(0, |c| c.values.len());
            let totals = over::totals_text(&mut w, &f)?.map_or(0, |t| t.len());
            Ok(tiles + chart + totals + over::ways(Some(7))?.len())
        });
        sweep(&|| over::distinct_pages(&storage, TODAY, utc_day).map(|n| n as usize));
        sweep(&|| cal::detail(&storage, &books, &days, TODAY, utc_day).map(|d| d.books.len()));
        sweep(&|| {
            let mut w = QuietWords(&catalog);
            let text = cal::day_text(&mut w, TODAY, &detail)?;
            let title = cal::title(&mut w, TODAY)?;
            let note = cal::streak_note(&mut w, &days, Some(TODAY))?;
            Ok(text.books.len() + title.len() + usize::from(note.is_some()))
        });
        sweep(&|| cal::short_month("Dezember").map(|s| s.len()));
    }
}
