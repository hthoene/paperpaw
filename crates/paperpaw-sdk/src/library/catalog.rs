//! The catalog: every book under the library folder with what the
//! library knows of it (author, title, series, number, tags, how far it
//! is read, whether it is a favourite or was opened, when it was added,
//! the fields plugins keep in it), for views of the library, its filters
//! and searches, without reading each book's details.
//!
//! It is kept in `.paperpaw/library/catalog.txt`, a book a line, and only
//! the kernel writes it, one change at a time, in the background: made
//! anew from the library folder (`Walk`) when the library is first shown
//! after a start, when books came or went without a word and for another
//! library folder, taking each book whose file did not change from the
//! old catalog, others from their details, opening only books not seen
//! before; caught up (`Rewrite`) after books came or moved. Until a new
//! one is complete, the old one is read; where there is none yet (a new
//! card, thousands of books), the walk puts the books it found so far
//! into one at checkpoints, which views hear of as of a catalog made, so
//! they can be read at once. It is read a piece at a time, never held in
//! memory.
//!
//! How far a book is read, whether it is a favourite, and that a book was
//! removed change while the catalog stands: they are appended to a short
//! log beside it (`status.log`) and read with it ([`Changes`]), and
//! folded into it at a calm moment once the log is long, so a page turned
//! never rewrites the catalog. What was noted before a catalog was made
//! is in it, and goes from the log.
//!
//! The first line of a catalog of this format is a header (`\tcatalog\t2`
//! and its [`generation`]); a catalog of format 1 (six columns, no
//! status) is still read.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::{Book, DIR};
use crate::{Storage, StorageError, files};
use paperpaw_digest::fnv1a64;

/// Between a book's tags (and its fields) in the catalog.
const SEPARATOR: char = '\u{1f}';
/// Books [`search`] finds at most: a page of them fits the smallest
/// device (a record takes some 200 bytes).
pub const MAX_FOUND: usize = 200;
/// The start of the first line of a catalog of this format; its
/// generation follows.
pub(super) const HEADER: &str = "\tcatalog\t2";
/// Lines the log holds before the kernel folds it into the catalog; read
/// with it, 16 bytes a book.
pub const MAX_LOG: usize = 256;

pub(crate) fn catalog_path() -> String {
    format!("{DIR}/catalog.txt")
}

fn building_path() -> String {
    format!("{DIR}/catalog.tmp")
}

pub(super) fn log_path() -> String {
    format!("{DIR}/status.log")
}

/// How far a book is read and what the reader made of it: what changes
/// while the catalog stands (noted beside it).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Status {
    /// How far it is read, in percent, once opened.
    pub progress: Option<u8>,
    pub favourite: bool,
    /// It was opened (it is among the books opened last, or was).
    pub opened: bool,
}

impl Status {
    pub fn new(progress: Option<u8>, favourite: bool, opened: bool) -> Self {
        Self {
            progress,
            favourite,
            opened,
        }
    }

    /// As written in a line: the progress (empty never), then `f` for a
    /// favourite and `o` for opened.
    fn columns(self) -> (String, String) {
        let progress = self.progress.map(|p| p.to_string()).unwrap_or_default();
        let mut flags = String::new();
        if self.favourite {
            flags.push('f');
        }
        if self.opened {
            flags.push('o');
        }
        (progress, flags)
    }

    fn parse(progress: &str, flags: &str) -> Self {
        Self {
            progress: progress.parse::<u8>().ok().map(|p| p.min(100)),
            favourite: flags.contains('f'),
            opened: flags.contains('o'),
        }
    }
}

/// A book in the catalog.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Record {
    pub path: String,
    pub author: String,
    pub title: String,
    pub series: String,
    /// Its number in the series.
    pub number: String,
    /// Separated by [`SEPARATOR`]; see [`Record::tags`].
    tags: String,
    pub status: Status,
    /// The day it was added ([`Book::added`]).
    pub added: Option<u32>,
    /// Fields plugins keep in the catalog (`id=value`, separated by
    /// [`SEPARATOR`]); see [`Record::field`].
    fields: String,
    /// How large its file was when it was catalogued: a file of another
    /// size is read anew.
    pub size: Option<u64>,
}

