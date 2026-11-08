//! Merging statistics with KOReader's, as KOReader's own sync does
//! (`ReaderStatistics.onSync`), except for what this device lost:
//!
//! - Books are the same when their file is (the partial MD5; for a book
//!   KOReader knows no file of, KOReader's key of title, authors and
//!   MD5). Their title and authors are taken from KOReader, so KOReader
//!   recognises them by its key (title, authors, MD5) when it merges the
//!   file back.
//! - What the last uploaded file (`cached`) has but the other side no
//!   longer has was deleted there: it goes here too. Books and single
//!   stays alike; but a book deleted there keeps the stays read here
//!   since the last upload, and so stays.
//! - Where the other side's file lacks too much of the last upload (all
//!   its books, or more than half of its stays), it is told apart what
//!   happened. Whole books gone, the others' stays there, and a stay of
//!   a book gone older than one there: they were deleted there. All that
//!   is lacking newer than all there, and a book there cut short: a
//!   backup was restored there. A file of another version is replaced
//!   too. What was replaced counts as not deleted there
//!   ([`Outcome::kept`]). Where it cannot be told (an empty file: all
//!   statistics reset there, or another device's file), nothing counts
//!   as deleted there until the user decides ([`Outcome::unclear`]).
//! - What the last upload has but this device no longer has is not taken
//!   for deleted here, as KOReader would take it: PaperPaw deletes no
//!   statistics, so it was lost, and comes back from the other side.
//! - The same stay (book, page, start) on both sides keeps the longer
//!   duration.
//!
//! - Stays of a book whose entry was lost here are kept, and books added
//!   take ids above theirs; they are not uploaded.
//!
//! Nothing is merged from what cannot be read whole: an unreadable file
//! of the other side (also one without a column needed), `books.ini` or
//! log ends the merge with an error before anything here is written. A
//! last upload that cannot be read is left out: then nothing counts as
//! deleted there.
//!
//! Everything goes through files sorted in small runs ([`extsort`]), so
//! memory stays small whatever the number of stays. What is kept in memory
//! (the books of both sides, the pages read per book) is taken with
//! allocation that can be refused: a refused one fails the merge with
//! [`StorageError::TooLarge`] (as [`Failure::Here`]) before the statistics
//! here are changed. The database reader and writer of `paperpaw-sqlite`
//! still allocate small, short-lived buffers per row on their own.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use paperpaw_sdk::digest::md5::Md5;
use paperpaw_sdk::{Storage, StorageError};
use paperpaw_sqlite::{Database, Error};

use crate::extsort::Record;
use crate::koreader::{self, KoBook, STAT, fields, stat_record};
use crate::store::{self, Book};
use crate::{extsort, room, totals};

#[cfg(test)]
use alloc::{format, string::ToString};

/// What a merge did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Outcome {
    pub books: usize,
    pub stays: usize,
    /// What the other side's file lacks of the last upload was not taken
    /// for deleted there: the file looks replaced (by another device's
    /// that started anew, or a backup), or is of another version.
    pub kept: bool,
    /// The other side's file lacks much of the last upload, and it cannot
    /// be told whether that was deleted there or the file replaced:
    /// nothing was deleted here, and the user is to be asked
    /// ([`merge_as`]).
    pub unclear: bool,
}

/// Why a merge failed. The statistics here are as they were, unless
/// writing them failed halfway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The other side's file cannot be read: no statistics database (e.g.
    /// a web page), cut short, or of a kind not understood.
    Income(Error),
    /// The statistics here cannot be read or written.
    Here(Error),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Income(e) | Failure::Here(e) => write!(f, "{e}"),
        }
    }
}

impl From<StorageError> for Failure {
    fn from(error: StorageError) -> Self {
        Failure::Here(Error::Storage(error))
    }
}

/// What tells books apart: KOReader's partial MD5 of the file; for a book
/// KOReader knows no file of, a digest of its key (title, authors, MD5),
/// so such books stay apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Key([u8; 16]);

impl Key {
    fn of(md5: &str, title: &str, authors: &str) -> Self {
        Self(Self::from_hex(md5).unwrap_or_else(|| {
            let mut digest = Md5::new();
            for part in [title, authors, md5] {
                digest.update(part.as_bytes());
                digest.update(&[0]);
            }
            let mut digits = [0u8; 32];
            Self::from_hex(digest.hex_into(&mut digits)).unwrap_or_default()
        }))
    }

