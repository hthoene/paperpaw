//! The to-do list as kept in `todo.txt`: any list written is read back
//! as it was (texts as the list's screen cleans them, done today or
//! open), lists done on other days go, the dates of items done agree
//! with the calendar, and files that are not lists (any bytes, huge or
//! negative years in the dates of items done) never panic. (The pure model of
//! `native_todo_list`; the file is a buffer here.)

extern crate std;

use alloc::string::String;
use alloc::vec::Vec;

use core::cell::RefCell;

use proptest::prelude::*;

use crate::native_dates::days_from_civil;
use crate::native_todo_list::{Item, List, MAX_ITEMS, MAX_TEXT, day_of, read};

fn parse(text: &str, today: i64) -> List {
    crate::native_todo_list::parse(text, today).unwrap()
}

fn format(list: &List) -> String {
    crate::native_todo_list::format(list).unwrap()
}

fn cleaned(text: &str) -> String {
    crate::native_todo_list::cleaned(text).unwrap()
}

fn date_of(day: i64) -> String {
    String::from(crate::native_todo_list::date_of(day).unwrap().as_str())
}

/// The file, in memory.
#[derive(Default)]
struct MemoryStorage(RefCell<Vec<u8>>);

impl MemoryStorage {
    fn new() -> Self {
        Self::default()
    }
}

fn save(storage: &MemoryStorage, list: &List) -> Result<(), ()> {
    *storage.0.borrow_mut() = format(list).into_bytes();
    Ok(())
}

fn load(storage: &MemoryStorage, today: i64) -> Result<List, ()> {
    read(&storage.0.borrow(), today).unwrap().ok_or(())
}

/// Wednesday, 14 October 2026, as days since 1970.
const TODAY: i64 = 20_740;

/// A list of just `items`, nothing after them.
fn list(items: &[Item]) -> List {
    List {
        items: items.to_vec(),
        rest: String::new(),
    }
}

/// Texts an item can have once typed on the list's screen.
fn text() -> impl Strategy<Value = String> {
    prop_oneof![
        "[a-zA-Z0-9 ,.!?-]{1,40}",
        "\\PC{1,60}",
        Just(String::from("Ünïcödé ✓ 日本語 עברית")),
    ]
    .prop_map(|t| cleaned(&t))
    .prop_filter("an item has a text", |t| !t.is_empty())
}

#[test]
fn today_is_the_fourteenth_of_october() {
    assert_eq!(date_of(TODAY), "2026-10-14");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]

    /// A list written is read back the same on the same day: its items,
    /// their order, which are done.
    #[test]
    fn lists_are_read_back_as_written(
        items in proptest::collection::vec((text(), any::<bool>()), 0..30),
    ) {
        let items: Vec<Item> = items
            .into_iter()
            .map(|(text, done)| Item { text, done: done.then_some(TODAY) })
            .collect();
        let storage = MemoryStorage::new();
        save(&storage, &list(&items)).unwrap();
        prop_assert_eq!(load(&storage, TODAY).unwrap().items, items.clone());
        // The next day, those done have gone, the open ones are kept.
        let open: Vec<Item> = items.iter().filter(|i| i.done.is_none()).cloned().collect();
        prop_assert_eq!(load(&storage, TODAY + 1).unwrap().items, open);
    }

    /// A text typed is one line, trimmed at its start (its end too, but
    /// where it is cut to the length allowed), never longer than allowed.
    #[test]
    fn typed_texts_are_cleaned(text in "\\PC{0,400}|[\\t\\r\\n a]{0,20}") {
        let clean = cleaned(&text);
        prop_assert!(clean.chars().count() <= MAX_TEXT);
        prop_assert!(!clean.contains(['\n', '\r', '\t']));
        prop_assert_eq!(clean.trim_start(), clean.as_str());
        prop_assert_eq!(cleaned(&clean), clean.trim_end());
    }

    /// Whatever the file holds, reading it never panics, keeps at most
    /// the items allowed, and none of them empty or done before today.
    #[test]
    fn any_file_is_read_without_panic(bytes in proptest::collection::vec(any::<u8>(), 0..2000)) {
        let text = String::from_utf8_lossy(&bytes);
        let items = parse(&text, TODAY).items;
        prop_assert!(items.len() <= MAX_ITEMS);
        for item in &items {
            prop_assert!(!item.text.is_empty());
            prop_assert!(item.done.is_none_or(|day| day >= TODAY));
        }
    }

    /// Lines in the form of todo.txt, with dates of any year: read
    /// without panic.
    #[test]
    fn done_lines_with_any_date_are_read(
        year in -1_000_000i64..1_000_000,
        month in 0u32..14,
        day in 0u32..33,
        text in "[a-z ]{0,10}",
    ) {
        let line = std::format!("x {year:04}-{month:02}-{day:02} {text}\nopen\n");
        let items = parse(&line, TODAY).items;
        prop_assert_eq!(items.last().map(|i| i.text.as_str()), Some("open"));
    }

    /// Days and their dates agree, as the calendar has them.
    #[test]
    fn days_and_dates_agree_for_any_day(day in -700_000i64..3_000_000) {
        let date = date_of(day);
        prop_assert_eq!(day_of(&date), Some(day), "{}", date);
    }
}

