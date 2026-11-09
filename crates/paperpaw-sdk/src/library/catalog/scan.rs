//! The catalog scanned and queried without an infallible allocation: one
//! reused line buffer of at most [`LINE_CAP`] bytes, every copy of a text
//! and every key reserved fallibly, and the reads' errors returned as errors
//! (a card that fails is not the end of the list). A device short of memory
//! gets [`Error::OutOfMemory`] instead of an abort, and nothing is returned
//! half made.
//!
//! The results are those of [`super::query_in`] and [`super::each_at`]. Where
//! they differ, it is in the lines that cannot be books: a line over
//! [`LINE_CAP`] bytes (damage; a line has at most a few kilobytes), one that
//! is not UTF-8 and one without a path are skipped, never read lossily; and
//! the lowercase of a text (keys, words) is made by the `casing` module, byte
//! for byte as `str::to_lowercase` makes it (the final sigma included), so
//! the order and the cursors of a query are those of the stored catalog.

use alloc::string::String;
use alloc::vec::Vec;

use super::{Query, Record, SEPARATOR, Sort, Status, series_order, try_changes};
use crate::data::Error;
use crate::{Storage, StorageError};

use super::casing::push_lower;
use super::fallible::{catalog_path_try, copy};

/// The longest catalog line read; a longer one is skipped as damaged.
pub const LINE_CAP: usize = 8 * 1024;

/// The books of a catalog (or any library): called with what to do with
/// each, where it can fail.
pub type TryBooks<'a> = &'a mut dyn FnMut(&mut dyn FnMut(u64, Record)) -> Result<(), Error>;

fn out_of_memory<T>(_: T) -> Error {
    Error::OutOfMemory
}

impl Record {
    /// [`Record::parse`] with the memory error: each text reserved
    /// fallibly, none for a line without a path.
    fn try_parse(text: &str) -> Result<Option<Record>, Error> {
        let mut fields = text.split('\t');
        let Some(path) = fields.next().filter(|p| !p.is_empty()) else {
            return Ok(None);
        };
        let path = copy(path)?;
        let mut next = || fields.next().unwrap_or("");
        let (author, title, series, number, tags) = (next(), next(), next(), next(), next());
        let (progress, flags, added, extra) = (next(), next(), next(), next());
        let size = next().parse().ok();
        Ok(Some(Record {
            path,
            author: copy(author)?,
            title: copy(title)?,
            series: copy(series)?,
            number: copy(number)?,
            tags: copy(tags)?,
            status: Status::parse(progress, flags),
            added: added.parse().ok(),
            fields: copy(extra)?,
            size,
        }))
    }

    /// [`Record::matches`] with the memory error: the lowercase of the
    /// title, author, series, tags and file name is made once.
    fn try_matches(&self, words: &[String]) -> Result<bool, Error> {
        let mut text = String::new();
        let size = self.title.len() + self.author.len() + self.series.len() + self.tags.len();
        text.try_reserve(size + self.path.len() + 5)
            .map_err(out_of_memory)?;
        for piece in [&self.title, &self.author, &self.series] {
            push_lower(&mut text, piece)?;
            push_str(&mut text, " ")?;
        }
        for tag in self.tags.split(SEPARATOR) {
            push_lower(&mut text, tag)?;
            push_str(&mut text, " ")?;
        }
        push_lower(&mut text, crate::files::name(&self.path))?;
        Ok(words.iter().all(|word| text.contains(word.as_str())))
    }
}

/// `tail` appended to `out`, reserved first: the lowercase before it may
/// have used what was reserved (a char can grow when lowercased).
fn push_str(out: &mut String, tail: &str) -> Result<(), Error> {
    out.try_reserve(tail.len()).map_err(out_of_memory)?;
    out.push_str(tail);
    Ok(())
}

/// A lowercase copy of `text`.
fn lowered(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    push_lower(&mut out, text)?;
    Ok(out)
}

/// Where a book goes in a query's order, as [`Query`] keeps it.
type Key = (u32, String, String);

impl Query {
    /// [`Query::finds`] with the memory error.
    fn try_finds(&self, record: &Record) -> Result<bool, Error> {
        if !self.fits(record) {
            return Ok(false);
        }
        if self.words.is_empty() {
            return Ok(true);
        }
        record.try_matches(&self.words)
    }

    /// [`Query::key`] with the memory error.
    fn try_key(&self, record: &Record) -> Result<Key, Error> {
        let title = lowered(&record.title)?;
        let joined = |first: &str, tail: &str| -> Result<String, Error> {
            let mut text = String::new();
            text.try_reserve_exact(first.len() + 1 + tail.len())
                .map_err(out_of_memory)?;
            push_lower(&mut text, first)?;
            push_str(&mut text, "\u{1}")?;
            push_str(&mut text, tail)?;
            Ok(text)
        };
        let (first, text) = match self.sort {
            Sort::Title | Sort::Other(_) => (0, title),
            Sort::Author => (0, joined(&record.author, &title)?),
            Sort::Series => {
                // Ten digits, as `{:010}`.
                let mut digits = [b'0'; 10];
                let mut rest = series_order(&record.number);
                for slot in digits.iter_mut().rev() {
                    *slot = b'0' + (rest % 10) as u8;
                    rest /= 10;
                }
                let digits = core::str::from_utf8(&digits).unwrap_or("0000000000");
                (0, joined(&record.series, digits)?)
            }
            Sort::Added => (u32::MAX - record.added.unwrap_or(0), title),
        };
        Ok((first, text, copy(&record.path)?))
    }
}

