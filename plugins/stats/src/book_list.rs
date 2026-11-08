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

/// The books the screen lists, as the file has them: the last [`MAX_ROWS`] read
/// of those with reading time, read one at a time and held cut to what a row
/// shows (title and authors, the other fields not at all), by id.
///
/// # Errors
/// `TooLarge` for a refused allocation, the storage's errors.
pub(crate) fn recent(
    storage: &dyn paperpaw_sdk::Storage,
) -> Result<Vec<Book>, paperpaw_sdk::StorageError> {
    let small = |error: Error| match error {
        Error::NoMemory => paperpaw_sdk::StorageError::TooLarge,
        _ => paperpaw_sdk::StorageError::InvalidData,
    };
    crate::store::most_recent(storage, MAX_ROWS, |book| {
        Ok(Book {
            id: book.id,
            title: cut(&book.title).map_err(small)?,
            authors: cut(&authors(&book.authors).map_err(small)?).map_err(small)?,
            last_open: book.last_open,
            ..Book::default()
        })
    })
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

/// The rows, in `places`' order (the texts as [`recent`] cut them): title, reading time, and below them the
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

    use crate::store::{self, PageStat};
    use crate::testing::heap::{fixture, peak};
    use paperpaw_sdk::MemoryStorage;

    /// A card with `n` books, each read once, opened in the order of their ids
    /// (the last one last); titles of 100 and authors of 40 bytes.
    fn library(n: u16) -> MemoryStorage {
        let storage = MemoryStorage::new();
        let books: Vec<Book> = (1..=n)
            .map(|id| Book {
                id,
                title: alloc::format!("{id:0>100}"),
                authors: alloc::format!("{id:0>20}\n{id:0>19}"),
                md5: "0123456789abcdef0123456789abcdef".to_string(),
                pages: 300,
                language: "en".to_string(),
                path: alloc::format!("Books/{id:0>40}.epub"),
                last_open: i64::from(id) * 100,
                ..Book::default()
            })
            .collect();
        store::save_books(&storage, &books).unwrap();
        for id in 1..=n {
            store::append(
                &storage,
                PageStat {
                    book: id,
                    page: 1,
                    start: 1_000_000 + u32::from(id),
                    duration: 30,
                    pages: 300,
                },
            )
            .unwrap();
        }
        storage
    }

    #[test]
    fn the_peak_of_reading_the_list_does_not_grow_with_the_library() {
        let small = fixture(|| library(300));
        let large = fixture(|| library(3000));
        let (shown_small, peak_small) = peak(|| recent(&small).unwrap());
        let (shown_large, peak_large) = peak(|| recent(&large).unwrap());
        assert_eq!(shown_small.len(), MAX_ROWS);
        assert_eq!(shown_large.len(), MAX_ROWS);
        // The same 200 rows' worth, a few strings more for the one being read.
        assert!(
            peak_large <= peak_small + 4096,
            "{peak_small} bytes for 300 books, {peak_large} for 3,000"
        );
        // And what 200 rows are: the books held (cut texts), the set of ids, the one read.
        assert!(peak_large < 96 * 1024, "{peak_large}");
        // What loading them all costs, to show the meter sees it.
        let (all, whole) = peak(|| store::load_books(&large).unwrap());
        assert_eq!(all.len(), 3000);
        assert!(whole > 3 * peak_large, "{whole} against {peak_large}");
    }

    #[test]
    fn counting_finding_and_naming_hold_one_book_at_a_time() {
        let large = fixture(|| library(3000));
        let (count, counting) = peak(|| store::count_books(&large).unwrap());
        assert_eq!(count, 3000);
        assert!(counting < 24 * 1024, "{counting}");
        let (found, finding) = peak(|| store::find_book(&large, 2500).unwrap());
        assert_eq!(found.unwrap().title, alloc::format!("{:0>100}", 2500));
        assert!(finding < 24 * 1024, "{finding}");
        let (titles, naming) =
            peak(|| store::titles(&large, &[7, 2999, 9999], |t| Ok(t.to_string())).unwrap());
        assert_eq!(
            titles[0].as_deref(),
            Some(alloc::format!("{:0>100}", 7).as_str())
        );
        assert!(titles[1].is_some());
        assert_eq!(titles[2], None);
        assert!(naming < 24 * 1024, "{naming}");
    }

    #[test]
    fn the_rows_are_the_last_read_cut_and_in_the_order_the_whole_list_gave() {
        let storage = fixture(|| library(500));
        let books = recent(&storage).unwrap();
        // Ids 301 to 500, by id; each as the screen shows it.
        assert_eq!(books.first().map(|b| b.id), Some(301));
        assert_eq!(books.last().map(|b| b.id), Some(500));
        assert!(books.iter().all(|b| b.md5.is_empty() && b.path.is_empty()));
        assert_eq!(
            books[0].title,
            alloc::format!("{}…", &alloc::format!("{:0>100}", 301)[..MAX_TEXT])
        );
        let seconds = crate::totals::seconds_per_book(&storage, &books).unwrap();
        let places = order(&books, &seconds).unwrap();
        // The last opened first.
        assert_eq!(books[places[0]].id, 500);
        assert_eq!(books[places[199]].id, 301);
    }
}