impl Record {
    /// The record of the book at `path` with the details `book` (its
    /// progress and the day it was added; neither favourite nor opened,
    /// see [`Record::status`]), without fields.
    pub fn of(path: &str, book: &Book) -> Record {
        Record {
            path: path.to_string(),
            author: book.author.clone().unwrap_or_default(),
            title: book.shown_title(path).into_owned(),
            series: book.series.clone().unwrap_or_default(),
            number: book.series_index.clone().unwrap_or_default(),
            tags: book
                .tags
                .iter()
                .map(|t| t.replace(SEPARATOR, " "))
                .collect::<Vec<_>>()
                .join(&SEPARATOR.to_string()),
            status: Status::new(book.progress, false, false),
            added: book.added,
            fields: String::new(),
            size: (book.size > 0).then_some(book.size),
        }
    }

    /// With `status`.
    pub fn with_status(mut self, status: Status) -> Self {
        self.status = status;
        self
    }

    /// With the fields of `book` named by `ids` (those kept in the
    /// catalog, [`crate::library::BookField::in_catalog`]).
    pub fn with_fields(mut self, book: &Book, ids: &[&str]) -> Self {
        self.fields = book
            .fields
            .iter()
            .filter(|(id, _)| ids.contains(&id.as_str()))
            .map(|(id, value)| format!("{id}={}", value.replace(SEPARATOR, " ")))
            .collect::<Vec<_>>()
            .join(&SEPARATOR.to_string());
        self
    }

    /// The book's tags.
    pub fn tags(&self) -> impl Iterator<Item = &str> {
        self.tags.split(SEPARATOR).filter(|t| !t.is_empty())
    }

    /// The value of the field `id` kept in the catalog, if set.
    pub fn field(&self, id: &str) -> Option<&str> {
        self.fields
            .split(SEPARATOR)
            .filter_map(|pair| pair.split_once('='))
            .find(|(key, _)| *key == id)
            .map(|(_, value)| value)
    }

    /// How far it is read, as the filters tell it apart.
    pub fn reading(&self) -> Reading {
        match self.status.progress {
            Some(100) => Reading::Finished,
            Some(p) if p > 0 => Reading::Begun,
            _ if self.status.opened => Reading::Begun,
            _ => Reading::Unread,
        }
    }

    pub(crate) fn parse(text: &str) -> Option<Record> {
        let mut fields = text.split('\t');
        let path = fields.next().filter(|p| !p.is_empty())?.to_string();
        let mut next = || fields.next().unwrap_or("").to_string();
        let (author, title, series, number, tags) = (next(), next(), next(), next(), next());
        let (progress, flags, added, extra) = (next(), next(), next(), next());
        let size = next().parse().ok();
        Some(Record {
            path,
            author,
            title,
            series,
            number,
            tags,
            status: Status::parse(&progress, &flags),
            added: added.parse().ok(),
            fields: extra,
            size,
        })
    }

    fn text(&self) -> String {
        let clean = |s: &str| s.replace(['\t', '\n', '\r'], " ");
        let (progress, flags) = self.status.columns();
        let added = self.added.map(|a| a.to_string()).unwrap_or_default();
        let size = self.size.map(|s| s.to_string()).unwrap_or_default();
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{progress}\t{flags}\t{added}\t{}\t{size}\n",
            clean(&self.path),
            clean(&self.author),
            clean(&self.title),
            clean(&self.series),
            clean(&self.number),
            clean(&self.tags),
            clean(&self.fields)
        )
    }

    /// Whether all of the words of `query` (lowercase) are in its title,
    /// author, series, tags or file name.
    pub fn matches(&self, query: &[String]) -> bool {
        let text = format!(
            "{} {} {} {} {}",
            self.title,
            self.author,
            self.series,
            self.tags.replace(SEPARATOR, " "),
            files::name(&self.path)
        )
        .to_lowercase();
        query.iter().all(|word| text.contains(word.as_str()))
    }
}