/// The catalog's line being read: its bytes so far, whether it is over the
/// cap (then skipped to its end), and where it starts.
struct Line {
    bytes: Vec<u8>,
    over: bool,
    start: u64,
}

impl Line {
    fn push(&mut self, piece: &[u8]) -> Result<(), Error> {
        if self.over {
            return Ok(());
        }
        if self.bytes.len() + piece.len() > LINE_CAP {
            self.over = true;
            self.bytes.clear();
            return Ok(());
        }
        self.bytes
            .try_reserve_exact(piece.len())
            .map_err(out_of_memory)?;
        self.bytes.extend_from_slice(piece);
        Ok(())
    }

    /// The line is over: its book, if it is one (valid text of a line that
    /// was not too long), as noted last; the next line starts at `next`.
    fn end(
        &mut self,
        next: u64,
        changes: &super::Changes,
        f: &mut dyn FnMut(u64, Record),
    ) -> Result<(), Error> {
        let start = core::mem::replace(&mut self.start, next);
        let over = core::mem::replace(&mut self.over, false);
        let bytes = core::mem::take(&mut self.bytes);
        let result = (|| {
            if over || bytes.is_empty() {
                return Ok(());
            }
            let Ok(text) = core::str::from_utf8(&bytes) else {
                return Ok(());
            };
            if let Some(record) = Record::try_parse(text.trim_end())?
                && let Some(record) = changes.apply(record)
            {
                f(start, record);
            }
            Ok(())
        })();
        // The buffer is reused for the next line.
        self.bytes = bytes;
        self.bytes.clear();
        result
    }
}

/// [`super::each_at`] with the memory error and the reads' errors: calls
/// `f` with each book of the catalog and where its line starts, as noted
/// last (those removed left out). One line at a time is held (at most
/// 8 KiB); a longer one, one that is not text and one without a
/// path are skipped.
///
/// # Errors
/// [`Error::OutOfMemory`]; the storage's error where the catalog or the log
/// cannot be read, however far the scan was (the books `f` has seen are no
/// complete list then). A catalog that is not there has no books.
pub fn try_each_at(storage: &dyn Storage, f: &mut dyn FnMut(u64, Record)) -> Result<(), Error> {
    let changes = try_changes(storage)?;
    let file = catalog_path_try()?;
    let mut chunk = [0_u8; 512];
    let mut line = Line {
        bytes: Vec::new(),
        over: false,
        start: 0,
    };
    let mut offset = 0_u64;
    loop {
        let n = match storage.read_at(&file, offset, &mut chunk) {
            Ok(n) => n,
            Err(StorageError::NotFound) if offset == 0 => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        if n == 0 {
            break;
        }
        let mut rest = &chunk[..n];
        let mut at = offset;
        offset += n as u64;
        while !rest.is_empty() {
            if let Some(end) = rest.iter().position(|&b| b == b'\n') {
                line.push(&rest[..end])?;
                at += end as u64 + 1;
                line.end(at, &changes, f)?;
                rest = &rest[end + 1..];
            } else {
                line.push(rest)?;
                rest = &[];
            }
        }
    }
    // A last line without its newline.
    line.end(offset, &changes, f)
}

/// [`super::query_in`] with the memory error and the books' errors: `n`
/// books `query` finds, in its order, those after `after`. The page held
/// across the scan (`n` books and their keys, one more) is reserved at once;
/// a book and its key are made, compared and dropped one at a time.
///
/// # Errors
/// [`Error::OutOfMemory`]; the error `books` gives. Nothing is returned half
/// made.
pub fn try_query_in(
    books: TryBooks<'_>,
    query: &Query,
    after: Option<&Record>,
    n: usize,
) -> Result<Vec<Record>, Error> {
    let after = after.map(|r| query.try_key(r)).transpose()?;
    let mut page: Vec<(Key, Record)> = Vec::new();
    // One more than the page: a book is put in before the last is dropped.
    page.try_reserve_exact(n.saturating_add(1))
        .map_err(out_of_memory)?;
    let mut failure = None;
    books(&mut |_, record| {
        if n == 0 || failure.is_some() {
            return;
        }
        match query.try_finds(&record) {
            Ok(true) => {}
            Ok(false) => return,
            Err(e) => {
                failure = Some(e);
                return;
            }
        }
        let key = match query.try_key(&record) {
            Ok(key) => key,
            Err(e) => {
                failure = Some(e);
                return;
            }
        };
        if after.as_ref().is_some_and(|a| key <= *a) {
            return;
        }
        let at = page.partition_point(|(k, _)| *k < key);
        if at < n {
            page.insert(at, (key, record));
            page.truncate(n);
        }
    })?;
    if let Some(e) = failure {
        return Err(e);
    }
    let mut found = Vec::new();
    found.try_reserve_exact(page.len()).map_err(out_of_memory)?;
    found.extend(page.into_iter().map(|(_, record)| record));
    Ok(found)
}

/// [`try_query_in`] in the catalog ([`try_each_at`]).
///
/// # Errors
/// As [`try_query_in`].
pub fn try_query(
    storage: &dyn Storage,
    query: &Query,
    after: Option<&Record>,
    n: usize,
) -> Result<Vec<Record>, Error> {
    try_query_in(&mut |f| try_each_at(storage, f), query, after, n)
}
