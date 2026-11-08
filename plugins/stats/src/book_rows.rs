//! One book's numbers as rows: time, pages, pace, time left, days, and the
//! first and last day it was read. Host-free: the shell shows them in an
//! `ItemList` (the chart of the last days is drawn apart).

use alloc::vec::Vec;

use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_ui_kit::Item;

use crate::book_list::authors;
use crate::store::Book;
use crate::texts::{self, Words};
use crate::totals::BookTotals;

/// The rows of `book`, from its `totals`.
pub(crate) fn rows(
    words: &mut impl Words,
    book: &Book,
    totals: &BookTotals,
) -> Result<Vec<Item>, Error> {
    let mut rows: Vec<Item> = Vec::new();
    // At most: authors, time, pages, pace, left, days, first, last.
    rows.try_reserve_exact(8).map_err(|_| Error::NoMemory)?;
    let by = authors(&book.authors)?;
    if !by.is_empty() {
        rows.push(Item::row(&words.copy("stats.authors")?, &by)?);
    }
    rows.push(Item::row(
        &words.copy("stats.time-read")?,
        &texts::duration(words, totals.seconds)?,
    )?);
    let pages = if book.pages > 0 {
        words.format(
            "stats.pages-of",
            &[
                Arg::Int {
                    name: "read",
                    value: i64::from(totals.pages_read),
                },
                Arg::Int {
                    name: "pages",
                    value: i64::from(book.pages),
                },
            ],
        )?
    } else {
        words.format("stats.pages", &[Arg::Count(i64::from(totals.pages_read))])?
    };
    rows.push(Item::row(&words.copy("stats.pages-read")?, &pages)?);
    if totals.pages_read > 0 {
        let per_page = totals.seconds / u64::from(totals.pages_read);
        rows.push(Item::row(
            &words.copy("stats.pace")?,
            &words.format(
                "stats.per-page",
                &[Arg::Int {
                    name: "seconds",
                    value: per_page as i64,
                }],
            )?,
        )?);
        let left = book.pages.saturating_sub(totals.pages_read);
        if book.pages > 0 && left > 0 {
            rows.push(Item::row(
                &words.copy("stats.time-left")?,
                &texts::duration(words, per_page.saturating_mul(u64::from(left)))?,
            )?);
        }
    }
    rows.push(Item::row(
        &words.copy("stats.reading-days-title")?,
        &words.format("stats.days", &[Arg::Count(totals.days.len() as i64)])?,
    )?);
    if let (Some((first, _)), Some((last, _))) = (totals.days.first(), totals.days.last()) {
        rows.push(Item::row(
            &words.copy("stats.first-read")?,
            &texts::date(words, *first)?,
        )?);
        if last != first {
            rows.push(Item::row(
                &words.copy("stats.last-read-title")?,
                &texts::date(words, *last)?,
            )?);
        }
    }
    Ok(rows)
}
