//! Sums over the log, as KOReader's statistics compute them.

use alloc::vec;
use alloc::vec::Vec;

use paperpaw_sdk::{Storage, StorageError};

use crate::store::{self, Book, PageStat};

/// What one book's log adds up to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct BookTotals {
    pub seconds: u64,
    /// Distinct pages read, in the book's current pages.
    pub pages_read: u32,
    /// Reading time per local day, in order.
    pub days: Vec<(i32, u32)>,
}

/// The most pages a book is taken to have: page numbers come from other
/// devices' files too, and size the bits of the pages read (8 KiB at
/// most).
pub(crate) const MOST_PAGES: u32 = 1 << 16;

/// The pages of the book's current `pages` a stay on page `page` of
/// `then` pages stands for (KOReader's `page_stat` view rescales alike,
/// so the counts agree after a change of font size); never beyond
/// [`MOST_PAGES`].
pub(crate) fn rescaled(page: u32, then: u32, pages: u32) -> core::ops::RangeInclusive<u32> {
    let page = page.clamp(1, MOST_PAGES);
    if then == 0 || pages == 0 {
        return page..=page;
    }
    let then = then.min(MOST_PAGES);
    let (page, then, pages) = (
        u64::from(page.min(then)),
        u64::from(then),
        u64::from(pages.min(MOST_PAGES)),
    );
    let first = (page - 1) * pages / then + 1;
    let last = first.max(page * pages / then);
    first as u32..=last as u32
}

/// Marks the pages a stay on page `page` of `then` pages stands for in
/// `read`, a bit per page of the book's `pages` (see [`rescaled`]).
pub(crate) fn mark_read(
    read: &mut Vec<u8>,
    page: u32,
    then: u32,
    pages: u32,
) -> Result<(), StorageError> {
    for page in rescaled(page, then, pages) {
        let (byte, bit) = ((page / 8) as usize, page % 8);
        if read.len() <= byte {
            read.try_reserve(byte + 1 - read.len())
                .map_err(|_| StorageError::TooLarge)?;
            read.resize(byte + 1, 0);
        }
        if let Some(bits) = read.get_mut(byte) {
            *bits |= 1 << bit;
        }
    }
    Ok(())
}

/// The totals of `book`; `day_of` turns Unix seconds into local days.
/// Where the log cannot be read to the end, those of what could be.
pub(crate) fn of_book(
    storage: &dyn Storage,
    book: &Book,
    day_of: impl Fn(u32) -> i32,
) -> BookTotals {
    let mut totals = BookTotals::default();
    // Pages read, as bits.
    let mut read: Vec<u8> = Vec::new();
    #[expect(
        clippy::let_underscore_must_use,
        reason = "a log that cannot be read to the end counts what could be"
    )]
    let _ = store::scan(storage, |stat: &PageStat| {
        if stat.book != book.id {
            return;
        }
        totals.seconds += u64::from(stat.duration);
        let pages = if book.pages > 0 {
            book.pages
        } else {
            stat.pages
        };
        if mark_read(&mut read, stat.page, stat.pages, pages).is_err() {
            // Not counted: this walk does what the heap allows.
            return;
        }
        let day = day_of(stat.start);
        match totals.days.binary_search_by_key(&day, |(d, _)| *d) {
            Ok(i) => totals.days[i].1 = totals.days[i].1.saturating_add(u32::from(stat.duration)),
            Err(i) => totals.days.insert(i, (day, u32::from(stat.duration))),
        }
    });
    totals.pages_read = read.iter().map(|b| b.count_ones()).sum();
    totals
}

/// Reading time of each of `books` (sorted by id), for lists; where the
/// log cannot be read to the end, that of what could be.
pub(crate) fn seconds_per_book(storage: &dyn Storage, books: &[Book]) -> Vec<u64> {
    let mut seconds = vec![0u64; books.len()];
    #[expect(
        clippy::let_underscore_must_use,
        reason = "a log that cannot be read to the end counts what could be, as said above"
    )]
    let _ = store::scan(storage, |stat| {
        if let Ok(i) = books.binary_search_by_key(&stat.book, |b| b.id) {
            seconds[i] += u64::from(stat.duration);
        }
    });
    seconds
}

/// The days in a row up to `today` (or yesterday, while today has no
/// reading yet) that have reading.
pub(crate) fn streak(days: &[store::Day], today: i32) -> u32 {
    streaks(days, today, 1).current
}

/// Days read in a row: up to today, and the most there ever were.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Streaks {
    /// Up to `today`, or up to yesterday while today does not count yet.
    pub current: u32,
    pub longest: u32,
}