    /// The 16 bytes of 32 hexadecimal digits (either case), if that is
    /// what `text` is. Allocates nothing.
    fn from_hex(text: &str) -> Option<[u8; 16]> {
        let digits = text.as_bytes();
        if digits.len() != 32 {
            return None;
        }
        let mut out = [0u8; 16];
        for (byte, [high, low]) in out.iter_mut().zip(digits.as_chunks::<2>().0) {
            let digit = |c: u8| char::from(c).to_digit(16);
            *byte = (digit(*high)? << 4 | digit(*low)?) as u8;
        }
        Some(out)
    }

    fn of_book(book: &Book) -> Self {
        Self::of(&book.md5, &book.title, &book.authors)
    }

    fn of_ko(book: &KoBook) -> Self {
        Self::of(&book.md5, &book.title, &book.authors)
    }
}

/// A book of the other side's file, as far as it matters once the books
/// are merged.
#[derive(Debug, Clone, Copy)]
struct Known {
    key: Key,
    /// Its id there.
    id: i64,
    notes: i64,
    highlights: i64,
}

/// A file of the other side (or the last upload): its books by key.
struct Side {
    path: String,
    books: Vec<Known>,
    /// Its version (`PRAGMA user_version`).
    version: u32,
}

impl Side {
    /// The side at `path`, and its books whole.
    fn open(storage: &dyn Storage, path: &str) -> Result<(Self, Vec<KoBook>), Error> {
        let db = Database::open(storage, path)?;
        let books = koreader::books(&db)?;
        let mut known: Vec<Known> = Vec::new();
        room::reserve(&mut known, books.len())?;
        for b in &books {
            known.push(Known {
                key: Key::of_ko(b),
                id: b.id,
                notes: b.notes,
                highlights: b.highlights,
            });
        }
        known.sort_unstable_by_key(|k| k.key);
        let side = Self {
            path: room::string(&[path])?,
            books: known,
            version: db.user_version,
        };
        Ok((side, books))
    }

    fn find(&self, key: Key) -> Option<&Known> {
        let at = self.books.binary_search_by_key(&key, |k| k.key).ok()?;
        self.books.get(at)
    }
}

/// The working files of a merge, in one folder.
struct Files {
    /// This device's stays, the other side's, the last upload's, sorted.
    local: String,
    income: String,
    cached: String,
    /// The stays merged, by start; then in the order of the log.
    merged: String,
    time: String,
    /// The stays of the upload: those of the books it has.
    upload: String,
    /// For sorting, and for writing the upload.
    runs: String,
    write: String,
}

impl Files {
    fn in_dir(dir: &str) -> Result<Self, StorageError> {
        let path = |name: &str| room::string(&[dir, "/", name]);
        Ok(Self {
            local: path("merge-local")?,
            income: path("merge-income")?,
            cached: path("merge-cached")?,
            merged: path("merge-out")?,
            time: path("merge-time")?,
            upload: path("merge-upload")?,
            runs: path("merge-run")?,
            write: path("merge-write")?,
        })
    }

    fn remove(&self, storage: &dyn Storage) {
        for file in [
            &self.local,
            &self.income,
            &self.cached,
            &self.merged,
            &self.time,
            &self.upload,
        ] {
            #[expect(
                clippy::let_underscore_must_use,
                reason = "a temporary file left behind is written over the next time"
            )]
            let _ = storage.remove(file);
        }
    }
}

/// Merges the statistics database `income` (another device's, if any)
/// into this device's statistics; `cached` is the file this device last
/// uploaded to the same place. With `out`, writes the merged statistics
/// there as a KOReader database. Working files go to `dir`; they are
/// gone afterwards, and so is `out` after an error.
pub(crate) fn merge(
    storage: &dyn Storage,
    income: Option<&str>,
    cached: Option<&str>,
    out: Option<&str>,
    dir: &str,
) -> Result<Outcome, Failure> {
    merge_as(storage, income, cached, out, dir, None)
}