/// How far a book is read, as filters tell it apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Reading {
    /// Never opened.
    Unread,
    /// Opened, not finished.
    Begun,
    Finished,
}

/// A book's number in its series in thousandths, to order a series by
/// (`"2"`, `"2.5"`, `"2,5"`); those without a number last.
pub fn series_order(number: &str) -> u32 {
    let number = number.trim();
    let (whole, part) = number.split_once(['.', ',']).unwrap_or((number, ""));
    let Ok(whole) = whole.parse::<u32>() else {
        return u32::MAX;
    };
    let mut thousandths = 0;
    for (n, digit) in part.bytes().take(3).enumerate() {
        if !digit.is_ascii_digit() {
            break;
        }
        thousandths += u32::from(digit - b'0') * [100, 10, 1][n];
    }
    whole
        .saturating_mul(1000)
        .saturating_add(thousandths)
        .min(u32::MAX - 1)
}

// ---- Reading the catalog ----------------------------------------------

/// Books whose changes [`Changes`] holds at most (the kernel folds the
/// log in long before: [`MAX_LOG`] lines).
const MAX_CHANGES: usize = 1024;

/// What was noted beside the catalog since it was made: each book's last
/// status, or that it was removed, by its path hashed (FNV-1a, 64 bits,
/// all of them, so that two books all but never share one: 12 bytes a book).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Changes {
    /// By the hash's low bits, then its high bits (the index finds a book
    /// by the low ones); `None` for a book removed.
    entries: Vec<(u32, u32, Option<Status>)>,
}

/// A hash as [`Changes`] keeps it: its low bits, then its high bits.
fn halves(hash: u64) -> (u32, u32) {
    (hash as u32, (hash >> 32) as u32)
}

impl Changes {
    fn get(&self, hash: u64) -> Option<Option<Status>> {
        let key = halves(hash);
        let at = self
            .entries
            .binary_search_by_key(&key, |(low, high, _)| (*low, *high))
            .ok()?;
        Some(self.entries[at].2)
    }

    /// The change noted of the book whose path's hash has these low bits
    /// ([`super::index::hash`], which keeps no more of it).
    pub(crate) fn get_short(&self, hash: u32) -> Option<Option<Status>> {
        let at = self.entries.partition_point(|(low, _, _)| *low < hash);
        self.entries
            .get(at)
            .filter(|(low, _, _)| *low == hash)
            .map(|(_, _, change)| *change)
    }

    fn note(&mut self, hash: u64, change: Option<Status>) {
        let key = halves(hash);
        match self
            .entries
            .binary_search_by_key(&key, |(low, high, _)| (*low, *high))
        {
            Ok(at) => self.entries[at].2 = change,
            Err(at) if self.entries.len() < MAX_CHANGES => {
                self.entries.insert(at, (key.0, key.1, change));
            }
            Err(_) => {}
        }
    }

    /// The status noted last of the book whose path hashes to `hash`.
    pub fn status(&self, hash: u64) -> Option<Status> {
        self.get(hash).flatten()
    }

