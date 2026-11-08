//! Recording reading: the host-free half of the plugin's events.
//!
//! The same rules as interface 6 (see the crate's documentation): a stay on
//! a page is timed on the monotonic clock and dated by the wall clock once the
//! device knows it; shorter ones than *Shortest page* are left out and longer
//! ones count as *Longest page*; reading pauses while the device sleeps and
//! ends when the book is closed. What the recorder asks of its host is
//! [`Surroundings`]; what it has to say is [`Told`].
//!
//! Differences from interface 6, on purpose (see `PORTING-7.md`): a book is
//! added with its title as the reader shows it (the event has no authors
//! or language: *N/A*, as for a book KOReader knows no file of), and an
//! allocation the heap refuses leaves the book out instead of aborting.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::digest::md5;
use paperpaw_sdk::{Storage, StorageError};

use crate::store::{self, Book, PageStat};
use crate::{fmt, room};

/// Most stays kept while the device does not know the time (about 4 KiB):
/// an hour or two of reading.
pub(crate) const UNDATED: usize = 128;

/// The settings the recorder follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Limits {
    pub enabled: bool,
    /// Stays shorter than this many seconds are not counted.
    pub min: i64,
    /// Stays count at most this many seconds.
    pub max: i64,
}

/// What the recorder asks of its host.
pub(crate) trait Surroundings {
    /// The plugin's data folder.
    fn data(&self) -> &dyn Storage;
    /// The user's files, where the books are (read only).
    fn files(&self) -> &dyn Storage;
    /// Now on the monotonic millisecond clock.
    fn now_ms(&self) -> u64;
    /// Unix seconds of a moment on that clock, once the device knows the
    /// time.
    fn unix_at(&self, at_ms: u64) -> Option<i64>;
    /// The local day (days since 1970) of a moment in Unix seconds.
    fn day_of(&self, unix: u32) -> i32;
    fn limits(&self) -> Limits;
}

/// A reading event: the book and where in it the reader is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Reading<'a> {
    pub path: &'a str,
    pub title: &'a str,
    /// Progress in millionths.
    pub progress_ppm: u32,
    pub page: Option<u32>,
    pub pages: Option<u32>,
    /// The position: section, offset in it, insert (see the event).
    pub at: (u32, u32, u32),
}

/// Why a question about a day is not answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Asked {
    /// Not a day (a wrong number of bytes).
    Invalid,
    /// A day not known: not today, or the date unknown, or not counted yet.
    Unknown,
}

/// What the recorder says after an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Told {
    /// The statistics could not be saved (said once until they can again).
    pub not_saved: bool,
    /// Today and the seconds read on it, when that changed (for the reading
    /// goal).
    pub today: Option<(i32, u32)>,
}

/// The book being read and the page it is on.
#[derive(Debug, Clone)]
struct Session {
    /// Its id in `books.ini`; `None` until the device knows the time (a
    /// book is added with when it was opened).
    book: Option<u16>,
    /// Which of the recorder's names it is (see [`Recorder::names`]).
    name: usize,
    page: u32,
    pages: u32,
    /// Where the page begins: pages counted by the progress may share a
    /// number.
    at: (u32, u32, u32),
    /// When reading the page began; `None` while paused.
    since: Option<u64>,
}

/// A stay on a page, timed on the monotonic clock: dated once the device
/// knows the time.
#[derive(Debug, Clone)]
struct Stay {
    name: usize,
    page: u32,
    pages: u32,
    start: u64,
    /// Seconds, as counted (see [`counted`]).
    duration: u16,
}

/// The recorder's state.
#[derive(Debug, Default)]
pub(crate) struct Recorder {
    session: Option<Session>,
    /// The path and title of the books in the session and the stays waiting:
    /// taken once each, and only as many as there are (a book is told by its
    /// path).
    names: Vec<(String, String)>,
    /// Stays read while the device did not know the time (or not written
    /// yet), oldest first, at most [`UNDATED`]; gone if the device is
    /// switched off before.
    undated: Vec<Stay>,
    /// Writing failed: `Some(false)` until that was said, `Some(true)`
    /// after; `None` while writing works.
    write_failed: Option<bool>,
    /// The day and the seconds read on it, summed once from the records and
    /// counted on as stays are recorded.
    today: Option<(i32, u32)>,
    /// What was told last of today.
    told: Option<(i32, u32)>,
}