/// [`merge`], with what the user decided where it is unclear whether
/// what the other side's file lacks was deleted there
/// ([`Outcome::unclear`]): `Some(true)` deletes it here too,
/// `Some(false)` keeps it.
pub(crate) fn merge_as(
    storage: &dyn Storage,
    income: Option<&str>,
    cached: Option<&str>,
    out: Option<&str>,
    dir: &str,
    decided: Option<bool>,
) -> Result<Outcome, Failure> {
    let files = Files::in_dir(dir)?;
    let result = merge_with(storage, income, cached, out, &files, decided);
    files.remove(storage);
    if result.is_err()
        && let Some(out) = out
    {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "a merge that failed leaves a file the next merge writes over"
        )]
        let _ = storage.remove(out);
    }
    result
}

fn merge_with(
    storage: &dyn Storage,
    income: Option<&str>,
    cached: Option<&str>,
    out: Option<&str>,
    files: &Files,
    decided: Option<bool>,
) -> Result<Outcome, Failure> {
    let income = match income {
        Some(path) => Some(Side::open(storage, path).map_err(Failure::Income)?),
        None => None,
    };
    let cached = cached.and_then(|path| Side::open(storage, path).ok().map(|(side, _)| side));
    let books = store::load_books(storage)?;
    let highest = write_local(storage, files)?;
    let (mut books, mut gone, income) = match income {
        Some((side, theirs)) => {
            let (books, gone) = merge_books(books, &side, theirs, cached.as_ref(), highest)?;
            (books, gone, Some(side))
        }
        None => (books, Vec::new(), None),
    };
    match &income {
        Some(side) => write_side(storage, side, &books, &files.income, &files.runs)?,
        None => storage.write(&files.income, &[])?,
    }
    let cached_read = match &cached {
        Some(side) => match write_side(storage, side, &books, &files.cached, &files.runs) {
            Ok(()) => true,
            // Its books could be read, its stays cannot: left out.
            Err(Failure::Income(_)) => false,
            Err(e) => return Err(e),
        },
        None => false,
    };
    if !cached_read {
        storage.write(&files.cached, &[])?;
    }
    // Deletions there are taken over only from a file that is a later
    // state of the last upload, or where the user decided so.
    let verdict = match (&income, &cached) {
        (Some(side), Some(before)) if cached_read => Some(judge(storage, side, before, files)?),
        _ => None,
    };
    let deletions = match verdict {
        Some(Verdict::Later) => true,
        Some(Verdict::Unclear) => decided == Some(true),
        Some(Verdict::Replaced) | None => false,
    };
    let stays = merge_stays(storage, files, deletions, &mut gone)?;
    if deletions {
        // Gone with their stays, unless one was read here since.
        books.retain(|b| match gone.binary_search_by_key(&b.id, |g| g.0) {
            Ok(at) => gone[at].1,
            Err(_) => true,
        });
    }
    let per_book = to_time_order(storage, &books, files, out.is_some())?;
    // Both new files are written before either replaces the old one, so
    // that a refusal or a full card leaves the log and the books as a pair.
    store::stage_log(storage, &files.time)?;
    if let Err(e) = store::stage_books(storage, &books) {
        store::unstage_log(storage);
        return Err(e.into());
    }
    store::commit_pair(storage, &books)?;
    let outcome = Outcome {
        books: books.len(),
        stays,
        kept: verdict == Some(Verdict::Replaced),
        unclear: verdict == Some(Verdict::Unclear) && decided.is_none(),
    };
    if let Some(out) = out {
        write_upload(storage, out, books, &per_book, income.as_ref(), files)
            .map_err(Failure::Here)?;
    }
    Ok(outcome)
}

/// Books deleted there, by id, each with whether a stay of it is kept.
type Gone = Vec<(u16, bool)>;