    /// Whether the book whose path hashes to `hash` was removed.
    pub fn removed(&self, hash: u64) -> bool {
        self.get(hash) == Some(None)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `record` as noted last: with its status, or `None` where the book
    /// was removed.
    pub fn apply(&self, mut record: Record) -> Option<Record> {
        if self.entries.is_empty() {
            return Some(record);
        }
        match self.get(fnv1a64(&record.path)) {
            Some(None) => None,
            Some(Some(status)) => {
                record.status = status;
                Some(record)
            }
            None => Some(record),
        }
    }
}

/// What was noted beside the catalog ([`Changes`]); none where the log
/// cannot be read.
pub fn changes(storage: &dyn Storage) -> Changes {
    read_changes(storage).unwrap_or_default()
}

fn read_changes(storage: &dyn Storage) -> Result<Changes, StorageError> {
    let mut changes = Changes::default();
    // As many as it has lines, at most: a list grown by doubling would
    // hold twice that for a moment.
    let count = count_lines(storage, &log_path())?.min(MAX_CHANGES);
    changes
        .entries
        .try_reserve_exact(count)
        .map_err(|_| StorageError::OutOfMemory)?;
    lines(storage, &log_path(), &mut |_, line| {
        if let Some((hash, change)) = parse_change(line) {
            changes.note(hash, change);
        }
        true
    })?;
    Ok(changes)
}

/// A line of the log: the path's hash, then its progress and flags, or
/// `-` for a book removed.
fn parse_change(line: &str) -> Option<(u64, Option<Status>)> {
    let mut columns = line.split('\t');
    let hash = u64::from_str_radix(columns.next()?, 16).ok()?;
    let progress = columns.next().unwrap_or("");
    if progress == "-" {
        return Some((hash, None));
    }
    Some((
        hash,
        Some(Status::parse(progress, columns.next().unwrap_or(""))),
    ))
}

/// Calls `f` with each line of the file at `path` from byte `from` on and
/// where it starts, reading it in pieces, until `f` says `false`; where
/// the next line starts. A file that is not there has no lines; any other
/// error ends it with the error.
pub(crate) fn lines_from(
    storage: &dyn Storage,
    path: &str,
    from: u64,
    f: &mut dyn FnMut(u64, &str) -> bool,
) -> Result<u64, StorageError> {
    let mut buf = [0u8; 1024];
    let mut rest: Vec<u8> = Vec::new();
    // Where `rest` starts in the file, and where the next read does.
    let (mut start, mut offset) = (from, from);
    loop {
        let n = match storage.read_at(path, offset, &mut buf) {
            Ok(n) => n,
            Err(StorageError::NotFound) if offset == from => return Ok(from),
            Err(e) => return Err(e),
        };
        if n == 0 {
            break;
        }
        offset += n as u64;
        rest.extend_from_slice(&buf[..n]);
        while let Some(end) = rest.iter().position(|&b| b == b'\n') {
            let go_on = f(start, String::from_utf8_lossy(&rest[..end]).trim_end());
            start += end as u64 + 1;
            rest.drain(..=end);
            if !go_on {
                return Ok(start);
            }
        }
    }
    if !rest.is_empty() {
        f(start, String::from_utf8_lossy(&rest).trim_end());
        start += rest.len() as u64;
    }
    Ok(start)
}

/// How many lines the file at `path` has (none where it is not there),
/// counted a piece at a time.
pub(super) fn count_lines(storage: &dyn Storage, path: &str) -> Result<usize, StorageError> {
    let mut buf = [0u8; 512];
    let (mut offset, mut count, mut last) = (0u64, 0, b'\n');
    loop {
        let n = match storage.read_at(path, offset, &mut buf) {
            Ok(n) => n,
            Err(StorageError::NotFound) if offset == 0 => return Ok(0),
            Err(e) => return Err(e),
        };
        if n == 0 {
            break;
        }
        offset += n as u64;
        count += buf[..n].iter().filter(|&&b| b == b'\n').count();
        last = buf[n - 1];
    }
    Ok(count + usize::from(last != b'\n'))
}

/// [`lines_from`] the start, every line.
fn lines(
    storage: &dyn Storage,
    path: &str,
    f: &mut dyn FnMut(u64, &str) -> bool,
) -> Result<(), StorageError> {
    lines_from(storage, path, 0, f).map(|_| ())
}

/// Calls `f` with each book of the catalog and where its line starts
/// (to read it again, [`read_page`]), as noted last: those removed left
/// out. Where the catalog cannot be read further, the books before.
pub fn each_at(storage: &dyn Storage, f: &mut dyn FnMut(u64, Record)) {
    let changes = changes(storage);
    #[expect(
        clippy::let_underscore_must_use,
        reason = "a catalog cut short gives the books before, as said above"
    )]
    let _ = lines(storage, &catalog_path(), &mut |offset, line| {
        if let Some(record) = Record::parse(line).and_then(|r| changes.apply(r)) {
            f(offset, record);
        }
        true
    });
}