/// The streaks of days with at least `least` seconds of reading (any
/// reading for 0 or 1, the daily goal for more); days after `today` are
/// left out.
pub(crate) fn streaks(days: &[store::Day], today: i32, least: u32) -> Streaks {
    let least = least.max(1);
    let (mut longest, mut run, mut last) = (0, 0, None);
    for day in days.iter().filter(|d| d.seconds >= least && d.day <= today) {
        run = if last == Some(day.day - 1) {
            run + 1
        } else {
            1
        };
        longest = longest.max(run);
        last = Some(day.day);
    }
    let counts = |day: i32| {
        days.binary_search_by_key(&day, |d| d.day)
            .is_ok_and(|i| days[i].seconds >= least)
    };
    let mut day = if counts(today) { today } else { today - 1 };
    let mut current = 0;
    while counts(day) {
        current += 1;
        day -= 1;
    }
    Streaks { current, longest }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Day;
    use paperpaw_sdk::MemoryStorage;

    #[test]
    fn pages_rescale_like_koreader() {
        // Read in 100 pages, now 200: page 10 stands for 19 and 20.
        assert_eq!(rescaled(10, 100, 200), 19..=20);
        // Read in 200, now 100: pages 19 and 20 both stand for page 10.
        assert_eq!(rescaled(19, 200, 100), 10..=10);
        assert_eq!(rescaled(20, 200, 100), 10..=10);
        assert_eq!(rescaled(5, 100, 100), 5..=5);
    }

    /// Page numbers from a damaged or hostile file stay within a book's
    /// pages, and the bits of the pages read small.
    #[test]
    fn page_numbers_are_clamped() {
        assert_eq!(rescaled(u32::MAX, 0, 0), MOST_PAGES..=MOST_PAGES);
        assert_eq!(rescaled(u32::MAX, 100, 100), 100..=100);
        assert_eq!(rescaled(1, u32::MAX, u32::MAX), 1..=1);
        assert_eq!(*rescaled(u32::MAX, u32::MAX, u32::MAX).end(), MOST_PAGES);
        let mut read = Vec::new();
        mark_read(&mut read, u32::MAX, u32::MAX, u32::MAX).unwrap();
        mark_read(&mut read, 4_000_000_000, 0, 0).unwrap();
        assert!(read.len() <= MOST_PAGES as usize / 8 + 1);
    }

    #[test]
    fn books_add_up() {
        let storage = MemoryStorage::new();
        let stat = |page, start, duration, pages| PageStat {
            book: 1,
            page,
            start,
            duration,
            pages,
        };
        store::append(&storage, stat(1, 100, 30, 100)).unwrap();
        store::append(&storage, stat(2, 140, 40, 100)).unwrap();
        // The same page again, the next day.
        store::append(&storage, stat(2, 1000, 20, 100)).unwrap();
        // Another book, and one of an id no book has.
        for book in [2, u16::MAX] {
            let other = PageStat {
                book,
                ..stat(9, 1100, 50, 100)
            };
            store::append(&storage, other).unwrap();
        }
        let book = |id| Book {
            id,
            pages: 100,
            ..Book::default()
        };
        let totals = of_book(&storage, &book(1), |unix| (unix / 500) as i32);
        assert_eq!(totals.seconds, 90);
        assert_eq!(totals.pages_read, 2);
        assert_eq!(totals.days, [(0, 70), (2, 20)]);
        assert_eq!(seconds_per_book(&storage, &[book(1), book(2)]), [90, 50]);
    }

    #[test]
    fn streaks_count_days_in_a_row() {
        let days: Vec<Day> = [3, 5, 6, 7]
            .into_iter()
            .map(|day| Day {
                day,
                seconds: 60,
                stays: 1,
            })
            .collect();
        assert_eq!(streak(&days, 7), 3);
        // Not read yet today: the streak up to yesterday still counts.
        assert_eq!(streak(&days, 8), 3);
        assert_eq!(streak(&days, 9), 0);
    }

    #[test]
    fn streaks_count_the_days_that_reach_the_goal() {
        let days: Vec<Day> = [(1, 600), (2, 900), (3, 1200), (4, 100), (6, 900), (7, 1000)]
            .into_iter()
            .map(|(day, seconds)| Day {
                day,
                seconds,
                stays: 1,
            })
            .collect();
        // Any reading: 1 to 4 in a row, then 6 and 7.
        assert_eq!(
            streaks(&days, 7, 0),
            Streaks {
                current: 2,
                longest: 4
            }
        );
        // A goal of 15 minutes: day 1 and day 4 fall short.
        let goal = streaks(&days, 7, 900);
        assert_eq!((goal.current, goal.longest), (2, 2));
        // Today not done yet: up to yesterday.
        assert_eq!(streaks(&days, 8, 900).current, 2);
        assert_eq!(streaks(&days, 9, 900).current, 0);
        // Days after today do not count.
        assert_eq!(streaks(&days, 3, 0).longest, 3);
        assert_eq!(streaks(&[], 3, 0), Streaks::default());
    }
}
