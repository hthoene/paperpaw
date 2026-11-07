//! Where the statistics live, in KOReader's data model so they can be
//! exchanged with its statistics:
//!
//! - `books.ini`: the books, one section `[book.<id>]` each, with title,
//!   authors (separated by line breaks, as KOReader does), the partial
//!   MD5 of the file, pages, language, series, when it was last opened,
//!   its path here and when it was last read to the end (`finished`, Unix
//!   seconds; added later, so missing in older files).
//! - `pages.bin`: every stay on a page, 16 bytes each, appended as they
//!   happen: book id (u16), page (u32, from 1), start (u32, Unix seconds),
//!   duration (u16, seconds) and the book's pages then (u32), like KOReader's
//!   `page_stat_data`.
//! - `days.bin`: reading time per local day for quick views, derived from
//!   `pages.bin`: a header of 36 bytes (`day2`; how many bytes of the log
//!   it covers, u64; the time zone the days are local to, two i32; the
//!   log's first stay, to tell a replaced log), then 12 bytes per day, in
//!   order: the day (i32, days since 1970), seconds (u32) and stays (u32).
//!   Brought up to date when a view needs it, so reading only ever appends
//!   16 bytes per page; summed anew when anything in the header no longer
//!   holds.
//!
//! Nothing is held in memory but what a view shows: the log is read in
//! small chunks, from the end for recent days.
//!
//! What cannot be read is never written over: changes to the books go
//! through [`update_books`], which leaves `books.ini` as it is when it
//! cannot be read whole.
//!
//! `books.ini` and a log written anew are written beside their place
//! first (`books.new`, `pages.new`) and then put there. Where the old file
//! is gone and the new one is not in its place yet (a power cut, or a
//! rename that failed after taking the old file away), [`recover`] puts it
//! there at the next start.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{Storage, StorageError};

/// A stay on a page (KOReader: a row of `page_stat_data`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PageStat {
    pub book: u16,
    /// From 1.
    pub page: u32,
    /// Unix seconds.
    pub start: u32,
    /// Seconds.
    pub duration: u16,
    /// The book's pages when it was read.
    pub pages: u32,
}

const RECORD: usize = 16;

impl PageStat {
    fn encode(&self) -> [u8; RECORD] {
        let mut out = [0u8; RECORD];
        out[0..2].copy_from_slice(&self.book.to_le_bytes());
        out[2..6].copy_from_slice(&self.page.to_le_bytes());
        out[6..10].copy_from_slice(&self.start.to_le_bytes());
        out[10..12].copy_from_slice(&self.duration.to_le_bytes());
        out[12..16].copy_from_slice(&self.pages.to_le_bytes());
        out
    }

    fn decode(b: &[u8]) -> Self {
        let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        Self {
            book: u16::from_le_bytes([b[0], b[1]]),
            page: u32_at(2),
            start: u32_at(6),
            duration: u16::from_le_bytes([b[10], b[11]]),
            pages: u32_at(12),
        }
    }
}

/// A book (KOReader: a row of `book`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Book {
    pub id: u16,
    pub title: String,
    /// Separated by line breaks.
    pub authors: String,
    /// KOReader's partial MD5 of the file.
    pub md5: String,
    pub pages: u32,
    pub language: String,
    pub series: String,
    /// Unix seconds.
    pub last_open: i64,
    /// Where it is on this device (empty if it is not).
    pub path: String,
    /// When it was last read to the end (Unix seconds, 0: not yet).
    pub finished: i64,
}

/// Reading on a local day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Day {
    /// Days since 1970-01-01, local time.
    pub day: i32,
    pub seconds: u32,
    /// Stays on pages (a page read twice counts twice).
    pub stays: u32,
}

const DAY: usize = 12;

impl Day {
    fn encode(&self) -> [u8; DAY] {
        let mut out = [0u8; DAY];
        out[0..4].copy_from_slice(&self.day.to_le_bytes());
        out[4..8].copy_from_slice(&self.seconds.to_le_bytes());
        out[8..12].copy_from_slice(&self.stays.to_le_bytes());
        out
    }

    fn decode(b: &[u8]) -> Self {
        let at = |i: usize| [b[i], b[i + 1], b[i + 2], b[i + 3]];
        Self {
            day: i32::from_le_bytes(at(0)),
            seconds: u32::from_le_bytes(at(4)),
            stays: u32::from_le_bytes(at(8)),
        }
    }
}

// The plugin's data folder is `.paperpaw/plugins/stats` (`Env::plugin_dir`).
// The names are whole strings: logging a page allocates nothing. The native
// storage (root 0) is that folder, so its adapter takes the folder off again.
const PAGES: &str = ".paperpaw/plugins/stats/pages.bin";
const PAGES_NEW: &str = ".paperpaw/plugins/stats/pages.new";
const BOOKS: &str = ".paperpaw/plugins/stats/books.ini";
const BOOKS_NEW: &str = ".paperpaw/plugins/stats/books.new";
const DAYS: &str = ".paperpaw/plugins/stats/days.bin";
const DAYS_NEW: &str = ".paperpaw/plugins/stats/days.new";
/// The most days a kept `days.bin` is read for: 4,096 days (11 years) are
/// 48 KiB, which the X4's heap holds. A longer one is summed anew.
const MOST_DAYS: u64 = 4096;

/// The sizes of the log and of the books: they change as one reads and
/// finishes books (or the statistics are merged).
pub(crate) fn sizes(storage: &dyn Storage) -> (u64, u64) {
    (
        storage.size(PAGES).unwrap_or(0),
        storage.size(BOOKS).unwrap_or(0),
    )
}

/// The longest line `books.ini` may have: a longer one is taken for a
/// damaged file.
const LINE: usize = 16 * 1024;

/// Calls `visit` with each line of the file at `path` (without its line
/// break) as it streams from the storage; a missing file has none. Invalid
/// UTF-8 and overlong lines are errors ([`StorageError::InvalidData`]),
/// not left out, so that nothing unread is written back.
fn each_line(
    storage: &dyn Storage,
    path: &str,
    mut visit: impl FnMut(&str) -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    let mut chunk = [0u8; 512];
    let mut line: Vec<u8> = Vec::new();
    let mut emit = |line: &mut Vec<u8>| -> Result<(), StorageError> {
        let text = core::str::from_utf8(line).map_err(|_| StorageError::InvalidData)?;
        visit(text.strip_suffix('\r').unwrap_or(text))?;
        line.clear();
        Ok(())
    };
    let mut at = 0u64;
    loop {
        let n = match storage.read_at(path, at, &mut chunk) {
            Ok(n) => n,
            Err(StorageError::NotFound) if at == 0 => 0,
            Err(e) => return Err(e),
        };
        if n == 0 {
            break;
        }
        at += n as u64;
        for piece in chunk[..n].split_inclusive(|b| *b == b'\n') {
            let (text, ends) = match piece.split_last() {
                Some((b'\n', text)) => (text, true),
                _ => (piece, false),
            };
            if line.len() + text.len() > LINE {
                return Err(StorageError::InvalidData);
            }
            line.try_reserve(text.len())
                .map_err(|_| StorageError::TooLarge)?;
            line.extend_from_slice(text);
            if ends {
                emit(&mut line)?;
            }
        }
    }
    if !line.is_empty() {
        emit(&mut line)?;
    }
    Ok(())
}