/// The page and the number of pages of a reading event: as the reader
/// counted them, else the progress in tenths of a percent of 1000 pages
/// (KOReader rescales pages to the book's count when it sums them).
pub(crate) fn page_of(reading: &Reading<'_>) -> (u32, u32) {
    if let (Some(page), Some(pages)) = (reading.page, reading.pages)
        && pages > 0
    {
        return (page.clamp(1, pages), pages);
    }
    ((reading.progress_ppm / 1000 + 1).clamp(1, 1000), 1000)
}

/// How long a stay counts: nothing if shorter than the minimum, at most the
/// maximum (KOReader's rule).
pub(crate) fn counted(seconds: i64, min: i64, max: i64) -> u16 {
    if seconds < min {
        0
    } else {
        seconds.min(max).clamp(0, i64::from(u16::MAX)) as u16
    }
}

impl Recorder {
    /// Whether the book at `path` is the one being read.
    pub(crate) fn reading(&self, path: &str) -> bool {
        self.session
            .as_ref()
            .and_then(|s| self.names.get(s.name))
            .is_some_and(|(own, _)| own == path)
    }

    /// The name of the book at `path`, noted if it is new; `None` where there
    /// is no room for it.
    fn name(&mut self, path: &str, title: &str) -> Option<usize> {
        if let Some(at) = self.names.iter().position(|(own, _)| own == path) {
            return Some(at);
        }
        let (path, title) = (room::string(&[path]).ok()?, room::string(&[title]).ok()?);
        room::push(&mut self.names, (path, title)).ok()?;
        Some(self.names.len() - 1)
    }

    /// Forgets the names nothing waits for: all but the session's and those of
    /// the stays still undated (at most [`UNDATED`], so at most one more
    /// name), however many books are opened before the device knows the time.
    /// Names move to the front in their order; the holders follow.
    fn prune(&mut self) {
        let mut kept = 0;
        for old in 0..self.names.len() {
            let used = self.session.as_ref().is_some_and(|s| s.name == old)
                || self.undated.iter().any(|stay| stay.name == old);
            if !used {
                continue;
            }
            self.names.swap(kept, old);
            if let Some(session) = self.session.as_mut().filter(|s| s.name == old) {
                session.name = kept;
            }
            for stay in self.undated.iter_mut().filter(|stay| stay.name == old) {
                stay.name = kept;
            }
            kept += 1;
        }
        self.names.truncate(kept);
    }

    fn path_title(&self, name: usize) -> (&str, &str) {
        self.names
            .get(name)
            .map_or(("", ""), |(path, title)| (path, title))
    }

    /// The id of the book being read, if it has one.
    pub(crate) fn current_book(&self) -> Option<u16> {
        self.session.as_ref().and_then(|session| session.book)
    }

    /// What `result` of writing holds; `None` where it failed, which is said
    /// once (see [`Recorder::tell`]).
    fn wrote<T>(&mut self, result: Result<T, StorageError>) -> Option<T> {
        match result {
            Ok(value) => {
                self.write_failed = None;
                Some(value)
            }
            Err(_) => {
                self.write_failed.get_or_insert(false);
                None
            }
        }
    }

    /// Says that the statistics could not be saved (once until writing works
    /// again) and how long was read today, when it changed.
    pub(crate) fn tell(&mut self, cx: &dyn Surroundings) -> Told {
        let mut told = Told::default();
        if self.write_failed == Some(false) {
            told.not_saved = true;
            self.write_failed = Some(true);
        }
        if let Some(today) = self.read_today(cx)
            && self.told != Some(today)
        {
            self.told = Some(today);
            told.today = Some(today);
        }
        told
    }

    /// Today and the seconds read on it, if the device knows the date:
    /// summed from the records on a new day, else as counted on.
    pub(crate) fn read_today(&mut self, cx: &dyn Surroundings) -> Option<(i32, u32)> {
        let day = today(cx)?;
        if let Some(today) = self.today.filter(|(d, _)| *d == day) {
            return Some(today);
        }
        // Not read: nothing is cached (a zero would be told as today's).
        let days = store::days(cx.data(), |unix| cx.day_of(unix)).ok()?;
        let seconds = days.iter().find(|d| d.day == day).map_or(0, |d| d.seconds);
        self.today = Some((day, seconds));
        self.today
    }