#[test]
fn dates_of_the_calendar() {
    assert_eq!(days_from_civil(1970, 1, 1), 0);
    assert_eq!(day_of("1969-12-31"), Some(-1));
    assert_eq!(day_of("2000-02-29"), Some(days_from_civil(2000, 3, 1) - 1));
    assert_eq!(day_of("2026-10-14"), Some(TODAY));
    // Not a date: no day.
    for text in [
        "",
        "2026",
        "2026-10",
        "2026-00-01",
        "2026-13-01",
        "2026-10-00",
        "2026-10-32",
        "--",
        "x-y-z",
        "2026-1-1-1",
    ] {
        assert_eq!(day_of(text), None, "{text:?}");
    }
}

#[test]
fn a_list_of_one_hundred_and_more_shows_one_hundred_and_keeps_the_rest() {
    let text: String = (0..150).map(|n| std::format!("item {n}\n")).collect();
    let list = parse(&text, TODAY);
    assert_eq!(list.items.len(), MAX_ITEMS);
    assert_eq!(list.items[0].text, "item 0");
    assert_eq!(list.items[MAX_ITEMS - 1].text, "item 99");
    // The lines after them are written back as they were.
    assert_eq!(format(&list), text);
}

#[test]
fn done_items_without_a_date_go_and_empty_ones_are_none() {
    let items = parse("x done long ago\n   \n\nx 2026-10-14\nkept\n", TODAY).items;
    assert_eq!(
        items,
        [Item {
            text: String::from("kept"),
            done: None
        }]
    );
    // A line of just "x" is an item called so.
    let items = parse("x\nx \n", TODAY).items;
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|i| i.text == "x" && i.done.is_none()));
    // Done later than today (a clock set back): still shown, done.
    let items = parse("x 2026-10-15 tomorrow\n", TODAY).items;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].done, Some(TODAY + 1));
}

/// A year too large for the reckoning of days must not panic (it once
/// overflowed `i64` in `days_of`, which a debug build stops at).
#[test]
fn huge_years_in_dates_do_not_panic() {
    for line in [
        "x 9223372036854775807-01-01 never\nopen\n",
        "x -9223372036854775808-01-01 never\nopen\n",
        "x 99999999999999999-01-01 never\nopen\n",
    ] {
        let items = parse(line, TODAY).items;
        assert_eq!(items.last().map(|i| i.text.as_str()), Some("open"));
    }
}

/// An open item whose text begins with "x " ("x-ray at 3") is kept with
/// a date in front, so that it is not read back as one done long ago
/// (and lost).
#[test]
fn open_items_beginning_with_x_are_kept() {
    let storage = MemoryStorage::new();
    let items = [Item {
        text: cleaned("x ray appointment at 3"),
        done: None,
    }];
    save(&storage, &list(&items)).unwrap();
    assert_eq!(load(&storage, TODAY).unwrap().items, items);
}

/// Dates before the "x " are kept too, and the date in front is the only
/// change to the file; a date before an open item that would not read as
/// done is part of its text, as before.
#[test]
fn open_items_with_dates_before_an_x_are_kept() {
    let items: Vec<Item> = ["2026-10-14 x ray", "2026-10-14  2026-10-15 x y", "x"]
        .into_iter()
        .map(|text| Item {
            text: String::from(text),
            done: None,
        })
        .collect();
    let storage = MemoryStorage::new();
    save(&storage, &list(&items)).unwrap();
    assert_eq!(load(&storage, TODAY).unwrap().items, items);
    let text = "2026-10-14 Call Anna
1970-01-01 x ray
";
    let items = parse(text, TODAY).items;
    assert_eq!(items[0].text, "2026-10-14 Call Anna");
    assert_eq!(items[1].text, "x ray");
    assert_eq!(format(&list(&items)), text);
}

#[test]
fn line_breaks_in_texts_do_not_make_new_items() {
    let items = [Item {
        text: String::from("one\ntwo\rthree"),
        done: None,
    }];
    let text = format(&list(&items));
    assert_eq!(text, "one two three\n");
    assert_eq!(parse(&text, TODAY).items.len(), 1);
}

/// A list that is there but is no text is not an empty list that the next
/// change would write over it; a mark of UTF-8 at the start is ignored.
#[test]
fn a_file_that_is_no_text_is_unreadable() {
    assert!(read(b"Br\xf6tchen holen\n", TODAY).unwrap().is_none());
    let items = read(b"\xef\xbb\xbfBuy bread\n", TODAY)
        .unwrap()
        .unwrap()
        .items;
    assert_eq!(items[0].text, "Buy bread");
    assert!(read(b"", TODAY).unwrap().unwrap().items.is_empty());
}

/// What version 1 wrote stays readable (see `docs/data-files.md`).
#[test]
fn the_list_of_version_1_is_read() {
    let items = parse(
        include_str!("../testdata/todo.txt.v1"),
        days_from_civil(2026, 10, 19),
    )
    .items;
    let texts: Vec<&str> = items.iter().map(|i| i.text.as_str()).collect();
    assert_eq!(texts, ["Buy bread", "Water the plants", "Read chapter 3"]);
    assert_eq!(items[1].done, Some(days_from_civil(2026, 10, 19)));
    assert_eq!(items[0].done, None);
}