/// The books, by id; none while there is no `books.ini`. An error where
/// it cannot be read whole (a read error, invalid UTF-8, a line that is
/// no entry): then it must not be saved over.
pub(crate) fn load_books(storage: &dyn Storage) -> Result<Vec<Book>, StorageError> {
    // Sorted by id as they are read; the first section of an id wins.
    let mut books: Vec<Book> = Vec::new();
    // The id of the section being read, if it is a book's.
    let mut section: Option<u16> = None;
    // The id of the book made last: a section of it goes on filling that book
    // (sections in a row, empty ones between, merge); one of an id read before
    // that is ignored, so the first book of an id is the book.
    let mut last: Option<u16> = None;
    let mut skip = false;
    each_line(storage, BOOKS, |line| {
        if let Some(header) = line.trim().strip_prefix('[') {
            let name = header.strip_suffix(']').ok_or(StorageError::InvalidData)?;
            section = name
                .trim()
                .strip_prefix("book.")
                .and_then(|id| id.parse::<u16>().ok());
            skip = false;
            return Ok(());
        }
        // Nothing for an empty line or a comment.
        let Some(entry) = paperpaw_sdk::config::parse(line).next() else {
            return Ok(());
        };
        let entry = entry.map_err(|_| StorageError::InvalidData)?;
        let Some(id) = section else {
            return Ok(());
        };
        if skip {
            return Ok(());
        }
        let at = match books.binary_search_by_key(&id, |b| b.id) {
            Ok(at) if last == Some(id) => at,
            // Another section of this id came before: the first one is the book.
            Ok(_) => {
                skip = true;
                return Ok(());
            }
            Err(at) => {
                books.try_reserve(1).map_err(|_| StorageError::TooLarge)?;
                books.insert(
                    at,
                    Book {
                        id,
                        ..Book::default()
                    },
                );
                last = Some(id);
                at
            }
        };
        let Some(book) = books.get_mut(at) else {
            return Ok(());
        };
        let value = entry.value.as_str();
        match entry.key {
            "title" => book.title = copy(&value)?,
            "authors" => book.authors = copy(&value)?,
            "md5" => book.md5 = copy(&value)?,
            "pages" => book.pages = value.parse().unwrap_or(0),
            "language" => book.language = copy(&value)?,
            "series" => book.series = copy(&value)?,
            "last-open" => book.last_open = value.parse().unwrap_or(0),
            "path" => book.path = copy(&value)?,
            "finished" => book.finished = value.parse().unwrap_or(0),
            _ => {}
        }
        Ok(())
    })?;
    Ok(books)
}

/// An owned copy of `text`, made with fallible allocation.
fn copy(text: &str) -> Result<String, StorageError> {
    let mut out = String::new();
    out.try_reserve_exact(text.len())
        .map_err(|_| StorageError::TooLarge)?;
    out.push_str(text);
    Ok(out)
}

/// The books, by id, to show them: none where `books.ini` cannot be read.
/// Never to be saved back: changes go through [`update_books`].
pub(crate) fn books(storage: &dyn Storage) -> Vec<Book> {
    load_books(storage).unwrap_or_default()
}

/// Changes the books: `change` gets all of them and says whether it
/// changed any, which are then saved. Whether it did; an error, with
/// nothing written, where `books.ini` cannot be read or written.
pub(crate) fn update_books(
    storage: &dyn Storage,
    change: impl FnOnce(&mut Vec<Book>) -> bool,
) -> Result<bool, StorageError> {
    // Books waiting beside their place are put there first: a new list
    // would take their place.
    recover_file(storage, BOOKS, BOOKS_NEW);
    let mut books = load_books(storage)?;
    if !change(&mut books) {
        return Ok(false);
    }
    save_books(storage, &books)?;
    Ok(true)
}

/// Writes `books.ini` with `books`, which replace all there were: read
/// them with [`load_books`] (or use [`update_books`]). The text is written a
/// piece at a time, never held whole.
pub(crate) fn save_books(storage: &dyn Storage, books: &[Book]) -> Result<(), StorageError> {
    let new = BOOKS_NEW;
    if let Err(e) = write_books(storage, new, books) {
        remove_temporary(storage, new);
        return Err(e);
    }
    let file = BOOKS;
    match storage.rename(new, file) {
        // Where files cannot be renamed, written in place.
        Err(StorageError::Unsupported) => {
            remove_temporary(storage, new);
            write_books(storage, file, books)
        }
        Err(e) => {
            keep_if_gone(storage, new, file);
            Err(e)
        }
        Ok(()) => Ok(()),
    }
}

/// The pieces of a file written a few KiB at a time: the first one replaces
/// the file, the rest are appended.
struct Pieces<'a> {
    storage: &'a dyn Storage,
    path: &'a str,
    buffer: Vec<u8>,
    begun: bool,
}

/// What a piece holds at most (the host takes 4 KiB a call).
const PIECE: usize = 3584;

impl<'a> Pieces<'a> {
    fn new(storage: &'a dyn Storage, path: &'a str) -> Result<Self, StorageError> {
        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(PIECE)
            .map_err(|_| StorageError::TooLarge)?;
        Ok(Self {
            storage,
            path,
            buffer,
            begun: false,
        })
    }

    fn put(&mut self, text: &str) -> Result<(), StorageError> {
        for part in text.as_bytes().chunks(PIECE) {
            if self.buffer.len() + part.len() > PIECE {
                self.flush()?;
            }
            self.buffer.extend_from_slice(part);
        }
        Ok(())
    }

    fn number(&mut self, value: u64) -> Result<(), StorageError> {
        let mut digits = [0u8; 20];
        let mut at = digits.len();
        let mut rest = value;
        loop {
            at -= 1;
            if let Some(slot) = digits.get_mut(at) {
                *slot = b'0' + (rest % 10) as u8;
            }
            rest /= 10;
            if rest == 0 {
                break;
            }
        }
        self.put(
            digits
                .get(at..)
                .and_then(|d| core::str::from_utf8(d).ok())
                .unwrap_or(""),
        )
    }

    fn signed(&mut self, value: i64) -> Result<(), StorageError> {
        if value < 0 {
            self.put("-")?;
        }
        self.number(value.unsigned_abs())
    }

    /// `value` as `config::quote` writes it: as it is, or in quotes with
    /// escapes if it would not survive as it is.
    fn quoted(&mut self, value: &str) -> Result<(), StorageError> {
        let plain = !value.is_empty()
            && value.trim() == value
            && !value.starts_with('"')
            && !value.contains(['\n', '\r']);
        if plain {
            return self.put(value);
        }
        self.put("\"")?;
        let mut utf8 = [0u8; 4];
        for c in value.chars() {
            match c {
                '"' => self.put("\\\"")?,
                '\\' => self.put("\\\\")?,
                '\n' => self.put("\\n")?,
                '\t' => self.put("\\t")?,
                '\r' => {}
                c => self.put(c.encode_utf8(&mut utf8))?,
            }
        }
        self.put("\"")
    }

