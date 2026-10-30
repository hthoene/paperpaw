//! The bookmarks and highlights of books, shared by plugins: the reader
//! makes and shows them, others export or sync them (e.g. with KOReader).
//! Whoever changes those of a book emits
//! [`ANNOTATIONS_CHANGED`](events::ANNOTATIONS_CHANGED)
//! (see [`changed`]), so readers show them anew and syncs pass them on.
//!
//! A book's annotations are kept in `.paperpaw/annotations/<hash>.txt`
//! (the hash of the book's path), one per line, sorted by position: kind
//! (`b` or `h`), start and end (section and offset, so they outlast font
//! sizes and layouts), the reading progress, when it was made and last
//! changed (seconds since 1970, 0 if not known), a snippet of the text
//! and the note on it, separated by tabs.
//!
//! Offsets are those of one conversion of the book: with its own styles
//! or without, say, the same text is at other offsets. So from version 2
//! on (first line `# format 2`) each line also says, after the times,
//! which conversion its offsets are in (16 hex digits, see
//! [`relocate`]) and its start and end as XPointers, which fit every
//! conversion (empty where there are none; the end is empty where it is
//! the start). The conversion is empty for one added since, and `?` for
//! one kept before conversions were, whose conversion is not known. A
//! file with only those of before stays in version 1.
//!
//! Which book they are of, by its content ([`Fingerprint`]), is said in
//! a line `# book <size> <md5>` after the version's (see [`stamp`]): a
//! line PaperPaw of before reads as none of its own and keeps as it
//! is, so the version stays.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::ops::ControlFlow;

use crate::builtin::events;
use crate::data::Error;
use crate::document::{Document, Locator};
use crate::library::Fingerprint;
use crate::{Command, Storage, StorageError, files};

/// Longest snippet kept with an annotation, in bytes, with the `…` that
/// ends a cut one.
const MAX_TEXT: usize = 240;
/// Longest note kept, in bytes: a line of the file stays within what
/// [`files::for_each_line`] reads whole.
const MAX_NOTE: usize = 3 * 1024;
/// Longest annotations file changed.
const MAX_FILE: u64 = 1 << 20;
/// The version of the file with conversions and XPointers.
const VERSION: u32 = 2;
/// How the line saying which book the file is of begins.
const BOOK: &str = "# book ";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Kind {
    Bookmark,
    Highlight,
}

/// Where an annotation is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Mark {
    pub kind: Kind,
    /// For a highlight, its first character; for a bookmark, where the
    /// page begins.
    pub start: Locator,
    /// Just after the highlight's last character.
    pub end: Locator,
    /// It has a note.
    pub noted: bool,
}

impl Mark {
    pub const fn new(kind: Kind, start: Locator, end: Locator, noted: bool) -> Mark {
        Mark {
            kind,
            start,
            end,
            noted,
        }
    }

    /// Whether the mark covers part of `start..end`.
    pub fn overlaps(&self, start: Locator, end: Locator) -> bool {
        self.start < end && start < self.end
    }
}

/// An annotation with what is shown of it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Annotation {
    pub mark: Mark,
    /// Reading progress at the start, in percent.
    pub percent: u8,
    /// When it was made and last changed, in seconds since 1970 (UTC); 0
    /// if not known.
    pub created: i64,
    pub updated: i64,
    /// A snippet of the text (see [`snippet`]).
    pub text: String,
    /// What the reader noted on it, if anything (one line).
    pub note: String,
}

impl Annotation {
    pub fn new(
        mark: Mark,
        percent: u8,
        created: i64,
        updated: i64,
        text: impl Into<String>,
        note: impl Into<String>,
    ) -> Annotation {
        Annotation {
            mark,
            percent,
            created,
            updated,
            text: text.into(),
            note: note.into(),
        }
    }
}

/// Which conversion of the book an annotation's offsets are in (`None`
/// for one kept before conversions were: not known), and its start and
/// end as XPointers (either empty where there is none).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Anchor {
    conversion: Option<u64>,
    start: String,
    end: String,
}

/// What an annotation is told by: its kind and place.
type Key = (Kind, Locator, Locator);

fn key(mark: &Mark) -> Key {
    (mark.kind, mark.start, mark.end)
}

