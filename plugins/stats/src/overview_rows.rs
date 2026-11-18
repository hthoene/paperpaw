//! The overview's contents: today, the week and the streak as tiles, the
//! last two weeks as bars, the totals, and the ways on. Host-free: the
//! shell lays them out and draws them (the tiles side by side, or one
//! below the other where they do not fit).

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_ui_kit::Item;

use crate::room;
use crate::store::{self, Day};
use crate::texts::{self, Words};
use crate::totals;

/// Days in the chart.
pub(crate) const CHART_DAYS: i32 = 14;

/// A way on from the overview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Way {
    ThisBook(u16),
    Calendar,
    Books,
    Sync,
}

/// A tile: what it counts, its value and a note below.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Tile {
    pub label: String,
    pub value: String,
    pub note: String,
}

/// The bars of the last days, today the last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Chart {
    pub title: String,
    /// Seconds read on each day.
    pub values: Vec<u32>,
    /// The weekday of each day, short.
    pub names: Vec<String>,
    /// The longest day's time, shown at the top; empty where nothing was read.
    pub top: String,
}

/// What the overview knows: the day it is (none without the clock), the days
/// read, the books, the pages read today and the book being read.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Facts<'a> {
    pub today: Option<i32>,
    pub days: &'a [Day],
    pub books: usize,
    pub today_pages: u32,
}

/// The tiles; none without the time.
pub(crate) fn tiles(words: &mut impl Words, facts: &Facts<'_>) -> Result<Option<[Tile; 3]>, Error> {
    let Some(today) = facts.today else {
        return Ok(None);
    };
    let week: u32 = (0..7)
        .map(|d| texts::seconds_on(facts.days, today - d))
        .fold(0, u32::saturating_add);
    let streak = totals::streak(facts.days, today);
    let today_time = texts::duration(words, u64::from(texts::seconds_on(facts.days, today)))?;
    let today_note = words.format("stats.pages", &[Arg::Count(i64::from(facts.today_pages))])?;
    let week_time = texts::duration(words, u64::from(week))?;
    let streak_text = words.format("stats.days", &[Arg::Count(i64::from(streak))])?;
    Ok(Some([
        Tile {
            label: words.copy("stats.today")?,
            value: today_time,
            note: today_note,
        },
        Tile {
            label: words.copy("stats.week")?,
            value: week_time,
            note: words.copy("stats.last-seven-days")?,
        },
        Tile {
            label: words.copy("stats.streak")?,
            value: streak_text,
            note: words.copy("stats.in-a-row")?,
        },
    ]))
}

/// The bars of the last [`CHART_DAYS`] days; none without the time.
pub(crate) fn chart(words: &mut impl Words, facts: &Facts<'_>) -> Result<Option<Chart>, Error> {
    let Some(today) = facts.today else {
        return Ok(None);
    };
    let first = today - CHART_DAYS + 1;
    let mut values = Vec::new();
    room::reserve(&mut values, CHART_DAYS as usize).map_err(|_| Error::NoMemory)?;
    let mut names = Vec::new();
    room::reserve(&mut names, CHART_DAYS as usize).map_err(|_| Error::NoMemory)?;
    for day in first..=today {
        values.push(texts::seconds_on(facts.days, day));
        names.push(texts::weekday_short(words, texts::date_of(day).weekday)?);
    }
    let max = values.iter().copied().max().unwrap_or(0);
    let top = if max > 0 {
        texts::duration(words, u64::from(max))?
    } else {
        String::new()
    };
    let title = words.format(
        "stats.last-days",
        &[Arg::Int {
            name: "count",
            value: i64::from(CHART_DAYS),
        }],
    )?;
    Ok(Some(Chart {
        title,
        values,
        names,
        top,
    }))
}

/// All reading: the time, the books and the days. Shown where the clock is
/// known or anything was read.
pub(crate) fn totals_text(
    words: &mut impl Words,
    facts: &Facts<'_>,
) -> Result<Option<String>, Error> {
    if facts.today.is_none() && facts.days.is_empty() {
        return Ok(None);
    }
    let total: u64 = facts.days.iter().map(|d| u64::from(d.seconds)).sum();
    let time = texts::duration(words, total)?;
    let books = words.format("stats.books-count", &[Arg::Count(facts.books as i64)])?;
    let days = words.format("stats.reading-days", &[Arg::Count(facts.days.len() as i64)])?;
    words
        .format(
            "stats.totals",
            &[
                Arg::Text {
                    name: "time",
                    value: &time,
                },
                Arg::Text {
                    name: "books",
                    value: &books,
                },
                Arg::Text {
                    name: "days",
                    value: &days,
                },
            ],
        )
        .map(Some)
}

/// The ways on: this book where one is being read, then the calendar, the
/// books and the sync.
pub(crate) fn ways(current_book: Option<u16>) -> Result<Vec<Way>, Error> {
    let mut ways = Vec::new();
    room::reserve(&mut ways, 4).map_err(|_| Error::NoMemory)?;
    if let Some(book) = current_book {
        ways.push(Way::ThisBook(book));
    }
    ways.extend([Way::Calendar, Way::Books, Way::Sync]);
    Ok(ways)
}

/// A way's row: its name and "›".
pub(crate) fn way_row(words: &mut impl Words, way: Way) -> Result<Item, Error> {
    let key = match way {
        Way::ThisBook(_) => "stats.this-book",
        Way::Calendar => "stats.calendar",
        Way::Books => "stats.books",
        Way::Sync => "stats.sync.title",
    };
    Item::row(&words.copy(key)?, "›")
}

/// Distinct pages read on local day `day`; `day_of` turns Unix seconds into
/// local days.
pub(crate) fn distinct_pages(
    storage: &dyn Storage,
    day: i32,
    day_of: impl Fn(u32) -> i32,
) -> Result<u32, Error> {
    let since = (i64::from(day) * 86_400).saturating_sub(14 * 3600).max(0) as u32;
    let mut pages: Vec<(u16, u32)> = Vec::new();
    let mut refused = false;
    store::scan_recent(storage, since, |stat| {
        if refused || day_of(stat.start) != day {
            return;
        }
        if pages.try_reserve(1).is_err() {
            refused = true;
            return;
        }
        pages.push((stat.book, stat.page));
    });
    if refused {
        return Err(Error::NoMemory);
    }
    paperpaw_sdk::sort::sort(&mut pages);
    pages.dedup();
    Ok(pages.len() as u32)
}