    /// The answer to the stage `reading-goal.read-today`: the day (as asked:
    /// `input` is empty for today, else a day as an `i32`, little-endian) and the
    /// seconds read on it, from what is held in memory: nothing is read from the
    /// card here. A day that is not the one held (today, since the last event
    /// that told) is not known.
    pub(crate) fn asked(&self, cx: &dyn Surroundings, input: &[u8]) -> Result<[u8; 8], Asked> {
        let day = match input {
            [] => today(cx).ok_or(Asked::Unknown)?,
            _ => i32::from_le_bytes(input.try_into().map_err(|_| Asked::Invalid)?),
        };
        let (_, seconds) = self
            .today
            .filter(|(held, _)| *held == day)
            .ok_or(Asked::Unknown)?;
        let mut bytes = [0u8; 8];
        bytes[..4].copy_from_slice(&day.to_le_bytes());
        bytes[4..].copy_from_slice(&seconds.to_le_bytes());
        Ok(bytes)
    }

    /// Ends the stay on the current page now: recorded, or kept until the
    /// device knows the time.
    pub(crate) fn finish_page(&mut self, cx: &dyn Surroundings) {
        let Some(session) = &mut self.session else {
            return;
        };
        let Some(since) = session.since.take() else {
            return;
        };
        let limits = cx.limits();
        let seconds = i64::try_from(cx.now_ms().saturating_sub(since) / 1000).unwrap_or(i64::MAX);
        let duration = counted(seconds, limits.min, limits.max);
        if duration == 0 || !limits.enabled {
            return;
        }
        if self.undated.len() < UNDATED && self.undated.try_reserve(1).is_ok() {
            self.undated.push(Stay {
                name: session.name,
                page: session.page,
                pages: session.pages,
                start: since,
                duration,
            });
        }
        self.date(cx);
    }

    /// Once the device knows the time: records the stays kept until then,
    /// and adds the book being read to the books if it is not yet.
    pub(crate) fn date(&mut self, cx: &dyn Surroundings) {
        if cx.unix_at(cx.now_ms()).is_none() {
            return;
        }
        if self.undated.is_empty() && self.session.as_ref().is_none_or(|s| s.book.is_some()) {
            return;
        }
        let unix = |at: u64| cx.unix_at(at).unwrap_or(0);
        // Each book's id, looked up (or the book added) once.
        let mut ids: Vec<(usize, Option<u16>)> = Vec::new();
        if let Some(Session {
            name,
            book: Some(book),
            ..
        }) = &self.session
            && ids.try_reserve(1).is_ok()
        {
            ids.push((*name, Some(*book)));
        }
        let mut stays = core::mem::take(&mut self.undated).into_iter();
        while let Some(stay) = stays.next() {
            let start = unix(stay.start);
            let book = match ids.iter().find(|(name, _)| *name == stay.name) {
                Some(&(_, id)) => id,
                None => {
                    let (path, title) = self.path_title(stay.name);
                    let id = register_book(cx, path, title, start, stay.pages);
                    if ids.try_reserve(1).is_ok() {
                        ids.push((stay.name, id));
                    }
                    id
                }
            };
            if let (Some(book), Ok(start @ 1..)) = (book, u32::try_from(start)) {
                let stat = PageStat {
                    book,
                    page: stay.page,
                    start,
                    duration: stay.duration,
                    pages: stay.pages,
                };
                let recorded = self.wrote(store::append(cx.data(), stat));
                if let Some((day, seconds)) = &mut self.today
                    && recorded.is_some()
                    && cx.day_of(start) == *day
                {
                    *seconds = seconds.saturating_add(u32::from(stay.duration));
                }
                if recorded.is_none() {
                    // Kept to be written with the next page.
                    if self.undated.try_reserve(1).is_ok() {
                        self.undated.push(stay);
                    }
                    for rest in stays {
                        if self.undated.try_reserve(1).is_err() {
                            break;
                        }
                        self.undated.push(rest);
                    }
                    break;
                }
            }
        }
        if let Some(session) = &mut self.session
            && session.book.is_none()
        {
            session.book = match ids.iter().find(|(name, _)| *name == session.name) {
                Some(&(_, id)) => id,
                None => {
                    let (path, title) = self
                        .names
                        .get(session.name)
                        .map_or(("", ""), |(p, t)| (p, t));
                    register_book(cx, path, title, unix(cx.now_ms()), session.pages)
                }
            };
            // A book that cannot be read is not recorded.
            if session.book.is_none() {
                self.session = None;
            }
        }
        self.prune();
    }