/// The file with the annotations of the book at `book`.
pub fn file(book: &str) -> String {
    format!(
        "{}/annotations/{}.txt",
        crate::data::DIR,
        crate::library::key(book)
    )
}

/// Every annotation of the book at `book`, by position, for showing them;
/// none where they cannot be read. Read them with [`load_for_update`] to
/// change them.
pub fn load(storage: &dyn Storage, book: &str) -> Vec<Annotation> {
    each(storage, book, parse).unwrap_or_default()
}

/// Every annotation of the book at `book`, by position, to change them
/// and [`save`] them: an error where the file is there but cannot be read
/// (or is larger than a megabyte), so it is not written over.
pub fn load_for_update(storage: &dyn Storage, book: &str) -> Result<Vec<Annotation>, StorageError> {
    match storage.size(&file(book)) {
        Err(StorageError::NotFound) => return Ok(Vec::new()),
        Err(e) => return Err(e),
        Ok(size) if size > MAX_FILE => return Err(StorageError::TooLarge),
        Ok(_) => {}
    }
    each(storage, book, parse)
}

/// Only where the annotations are, for drawing them: read line by line,
/// without their texts.
pub fn marks(storage: &dyn Storage, book: &str) -> Vec<Mark> {
    each(storage, book, |line, version| {
        fields(line, version).map(|(mark, _)| mark)
    })
    .unwrap_or_default()
}

/// What `parse` makes of each line of the book's file (with the file's
/// version), as it streams; a missing file has none. A file a newer
/// PaperPaw marked with a later version
/// ([`data::Format`](crate::data::Format)) is
/// [`StorageError::InvalidData`]: not read, not written over.
fn each<T>(
    storage: &dyn Storage,
    book: &str,
    parse: impl Fn(&str, u32) -> Option<T>,
) -> Result<Vec<T>, StorageError> {
    let mut all = Vec::new();
    let mut first = true;
    let mut newer = false;
    let mut version = 1;
    let read = files::for_each_line(storage, &file(book), &mut |line| {
        if core::mem::take(&mut first) && line.starts_with(crate::data::MARK) {
            version = crate::data::Format::version_of(line);
            newer = !(1..=VERSION).contains(&version);
            return if newer {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            };
        }
        all.extend(parse(line, version));
        ControlFlow::Continue(())
    });
    match read {
        _ if newer => Err(StorageError::InvalidData),
        Ok(()) | Err(StorageError::NotFound) => Ok(all),
        Err(e) => Err(e),
    }
}

/// The mark a line of a file of `version` describes, and the line's
/// fields.
fn fields(line: &str, version: u32) -> Option<(Mark, Vec<&str>)> {
    let most = if version >= VERSION { 13 } else { 10 };
    let fields: Vec<&str> = line.splitn(most, '\t').collect();
    let kind = match *fields.first()? {
        "b" => Kind::Bookmark,
        "h" => Kind::Highlight,
        _ => return None,
    };
    let number = |i: usize| fields.get(i)?.parse::<u32>().ok();
    let locator = |i: usize| Some(Locator::new(number(i)?, number(i + 1)?));
    let (start, end) = (locator(1)?, locator(3)?);
    let noted = fields
        .get(text_field(&fields) + 1)
        .is_some_and(|note| !note.is_empty());
    let mark = Mark {
        kind,
        start,
        end,
        noted,
    };
    Some((mark, fields))
}

/// Where the text is among a line's fields: before times were kept, it
/// followed the progress; from version 2 on, the conversion and the
/// XPointers come before it.
fn text_field(fields: &[&str]) -> usize {
    match fields.len() {
        13 => 11,
        10 => 8,
        _ => 6,
    }
}

fn parse(line: &str, version: u32) -> Option<Annotation> {
    parse_anchored(line, version).map(|(a, _)| a)
}