/// Calls `f` with each book of the catalog, reading it in pieces.
pub fn each(storage: &dyn Storage, f: &mut dyn FnMut(Record)) {
    each_at(storage, &mut |_, record| f(record));
}

/// The line of the catalog at `path` that starts at `offset`, as it is
/// written.
fn read_line(
    storage: &dyn Storage,
    path: &str,
    offset: u64,
) -> Result<Option<Record>, StorageError> {
    let mut buf = [0u8; 256];
    let mut bytes = Vec::new();
    let mut at = offset;
    loop {
        let n = storage.read_at(path, at, &mut buf)?;
        if n == 0 {
            break;
        }
        if let Some(end) = buf[..n].iter().position(|&b| b == b'\n') {
            bytes.extend_from_slice(&buf[..end]);
            break;
        }
        bytes.extend_from_slice(&buf[..n]);
        at += n as u64;
        // Values are at most a kilobyte each: a line is never this long.
        if bytes.len() > 16 * 1024 {
            break;
        }
    }
    Ok(Record::parse(String::from_utf8_lossy(&bytes).trim_end()))
}

/// The book whose line starts at `offset` in the catalog (as [`each_at`]
/// told), as noted last; for more than one, [`read_page`] reads the log
/// once.
pub fn read_at(storage: &dyn Storage, offset: u64) -> Option<Record> {
    read_page(storage, &[offset]).pop().flatten()
}

/// The books whose lines start at `offsets` (a page of a view), as noted
/// last: the log read once. `None` for one removed or not there.
pub fn read_page(storage: &dyn Storage, offsets: &[u64]) -> Vec<Option<Record>> {
    let changes = changes(storage);
    let path = catalog_path();
    offsets
        .iter()
        .map(|&offset| {
            read_line(storage, &path, offset)
                .ok()
                .flatten()
                .and_then(|r| changes.apply(r))
        })
        .collect()
}

/// Whether there is a catalog, and whether it is being made anew.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum State {
    /// None was made yet.
    Missing,
    /// None yet, one is being made.
    Building,
    /// There is one; `stamp` changes when it or its log does (views that
    /// keep something of it read it again then).
    Ready { stamp: u64 },
}

/// Where a walk that began without a catalog notes that the catalog
/// holds the books found so far: gone when a walk ends, so a walk that
/// goes on from one cut short (the power gone, the walk started again)
/// finds it.
fn first_path() -> String {
    format!("{DIR}/walk.first")
}

/// Whether the catalog's books are those found so far: a walk that began
/// without one is under way, or one that goes on from it. A walk over a
/// whole catalog (each start) is not.
pub(crate) fn is_growing(storage: &dyn Storage) -> bool {
    storage.exists(&building_path()) && storage.exists(&first_path())
}

/// Whether there is a catalog ([`State`]).
pub fn state(storage: &dyn Storage) -> State {
    match storage.size(&catalog_path()) {
        Ok(size) => {
            let log = storage.size(&log_path()).unwrap_or(0);
            State::Ready {
                stamp: (size << 20) ^ log,
            }
        }
        Err(_) if storage.exists(&building_path()) => State::Building,
        Err(_) => State::Missing,
    }
}