/// The books here after the merge, by id: the other side's added (named
/// as there, with ids above `highest`, the log's highest); and the ids of
/// those deleted there (in the last upload, not in the other side's
/// file), each with `false` for no stay of it kept yet. They go once the
/// stays are merged, unless a stay of them was read here since.
fn merge_books(
    mut books: Vec<Book>,
    income: &Side,
    theirs: Vec<KoBook>,
    cached: Option<&Side>,
    highest: u16,
) -> Result<(Vec<Book>, Gone), Failure> {
    let mut gone: Gone = Vec::new();
    if let Some(cached) = cached {
        for b in &books {
            let key = Key::of_book(b);
            if cached.find(key).is_some() && income.find(key).is_none() {
                room::push(&mut gone, (b.id, false))?;
            }
        }
    }
    let mut last = books.iter().map(|b| b.id).max().unwrap_or(0).max(highest);
    // This device's books by key (their places in `books`), with those
    // added.
    let mut here: Vec<(Key, usize)> = Vec::new();
    // Room for the books of both sides: nothing below can be refused
    // halfway through a book.
    room::reserve(&mut here, books.len().saturating_add(theirs.len()))?;
    room::reserve(&mut books, theirs.len())?;
    for (at, b) in books.iter().enumerate() {
        here.push((Key::of_book(b), at));
    }
    here.sort_unstable();
    for theirs in theirs {
        let key = Key::of_ko(&theirs);
        match here.binary_search_by_key(&key, |(k, _)| *k) {
            Ok(at) => {
                if let Some(ours) = here.get(at).and_then(|(_, i)| books.get_mut(*i)) {
                    take_names(ours, &theirs)?;
                }
            }
            Err(at) => {
                last = last
                    .checked_add(1)
                    .ok_or(Failure::Here(Error::Corrupt("too many books")))?;
                here.insert(at, (key, books.len()));
                books.push(Book {
                    id: last,
                    pages: theirs.pages.clamp(0, i64::from(u32::MAX)) as u32,
                    last_open: theirs.last_open,
                    title: theirs.title,
                    authors: theirs.authors,
                    md5: theirs.md5,
                    language: theirs.language,
                    series: theirs.series,
                    path: String::new(),
                    finished: 0,
                });
            }
        }
    }
    Ok((books, gone))
}

/// What the other side's file is to the last upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// A later state of it: what it lacks was deleted there.
    Later,
    /// Another file (a backup, or of another version): what it lacks was
    /// not deleted there.
    Replaced,
    /// Either: the user decides.
    Unclear,
}

/// What the other side's file is to the last upload: of another version,
/// it was replaced. Lacking little (not all books, at most half the
/// stays), it is a later state. Lacking more, see [`Lack::verdict`].
fn judge(
    storage: &dyn Storage,
    income: &Side,
    cached: &Side,
    files: &Files,
) -> Result<Verdict, Failure> {
    if income.version != koreader::SCHEMA_VERSION {
        return Ok(Verdict::Replaced);
    }
    let all_gone =
        !cached.books.is_empty() && cached.books.iter().all(|b| income.find(b.key).is_none());
    let lack = Lack::of(storage, files)?;
    if !all_gone && lack.lacking * 2 <= lack.uploaded {
        return Ok(Verdict::Later);
    }
    Ok(lack.verdict())
}

/// What the other side's stays lack of the last upload's.
#[derive(Debug, Default)]
struct Lack {
    /// The last upload's stays, and those lacking there.
    uploaded: u64,
    lacking: u64,
    /// The same, of the books with stays there.
    kept: u64,
    cut: u64,
    /// The start of the newest stay there, of the oldest lacking, and of
    /// the oldest of a book without stays there.
    newest: Option<u32>,
    oldest: Option<u32>,
    oldest_gone: Option<u32>,
}

/// One book's stays, as they are walked.
#[derive(Debug, Default)]
struct Group {
    book: u32,
    there: bool,
    uploaded: u64,
    lacking: u64,
    oldest: Option<u32>,
}

impl Lack {
    /// Walks both sorted files side by side, a book at a time.
    fn of(storage: &dyn Storage, files: &Files) -> Result<Self, Failure> {
        let mut there = Source::new(storage, &files.income)?;
        let mut before = Source::new(storage, &files.cached)?;
        let mut lack = Self::default();
        let mut group = Group::default();
        while let Some(next) = [there.peek(), before.peek()]
            .into_iter()
            .flatten()
            .map(|r| key(r))
            .min()
        {
            let book = u32::from_be_bytes([next[0], next[1], next[2], next[3]]);
            if book != group.book {
                lack.add(core::mem::take(&mut group));
                group.book = book;
            }
            let theirs = there.peek().is_some_and(|r| key(r) == next);
            while there.peek().is_some_and(|r| key(r) == next) {
                there.next();
            }
            let ours = before.peek().is_some_and(|r| key(r) == next);
            while before.peek().is_some_and(|r| key(r) == next) {
                before.next();
            }
            let start = u32::from_be_bytes([next[8], next[9], next[10], next[11]]);
            if theirs {
                group.there = true;
                lack.newest = lack.newest.max(Some(start));
            } else if ours {
                group.lacking += 1;
                group.oldest = Some(group.oldest.map_or(start, |o| o.min(start)));
            }
            group.uploaded += u64::from(ours);
        }
        lack.add(group);
        for source in [&mut there, &mut before] {
            if let Some(e) = source.records.failed.take() {
                return Err(e.into());
            }
        }
        Ok(lack)
    }