    fn flush(&mut self) -> Result<(), StorageError> {
        if self.begun {
            self.storage.append(self.path, &self.buffer)?;
        } else {
            self.storage.write(self.path, &self.buffer)?;
            self.begun = true;
        }
        self.buffer.clear();
        Ok(())
    }

    /// The last piece; an empty file is written as one.
    fn finish(mut self) -> Result<(), StorageError> {
        if self.begun && self.buffer.is_empty() {
            return Ok(());
        }
        self.flush()
    }
}

fn write_books(storage: &dyn Storage, path: &str, books: &[Book]) -> Result<(), StorageError> {
    let mut out = Pieces::new(storage, path)?;
    for b in books {
        out.put("[book.")?;
        out.number(u64::from(b.id))?;
        out.put("]\ntitle = ")?;
        out.quoted(&b.title)?;
        out.put("\nauthors = ")?;
        out.quoted(&b.authors)?;
        out.put("\nmd5 = ")?;
        out.put(&b.md5)?;
        out.put("\npages = ")?;
        out.number(u64::from(b.pages))?;
        out.put("\nlanguage = ")?;
        out.quoted(&b.language)?;
        out.put("\nseries = ")?;
        out.quoted(&b.series)?;
        out.put("\nlast-open = ")?;
        out.signed(b.last_open)?;
        out.put("\npath = ")?;
        out.quoted(&b.path)?;
        out.put("\nfinished = ")?;
        out.signed(b.finished)?;
        out.put("\n\n")?;
    }
    out.finish()
}

/// Puts the file written at `new` in the place of `file`. Where that
/// fails, `new` goes only while `file` is still there: a failed rename
/// may have taken it away (FAT removes it first), and then `new` is all
/// there is, put in place by [`recover`].
fn put_in_place(storage: &dyn Storage, new: &str, file: &str) -> Result<(), StorageError> {
    let moved = storage.rename(new, file);
    if moved.is_err() {
        keep_if_gone(storage, new, file);
    }
    moved
}

/// After a failed move of `new` to `file`: `new` goes, unless `file` is
/// gone and `new` is all there is.
fn keep_if_gone(storage: &dyn Storage, new: &str, file: &str) {
    if storage.exists(file) {
        remove_temporary(storage, new);
    }
}

fn remove_temporary(storage: &dyn Storage, new: &str) {
    #[expect(
        clippy::let_underscore_must_use,
        reason = "a temporary file left behind is written over the next time"
    )]
    let _ = storage.remove(new);
}

/// After a cut while `file` was written anew: the new one where the old
/// one is gone already, else the unfinished new one goes.
fn recover_file(storage: &dyn Storage, file: &str, new: &str) {
    if !storage.exists(new) {
        return;
    }
    if storage.exists(file) {
        remove_temporary(storage, new);
    } else {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "a file that cannot be moved now stays beside its place for the next try"
        )]
        let _ = storage.rename(new, file);
    }
}

/// Puts the log and the books written anew in their place where a cut
/// left them beside it. Called at start.
pub(crate) fn recover(storage: &dyn Storage) {
    recover_file(storage, PAGES, PAGES_NEW);
    recover_file(storage, BOOKS, BOOKS_NEW);
}

/// An id for a new book: above those of `books` and of every stay in the
/// log, so that a new book never takes over the stays of one whose entry
/// was lost. [`StorageError::Full`] when there is none left.
pub(crate) fn new_id(storage: &dyn Storage, books: &[Book]) -> Result<u16, StorageError> {
    let mut last = books.iter().map(|b| b.id).max().unwrap_or(0);
    scan(storage, |s| last = last.max(s.book))?;
    last.checked_add(1).ok_or(StorageError::Full)
}

/// Records a stay on a page. A record left cut off at the log's end (a
/// power cut or a full card while appending) is dropped first, so that
/// the stays after it are not read shifted.
pub(crate) fn append(storage: &dyn Storage, stat: PageStat) -> Result<(), StorageError> {
    let log = PAGES;
    match storage.size(log) {
        Ok(size) if size % RECORD as u64 != 0 => drop_cut_record(storage, log, size)?,
        _ => {}
    }
    storage.append(log, &stat.encode())
}

/// Writes the log's whole records again without the cut one at its end
/// (`size` bytes now), through a new file moved over it.
fn drop_cut_record(storage: &dyn Storage, log: &str, size: u64) -> Result<(), StorageError> {
    let whole = size - size % RECORD as u64;
    let new = PAGES_NEW;
    let copied = (|| {
        storage.write(new, &[])?;
        let mut chunk = [0u8; RECORD * 64];
        let mut at = 0u64;
        while at < whole {
            let len = (whole - at).min(chunk.len() as u64) as usize;
            let n = storage.read_at(log, at, &mut chunk[..len])?;
            if n == 0 {
                return Err(StorageError::Io);
            }
            storage.append(new, &chunk[..n])?;
            at += n as u64;
        }
        Ok(())
    })();
    if let Err(e) = copied {
        remove_temporary(storage, new);
        return Err(e);
    }
    put_in_place(storage, new, log)
}

/// Replaces the log with the stays in `file` (records of book, page,
/// start, duration and pages as big-endian u32, in time order), e.g.
/// after merging with another device's; the day totals are summed anew.
/// After an error the log is as it was.
pub(crate) fn replace_log(storage: &dyn Storage, file: &str) -> Result<(), StorageError> {
    let new = PAGES_NEW;
    if let Err(e) = write_log(storage, file, new) {
        remove_temporary(storage, new);
        return Err(e);
    }
    // Swapped in at once, by a rename where the storage can.
    put_in_place(storage, new, PAGES)
}