/// Which catalog there is: a number that grows each time it is written
/// anew (made, caught up), so that what a view keeps of it (where its
/// lines start) is known to be of this one; `None` without a catalog.
/// Only the kernel writes it, one at a time.
pub fn generation(storage: &dyn Storage) -> Option<u64> {
    let mut buf = [0u8; 48];
    let n = storage.read_at(&catalog_path(), 0, &mut buf).ok()?;
    let line = &buf[..n];
    let line = &line[..line.iter().position(|&b| b == b'\n').unwrap_or(n)];
    let text = core::str::from_utf8(line).unwrap_or("");
    let Some(rest) = text.strip_prefix(HEADER) else {
        // Of format 1, or without a header: the first.
        return Some(0);
    };
    Some(rest.trim_start_matches('\t').trim().parse().unwrap_or(0))
}

/// How long the catalog is, in bytes.
pub(crate) fn size(storage: &dyn Storage) -> Option<u64> {
    storage.size(&catalog_path()).ok()
}

/// Generations a stamp of the catalog tells apart (an index of it keeps
/// the low 24 bits, see [`super::index::catalog_version`]).
const GENERATIONS: u64 = (1 << 24) - 1;

/// The generation of a catalog made where there is none: drawn at random
/// (else from the time, else the first), so that it is not the one of a
/// catalog gone, and what was kept of that one (a sorted index of it,
/// where its lines start) is not taken for this one's.
pub(crate) fn first_generation(env: &crate::Env<'_>) -> u64 {
    let mut drawn = [0u8; 8];
    let start = if env.random().fill(&mut drawn) {
        u64::from_le_bytes(drawn)
    } else {
        crate::time::unix_seconds(env).map_or(0, i64::unsigned_abs)
    };
    (start & GENERATIONS).max(1)
}

/// The header of the catalog written next: of the generation after the
/// one there is, `first` where there is none.
fn next_header(storage: &dyn Storage, first: u64) -> String {
    let generation = generation(storage).map_or(first, |g| g.wrapping_add(1));
    format!("{HEADER}\t{generation}\n")
}

// ---- Status changes ---------------------------------------------------

/// Notes the status of the book at `path` (how far it is read, favourite,
/// opened) beside the catalog, read with it at once. The kernel calls
/// this from the reading events and the library's setters
/// ([`super::record`], [`super::set_finished`], [`super::set_favourite`]),
/// and folds the log into the catalog at a calm moment once it is long.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn note(storage: &dyn Storage, path: &str, status: Status) -> Result<(), StorageError> {
    noted(storage, path, status)
}

pub(crate) fn noted(storage: &dyn Storage, path: &str, status: Status) -> Result<(), StorageError> {
    if !storage.exists(&catalog_path()) {
        // Made anew from the books' details, which have it.
        return Ok(());
    }
    let (progress, flags) = status.columns();
    let line = format!("{:016x}\t{progress}\t{flags}\n", fnv1a64(path));
    storage.append(&log_path(), line.as_bytes())
}

/// Notes beside the catalog that the book at `path` is no longer there
/// (it was removed, or moved away): views leave it out at once, and what
/// was noted of it before no longer counts.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn forget(storage: &dyn Storage, path: &str) -> Result<(), StorageError> {
    if !storage.exists(&catalog_path()) {
        return Ok(());
    }
    let line = format!("{:016x}\t-\t\n", fnv1a64(path));
    storage.append(&log_path(), line.as_bytes())
}

/// Whether the log beside the catalog is long enough to be folded into
/// it ([`MAX_LOG`] lines; [`Rewrite`]).
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn log_is_long(storage: &dyn Storage) -> bool {
    // A line takes some 22 bytes.
    storage.size(&log_path()).unwrap_or(0) > (MAX_LOG * 22) as u64
}

// ---- Queries ----------------------------------------------------------

/// Which books a query finds by how far they are read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Show {
    #[default]
    All,
    Unread,
    /// Opened, not finished.
    Reading,
    Finished,
    Favourites,
    /// A value a later build adds, by its name (a constant of that build);
    /// a build that doesn't know it: it shows the default.
    Other(&'static str),
}