    fn add(&mut self, group: Group) {
        let min = |a: Option<u32>, b: Option<u32>| a.into_iter().chain(b).min();
        self.uploaded += group.uploaded;
        self.lacking += group.lacking;
        self.oldest = min(self.oldest, group.oldest);
        if group.there {
            self.kept += group.uploaded;
            self.cut += group.lacking;
        } else {
            self.oldest_gone = min(self.oldest_gone, group.oldest);
        }
    }

    /// Lacking much: deleted there where whole books are gone, the
    /// others (nearly) whole, and a stay of a book gone is older than one
    /// there (a backup has every stay before its newest). Replaced where
    /// all that is lacking is newer than all there, and a book there is
    /// cut short (a backup restored). Else, or with no stays there (all
    /// reset there, or a fresh device's file), it cannot be told.
    fn verdict(&self) -> Verdict {
        let Some(newest) = self.newest else {
            return Verdict::Unclear;
        };
        if self.kept > 0
            && self.cut * 2 <= self.kept
            && self.oldest_gone.is_some_and(|o| o < newest)
        {
            return Verdict::Later;
        }
        if self.cut > 0 && self.oldest.is_some_and(|o| o > newest) {
            return Verdict::Replaced;
        }
        Verdict::Unclear
    }
}

/// What tells stays apart: book, page and start.
fn key(record: &[u8]) -> [u8; 12] {
    let mut k = [0u8; 12];
    for (to, from) in k.iter_mut().zip(record) {
        *to = *from;
    }
    k
}

/// A book here, named as on the other side, with what that side knows
/// and this one does not.
fn take_names(ours: &mut Book, theirs: &KoBook) -> Result<(), StorageError> {
    // All copies are made before anything is changed.
    let title = room::string(&[&theirs.title])?;
    let authors = room::string(&[&theirs.authors])?;
    let language = if ours.language == "N/A" || ours.language.is_empty() {
        Some(room::string(&[&theirs.language])?)
    } else {
        None
    };
    let series = if ours.series == "N/A" || ours.series.is_empty() {
        Some(room::string(&[&theirs.series])?)
    } else {
        None
    };
    ours.title = title;
    ours.authors = authors;
    if let Some(language) = language {
        ours.language = language;
    }
    if let Some(series) = series {
        ours.series = series;
    }
    ours.last_open = ours.last_open.max(theirs.last_open);
    if ours.pages == 0 {
        ours.pages = theirs.pages.clamp(0, i64::from(u32::MAX)) as u32;
    }
    Ok(())
}

/// All of this device's stays, sorted, also those of books without an
/// entry; the highest book id of them. A read error fails the merge: what
/// was not read would look deleted.
fn write_local(storage: &dyn Storage, files: &Files) -> Result<u16, Failure> {
    let mut highest = 0;
    let mut file = Appender::new(storage, &files.local)?;
    store::scan(storage, |s| {
        highest = highest.max(s.book);
        file.push(&stat_record([
            u32::from(s.book),
            s.page,
            s.start,
            u32::from(s.duration),
            s.pages,
        ]));
    })?;
    file.finish()?;
    extsort::sort(storage, &files.local, &files.local, STAT, &files.runs)?;
    Ok(highest)
}

