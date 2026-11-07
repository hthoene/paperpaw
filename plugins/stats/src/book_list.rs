//! The books screen's rows: every book with reading time, the last one read
//! first. Host-free: the shell shows them in an `ItemList`.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_ui_kit::Item;

use crate::store::Book;
use crate::texts::{self, Words};

/// The places in `books` of those with reading time (`seconds` by place),
/// the last one opened first; books opened at the same time in the order of
/// their ids.
pub(crate) fn order(books: &[Book], seconds: &[u64]) -> Result<Vec<usize>, Error> {
    let mut places: Vec<usize> = Vec::new();
    places
        .try_reserve_exact(books.len())
        .map_err(|_| Error::NoMemory)?;
    places.extend((0..books.len()).filter(|&at| seconds.get(at).is_some_and(|s| *s > 0)));
    places.sort_unstable_by_key(|&at| {
        books.get(at).map_or((core::cmp::Reverse(0), 0), |b| {
            (core::cmp::Reverse(b.last_open), b.id)
        })
    });
    Ok(places)
}

/// The rows, in `places`' order: title, reading time, and below them the
/// authors and when it was last read; `day_of` turns Unix seconds into a local
/// day. A book's last open is taken as a `u32` of Unix seconds, as in interface
/// 6 (a later time wraps).
pub(crate) fn items(
    words: &mut impl Words,
    books: &[Book],
    seconds: &[u64],
    places: &[usize],
    day_of: impl Fn(u32) -> i32,
) -> Result<Vec<Item>, Error> {
    let mut items = Vec::new();
    items
        .try_reserve_exact(places.len())
        .map_err(|_| Error::NoMemory)?;
    for &at in places {
        let (Some(book), Some(read)) = (books.get(at), seconds.get(at)) else {
            continue;
        };
        let mut note = authors(&book.authors)?;
        if book.last_open > 0 {
            let day = day_of(book.last_open as u32);
            let date = texts::date(words, day)?;
            let last = words.format(
                "stats.last-read",
                &[Arg::Text {
                    name: "date",
                    value: &date,
                }],
            )?;
            if !note.is_empty() {
                push(&mut note, " · ")?;
            }
            push(&mut note, &last)?;
        }
        let value = texts::duration(words, *read)?;
        items.push(Item::noted(&book.title, &value, &note)?);
    }
    Ok(items)
}

/// Authors as shown: on one line, nothing for KOReader's `N/A`.
pub(crate) fn authors(authors: &str) -> Result<String, Error> {
    let mut out = String::new();
    if authors == "N/A" {
        return Ok(out);
    }
    out.try_reserve_exact(authors.len().saturating_add(authors.matches('\n').count()))
        .map_err(|_| Error::NoMemory)?;
    for (n, one) in authors.split('\n').enumerate() {
        if n > 0 {
            out.push_str(", ");
        }
        out.push_str(one);
    }
    Ok(out)
}

fn push(to: &mut String, text: &str) -> Result<(), Error> {
    to.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
    to.push_str(text);
    Ok(())
}