    /// The reader shows the page of `reading`.
    pub(crate) fn turn(&mut self, cx: &dyn Surroundings, reading: &Reading<'_>) {
        let ((page, pages), at) = (page_of(reading), reading.at);
        let unchanged = self
            .session
            .as_ref()
            .is_none_or(|s| (s.page, s.pages, s.at) == (page, pages, at) && s.since.is_some());
        if unchanged {
            return;
        }
        self.finish_page(cx);
        let Some(session) = &mut self.session else {
            return;
        };
        // The reader has counted the pages (or counted anew).
        let counted = session
            .book
            .filter(|_| pages != session.pages && pages != 1000);
        session.page = page;
        session.pages = pages;
        session.at = at;
        session.since = Some(cx.now_ms());
        if let Some(id) = counted {
            let result = store::update_books(cx.data(), |books| {
                match books.iter_mut().find(|b| b.id == id) {
                    Some(book) => {
                        book.pages = pages;
                        true
                    }
                    None => false,
                }
            });
            self.wrote(result);
        }
    }

    /// A book is shown: the same one again (after a menu), or another.
    pub(crate) fn open(&mut self, cx: &dyn Surroundings, reading: &Reading<'_>) {
        if self.reading(reading.path) {
            return self.turn(cx, reading);
        }
        self.finish_page(cx);
        let (page, pages) = page_of(reading);
        // A new session first: the names of the old one wait only for its stays.
        self.session = None;
        self.prune();
        let Some(name) = self.name(reading.path, reading.title) else {
            // No room to note the book: it is not recorded.
            return;
        };
        self.session = Some(Session {
            book: None,
            name,
            page,
            pages,
            at: reading.at,
            since: Some(cx.now_ms()),
        });
        self.date(cx);
    }

    /// The reader shows a page of the book being read (not of another one).
    pub(crate) fn page_turned(&mut self, cx: &dyn Surroundings, reading: &Reading<'_>) -> Told {
        if self.reading(reading.path) && cx.limits().enabled {
            self.turn(cx, reading);
        }
        self.tell(cx)
    }

    /// A book is shown.
    pub(crate) fn book_opened(&mut self, cx: &dyn Surroundings, reading: &Reading<'_>) -> Told {
        if cx.limits().enabled {
            self.open(cx, reading);
        }
        self.tell(cx)
    }

    /// The book at `path` was closed: its page ends, and nothing is read
    /// until a book is shown again.
    pub(crate) fn book_closed(&mut self, cx: &dyn Surroundings, path: &str) -> Told {
        if self.reading(path) {
            self.finish_page(cx);
            self.session = None;
        }
        self.tell(cx)
    }

    /// The book at `from` is at `to` now.
    pub(crate) fn book_moved(&mut self, cx: &dyn Surroundings, from: &str, to: &str) -> Told {
        let result = store::update_books(cx.data(), |books| {
            let mut changed = false;
            for book in books.iter_mut().filter(|b| b.path == from) {
                match room::string(&[to]) {
                    Ok(path) => {
                        book.path = path;
                        changed = true;
                    }
                    Err(_) => return changed,
                }
            }
            changed
        });
        self.wrote(result);
        self.tell(cx)
    }

    /// The book at `path` was read to the end: kept as when it was finished
    /// (see [`mark_finished`]).
    pub(crate) fn book_finished(&mut self, cx: &dyn Surroundings, path: &str) {
        if let Some(now) = cx.unix_at(cx.now_ms()).filter(|_| cx.limits().enabled) {
            let result = mark_finished(cx, path, now);
            self.wrote(result);
        }
    }

    /// The device sleeps or stops: the page ends.
    pub(crate) fn suspended(&mut self, cx: &dyn Surroundings) {
        self.finish_page(cx);
    }

    /// The device wakes: only a book still open goes on being read.
    pub(crate) fn resumed(&mut self, cx: &dyn Surroundings) {
        if let Some(session) = &mut self.session {
            session.since.get_or_insert(cx.now_ms());
        }
    }