/// In what order a query finds books.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Sort {
    /// By title, regardless of case.
    #[default]
    Title,
    /// By author, then title.
    Author,
    /// By series, then the number in it.
    Series,
    /// The added last first.
    Added,
    /// A value a later build adds, by its name (a constant of that build);
    /// a build that doesn't know it: it sorts by the default.
    Other(&'static str),
}

/// What books to find in the catalog ([`query`]): all of the conditions set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Query {
    pub show: Show,
    /// Words, each in its title, author, series, tags or file name.
    pub words: Vec<String>,
    pub author: Option<String>,
    pub series: Option<String>,
    pub tag: Option<String>,
    /// Books in this folder or below it.
    pub folder: Option<String>,
    /// Books added on this day or later.
    pub added_since: Option<u32>,
    pub sort: Sort,
}

impl Query {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn show(mut self, show: Show) -> Self {
        self.show = show;
        self
    }

    /// The words of `text` (any case).
    pub fn words(mut self, text: &str) -> Self {
        self.words = query_words(text);
        self
    }

    pub fn author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    pub fn series(mut self, series: impl Into<String>) -> Self {
        self.series = Some(series.into());
        self
    }

    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    pub fn folder(mut self, folder: impl Into<String>) -> Self {
        self.folder = Some(folder.into());
        self
    }

    pub fn added_since(mut self, day: u32) -> Self {
        self.added_since = Some(day);
        self
    }

    pub fn sort(mut self, sort: Sort) -> Self {
        self.sort = sort;
        self
    }

    /// Whether `record` is found.
    pub fn finds(&self, record: &Record) -> bool {
        let shown = match self.show {
            // A choice of a later build: everything.
            Show::All | Show::Other(_) => true,
            Show::Unread => record.reading() == Reading::Unread,
            Show::Reading => record.reading() == Reading::Begun,
            Show::Finished => record.reading() == Reading::Finished,
            Show::Favourites => record.status.favourite,
        };
        let folder = self.folder.as_deref().map(|f| f.trim_end_matches('/'));
        shown
            && self.author.as_deref().is_none_or(|a| record.author == a)
            && self.series.as_deref().is_none_or(|s| record.series == s)
            && self
                .tag
                .as_deref()
                .is_none_or(|t| record.tags().any(|x| x == t))
            && folder.is_none_or(|f| {
                record
                    .path
                    .strip_prefix(f)
                    .is_some_and(|rest| rest.starts_with('/'))
            })
            && self
                .added_since
                .is_none_or(|d| record.added.is_some_and(|a| a >= d))
            && (self.words.is_empty() || record.matches(&self.words))
    }

    /// Where `record` goes in this query's order.
    fn key(&self, record: &Record) -> (u32, String, String) {
        let title = record.title.to_lowercase();
        let (first, text) = match self.sort {
            // An order of a later build: by title.
            Sort::Title | Sort::Other(_) => (0, title),
            Sort::Author => (0, format!("{}\u{1}{title}", record.author.to_lowercase())),
            Sort::Series => (
                0,
                format!(
                    "{}\u{1}{:010}",
                    record.series.to_lowercase(),
                    series_order(&record.number)
                ),
            ),
            Sort::Added => (u32::MAX - record.added.unwrap_or(0), title),
        };
        (first, text, record.path.clone())
    }
}

/// A library's books, each with its key ([`each_at`] for the catalog's,
/// [`crate::library::Library::each`] for any library's): called with what
/// to do with each.
pub type Books<'a> = &'a mut dyn FnMut(&mut dyn FnMut(u64, Record));

/// `n` books `query` finds, in its order, those after `after` (from the
/// first without): one reading of the catalog that holds only `n` books
/// at a time (keyset pagination).
pub fn query(
    storage: &dyn Storage,
    query: &Query,
    after: Option<&Record>,
    n: usize,
) -> Vec<Record> {
    query_in(&mut |f| each_at(storage, f), query, after, n)
}