/// A side's stays, with the ids of its books here, sorted. What cannot
/// be read of the side is [`Failure::Income`].
fn write_side(
    storage: &dyn Storage,
    side: &Side,
    books: &[Book],
    path: &str,
    runs: &str,
) -> Result<(), Failure> {
    // The side's book ids as ids here, by theirs.
    let mut here: Vec<(Key, u16)> = Vec::new();
    room::reserve(&mut here, books.len())?;
    for b in books {
        here.push((Key::of_book(b), b.id));
    }
    here.sort_unstable();
    let mut ids: Vec<(i64, u16)> = Vec::new();
    room::reserve(&mut ids, side.books.len())?;
    for theirs in &side.books {
        if let Ok(at) = here.binary_search_by_key(&theirs.key, |(k, _)| *k)
            && let Some((_, id)) = here.get(at)
        {
            ids.push((theirs.id, *id));
        }
    }
    ids.sort_unstable();
    drop(here);
    let mut file = Appender::new(storage, path)?;
    let db = Database::open(storage, &side.path).map_err(Failure::Income)?;
    koreader::stats(&db, |s| {
        let Ok(at) = ids.binary_search_by_key(&s.book, |(theirs, _)| *theirs) else {
            return;
        };
        let clamp = |v: i64| v.clamp(0, i64::from(u32::MAX)) as u32;
        file.push(&stat_record([
            u32::from(ids.get(at).map_or(0, |(_, id)| *id)),
            clamp(s.page),
            clamp(s.start),
            clamp(s.duration),
            clamp(s.pages),
        ]));
    })
    .map_err(|e| match e {
        // Writing the working file was not the side's fault.
        Error::Storage(e) => Failure::from(e),
        e => Failure::Income(e),
    })?;
    file.finish()?;
    Ok(extsort::sort(storage, path, path, STAT, runs)?)
}

/// The three files side by side by (book, page, start): with
/// `deletions`, what was deleted on the other side left out; the longer
/// of a stay on both sides kept; written by start. The books `gone`
/// (deleted there, by id) marked where a stay of them is kept. How many
/// stays there are.
fn merge_stays(
    storage: &dyn Storage,
    files: &Files,
    deletions: bool,
    gone: &mut [(u16, bool)],
) -> Result<usize, Failure> {
    let mut merged = Appender::new(storage, &files.merged)?;
    let mut sources = [
        Source::new(storage, &files.local)?,
        Source::new(storage, &files.income)?,
        Source::new(storage, &files.cached)?,
    ];
    // Whether `b` was read longer than `a`.
    let longer = |a: &Record, b: &Record| fields(b)[3] > fields(a)[3];
    let mut stays = 0;
    while let Some(next) = sources
        .iter()
        .filter_map(|s| s.peek().map(|r| key(r)))
        .min()
    {
        // Each side's record for this key, if any (duplicates within a
        // side are merged too).
        let mut found: [Option<Record>; 3] = [None, None, None];
        for (n, source) in sources.iter_mut().enumerate() {
            while source.peek().is_some_and(|r| key(r) == next) {
                let Some(r) = source.next() else {
                    break;
                };
                if let Some(slot) = found.get_mut(n)
                    && slot.as_ref().is_none_or(|f| longer(f, &r))
                {
                    *slot = Some(r);
                }
            }
        }
        let [here, there, before] = found;
        let deleted_there = deletions && before.is_some() && there.is_none();
        if deleted_there {
            continue;
        }
        let record = match (here, there) {
            (Some(a), Some(b)) if longer(&a, &b) => b,
            (Some(a), _) => a,
            (None, Some(b)) => b,
            (None, None) => continue,
        };
        // In time order next: start first.
        let [book, page, start, duration, pages] = fields(&record).map(|v| v as u32);
        if let Ok(at) = gone.binary_search_by_key(&(book as u16), |g| g.0)
            && let Some(entry) = gone.get_mut(at)
        {
            entry.1 = true;
        }
        merged.push(&stat_record([start, book, page, duration, pages]));
        stays += 1;
    }
    merged.finish()?;
    for source in &mut sources {
        // A read error ends a source early: better no merge than a partial.
        if let Some(e) = source.records.failed.take() {
            return Err(e.into());
        }
    }
    extsort::sort(storage, &files.merged, &files.merged, STAT, &files.runs)?;
    Ok(stays)
}

