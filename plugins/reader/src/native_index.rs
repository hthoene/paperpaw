//! The count of a book's pages (view slice R2b): the start of every page of
//! one book in one layout, laid out a few at a time in the background and
//! kept in a file of 8 bytes a page (`paperpaw_sdk::reading::PageIndex`, as
//! interface 6 kept it), so that a book of thousands of pages needs no RAM
//! for it. Another layout (or another edition of the book) has a file of its
//! own; the file of the layout left is removed.

use alloc::string::String;
use core::fmt::Write as _;

use paperpaw_sdk::document::Locator;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::reading::PageIndex;

use crate::native_pages::Reading;
use paperpaw_sdk::Storage;
use paperpaw_sdk::digest::fnv1a64;

/// Pages laid out in one step (a step is one `core.work` run).
pub const PAGES_PER_STEP: usize = 8;
/// The same on a small device (the X4 lays a page out slowly).
pub const PAGES_PER_STEP_SMALL: usize = 1;
/// The wait between two steps, in milliseconds.
pub const DELAY_MS: u32 = 2;
/// The same on a small device.
pub const DELAY_SMALL_MS: u32 = 500;
/// The key of the work that counts.
pub const WORK_KEY: &str = "count";

/// One page, as a step needs to know it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Laid {
    pub start: Locator,
    pub end: Locator,
    /// The place is past the end of the book: there is no page.
    pub past_end: bool,
}

/// What one step did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// More pages remain.
    More,
    /// The book is counted to its end.
    Done,
    /// A page could not be laid out or the file takes no more: the pages
    /// counted so far stay in use.
    Stopped,
}

/// A display of this many pixels or fewer is a small device's (the X4's 800 by
/// 480): the native snapshot does not tell the memory class (`PORTING-7.md`).
const SMALL_PIXELS: u32 = 800 * 480;

/// Pages a step lays out and the wait before the next, for a display of
/// `width` by `height` pixels.
#[must_use]
pub const fn pace(width: u16, height: u16) -> (usize, u32) {
    if width as u32 * height as u32 <= SMALL_PIXELS {
        (PAGES_PER_STEP_SMALL, DELAY_SMALL_MS)
    } else {
        (PAGES_PER_STEP, DELAY_MS)
    }
}

/// What a layout is made of, as text: the settings and the display, in whole
/// numbers (no float formatting on the small device).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn layout_text(
    reading: &Reading,
    display: (u16, u16, u16),
    whole_pages: bool,
) -> Result<String, Error> {
    let mut text = String::new();
    text.try_reserve(64).map_err(|_| Error::NoMemory)?;
    write!(
        text,
        "{}/{}/{}/{}/{}/{}/{}x{}x{}/{}",
        (reading.size_pt * 10.0) as u32,
        reading.line_spacing,
        reading.word_spacing,
        reading.margin,
        reading.margin_ends,
        u8::from(reading.justify),
        display.0,
        display.1,
        display.2,
        u8::from(whole_pages),
    )
    .map_err(|_| Error::NoMemory)?;
    Ok(text)
}

/// The file of the index of this book in this layout: the book's path, the
/// conversion its places belong to (an edition) and the layout's key tell
/// them apart.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn file_name(path: &str, conversion: u64, layout: &str) -> Result<String, Error> {
    let mut hashed = String::new();
    hashed
        .try_reserve(path.len() + layout.len() + 24)
        .map_err(|_| Error::NoMemory)?;
    hashed.push_str(path);
    write!(hashed, "|{conversion:x}|").map_err(|_| Error::NoMemory)?;
    hashed.push_str(layout);
    let mut name = String::new();
    name.try_reserve(26).map_err(|_| Error::NoMemory)?;
    write!(name, "pages-{:016x}.bin", fnv1a64(&hashed)).map_err(|_| Error::NoMemory)?;
    Ok(name)
}

/// The count of one open book in one layout.
#[derive(Debug)]
pub struct Counter {
    index: PageIndex,
    name: String,
    /// Where the next step goes on (none: after the last page counted).
    next: Option<Locator>,
    stopped: bool,
}

impl Counter {
    /// The index in the file `name`, as far as it goes.
    #[must_use]
    pub fn open(storage: &dyn Storage, name: String) -> Self {
        Self {
            index: PageIndex::open(storage, name.clone()),
            name,
            next: None,
            stopped: false,
        }
    }

    /// Whether pages remain to be counted (a stopped count has none to wait
    /// for).
    #[must_use]
    pub fn counting(&self) -> bool {
        !self.stopped && !self.index.is_complete()
    }

    /// The file's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The page that starts at `at` and the pages of the book, counted from 1,
    /// once the count is complete and `at` is the start of a page.
    #[must_use]
    pub fn page_at(&self, storage: &dyn Storage, at: (u32, u32)) -> Option<(u32, u32)> {
        if !self.index.is_complete() {
            return None;
        }
        let (page, exact) = self.index.locate(storage, Locator::new(at.0, at.1))?;
        exact.then(|| (page + 1, self.index.pages()))
    }

    /// The pages of the book, once counted.
    #[must_use]
    pub fn pages(&self) -> Option<u32> {
        self.index.is_complete().then(|| self.index.pages())
    }

    /// The count is thrown away (a page the index promised did not come).
    pub fn forget(self, storage: &dyn Storage) {
        // A file that stays takes room, not data; the next layout names its own.
        storage.remove(&self.name).ok();
    }