/// The annotation a line describes, and where it was made; `None` for
/// one added since (in the conversion of whoever added it).
fn parse_anchored(line: &str, version: u32) -> Option<(Annotation, Option<Anchor>)> {
    let (mark, fields) = fields(line, version)?;
    let number = |i: usize| fields.get(i)?.parse::<i64>().ok();
    let percent = number(5)?.clamp(0, 100) as u8;
    let rest = text_field(&fields);
    let (created, updated) = if rest >= 8 {
        (number(6)?, number(7)?)
    } else {
        (0, 0)
    };
    let field = |i: usize| fields.get(i).copied().unwrap_or_default().to_string();
    // Kept before conversions were (a line of version 1, or marked so):
    // made in a conversion not known.
    let anchor = match fields.get(8) {
        Some(&"") if rest == 11 => None,
        Some(conversion) if rest == 11 => Some(Anchor {
            conversion: u64::from_str_radix(conversion, 16).ok(),
            start: field(9),
            end: field(10),
        }),
        _ => Some(Anchor {
            conversion: None,
            start: String::new(),
            end: String::new(),
        }),
    };
    let annotation = Annotation {
        mark,
        percent,
        created,
        updated,
        text: field(rest),
        note: field(rest + 1),
    };
    Some((annotation, anchor))
}

/// A line of the file as [`save`] keeps it: one it does not read, or
/// where an annotation was made.
enum Kept {
    Unread(String),
    Anchor(Key, Anchor),
}

/// Keeps `annotations` as those of the book at `book`, sorted by
/// position (a note cut at 3 KB); an error where they could not be
/// written. Lines of the file this version does not read (a kind a newer
/// PaperPaw added, or damaged) stay as they are; where the file cannot be
/// read for them, nothing is written.
///
/// Where each was made stays known (see [`relocate`]) for those at the
/// same place as before.
pub fn save(storage: &dyn Storage, book: &str, annotations: &[Annotation]) -> Result<(), Error> {
    let kept = |line: &str, version| match parse_anchored(line, version) {
        Some((a, Some(anchor))) => Some(Kept::Anchor(key(&a.mark), anchor)),
        Some((_, None)) => None,
        None => (!line.trim().is_empty()).then(|| Kept::Unread(line.to_string())),
    };
    let kept = each(storage, book, kept)?;
    let (mut anchors, mut unread) = (Vec::new(), Vec::new());
    for line in kept {
        match line {
            Kept::Unread(line) => unread.push(line),
            Kept::Anchor(key, anchor) => anchors.push((key, anchor)),
        }
    }
    let all = annotations
        .iter()
        .map(|a| {
            let found = anchors.iter().position(|(k, _)| *k == key(&a.mark));
            (a, found.map(|i| anchors.swap_remove(i).1))
        })
        .collect();
    write(storage, book, all, &unread, None)
}

/// Writes `all` (each with where it was made, if known) and the lines
/// `unread` as the annotations of the book at `book`, which the file
/// says is `fingerprint` (else as the line of it among `unread` says);
/// an error where they could not be written.
fn write(
    storage: &dyn Storage,
    book: &str,
    mut all: Vec<(&Annotation, Option<Anchor>)>,
    unread: &[String],
    fingerprint: Option<&Fingerprint>,
) -> Result<(), Error> {
    all.sort_by_cached_key(|(a, _)| (a.mark.start, a.mark.end));
    // After the version, wherever it was (PaperPaw of before put it
    // last).
    let (books, unread): (Vec<&String>, Vec<&String>) =
        unread.iter().partition(|line| line.starts_with(BOOK));
    let fingerprint = fingerprint
        .map(|f| format!("{BOOK}{}", f.text()))
        .or_else(|| books.first().map(|line| line.to_string()));
    // Only those of before: version 1, as they were.
    let anchored = all
        .iter()
        .any(|(_, anchor)| anchor.as_ref().is_none_or(|x| x.conversion.is_some()));
    let mut out = String::new();
    if anchored {
        out.push_str(&format!("{}{VERSION}\n", crate::data::MARK));
    }
    let none = all.is_empty() && unread.is_empty();
    if let Some(line) = fingerprint.filter(|_| !none) {
        out.push_str(&line);
        out.push('\n');
    }
    for (a, anchor) in all {
        let kind = match a.mark.kind {
            Kind::Bookmark => 'b',
            Kind::Highlight => 'h',
        };
        out.push_str(&format!(
            "{kind}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t",
            a.mark.start.section,
            a.mark.start.offset,
            a.mark.end.section,
            a.mark.end.offset,
            a.percent,
            a.created,
            a.updated,
        ));
        match anchor {
            Some(x) if anchored => match x.conversion {
                Some(conversion) => {
                    out.push_str(&format!("{conversion:016x}\t{}\t{}\t", x.start, x.end));
                }
                None => out.push_str(&format!("?\t{}\t{}\t", x.start, x.end)),
            },
            None => out.push_str("\t\t\t"),
            Some(_) => {}
        }
        out.push_str(&format!(
            "{}\t{}\n",
            one_line(&a.text),
            stored_note(&a.note)
        ));
    }
    for line in unread {
        out.push_str(line);
        out.push('\n');
    }
    let path = file(book);
    if none {
        storage.remove(&path)?;
    } else {
        storage.write(&path, out.as_bytes())?;
    }
    Ok(())
}