    /// The start: files a cut left beside their place are put there before
    /// anything is recorded; today's reading is told, for those who show it.
    pub(crate) fn started(&mut self, cx: &dyn Surroundings) -> Told {
        store::recover(cx.data());
        self.tell(cx)
    }

    /// The time became known (or was set anew): what was read before is
    /// recorded.
    pub(crate) fn clock_changed(&mut self, cx: &dyn Surroundings) -> Told {
        self.date(cx);
        self.tell(cx)
    }

    /// Records what the view is to include: the current page.
    pub(crate) fn settle(&mut self, cx: &dyn Surroundings) {
        let reading = self.session.as_ref().is_some_and(|s| s.since.is_some());
        self.finish_page(cx);
        if let Some(session) = &mut self.session
            && reading
        {
            session.since = Some(cx.now_ms());
        }
    }
}

/// Unix seconds of the moment `at_ms` on the monotonic clock, where a snapshot
/// said `unix` seconds at `now_ms` on it (rounded down); `None` out of range.
pub(crate) fn unix_at(unix: i64, now_ms: u64, at_ms: u64) -> Option<i64> {
    let since = i64::try_from(at_ms)
        .ok()?
        .checked_sub(i64::try_from(now_ms).ok()?)?;
    unix.checked_add(since.div_euclid(1000))
}

/// Today, if the device knows the time.
fn today(cx: &dyn Surroundings) -> Option<i32> {
    let unix = cx.unix_at(cx.now_ms())?;
    Some(cx.day_of(u32::try_from(unix.max(0)).unwrap_or(u32::MAX)))
}

/// The book at `path` in `books.ini`, added if new: recognised by its
/// fingerprint, with title as the reader shows it. `None` where `books.ini`
/// cannot be read or written: it then stays as it was, and nothing is
/// recorded for the book.
fn register_book(
    cx: &dyn Surroundings,
    path: &str,
    title: &str,
    now: i64,
    pages: u32,
) -> Option<u16> {
    let mut digits = [0u8; 32];
    let md5 = md5::partial_into(cx.files(), path, &mut digits)?;
    let mut id = None;
    store::update_books(cx.data(), |books| {
        let at = match books.iter().position(|b| b.md5 == md5) {
            Some(at) => at,
            None => {
                let Ok(new) = store::new_id(cx.data(), books) else {
                    return false;
                };
                let file = path.rsplit('/').next().unwrap_or(path);
                let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
                let named = if title.is_empty() { stem } else { title };
                let (Ok(title), Ok(md5), Ok(authors), Ok(language), Ok(series)) = (
                    room::string(&[named]),
                    room::string(&[md5]),
                    room::string(&["N/A"]),
                    room::string(&["N/A"]),
                    room::string(&["N/A"]),
                ) else {
                    return false;
                };
                let book = Book {
                    id: new,
                    title,
                    authors,
                    md5,
                    pages: 0,
                    language,
                    series,
                    last_open: 0,
                    path: String::new(),
                    finished: 0,
                };
                if room::push(books, book).is_err() {
                    return false;
                }
                books.len() - 1
            }
        };
        let Some(book) = books.get_mut(at) else {
            return false;
        };
        let Ok(own) = room::string(&[path]) else {
            return false;
        };
        book.last_open = now;
        book.path = own;
        // Counted pages, not the per-mille stand-in.
        if pages != 1000 || book.pages == 0 {
            book.pages = pages;
        }
        id = Some(book.id);
        true
    })
    .ok()?;
    id
}

/// The book at `path` was read to the end `now`: kept as when it was
/// finished, unless it was finished earlier the same year (its last page
/// reached again), so that a book read again in a later year counts then
/// too. Where `books.ini` cannot be read or written, it stays as it was.
fn mark_finished(cx: &dyn Surroundings, path: &str, now: i64) -> Result<(), StorageError> {
    let year = |unix: i64| fmt::date_of(cx.day_of(unix.max(0) as u32)).year;
    store::update_books(cx.data(), |books| {
        match books.iter_mut().find(|b| b.path == path) {
            Some(book) if book.finished <= 0 || year(book.finished) != year(now) => {
                book.finished = now;
                true
            }
            _ => false,
        }
    })
    .map(|_| ())
}

#[cfg(test)]
mod alloc_tests;
#[cfg(test)]
mod tests;
