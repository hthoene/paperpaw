//! The overview's contents over the plugin's catalogs and files.

use alloc::string::ToString;
use core::cell::RefCell;

use paperpaw_sdk::MemoryStorage;

use super::OverviewView;
use crate::overview_rows::Way;
use crate::screens_tests::{QuietWords, sweep, words};
use crate::store::{self, Book, PageStat};
use crate::testing::Quiet;
use crate::testing::heap::fixture;

const DAY: u32 = 86_400;
/// Day 20,000, UTC.
const TODAY: i32 = 20_000;

fn read(storage: &MemoryStorage, day: i32, book: u16, page: u32, seconds: u16) {
    store::append(
        storage,
        PageStat {
            book,
            page,
            start: day as u32 * DAY + 8 * 3_600,
            duration: seconds,
            pages: 100,
        },
    )
    .unwrap();
}

/// Two books; 23 minutes today on pages 1 and 2, 41 yesterday.
fn card() -> Quiet {
    let storage = MemoryStorage::new();
    let book = |id: u16, title: &str| Book {
        id,
        title: title.to_string(),
        authors: "N/A".to_string(),
        pages: 100,
        ..Book::default()
    };
    store::save_books(&storage, &[book(1, "A"), book(2, "B")]).unwrap();
    read(&storage, TODAY, 1, 1, 600);
    read(&storage, TODAY, 1, 2, 780);
    read(&storage, TODAY - 1, 2, 5, 41 * 60);
    Quiet(storage)
}

#[test]
fn the_tiles_the_chart_and_the_totals_in_english() {
    let data = card();
    let view = OverviewView::build(&mut words("en"), &data, Some(TODAY), Some(1)).unwrap();
    let tiles = view.tiles.as_ref().unwrap();
    assert_eq!(tiles[0].label, "Today");
    assert_eq!(tiles[0].value, "23 min");
    assert_eq!(tiles[0].note, "2 pages");
    assert_eq!(tiles[1].value, "1 h 04 min");
    assert_eq!(tiles[2].value, "2 days");
    let chart = view.chart.as_ref().unwrap();
    assert_eq!(chart.values.len(), 14);
    assert_eq!(chart.values[13], 1_380);
    assert_eq!(chart.values[12], 2_460);
    assert!(view.totals.as_deref().unwrap().contains("2 books"));
    assert_eq!(view.ways, [Way::ThisBook(1), Way::Books]);
    // The tiles are rows of the one column, then the ways.
    assert_eq!(view.list.items().len(), 5);
}

#[test]
fn in_german_and_without_a_book_being_read() {
    let data = card();
    let view = OverviewView::build(&mut words("de"), &data, Some(TODAY), None).unwrap();
    assert_eq!(view.tiles.as_ref().unwrap()[0].label, "Heute");
    assert_eq!(view.ways, [Way::Books]);
}

#[test]
fn without_the_clock_there_are_no_tiles_but_the_ways_and_the_hint() {
    let data = card();
    let view = OverviewView::build(&mut words("en"), &data, None, None).unwrap();
    assert!(view.tiles.is_none() && view.chart.is_none());
    assert!(view.no_clock.starts_with("Statistics need the time of day"));
    // What was read still adds up.
    assert!(view.totals.is_some());
    assert_eq!(view.list.items().len(), 1);
    // Nothing read and no clock: no totals either.
    let none = Quiet(MemoryStorage::new());
    let empty = OverviewView::build(&mut words("en"), &none, None, None).unwrap();
    assert!(empty.totals.is_none());
}

#[test]
fn the_overview_refuses_cleanly() {
    let data = card();
    let catalog = RefCell::new(fixture(|| words("en")));
    for (today, current) in [(Some(TODAY), Some(1)), (None, None)] {
        sweep(&|| {
            OverviewView::build(&mut QuietWords(&catalog), &data, today, current)
                .map(|view| view.list.items().len())
        });
    }
}