/// [`query`] in `books`.
pub fn query_in(books: Books<'_>, query: &Query, after: Option<&Record>, n: usize) -> Vec<Record> {
    let after = after.map(|r| query.key(r));
    let mut page: Vec<((u32, String, String), Record)> = Vec::new();
    books(&mut |_, record| {
        if n == 0 || !query.finds(&record) {
            return;
        }
        let key = query.key(&record);
        if after.as_ref().is_some_and(|a| key <= *a) {
            return;
        }
        let at = page.partition_point(|(k, _)| *k < key);
        if at < n {
            page.insert(at, (key, record));
            page.truncate(n);
        }
    });
    page.into_iter().map(|(_, record)| record).collect()
}

/// How many books `query` finds.
pub fn found(storage: &dyn Storage, query: &Query) -> usize {
    found_in(&mut |f| each_at(storage, f), query)
}

/// [`found`] in `books`.
pub fn found_in(books: Books<'_>, query: &Query) -> usize {
    let mut count = 0;
    books(&mut |_, record| count += usize::from(query.finds(&record)));
    count
}

/// `n` records of the catalog that `keep` keeps, by title regardless of
/// case (then by path), those after `after` (from the first without).
pub fn page(
    storage: &dyn Storage,
    keep: &dyn Fn(&Record) -> bool,
    after: Option<&Record>,
    n: usize,
) -> Vec<Record> {
    let by_title = Query::new();
    let after = after.map(|r| by_title.key(r));
    let mut page: Vec<((u32, String, String), Record)> = Vec::new();
    each(storage, &mut |record| {
        if n == 0 || !keep(&record) {
            return;
        }
        let key = by_title.key(&record);
        if after.as_ref().is_some_and(|a| key <= *a) {
            return;
        }
        let at = page.partition_point(|(k, _)| *k < key);
        if at < n {
            page.insert(at, (key, record));
            page.truncate(n);
        }
    });
    page.into_iter().map(|(_, record)| record).collect()
}

/// How many records of the catalog `keep` keeps.
pub fn count(storage: &dyn Storage, keep: &dyn Fn(&Record) -> bool) -> usize {
    let mut count = 0;
    each(storage, &mut |record| count += usize::from(keep(&record)));
    count
}

/// The words of a query, lowercase, as [`Record::matches`] takes them.
pub fn query_words(query: &str) -> Vec<String> {
    query
        .to_lowercase()
        .split_whitespace()
        .map(String::from)
        .collect()
}

/// The books of the catalog matching `query` (words, any case), by
/// title: the first [`MAX_FOUND`]. [`search_after`] pages through more.
pub fn search(storage: &dyn Storage, query: &str) -> Vec<Record> {
    search_after(storage, query, None, MAX_FOUND)
}

/// `n` books of the catalog matching `query` after `after`, by title (see
/// [`page`]); none for an empty query.
pub fn search_after(
    storage: &dyn Storage,
    query: &str,
    after: Option<&Record>,
    n: usize,
) -> Vec<Record> {
    let words = query_words(query);
    if words.is_empty() {
        return Vec::new();
    }
    page(storage, &|record| record.matches(&words), after, n)
}

/// Writes `records` as the catalog (emptying the log), e.g. for tests of views.
#[cfg(any(test, feature = "kernel"))]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn write(storage: &dyn Storage, records: &[Record]) -> Result<(), crate::data::Error> {
    let mut text = next_header(storage, 1);
    text.extend(records.iter().map(Record::text));
    storage.write(&catalog_path(), text.as_bytes())?;
    match storage.remove(&log_path()) {
        Ok(()) | Err(StorageError::NotFound) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

mod fallible;
pub(crate) use fallible::each_head;
pub use fallible::{try_changes, try_count, try_path_at};

#[cfg(any(test, feature = "kernel"))]
mod making;
#[cfg(any(test, feature = "kernel"))]
mod publish;
#[cfg(any(test, feature = "kernel"))]
pub use making::{Rewrite, Walk};

#[cfg(test)]
mod tests;