/// Back to (book, page, start, …) in time order, for the new log and,
/// with `upload`, the upload's rows (those of the books); each book's
/// totals: the seconds read, and the pages read as bits.
fn to_time_order(
    storage: &dyn Storage,
    books: &[Book],
    files: &Files,
    upload: bool,
) -> Result<Vec<(u64, Vec<u8>)>, Failure> {
    let mut time = Appender::new(storage, &files.time)?;
    let mut rows = if upload {
        Some(Appender::new(storage, &files.upload)?)
    } else {
        None
    };
    let mut per_book: Vec<(u64, Vec<u8>)> = Vec::new();
    room::reserve(&mut per_book, books.len())?;
    per_book.resize_with(books.len(), || (0, Vec::new()));
    extsort::each(storage, &files.merged, STAT, |r| {
        let [start, book, page, duration, pages] = fields(r).map(|v| v as u32);
        let record = stat_record([book, page, start, duration, pages]);
        time.push(&record);
        if let Ok(at) = books.binary_search_by_key(&(book as u16), |b| b.id)
            && let Some((total, book)) = per_book.get_mut(at).zip(books.get(at))
        {
            if let Some(rows) = &mut rows {
                rows.push(&record);
            }
            total.0 += u64::from(duration);
            let now = if book.pages > 0 { book.pages } else { pages };
            totals::mark_read(&mut total.1, page, pages, now)?;
        }
        Ok(())
    })?;
    time.finish()?;
    if let Some(rows) = rows {
        rows.finish()?;
    }
    Ok(per_book)
}

/// The merged statistics as a KOReader database at `out`, the books'
/// notes and highlights as the other side counted them.
fn write_upload(
    storage: &dyn Storage,
    out: &str,
    books: Vec<Book>,
    per_book: &[(u64, Vec<u8>)],
    income: Option<&Side>,
    files: &Files,
) -> Result<(), Error> {
    let mut ko_books: Vec<KoBook> = Vec::new();
    room::reserve(&mut ko_books, books.len())?;
    for (b, (seconds, read)) in books.into_iter().zip(per_book.iter()) {
        let theirs = income.and_then(|s| s.find(Key::of_book(&b)));
        ko_books.push(KoBook {
            id: i64::from(b.id),
            notes: theirs.map_or(0, |t| t.notes),
            last_open: b.last_open,
            highlights: theirs.map_or(0, |t| t.highlights),
            pages: i64::from(b.pages),
            series: na(b.series)?,
            language: na(b.language)?,
            total_read_time: *seconds as i64,
            total_read_pages: read.iter().map(|x| x.count_ones()).sum::<u32>() as i64,
            title: b.title,
            authors: b.authors,
            md5: b.md5,
        });
    }
    koreader::write(storage, out, &ko_books, &files.upload, &files.write)
}

/// Records added to a file a few kilobytes at a time; the first error
/// kept for the end.
struct Appender<'s> {
    storage: &'s dyn Storage,
    path: &'s str,
    buffer: Vec<u8>,
    failed: Option<StorageError>,
}

impl<'s> Appender<'s> {
    /// The file at `path`, emptied.
    fn new(storage: &'s dyn Storage, path: &'s str) -> Result<Self, StorageError> {
        storage.write(path, &[])?;
        // Room for the limit and one more record: pushing never grows it.
        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(4096 + extsort::MAX_RECORD)
            .map_err(|_| StorageError::TooLarge)?;
        Ok(Self {
            storage,
            path,
            buffer,
            failed: None,
        })
    }

    fn push(&mut self, record: &[u8]) {
        self.buffer.extend_from_slice(record);
        if self.buffer.len() >= 4096 {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if let Err(e) = self.storage.append(self.path, &self.buffer) {
            self.failed.get_or_insert(e);
        }
        self.buffer.clear();
    }

    fn finish(mut self) -> Result<(), StorageError> {
        self.flush();
        match self.failed {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

/// KOReader writes `N/A` for what a book does not say.
fn na(text: String) -> Result<String, StorageError> {
    if text.is_empty() {
        room::string(&["N/A"])
    } else {
        Ok(text)
    }
}

/// A sorted file of stays, read ahead by one.
struct Source<'s> {
    records: extsort::Records<'s>,
    head: Option<Record>,
}

impl<'s> Source<'s> {
    fn new(storage: &'s dyn Storage, path: &str) -> Result<Self, StorageError> {
        let mut records = extsort::Records::new(storage, path, STAT)?;
        let head = records.next();
        Ok(Self { records, head })
    }

    fn peek(&self) -> Option<&Record> {
        self.head.as_ref()
    }

    fn next(&mut self) -> Option<Record> {
        let head = self.head.take();
        self.head = self.records.next();
        head
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod alloc_tests;

#[cfg(test)]
mod props_tests;
