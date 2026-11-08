//! The books screen's rows: the books with reading time, the last 200 read,
//! the last one read first. Host-free: the shell shows them in an `ItemList`.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_ui_kit::Item;

use crate::store::Book;
use crate::texts::{self, Words};

/// The most rows the books screen keeps: the 200 books read last. The list holds
/// each row's texts for as long as the screen is open, on a heap of 256 KiB
/// (interface 6 kept every book).
pub(crate) const MAX_ROWS: usize = 200;

/// The longest title or author line kept, in bytes: what a row can show anyway.
pub(crate) const MAX_TEXT: usize = 80;

/// `text` cut to [`MAX_TEXT`] bytes at a character, `…` after a cut one.
pub(crate) fn cut(text: &str) -> Result<String, Error> {
    let mut end = text.len().min(MAX_TEXT);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let cut = end < text.len();
    let mut out = String::new();
    out.try_reserve_exact(end + if cut { '…'.len_utf8() } else { 0 })
        .map_err(|_| Error::NoMemory)?;
    out.push_str(text.get(..end).unwrap_or_default());
    if cut {
        out.push('…');
    }
    Ok(out)
}

/// The places in `books` of those with reading time (`seconds` by place),
/// the last one opened first (at most [`MAX_ROWS`]); books opened at the same
/// time in the order of their ids.
pub(crate) fn order(books: &[Book], seconds: &[u64]) -> Result<Vec<usize>, Error> {
    let mut places: Vec<usize> = Vec::new();
    places
        .try_reserve_exact(books.len())
        .map_err(|_| Error::NoMemory)?;
    places.extend((0..books.len()).filter(|&at| seconds.get(at).is_some_and(|s| *s > 0)));
    paperpaw_sdk::sort::sort_by_key(&mut places, |&at| {
        books.get(at).map_or((core::cmp::Reverse(0), 0), |b| {
            (core::cmp::Reverse(b.last_open), b.id)
        })
    });
    places.truncate(MAX_ROWS);
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
        let mut note = cut(&authors(&book.authors)?)?;
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
        items.push(Item::noted(&cut(&book.title)?, &value, &note)?);
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

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    fn book(id: u16, last_open: i64, title: &str) -> Book {
        Book {
            id,
            title: title.to_string(),
            last_open,
            ..Book::default()
        }
    }

    #[test]
    fn a_title_is_cut_at_a_character_and_marked() {
        assert_eq!(cut("Short").unwrap(), "Short");
        let exact = "a".repeat(MAX_TEXT);
        assert_eq!(cut(&exact).unwrap(), exact);
        let long = "a".repeat(MAX_TEXT + 1);
        assert_eq!(
            cut(&long).unwrap(),
            alloc::format!("{}…", "a".repeat(MAX_TEXT))
        );
        // Two-byte letters: the cut falls between characters, never inside one.
        let wide = "ж".repeat(100);
        let out = cut(&wide).unwrap();
        assert_eq!(out, alloc::format!("{}…", "ж".repeat(MAX_TEXT / 2)));
        // Four-byte signs, cut off-boundary at 80 = 20 signs exactly and at 81.
        let emoji = "😀".repeat(30);
        assert_eq!(cut(&emoji).unwrap(), alloc::format!("{}…", "😀".repeat(20)));
        assert_eq!(cut("").unwrap(), "");
    }

    #[test]
    fn only_the_last_read_books_are_listed_most_recent_first() {
        let books: Vec<Book> = (0..300u16)
            .map(|id| book(id, i64::from(id) * 10, "t"))
            .collect();
        let seconds = alloc::vec![60u64; books.len()];
        let places = order(&books, &seconds).unwrap();
        assert_eq!(places.len(), MAX_ROWS);
        // The newest first, and the oldest 100 are not there.
        assert_eq!(places.first().copied(), Some(299));
        assert_eq!(places.last().copied(), Some(100));
    }

    #[test]
    fn books_without_reading_time_are_not_listed_and_the_cap_counts_the_rest() {
        let books: Vec<Book> = (0..250u16).map(|id| book(id, i64::from(id), "t")).collect();
        let seconds: Vec<u64> = (0..250u64).map(|id| id % 2).collect();
        let places = order(&books, &seconds).unwrap();
        assert_eq!(places.len(), 125);
        assert!(places.iter().all(|&at| at % 2 == 1));
    }

    #[test]
    fn the_calendar_keeps_an_id_and_a_cut_title_only() {
        let mut one = book(7, 5, &"x".repeat(200));
        one.authors = "A\nB".to_string();
        one.path = "Books/a.epub".to_string();
        let named = crate::calendar_model::names(&[one]).unwrap();
        assert_eq!(named.len(), 1);
        assert_eq!(named[0].id, 7);
        assert_eq!(named[0].title.len(), MAX_TEXT + '…'.len_utf8());
    }
}