/// `text` on one line, at most 240 bytes (with the `…` that ends a cut
/// one).
pub fn snippet(text: &str) -> String {
    let out: String = text
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    cut(out, MAX_TEXT)
}

/// `text` in at most `max` bytes, ending in `…` where it is cut.
fn cut(mut text: String, max: usize) -> String {
    if text.len() > max {
        let mut end = max - '…'.len_utf8();
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push('…');
    }
    text
}

/// `note` as it is kept: on one line, and cut where it is longer than
/// a note may be (ending in `…`). For those who compare a note from
/// elsewhere with one kept here, e.g. a sync.
pub fn stored_note(note: &str) -> String {
    cut(one_line(note), MAX_NOTE)
}

/// `text` on one line: tabs and line breaks become spaces.
pub fn one_line(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c == '\t' || c == '\n' || c == '\r' {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Puts `note` on the highlight that starts at `start` (an empty one
/// removes it), changed at `now` (seconds since 1970, 0 if not known);
/// `Ok(false)` if there is no such highlight, an error where the
/// annotations could not be read or written.
pub fn set_note(
    storage: &dyn Storage,
    book: &str,
    start: Locator,
    note: &str,
    now: i64,
) -> Result<bool, Error> {
    let mut all = load_for_update(storage, book)?;
    let Some(a) = all
        .iter_mut()
        .find(|a| a.mark.kind == Kind::Highlight && a.mark.start == start)
    else {
        return Ok(false);
    };
    a.note = one_line(note);
    a.mark.noted = !a.note.is_empty();
    a.updated = now;
    save(storage, book, &all).map(|()| true)
}

/// The note on the highlight that starts at `start`.
pub fn note_of(storage: &dyn Storage, book: &str, start: Locator) -> String {
    load(storage, book)
        .into_iter()
        .find(|a| a.mark.kind == Kind::Highlight && a.mark.start == start)
        .map(|a| a.note)
        .unwrap_or_default()
}

/// Adds an annotation; an error where the annotations could not be read
/// or written (those there are kept as they were).
pub fn add(storage: &dyn Storage, book: &str, annotation: Annotation) -> Result<(), Error> {
    let mut all = load_for_update(storage, book)?;
    all.push(annotation);
    save(storage, book, &all)
}

/// Removes the annotations `remove` picks: `Ok(true)` if there were any
/// and they are gone, `Ok(false)` if there were none, an error where the
/// annotations could not be read or written.
pub fn remove(
    storage: &dyn Storage,
    book: &str,
    remove: impl Fn(&Mark) -> bool,
) -> Result<bool, Error> {
    let mut all = load_for_update(storage, book)?;
    let before = all.len();
    all.retain(|a| !remove(&a.mark));
    if all.len() == before {
        return Ok(false);
    }
    save(storage, book, &all).map(|()| true)
}

/// Finds the annotations of the book at `book` in conversion
/// `conversion` of it (as `reading::conversion` names it), opened as
/// `doc`: those made in another conversion by their XPointers, so that
/// they cover the same text. Those added since are taken as of this one,
/// and their XPointers kept. Those kept before conversions were may be
/// made in any: they are never taken as of one by a guess, and stay where
/// they are, as before. One that cannot be found stays as it is, never
/// removed. `Ok(true)` where they changed and are saved; an
/// error where the file cannot be read or written, nothing changed then.
///
/// A reader calls it when it opens a book and after annotations were
/// added, before it shows them; so does a plugin that adds annotations
/// at the places of a conversion it opened. Where all are in this
/// conversion, the file is only read through, line by line.
pub fn relocate(
    storage: &dyn Storage,
    book: &str,
    conversion: u64,
    doc: &dyn Document,
) -> Result<bool, StorageError> {
    // Without XPointers nothing can be found again: the offsets stay.
    if doc.xpointer(storage, Locator::START).is_none() {
        return Ok(false);
    }
    match storage.size(&file(book)) {
        Err(StorageError::NotFound) => return Ok(false),
        Err(e) => return Err(e),
        Ok(size) if size > MAX_FILE => return Err(StorageError::TooLarge),
        Ok(_) => {}
    }
    // In another conversion, with XPointers to find it by.
    let elsewhere =
        |x: &Anchor| x.conversion.is_some_and(|c| c != conversion) && !x.start.is_empty();
    let due = |line: &str, version| {
        let (_, anchor) = parse_anchored(line, version)?;
        anchor.as_ref().is_none_or(elsewhere).then_some(())
    };
    if each(storage, book, due)?.is_empty() {
        return Ok(false);
    }
    let read = |line: &str, version| match parse_anchored(line, version) {
        Some(read) => Some(Ok(read)),
        None => (!line.trim().is_empty()).then(|| Err(line.to_string())),
    };
    let (mut all, mut unread) = (Vec::new(), Vec::new());
    for line in each(storage, book, read)? {
        match line {
            Ok(read) => all.push(read),
            Err(line) => unread.push(line),
        }
    }
    let mut changed = false;
    for (a, anchor) in &mut all {
        match anchor {
            Some(x) if !elsewhere(x) => {}
            Some(x) => {
                if let Some((start, end)) = found(storage, doc, x) {
                    (a.mark.start, a.mark.end) = (start, end);
                    x.conversion = Some(conversion);
                    changed = true;
                }
            }
            None => {
                *anchor = Some(anchor_of(storage, doc, conversion, &a.mark));
                changed = true;
            }
        }
    }
    if !changed {
        return Ok(false);
    }
    let all = all.iter_mut().map(|(a, x)| (&*a, x.take())).collect();
    if write(storage, book, all, &unread, None).is_ok() {
        Ok(true)
    } else {
        Err(StorageError::Io)
    }
}

/// Where the annotation made at `x` is in `doc`, by its XPointers: its
/// start and end.
fn found(storage: &dyn Storage, doc: &dyn Document, x: &Anchor) -> Option<(Locator, Locator)> {
    let start = doc.locate_xpointer(storage, &x.start)?;
    if x.end.is_empty() {
        return Some((start, start));
    }
    let end = doc.locate_xpointer(storage, &x.end)?;
    (start < end).then_some((start, end))
}

/// Where `mark` is made in conversion `conversion`, opened as `doc`:
/// without XPointers where it has none.
fn anchor_of(storage: &dyn Storage, doc: &dyn Document, conversion: u64, mark: &Mark) -> Anchor {
    let xpointer = |at| {
        doc.xpointer(storage, at)
            .filter(|x| !x.is_empty() && !x.contains(['\t', '\n', '\r']))
    };
    let end = if mark.end == mark.start {
        Some(String::new())
    } else {
        xpointer(mark.end)
    };
    let (start, end) = xpointer(mark.start).zip(end).unwrap_or_default();
    Anchor {
        conversion: Some(conversion),
        start,
        end,
    }
}

/// Which book the annotations of the book at `book` are of, as their
/// file says: `None` where it does not say (or there are none).
pub fn book(storage: &dyn Storage, book: &str) -> Option<Fingerprint> {
    let mut found = None;
    // The line is among the first; PaperPaw of before put it last.
    let read = files::for_each_line(storage, &file(book), &mut |line| {
        found = line.strip_prefix(BOOK).and_then(Fingerprint::parse);
        if found.is_some() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    });
    read.ok().and(found)
}

/// Says in the annotations of the book at `book` which book they are
/// of, `fingerprint`, where they say another or none: `Ok(true)` where
/// it was written. A book without annotations gets no file. A reader
/// calls it when it opens a book and after annotations were added, with
/// the fingerprint it took when it opened the book; where it is said
/// already, the file is only read up to it.
pub fn stamp(
    storage: &dyn Storage,
    book: &str,
    fingerprint: &Fingerprint,
) -> Result<bool, StorageError> {
    match storage.size(&file(book)) {
        Err(StorageError::NotFound) => return Ok(false),
        Err(e) => return Err(e),
        Ok(size) if size > MAX_FILE => return Err(StorageError::TooLarge),
        Ok(_) => {}
    }
    if self::book(storage, book).as_ref() == Some(fingerprint) {
        return Ok(false);
    }
    let read = |line: &str, version| match parse_anchored(line, version) {
        Some(read) => Some(Ok(read)),
        None => (!line.trim().is_empty()).then(|| Err(line.to_string())),
    };
    let (mut all, mut unread) = (Vec::new(), Vec::new());
    for line in each(storage, book, read)? {
        match line {
            Ok(read) => all.push(read),
            Err(line) => unread.push(line),
        }
    }
    if all.is_empty() && unread.iter().all(|line| line.starts_with(BOOK)) {
        return Ok(false);
    }
    let all = all.iter_mut().map(|(a, x)| (&*a, x.take())).collect();
    if write(storage, book, all, &unread, Some(fingerprint)).is_ok() {
        Ok(true)
    } else {
        Err(StorageError::Io)
    }
}

/// The event telling that the annotations of the book at `book` changed.
pub fn changed(book: &str) -> Command {
    events::ANNOTATIONS_CHANGED.emit(events::AnnotationsChanged {
        path: book.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;
    use crate::memory_storage::Unreadable;

    fn highlight(start: u32, end: u32, text: &str) -> Annotation {
        Annotation {
            mark: Mark {
                kind: Kind::Highlight,
                start: Locator::new(1, start),
                end: Locator::new(1, end),
                noted: false,
            },
            percent: 3,
            created: 1_700_000_000,
            updated: 0,
            text: String::from(text),
            note: String::new(),
        }
    }

    #[test]
    fn kept_by_position_with_notes_and_times() {
        let storage = MemoryStorage::new();
        let book = "books/a.epub";
        add(&storage, book, highlight(50, 60, "later")).unwrap();
        add(&storage, book, highlight(10, 20, "first\tline")).unwrap();
        assert!(
            set_note(
                &storage,
                book,
                Locator::new(1, 50),
                "a\nnote",
                1_700_000_100
            )
            .unwrap()
        );
        let all = load(&storage, book);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].text, "first line");
        assert_eq!(all[1].note, "a note");
        assert!(all[1].mark.noted);
        assert_eq!(
            (all[1].created, all[1].updated),
            (1_700_000_000, 1_700_000_100)
        );
        assert_eq!(note_of(&storage, book, Locator::new(1, 50)), "a note");
        let marks = marks(&storage, book);
        assert_eq!(marks, [all[0].mark, all[1].mark]);
        // Gone when the last is removed.
        assert!(remove(&storage, book, |_| true).unwrap());
        assert!(!storage.exists(&file(book)));
    }

    #[test]
    fn unreadable_annotations_are_not_written_over() {
        let book = "books/a.epub";
        let kept = MemoryStorage::new();
        assert!(add(&kept, book, highlight(10, 20, "kept")).is_ok());
        let before = kept.get(&file(book)).unwrap();
        let storage = Unreadable(kept);
        assert_eq!(load_for_update(&storage, book), Err(StorageError::Io));
        assert!(add(&storage, book, highlight(30, 40, "new")).is_err());
        assert!(remove(&storage, book, |_| true).is_err());
        assert!(set_note(&storage, book, Locator::new(1, 10), "x", 0).is_err());
        assert_eq!(storage.0.get(&file(book)), Some(before));
        // Written by a newer PaperPaw: left as it is.
        let newer = MemoryStorage::new().with(&file(book), "# format 3\nh\t1\t2\t1\t3\t0\tx\t");
        assert!(add(&newer, book, highlight(1, 2, "x")).is_err());
        assert!(load(&newer, book).is_empty());
        assert_eq!(
            load_for_update(&newer, book),
            Err(StorageError::InvalidData)
        );
        // Too large to be changed whole.
        let big = MemoryStorage::new().with(&file(book), alloc::vec![b'\n'; (1 << 20) + 1]);
        assert!(add(&big, book, highlight(1, 2, "x")).is_err());
        assert_eq!(big.size(&file(book)), Ok((1 << 20) + 1));
    }

    /// Which book the annotations are of is said after the version, kept
    /// as they change, said anew where it is another, and gone with the
    /// last of them; a book without annotations gets no file for it.
    #[test]
    fn the_annotations_say_which_book_they_are_of() {
        let storage = MemoryStorage::new();
        let book = "books/a.epub";
        let one = Fingerprint::parse("482133 0b6e8f2a6c1d4e5f8a9b0c1d2e3f4a5b").unwrap();
        let other = Fingerprint::parse("482140 1b6e8f2a6c1d4e5f8a9b0c1d2e3f4a5b").unwrap();
        assert_eq!(stamp(&storage, book, &one), Ok(false));
        assert!(!storage.exists(&file(book)));
        add(&storage, book, highlight(10, 20, "first")).unwrap();
        assert_eq!(stamp(&storage, book, &one), Ok(true));
        assert_eq!(stamp(&storage, book, &one), Ok(false));
        assert_eq!(self::book(&storage, book), Some(one.clone()));
        add(&storage, book, highlight(30, 40, "second")).unwrap();
        let text = String::from_utf8(storage.get(&file(book)).unwrap()).unwrap();
        assert!(
            text.starts_with("# format 2\n# book 482133 0b6e8f2a6c1d4e5f8a9b0c1d2e3f4a5b\n"),
            "{text}"
        );
        assert_eq!(load(&storage, book).len(), 2);
        assert_eq!(stamp(&storage, book, &other), Ok(true));
        assert_eq!(self::book(&storage, book), Some(other));
        assert!(remove(&storage, book, |_| true).unwrap());
        assert!(!storage.exists(&file(book)));
    }

    /// PaperPaw of before reads the line as none of its own and keeps it
    /// last: it is read there, and put back after the version.
    #[test]
    fn the_book_s_line_moved_last_is_read_and_put_back() {
        let book = "books/a.epub";
        let moved = "# format 2\n\
                     h\t1\t10\t1\t20\t3\t0\t0\t\t\t\tsome text\t\n\
                     # book 482133 0b6e8f2a6c1d4e5f8a9b0c1d2e3f4a5b\n";
        let storage = MemoryStorage::new().with(&file(book), moved);
        let one = Fingerprint::parse("482133 0b6e8f2a6c1d4e5f8a9b0c1d2e3f4a5b");
        assert_eq!(self::book(&storage, book), one);
        assert_eq!(load(&storage, book).len(), 1);
        add(&storage, book, highlight(30, 40, "second")).unwrap();
        let text = String::from_utf8(storage.get(&file(book)).unwrap()).unwrap();
        assert_eq!(text.matches("# book ").count(), 1, "{text}");
        assert!(
            text.lines().nth(1).unwrap().starts_with("# book "),
            "{text}"
        );
    }

    #[test]
    fn texts_and_notes_are_cut() {
        let long = "\u{e4}".repeat(200);
        let text = snippet(&long);
        assert!(
            text.len() <= MAX_TEXT && text.ends_with('…'),
            "{}",
            text.len()
        );
        let storage = MemoryStorage::new();
        let mut a = highlight(1, 2, &text);
        a.note = "n".repeat(10_000);
        assert!(add(&storage, "a.epub", a).is_ok());
        let read = load(&storage, "a.epub");
        assert_eq!(read[0].text, text);
        assert!(read[0].note.len() <= MAX_NOTE && read[0].note.ends_with('…'));
        assert!(storage.size(&file("a.epub")).unwrap() < files::MAX_LINE as u64);
    }

    #[test]
    fn lines_without_times_are_read() {
        let a = parse("h\t1\t10\t1\t20\t5\tsome text\tits note", 1).unwrap();
        assert_eq!((a.created, a.updated), (0, 0));
        assert_eq!(
            (a.text.as_str(), a.note.as_str()),
            ("some text", "its note")
        );
        assert_eq!(a.mark.end, Locator::new(1, 20));
    }
}