fn write_log(storage: &dyn Storage, file: &str, new: &str) -> Result<(), StorageError> {
    storage.write(new, &[])?;
    let mut chunk = [0u8; 20 * 64];
    let mut out = Vec::with_capacity(64 * RECORD);
    let mut at = 0u64;
    loop {
        let n = storage.read_at(file, at, &mut chunk)?;
        let whole = n - n % 20;
        out.clear();
        for r in chunk[..whole].as_chunks::<20>().0 {
            let v = |i: usize| u32::from_be_bytes([r[i], r[i + 1], r[i + 2], r[i + 3]]);
            let stat = PageStat {
                book: v(0).min(u32::from(u16::MAX)) as u16,
                page: v(4),
                start: v(8),
                duration: v(12).min(u32::from(u16::MAX)) as u16,
                pages: v(16),
            };
            out.extend_from_slice(&stat.encode());
        }
        storage.append(new, &out)?;
        // Only a read without a whole record is the end: a read may
        // return less than asked for.
        if whole == 0 {
            break;
        }
        at += whole as u64;
    }
    // The day totals go first: whatever happens next, they are never
    // taken for those of the new log.
    match storage.remove(DAYS) {
        Ok(()) | Err(StorageError::NotFound) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Bytes before the days in `days.bin`.
const HEADER: usize = 36;
const MAGIC: &[u8; 4] = b"day2";

/// What the day totals were summed from: when any of it changed, they
/// are summed anew.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Summed {
    /// Bytes of the log.
    covered: u64,
    /// The time zone ([`zone`]).
    zone: [i32; 2],
    /// The log's first stay (zeros while it has none).
    first: [u8; RECORD],
}

impl Summed {
    fn encode(&self) -> [u8; HEADER] {
        let mut out = [0u8; HEADER];
        out[0..4].copy_from_slice(MAGIC);
        out[4..12].copy_from_slice(&self.covered.to_le_bytes());
        out[12..16].copy_from_slice(&self.zone[0].to_le_bytes());
        out[16..20].copy_from_slice(&self.zone[1].to_le_bytes());
        out[20..36].copy_from_slice(&self.first);
        out
    }

    fn decode(b: &[u8; HEADER]) -> Option<Self> {
        if &b[0..4] != MAGIC {
            return None;
        }
        let at = |i: usize| [b[i], b[i + 1], b[i + 2], b[i + 3]];
        let mut covered = [0u8; 8];
        covered.copy_from_slice(&b[4..12]);
        let mut first = [0u8; RECORD];
        first.copy_from_slice(&b[20..36]);
        Some(Self {
            covered: u64::from_le_bytes(covered),
            zone: [i32::from_le_bytes(at(12)), i32::from_le_bytes(at(16))],
            first,
        })
    }
}

/// The time zone as `day_of` applies it, as numbers that change with it:
/// on which side of midnight UTC of 15 January and of 15 July 2024
/// (winter and summer time) a local day begins, and in which quarter
/// hour.
fn zone(day_of: &impl Fn(u32) -> i32) -> [i32; 2] {
    const QUARTER: u32 = 900;
    const QUARTERS: u32 = 96;
    [1_705_276_800u32, 1_721_001_600].map(|midnight| {
        let first = day_of(midnight);
        // Another day begins within a day: the first quarter hour in it.
        let (mut same, mut other) = (0, QUARTERS);
        while same + 1 < other {
            let mid = (same + other) / 2;
            if day_of(midnight + mid * QUARTER) == first {
                same = mid;
            } else {
                other = mid;
            }
        }
        let side = first.wrapping_sub((midnight / 86_400) as i32);
        side.wrapping_mul(QUARTERS as i32)
            .wrapping_add(other as i32)
    })
}

/// Every day with reading, in order, local days as `day_of` makes them
/// from Unix seconds; brings `days.bin` up to date with the log first.
///
/// # Errors
/// [`StorageError::TooLarge`] where the days do not fit in memory; the log's
/// own read errors end the summing and leave the days of the part read.
pub(crate) fn days(
    storage: &dyn Storage,
    day_of: impl Fn(u32) -> i32,
) -> Result<Vec<Day>, StorageError> {
    let size = storage.size(PAGES).unwrap_or(0);
    let mut first = [0u8; RECORD];
    if storage.read_at(PAGES, 0, &mut first).ok() != Some(RECORD) {
        first = [0; RECORD];
    }
    let zone = zone(&day_of);
    // Derived from the log: what cannot be read is summed anew.
    let (covered, mut days) = match kept_days(storage, zone, first, size)? {
        Some((covered, days)) => (covered, days),
        None => (0, Vec::new()),
    };
    if covered + RECORD as u64 > size {
        return Ok(days);
    }
    let mut chunk = [0u8; RECORD * 64];
    let mut at = covered;
    while let Ok(n) = storage.read_at(PAGES, at, &mut chunk) {
        let whole = n - n % RECORD;
        for record in chunk.get(..whole).unwrap_or(&[]).as_chunks::<RECORD>().0 {
            let stat = PageStat::decode(record);
            let day = day_of(stat.start);
            match days.binary_search_by_key(&day, |d| d.day) {
                Ok(i) => {
                    if let Some(entry) = days.get_mut(i) {
                        // Saturating: a hostile merged log cannot overflow it.
                        entry.seconds = entry.seconds.saturating_add(u32::from(stat.duration));
                        entry.stays = entry.stays.saturating_add(1);
                    }
                }
                Err(i) => {
                    days.try_reserve(1).map_err(|_| StorageError::TooLarge)?;
                    days.insert(
                        i,
                        Day {
                            day,
                            seconds: u32::from(stat.duration),
                            stays: 1,
                        },
                    );
                }
            }
        }
        at += whole as u64;
        if n < chunk.len() || whole == 0 {
            break;
        }
    }
    // Only a shortcut: if it cannot be kept, the days are summed anew.
    keep_days(
        storage,
        &days,
        Summed {
            covered: at,
            zone,
            first,
        },
    );
    Ok(days)
}

/// The days `days.bin` holds, with the bytes of the log they cover, if they
/// were summed from this log in this time zone.
fn kept_days(
    storage: &dyn Storage,
    zone: [i32; 2],
    first: [u8; RECORD],
    size: u64,
) -> Result<Option<(u64, Vec<Day>)>, StorageError> {
    let mut head = [0u8; HEADER];
    if storage.read_at(DAYS, 0, &mut head).ok() != Some(HEADER) {
        return Ok(None);
    }
    let Some(summed) = Summed::decode(&head) else {
        return Ok(None);
    };
    if summed.zone != zone || summed.first != first || summed.covered > size {
        return Ok(None);
    }
    let Ok(file) = storage.size(DAYS) else {
        return Ok(None);
    };
    // A file too large is not one of ours: summed anew.
    let count = file.saturating_sub(HEADER as u64) / DAY as u64;
    if count > MOST_DAYS {
        return Ok(None);
    }
    let mut days: Vec<Day> = Vec::new();
    // No room for the kept days: they are summed anew, not refused.
    if days.try_reserve_exact(count as usize).is_err() {
        return Ok(None);
    }
    let mut chunk = [0u8; DAY * 64];
    let mut at = HEADER as u64;
    while (days.len() as u64) < count {
        let Ok(n) = storage.read_at(DAYS, at, &mut chunk) else {
            return Ok(None);
        };
        let whole = n - n % DAY;
        if whole == 0 {
            return Ok(None);
        }
        for day in chunk.get(..whole).unwrap_or(&[]).as_chunks::<DAY>().0 {
            if (days.len() as u64) < count {
                days.push(Day::decode(day));
            }
        }
        at += whole as u64;
    }
    Ok(Some((summed.covered, days)))
}

/// Writes `days.bin` beside its place and puts it there; on any failure the
/// shortcut is simply not kept.
fn keep_days(storage: &dyn Storage, days: &[Day], summed: Summed) {
    let written = (|| {
        storage.write(DAYS_NEW, &summed.encode())?;
        let mut chunk = [0u8; DAY * 64];
        for part in days.chunks(64) {
            let mut len = 0;
            for day in part {
                if let Some(slot) = chunk.get_mut(len..len + DAY) {
                    slot.copy_from_slice(&day.encode());
                    len += DAY;
                }
            }
            storage.append(DAYS_NEW, chunk.get(..len).unwrap_or(&[]))?;
        }
        storage.rename(DAYS_NEW, DAYS)
    })();
    if written.is_err() {
        remove_temporary(storage, DAYS_NEW);
    }
}

/// Calls `visit` for every stay, oldest first. A read error ends the
/// stays and is returned: then those visited are not all there are.
pub(crate) fn scan(
    storage: &dyn Storage,
    mut visit: impl FnMut(&PageStat),
) -> Result<(), StorageError> {
    let file = PAGES;
    let mut chunk = [0u8; RECORD * 64];
    let mut at = 0u64;
    loop {
        let n = match storage.read_at(file, at, &mut chunk) {
            Ok(n) => n,
            // No log yet.
            Err(StorageError::NotFound) if at == 0 => 0,
            Err(e) => return Err(e),
        };
        let whole = n - n % RECORD;
        for record in chunk[..whole].as_chunks::<RECORD>().0 {
            visit(&PageStat::decode(record));
        }
        // Only a read without a whole record is the end: a read may
        // return less than asked for.
        if whole == 0 {
            return Ok(());
        }
        at += whole as u64;
    }
}

/// Calls `visit` for the stays that started at `since` (Unix seconds) or
/// later, newest first; reads only the end of the log. Stays are logged
/// as they end (a merged log is in order of their starts, which is no
/// later), so all stays before one that ended before `since` started
/// before it too.
pub(crate) fn scan_recent(storage: &dyn Storage, since: u32, mut visit: impl FnMut(&PageStat)) {
    let file = PAGES;
    let size = storage.size(file).unwrap_or(0);
    let mut end = size - size % RECORD as u64;
    let mut chunk = [0u8; RECORD * 64];
    let margin = CLOCK_LEEWAY;
    while end > 0 {
        let start = end.saturating_sub(chunk.len() as u64);
        let len = (end - start) as usize;
        if storage.read_at(file, start, &mut chunk[..len]).ok() != Some(len) {
            return;
        }
        let mut older = false;
        for record in chunk[..len].as_chunks::<RECORD>().0.iter().rev() {
            let stat = PageStat::decode(record);
            let ended = u64::from(stat.start) + u64::from(stat.duration);
            if stat.start >= since {
                visit(&stat);
            } else if ended + margin < u64::from(since) {
                older = true;
            }
        }
        if older {
            return;
        }
        end = start;
    }
}

/// Leeway for a clock set a little back between stays, which puts the
/// log a little out of order.
const CLOCK_LEEWAY: u64 = 3600;

/// Calls `visit` for the stays that started from `from` up to before `to`
/// (Unix seconds), in the order of the log; reads only about that part of
/// it. As stays are logged as they end (see [`scan_recent`]), the first
/// that may have started in time is found by halving, and the reading
/// ends at one that started so late that no later stay can have started
/// before `to`, however long it lasted.
pub(crate) fn scan_range(
    storage: &dyn Storage,
    from: u32,
    to: u32,
    mut visit: impl FnMut(&PageStat),
) {
    let file = PAGES;
    let count = storage.size(file).unwrap_or(0) / RECORD as u64;
    let ended = |s: &PageStat| u64::from(s.start) + u64::from(s.duration);
    let (mut low, mut high) = (0, count);
    while low < high {
        let mid = low + (high - low) / 2;
        let mut record = [0u8; RECORD];
        if storage.read_at(file, mid * RECORD as u64, &mut record).ok() != Some(RECORD) {
            return;
        }
        if ended(&PageStat::decode(&record)) + CLOCK_LEEWAY < u64::from(from) {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    let last_start = u64::from(to) + u64::from(u16::MAX) + CLOCK_LEEWAY;
    let mut chunk = [0u8; RECORD * 64];
    let mut at = low * RECORD as u64;
    // Stays in a row that started too late: one alone (a clock that was
    // wrong once) does not end the reading, a whole chunk of them does.
    let mut late = 0;
    while let Ok(n) = storage.read_at(file, at, &mut chunk) {
        let whole = n - n % RECORD;
        for record in chunk[..whole].as_chunks::<RECORD>().0 {
            let stat = PageStat::decode(record);
            if (from..to).contains(&stat.start) {
                visit(&stat);
            }
            if u64::from(stat.start) >= last_start {
                late += 1;
                if late >= 64 {
                    return;
                }
            } else {
                late = 0;
            }
        }
        if n < chunk.len() || whole == 0 {
            return;
        }
        at += whole as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Faulty;
    use alloc::format;

    /// `days` for tests: the memory of a test is not refused.
    fn days(storage: &dyn Storage, day_of: impl Fn(u32) -> i32) -> Vec<Day> {
        super::days(storage, day_of).unwrap()
    }
    use paperpaw_sdk::MemoryStorage;

    /// The files as version 1 wrote them stay readable (see
    /// `docs/data-files.md`).
    #[test]
    fn files_of_version_1_are_read() {
        let storage = MemoryStorage::new()
            .with(BOOKS, include_str!("../testdata/books.ini.v1"))
            .with(PAGES, include_bytes!("../testdata/pages.bin.v1").to_vec());
        let books = load_books(&storage).unwrap();
        assert_eq!(books.len(), 2);
        assert_eq!(books[0].title, "Anna Karenina");
        assert_eq!(books[0].authors, "Leo Tolstoy\nRosamund Bartlett");
        assert_eq!(books[0].md5, "0f343b0931126a20f133d67c2b018a3b");
        assert_eq!((books[0].pages, books[0].last_open), (864, 1_760_000_000));
        assert_eq!(books[0].path, "books/Anna Karenina.epub");
        assert_eq!(books[1].title, "Short \"stories\"");
        let mut stays = Vec::new();
        scan(&storage, |s| stays.push(*s)).unwrap();
        assert_eq!(
            stays,
            [
                PageStat {
                    book: 1,
                    page: 5,
                    start: 1_760_000_000,
                    duration: 42,
                    pages: 864
                },
                PageStat {
                    book: 2,
                    page: 1,
                    start: 1_760_003_600,
                    duration: 65000,
                    pages: 12
                },
            ]
        );
    }

    fn stat(book: u16, page: u32, start: u32, duration: u16) -> PageStat {
        PageStat {
            book,
            page,
            start,
            duration,
            pages: 300,
        }
    }

    #[test]
    fn books_survive_the_file() {
        let storage = MemoryStorage::new();
        let books_in = [
            Book {
                id: 1,
                title: String::from("Fjord \"1\""),
                authors: String::from("A. Author\nB. Author"),
                md5: String::from("abc"),
                pages: 614,
                language: String::from("no"),
                series: String::from("N/A"),
                last_open: 1_760_000_000,
                path: String::from("books/d.epub"),
                finished: 1_760_100_000,
            },
            Book {
                id: 2,
                title: String::from("Zwei"),
                ..Book::default()
            },
        ];
        save_books(&storage, &books_in).unwrap();
        assert_eq!(load_books(&storage).unwrap(), books_in);
        // Read in pieces: a long list of books.
        let many: Vec<Book> = (1..=300)
            .map(|id| Book {
                id,
                title: format!("Book {id} with a title of some length"),
                ..Book::default()
            })
            .collect();
        save_books(&storage, &many).unwrap();
        assert_eq!(load_books(&storage).unwrap(), many);
    }

    /// A `books.ini` that cannot be read whole is an error, and changes
    /// leave it as it is; only a missing one is empty.
    #[test]
    fn unreadable_books_are_never_saved_over() {
        let one = "[book.1]\ntitle = Eins\nmd5 = aaa\n\n[book.2]\ntitle = Zwei\n";
        let add = |books: &mut Vec<Book>| {
            books.push(Book {
                id: 3,
                ..Book::default()
            });
            true
        };
        let cases: [(&str, &[u8]); 3] = [
            // Saved as Latin-1 on a computer.
            ("ANSI", b"[book.1]\ntitle = M\xfcller\n"),
            ("not an entry", b"[book.1]\ntitle = Eins\nsomething odd\n"),
            ("cut header", b"[book.1\ntitle = Eins\n"),
        ];
        for (what, data) in cases {
            let storage = MemoryStorage::new().with(BOOKS, data.to_vec());
            assert!(load_books(&storage).is_err(), "{what}");
            assert!(update_books(&storage, add).is_err(), "{what}");
            assert_eq!(storage.get(BOOKS).unwrap(), data, "{what}");
        }
        // A read error halfway.
        let storage = Faulty::new(MemoryStorage::new().with(BOOKS, one));
        storage.break_at(BOOKS, 20);
        assert_eq!(load_books(&storage), Err(StorageError::Io));
        assert_eq!(update_books(&storage, add), Err(StorageError::Io));
        assert_eq!(storage.inner.get(BOOKS).unwrap(), one.as_bytes());
        assert!(books(&storage).is_empty());
        // None yet.
        let storage = MemoryStorage::new();
        assert_eq!(update_books(&storage, add), Ok(true));
        assert_eq!(load_books(&storage).unwrap().len(), 1);
    }

    /// A card that is full: the error comes back, nothing is lost quietly.
    #[test]
    fn write_errors_are_returned() {
        let storage = Faulty::new(MemoryStorage::new());
        storage.fill_after(0);
        assert_eq!(
            append(&storage, stat(1, 1, 100, 10)),
            Err(StorageError::Full)
        );
        assert_eq!(save_books(&storage, &[]), Err(StorageError::Full));
        assert_eq!(update_books(&storage, |_| true), Err(StorageError::Full));
    }

    #[test]
    fn stays_are_logged_and_summed_per_day() {
        let storage = MemoryStorage::new();
        // A day of 100 seconds, for short numbers.
        let day_of = |unix: u32| (unix / 100) as i32;
        append(&storage, stat(1, 1, 1000, 30)).unwrap();
        append(&storage, stat(1, 2, 1030, 40)).unwrap();
        append(&storage, stat(2, 7, 1100, 20)).unwrap();
        assert_eq!(
            days(&storage, day_of),
            [
                Day {
                    day: 10,
                    seconds: 70,
                    stays: 2
                },
                Day {
                    day: 11,
                    seconds: 20,
                    stays: 1
                },
            ]
        );
        // Later stays are added to what was summed, also earlier days.
        append(&storage, stat(1, 3, 950, 10)).unwrap();
        append(&storage, stat(1, 4, 1150, 5)).unwrap();
        assert_eq!(
            days(&storage, day_of),
            [
                Day {
                    day: 9,
                    seconds: 10,
                    stays: 1
                },
                Day {
                    day: 10,
                    seconds: 70,
                    stays: 2
                },
                Day {
                    day: 11,
                    seconds: 25,
                    stays: 2
                },
            ]
        );
        // A replaced, shorter log is summed anew.
        storage.write(PAGES, &stat(3, 1, 1200, 9).encode()).unwrap();
        assert_eq!(
            days(&storage, day_of),
            [Day {
                day: 12,
                seconds: 9,
                stays: 1
            }]
        );
        let mut all = Vec::new();
        scan(&storage, |s| all.push(*s)).unwrap();
        assert_eq!(all, [stat(3, 1, 1200, 9)]);
    }

    /// The totals are summed anew in another time zone, and for a log
    /// replaced by one as long or longer (e.g. when the device lost power
    /// right after a merge swapped it in).
    #[test]
    fn day_totals_follow_the_zone_and_the_log() {
        let storage = MemoryStorage::new();
        let at = |hours: u32| 1_760_000_000 - 1_760_000_000 % 86_400 + hours * 3600;
        let in_zone =
            |offset: i64| move |unix: u32| (i64::from(unix) + offset).div_euclid(86_400) as i32;
        append(&storage, stat(1, 1, at(23), 60)).unwrap();
        let utc = days(&storage, in_zone(0));
        assert_eq!(utc.len(), 1);
        // Two hours ahead, 23:00 UTC is the next day.
        let berlin = days(&storage, in_zone(7200));
        assert_eq!(berlin[0].day, utc[0].day + 1);
        assert_eq!(days(&storage, in_zone(0)), utc);
        // Another log of the same length: its own days.
        storage
            .write(PAGES, &stat(2, 1, at(5), 30).encode())
            .unwrap();
        assert_eq!(
            days(&storage, in_zone(0)),
            [Day {
                day: utc[0].day,
                seconds: 30,
                stays: 1
            }]
        );
        // The totals of earlier versions are summed anew.
        let mut old = 16u64.to_le_bytes().to_vec();
        old.extend_from_slice(
            &Day {
                day: 1,
                seconds: 999,
                stays: 1,
            }
            .encode(),
        );
        storage.write(DAYS, &old).unwrap();
        assert_eq!(days(&storage, in_zone(0))[0].seconds, 30);
    }

    /// A merged log replaces this one; the day totals go with it.
    #[test]
    fn the_log_is_replaced() {
        let storage = MemoryStorage::new();
        append(&storage, stat(1, 1, 100, 10)).unwrap();
        let day_of = |unix: u32| (unix / 100) as i32;
        assert_eq!(days(&storage, day_of).len(), 1);
        let merged: Vec<u8> = [[2u32, 3, 150, 70_000, 200], [1, 1, 300, 20, 300]]
            .iter()
            .flat_map(|r| r.iter().flat_map(|v| v.to_be_bytes()))
            .collect();
        storage.write("merged", &merged).unwrap();
        replace_log(&storage, "merged").unwrap();
        let mut all = Vec::new();
        scan(&storage, |s| all.push(*s)).unwrap();
        assert_eq!(
            all,
            [
                PageStat {
                    book: 2,
                    page: 3,
                    start: 150,
                    duration: u16::MAX,
                    pages: 200
                },
                stat(1, 1, 300, 20)
            ]
        );
        assert!(!storage.exists(PAGES_NEW));
        assert_eq!(days(&storage, day_of).len(), 2);
        // A card that fills up halfway: the log stays as it was.
        let storage = Faulty::new(MemoryStorage::new().with("merged", merged));
        append(&storage, stat(1, 1, 100, 10)).unwrap();
        storage.fill_after(1);
        assert_eq!(replace_log(&storage, "merged"), Err(StorageError::Full));
        let mut all = Vec::new();
        scan(&storage, |s| all.push(*s)).unwrap();
        assert_eq!(all, [stat(1, 1, 100, 10)]);
        assert!(!storage.exists(PAGES_NEW));
    }

    #[test]
    fn a_read_error_in_the_log_is_returned() {
        let storage = Faulty::new(MemoryStorage::new());
        for i in 0..100 {
            append(&storage, stat(1, i + 1, 1000 + i * 10, 9)).unwrap();
        }
        storage.break_at(PAGES, 16 * 70);
        let mut count = 0;
        assert_eq!(scan(&storage, |_| count += 1), Err(StorageError::Io));
        assert_eq!(count, 64);
        // No log yet: no stays.
        assert_eq!(scan(&MemoryStorage::new(), |_| count += 1), Ok(()));
    }

    #[test]
    fn recent_stays_come_from_the_end() {
        let storage = MemoryStorage::new();
        for (i, start) in [100u32, 130, 90_000].into_iter().enumerate() {
            append(&storage, stat(1, i as u32 + 1, start, 10)).unwrap();
        }
        let mut recent = Vec::new();
        scan_recent(&storage, 120, |s| recent.push(s.start));
        assert_eq!(recent, [90_000, 130]);
    }

    /// A stay that began long before but ended after one that began
    /// recently (e.g. KOReader's, of hours on one page) does not end the
    /// search early.
    #[test]
    fn recent_stays_come_after_a_long_one() {
        let storage = MemoryStorage::new();
        append(&storage, stat(1, 1, 10_000, 10)).unwrap();
        for page in 2..70 {
            append(&storage, stat(1, page, 1000, 9_500)).unwrap();
        }
        let mut recent = Vec::new();
        scan_recent(&storage, 10_000, |s| recent.push(s.page));
        assert_eq!(recent, [1]);
    }

    /// The stays of a time range, from a log of many days in the order
    /// the stays ended.
    #[test]
    fn stays_of_a_range() {
        let storage = MemoryStorage::new();
        let mut stays = Vec::new();
        let mut expected = Vec::new();
        for day in 0..30u32 {
            for minute in 0..20u32 {
                let start = 1_000_000 + day * 86_400 + minute * 60;
                stays.push(stat(1, minute + 1, start, 50));
                if day == 12 {
                    expected.push(start);
                }
            }
        }
        // A stay of 18 hours that began the day before ends on day 12.
        let long = 1_000_000 + 11 * 86_400 + 20 * 3600;
        stays.push(stat(2, 1, long, u16::MAX));
        // One that began on day 12 and ended on day 13.
        let late = 1_000_000 + 12 * 86_400 + 86_000;
        stays.push(stat(2, 2, late, 600));
        expected.push(late);
        stays.sort_by_key(|s| s.start + u32::from(s.duration));
        for s in stays {
            append(&storage, s).unwrap();
        }
        let (from, to) = (1_000_000 + 12 * 86_400, 1_000_000 + 13 * 86_400);
        let mut found = Vec::new();
        scan_range(&storage, from, to, |s| found.push(s.start));
        found.sort_unstable();
        expected.sort_unstable();
        assert_eq!(found, expected);
        // Nothing before the log, all of it around it.
        let mut none = 0;
        scan_range(&storage, 0, 1000, |_| none += 1);
        assert_eq!(none, 0);
        let mut all = 0;
        scan_range(&storage, 0, u32::MAX, |_| all += 1);
        assert_eq!(all, 30 * 20 + 2);
    }

    #[test]
    fn long_logs_are_read_in_chunks() {
        let storage = MemoryStorage::new();
        for i in 0..200u32 {
            append(&storage, stat(1, i + 1, 1000 + i * 10, 9)).unwrap();
        }
        let mut count = 0;
        scan(&storage, |_| count += 1).unwrap();
        assert_eq!(count, 200);
        let mut recent = 0;
        scan_recent(&storage, 1000 + 150 * 10, |_| recent += 1);
        assert_eq!(recent, 50);
    }

    /// A rename that takes the log away and then fails (as FAT may): the
    /// new log stays, as it is all there is, and is put in place at the
    /// next start; also where a cut record was dropped.
    #[test]
    fn a_failed_rename_leaves_the_new_log() {
        let merged: Vec<u8> = [[1u32, 1, 100, 10, 300], [1, 2, 200, 20, 300]]
            .iter()
            .flat_map(|r| r.iter().flat_map(|v| v.to_be_bytes()))
            .collect();
        let storage = Faulty::new(MemoryStorage::new().with("merged", merged));
        append(&storage, stat(1, 1, 100, 10)).unwrap();
        storage.drop_target_on_rename();
        assert!(replace_log(&storage, "merged").is_err());
        assert!(storage.exists(PAGES) || storage.exists(PAGES_NEW));
        recover(&storage.inner);
        let mut all = Vec::new();
        scan(&storage, |s| all.push(*s)).unwrap();
        assert_eq!(all, [stat(1, 1, 100, 10), stat(1, 2, 200, 20)]);
        // A cut record dropped.
        let storage = Faulty::new(MemoryStorage::new());
        append(&storage, stat(1, 1, 100, 10)).unwrap();
        storage.inner.append(PAGES, &[1, 2, 3]).unwrap();
        storage.drop_target_on_rename();
        assert!(append(&storage, stat(1, 2, 200, 20)).is_err());
        assert!(storage.exists(PAGES) || storage.exists(PAGES_NEW));
        recover(&storage.inner);
        let mut all = Vec::new();
        scan(&storage, |s| all.push(*s)).unwrap();
        assert_eq!(all, [stat(1, 1, 100, 10)]);
        // The books, too.
        let storage = Faulty::new(MemoryStorage::new());
        let one = [Book {
            id: 1,
            title: String::from("Eins"),
            ..Book::default()
        }];
        save_books(&storage, &one).unwrap();
        storage.drop_target_on_rename();
        let two = [
            one[0].clone(),
            Book {
                id: 2,
                ..Book::default()
            },
        ];
        assert!(save_books(&storage, &two).is_err());
        recover(&storage.inner);
        assert_eq!(load_books(&storage.inner).unwrap(), two);
    }

    /// After a power cut between taking the old file away and putting the
    /// new one in its place, the new one is put there; an unfinished new
    /// one beside the old goes.
    #[test]
    fn files_left_beside_their_place_are_recovered() {
        let storage = MemoryStorage::new();
        let log = stat(1, 1, 100, 10).encode();
        storage.write(PAGES_NEW, &log).unwrap();
        storage
            .write(
                BOOKS_NEW,
                b"[book.1]
title = Eins
",
            )
            .unwrap();
        recover(&storage);
        assert_eq!(storage.get(PAGES).unwrap(), log);
        assert_eq!(load_books(&storage).unwrap()[0].title, "Eins");
        assert!(!storage.exists(PAGES_NEW));
        assert!(!storage.exists(BOOKS_NEW));
        // Unfinished, beside the old ones.
        storage.write(PAGES_NEW, &[1, 2]).unwrap();
        storage.write(BOOKS_NEW, b"[book.1").unwrap();
        recover(&storage);
        assert_eq!(storage.get(PAGES).unwrap(), log);
        assert_eq!(load_books(&storage).unwrap()[0].title, "Eins");
        assert!(!storage.exists(PAGES_NEW));
        assert!(!storage.exists(BOOKS_NEW));
        // A change to the books before the next start recovers them first,
        // rather than starting a new list.
        let storage = MemoryStorage::new();
        storage
            .write(
                BOOKS_NEW,
                b"[book.1]
title = Eins
",
            )
            .unwrap();
        let added = update_books(&storage, |books| {
            books.push(Book {
                id: 2,
                ..Book::default()
            });
            true
        });
        assert_eq!(added, Ok(true));
        assert_eq!(load_books(&storage).unwrap().len(), 2);
    }

    /// Reads that return less than asked for are no end of the log.
    #[test]
    fn short_reads_are_read_on() {
        let storage = Faulty::new(MemoryStorage::new());
        for i in 0..100u32 {
            append(&storage, stat(1, i + 1, 1000 + i * 10, 9)).unwrap();
        }
        storage.read_short(100);
        let mut count = 0;
        scan(&storage, |_| count += 1).unwrap();
        assert_eq!(count, 100);
        let merged: Vec<u8> = (0..100u32)
            .flat_map(|i| [1u32, i + 1, 2000 + i, 5, 300])
            .flat_map(u32::to_be_bytes)
            .collect();
        storage.inner.write("merged", &merged).unwrap();
        replace_log(&storage, "merged").unwrap();
        let mut count = 0;
        scan(&storage.inner, |_| count += 1).unwrap();
        assert_eq!(count, 100);
    }

    #[test]
    fn kept_days_over_the_limit_are_summed_anew_not_refused() {
        let storage = MemoryStorage::new();
        append(&storage, stat(1, 1, 100, 10)).unwrap();
        let day_of = |unix: u32| (unix / 100) as i32;
        let first = {
            let mut first = [0u8; RECORD];
            storage.read_at(PAGES, 0, &mut first).unwrap();
            first
        };
        // A `days.bin` that claims the whole log is summed but holds too many days.
        let header = Summed {
            covered: RECORD as u64,
            zone: zone(&day_of),
            first,
        };
        let mut kept = header.encode().to_vec();
        for day in 0..=MOST_DAYS as i32 {
            kept.extend_from_slice(
                &Day {
                    day,
                    seconds: 99,
                    stays: 1,
                }
                .encode(),
            );
        }
        storage.write(DAYS, &kept).unwrap();
        let summed = days(&storage, day_of);
        assert_eq!(
            summed,
            [Day {
                day: 1,
                seconds: 10,
                stays: 1
            }]
        );
    }

    /// Odd books: quotes, newlines, spaces, empty fields, negative times.
    fn odd_books(count: u16) -> Vec<Book> {
        (1..=count)
            .map(|id| Book {
                id,
                title: alloc::format!("  \"Quoted\" title\nline two\ttab \\ {id} \u{e9}\u{1f600}"),
                authors: if id % 2 == 0 {
                    "".into()
                } else {
                    "A. Author\nB. Author".into()
                },
                md5: "0123456789abcdef0123456789abcdef".into(),
                pages: id.into(),
                language: "de".into(),
                series: " lead".into(),
                last_open: -5 * i64::from(id),
                path: alloc::format!("Books/{id}.epub"),
                finished: i64::from(id) * 1_000_000_000_000,
            })
            .collect()
    }

    /// A storage that takes at most 4 KiB a call, as the host does.
    struct Limited(MemoryStorage);
    impl Storage for Limited {
        fn read_at(&self, p: &str, o: u64, b: &mut [u8]) -> Result<usize, StorageError> {
            self.0.read_at(p, o, b)
        }
        fn size(&self, p: &str) -> Result<u64, StorageError> {
            self.0.size(p)
        }
        fn write(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
            assert!(d.len() <= 4096, "a write of {} bytes", d.len());
            self.0.write(p, d)
        }
        fn append(&self, p: &str, d: &[u8]) -> Result<(), StorageError> {
            assert!(d.len() <= 4096, "an append of {} bytes", d.len());
            self.0.append(p, d)
        }
        fn list(
            &self,
            d: &str,
            v: &mut dyn FnMut(paperpaw_sdk::DirEntry<'_>),
        ) -> Result<(), StorageError> {
            self.0.list(d, v)
        }
        fn remove(&self, p: &str) -> Result<(), StorageError> {
            self.0.remove(p)
        }
        fn rename(&self, a: &str, b: &str) -> Result<(), StorageError> {
            self.0.rename(a, b)
        }
    }

    #[test]
    fn books_are_written_in_pieces_as_config_would_write_them() {
        let storage = Limited(MemoryStorage::new());
        for books in [odd_books(0), odd_books(1), odd_books(120)] {
            save_books(&storage, &books).unwrap();
            assert_eq!(load_books(&storage).unwrap(), books);
            // The text is what the whole-string writer made.
            let mut expected = String::new();
            for b in &books {
                expected.push_str(&alloc::format!(
                    "[book.{}]\ntitle = {}\nauthors = {}\nmd5 = {}\npages = {}\nlanguage = {}\nseries = {}\nlast-open = {}\npath = {}\nfinished = {}\n\n",
                    b.id,
                    paperpaw_sdk::config::quote(&b.title),
                    paperpaw_sdk::config::quote(&b.authors),
                    b.md5,
                    b.pages,
                    paperpaw_sdk::config::quote(&b.language),
                    paperpaw_sdk::config::quote(&b.series),
                    b.last_open,
                    paperpaw_sdk::config::quote(&b.path),
                    b.finished,
                ));
            }
            let size = storage.size(BOOKS).unwrap() as usize;
            let mut got = alloc::vec![0u8; size];
            storage.read_at(BOOKS, 0, &mut got).unwrap();
            assert_eq!(core::str::from_utf8(&got).unwrap(), expected);
            assert!(!storage.exists(BOOKS_NEW));
        }
    }

    #[test]
    fn sections_in_a_row_merge_a_later_one_of_an_id_read_before_is_ignored() {
        let storage = MemoryStorage::new().with(
            BOOKS,
            b"[book.7]\ntitle = First\n\n[book.3]\n\n[book.7]\npages = 9\n\n[book.2]\ntitle = Two\n\n[book.7]\ntitle = Late\npages = 1\n".to_vec(),
        );
        let books = load_books(&storage).unwrap();
        let seen: Vec<_> = books
            .iter()
            .map(|b| (b.id, b.title.as_str(), b.pages))
            .collect();
        // 7 and the empty 3 and 7 again merge; the last 7 comes after 2: ignored.
        assert_eq!(seen, [(2, "Two", 0), (7, "First", 9)]);
    }
}

#[cfg(test)]
mod hostile_tests;