    /// Lays out up to `per_step` pages with `lay` and adds them.
    pub fn step(
        &mut self,
        storage: &dyn Storage,
        per_step: usize,
        lay: &mut dyn FnMut(Locator) -> Result<Laid, Error>,
    ) -> Step {
        if self.stopped {
            return Step::Stopped;
        }
        if self.index.is_complete() {
            return Step::Done;
        }
        let Some(mut next) = self.resume(storage, lay) else {
            self.stopped = true;
            return Step::Stopped;
        };
        let mut starts = [Locator::START; PAGES_PER_STEP];
        let mut n = 0;
        let mut done = false;
        for slot in starts.iter_mut().take(per_step.min(PAGES_PER_STEP)) {
            let Ok(page) = lay(next) else {
                self.stopped = true;
                return Step::Stopped;
            };
            if page.past_end {
                done = true;
                break;
            }
            *slot = page.start;
            n += 1;
            if page.end <= page.start {
                // A page that ends where it started would never move on.
                done = true;
                break;
            }
            next = page.end;
        }
        if n > 0 && !self.index.append(storage, &starts[..n]) {
            self.stopped = true;
            return Step::Stopped;
        }
        self.next = Some(next);
        if done {
            self.index.finish(storage);
            if !self.index.is_complete() {
                self.stopped = true;
                return Step::Stopped;
            }
            return Step::Done;
        }
        Step::More
    }

    /// Where the count goes on: after the last page of the file.
    fn resume(
        &self,
        storage: &dyn Storage,
        lay: &mut dyn FnMut(Locator) -> Result<Laid, Error>,
    ) -> Option<Locator> {
        if let Some(next) = self.next {
            return Some(next);
        }
        match self.index.last(storage) {
            Some(last) => lay(last).ok().map(|page| page.end),
            None if self.index.pages() == 0 => Some(Locator::START),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    /// A book of `pages` pages, 10 characters each, in one section.
    fn book(pages: u32) -> impl FnMut(Locator) -> Result<Laid, Error> {
        move |at| {
            let page = at.offset / 10;
            Ok(Laid {
                start: at,
                end: Locator::new(at.section, at.offset + 10),
                past_end: page >= pages,
            })
        }
    }

    #[test]
    fn counts_in_steps_and_finds_the_page() {
        let storage = MemoryStorage::new();
        let mut counter = Counter::open(&storage, String::from("pages-x.bin"));
        let mut lay = book(20);
        assert!(counter.counting());
        assert_eq!(counter.step(&storage, 8, &mut lay), Step::More);
        assert_eq!(counter.step(&storage, 8, &mut lay), Step::More);
        assert_eq!(counter.page_at(&storage, (0, 0)), None);
        assert_eq!(counter.step(&storage, 8, &mut lay), Step::Done);
        assert!(!counter.counting());
        assert_eq!(counter.pages(), Some(20));
        assert_eq!(counter.page_at(&storage, (0, 0)), Some((1, 20)));
        assert_eq!(counter.page_at(&storage, (0, 190)), Some((20, 20)));
        assert_eq!(counter.page_at(&storage, (0, 195)), None);
    }

    #[test]
    fn goes_on_from_the_file_after_a_new_open() {
        let storage = MemoryStorage::new();
        let mut first = Counter::open(&storage, String::from("pages-y.bin"));
        assert_eq!(first.step(&storage, 8, &mut book(12)), Step::More);
        let mut again = Counter::open(&storage, String::from("pages-y.bin"));
        assert_eq!(again.step(&storage, 8, &mut book(12)), Step::Done);
        assert_eq!(again.pages(), Some(12));
    }

    #[test]
    fn a_page_that_cannot_be_laid_out_stops_the_count_and_keeps_the_pages() {
        let storage = MemoryStorage::new();
        let mut counter = Counter::open(&storage, String::from("pages-z.bin"));
        let mut lay = |at: Locator| {
            if at.offset >= 30 {
                Err(Error::NoMemory)
            } else {
                book(100)(at)
            }
        };
        assert_eq!(counter.step(&storage, 8, &mut lay), Step::Stopped);
        assert!(!counter.counting());
        assert_eq!(counter.pages(), None);
    }

    #[test]
    fn a_small_device_lays_out_a_page_a_step() {
        let storage = MemoryStorage::new();
        let mut counter = Counter::open(&storage, String::from("pages-s.bin"));
        let mut lay = book(3);
        for _ in 0..3 {
            assert_eq!(
                counter.step(&storage, PAGES_PER_STEP_SMALL, &mut lay),
                Step::More
            );
        }
        assert_eq!(
            counter.step(&storage, PAGES_PER_STEP_SMALL, &mut lay),
            Step::Done
        );
        assert_eq!(counter.pages(), Some(3));
    }

    #[test]
    fn the_name_follows_book_edition_and_layout() {
        let a = file_name("/b.epub", 1, "12/100").unwrap();
        assert_eq!(a, file_name("/b.epub", 1, "12/100").unwrap());
        assert_ne!(a, file_name("/b.epub", 2, "12/100").unwrap());
        assert_ne!(a, file_name("/b.epub", 1, "14/100").unwrap());
        assert_ne!(a, file_name("/c.epub", 1, "12/100").unwrap());
        assert!(a.starts_with("pages-") && a.ends_with(".bin"));
    }
}
