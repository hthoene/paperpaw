//! KOReader: the bookmarks and highlights of a book go back and forth
//! with KOReader, for books read in both on one device (or one library
//! folder synced between devices).
//!
//! KOReader keeps what it knows of a book beside it, in
//! `Book.sdr/metadata.epub.lua`. When PaperPaw opens a book that has such
//! a file, and when its bookmarks or highlights change, this plugin reads
//! it, takes over what is new there, and leaves what is new here in
//! `Book.sdr/Book.epub.annotations.lua`: KOReader takes that over the
//! next time it opens the book, and removes the file. Positions are
//! KOReader's XPointers
//! ([`Document::xpointer`](paperpaw_sdk::document::Document)), so EPUBs
//! only.
//!
//! What is removed on one side goes on the other too: the plugin keeps
//! which annotations both had at the last exchange, and their notes then.
//! A note changed on one side only goes to the other; one changed on both
//! keeps the later change.
//!
//! A file of either side that is there but cannot be read or understood
//! stops the exchange: nothing changes on either side until it can be.
//! So does a missing one of KOReader (as while a sync has brought the
//! folder but not yet the file in it), and one side without any of what
//! both had (book data moved halfway, a sync not complete), unless that
//! side removed them all: here the reader said so, and KOReader's file
//! keeps an empty list where it was seen with annotations at the last
//! exchange. KOReader writes an empty list for every book it opened, so
//! one in a file never seen with any (a fresh one from another device)
//! removes nothing: what both had is offered to KOReader again. Where
//! KOReader's file, seen with annotations before, has none any more
//! (removed there, or a fresh file synced in from another device), the
//! reader is asked in the book whether to remove them here too; nothing
//! changes before, *Keep* (where the focus starts) offers them to
//! KOReader again. Before everything is removed here, the annotations
//! are kept aside (`<key>.txt.removed` beside them). What could
//! not be written for KOReader is not taken as known, so it goes again
//! next time. Annotations go to KOReader with the time they were made:
//! while neither they nor the clock tell it, they wait here.

// What goes wrong with the user's data is told, never dropped unseen:
// a result left unread says why (see CONTRIBUTING.md).
#![deny(clippy::let_underscore_must_use)]
#![no_std]

extern crate alloc;

pub mod lua;

use alloc::borrow::Cow;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::annotations::{self, Annotation, Kind, Mark};
use paperpaw_sdk::builtin::events;
use paperpaw_sdk::document::{Document, Locator};
use paperpaw_sdk::time::{self, LocalTime};
use paperpaw_sdk::{
    Env, Id, Manifest, MemoryClass, Plugin, RegisterError, Registrar, Storage, StorageError,
};

use lua::{Table, Value};

pub const PLUGIN: Id = Id::new_static("koreader");
/// Who wrote the annotations file, for KOReader (which takes over only
/// those of other devices).
const DEVICE: &str = "PaperPaw";
/// Longest KOReader file read.
pub const MAX_FILE: usize = 4 << 20;
/// Longest KOReader file read on devices with a few hundred kilobytes
/// ([`MemoryClass::Small`]): some fifty annotations. The books of larger
/// ones are not exchanged there.
pub const MAX_FILE_SMALL: usize = 32 << 10;
/// Longest file of what is known read.
const MAX_KNOWN: usize = 1 << 20;
/// Most text taken from a book for a highlight.
const MAX_TEXT: usize = 4096;

/// A file that is there but cannot be read or understood (too large, an
/// error of the card, not KOReader's form): the exchange stops without
/// changing anything on either side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unreadable;

/// The longest KOReader file read on a device with `memory`.
pub fn max_file(memory: MemoryClass) -> usize {
    if memory == MemoryClass::Small {
        MAX_FILE_SMALL
    } else {
        MAX_FILE
    }
}

/// The file at `path`, `None` if there is none. At most `limit` bytes,
/// in memory asked for rather than taken, read whole or not at all.
fn load(storage: &dyn Storage, path: &str, limit: usize) -> Result<Option<Vec<u8>>, Unreadable> {
    let size = match storage.size(path) {
        Ok(size) => usize::try_from(size).map_err(|_| Unreadable)?,
        Err(StorageError::NotFound) => return Ok(None),
        Err(_) => return Err(Unreadable),
    };
    if size > limit {
        return Err(Unreadable);
    }
    let mut data = Vec::new();
    data.try_reserve_exact(size).map_err(|_| Unreadable)?;
    data.resize(size, 0);
    let mut filled = 0;
    while filled < size {
        match storage.read_at(path, filled as u64, &mut data[filled..]) {
            Ok(0) | Err(_) => return Err(Unreadable),
            Ok(n) => filled += n,
        }
    }
    Ok(Some(data))
}

/// KOReader's folder for the book at `book`: `Book.sdr` beside it.
pub fn sidecar(book: &str) -> String {
    let stem = book.rsplit_once('.').map_or(book, |(stem, _)| stem);
    format!("{stem}.sdr")
}

/// KOReader's own file in it: `metadata.<suffix>.lua`.
fn metadata_file(book: &str) -> String {
    let suffix = book.rsplit_once('.').map_or("_", |(_, suffix)| suffix);
    format!("{}/metadata.{suffix}.lua", sidecar(book))
}

/// Annotations for KOReader to take over: `<file name>.annotations.lua`.
fn exchange_file(book: &str) -> String {
    format!(
        "{}/{}.annotations.lua",
        sidecar(book),
        paperpaw_sdk::files::name(book)
    )
}

/// Where the annotations both sides had at the last exchange are kept.
/// Named by the book's key, so that the kernel moves and removes it with
/// the book (registered as book data).
fn known_file(book: &str) -> String {
    format!(
        "{}/{}.txt",
        Env::plugin_dir(PLUGIN.as_str()),
        paperpaw_sdk::library::key(book)
    )
}

/// The note of KOReader's annotation `item` as it is kept here: on one
/// line, cut as `annotations::save` cuts it (ending in `…`). Compared so,
/// a long note is not taken for one changed here.
fn note_of(item: &Table) -> String {
    annotations::stored_note(item.str("note").unwrap_or_default())
}

/// What an annotation is known by: its kind and place.
type Key = (Kind, Locator, Locator);

fn key_of(mark: &Mark) -> Key {
    match mark.kind {
        Kind::Bookmark => (Kind::Bookmark, mark.start, mark.start),
        Kind::Highlight => (Kind::Highlight, mark.start, mark.end),
        _ => (mark.kind, mark.start, mark.end),
    }
}

/// An annotation both sides had at the last exchange: where it is, and
/// a fingerprint of the note both had on it then (`None` where their
/// notes differed, or in what an older version kept).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Known {
    key: Key,
    note: Option<u64>,
}

/// A fingerprint of `note` (64-bit FNV-1a): whether it changed since.
fn fingerprint(note: &str) -> u64 {
    note.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The line in what is known telling that every annotation here was
/// removed since the last exchange.
const EMPTIED: &str = "emptied";
/// The start of the line in what is known telling how many annotations
/// KOReader's own file had at the last exchange that read it.
const THEIRS: &str = "theirs ";

/// What is known of the last exchange.
#[derive(Debug, Default)]
struct Last {
    /// What both sides had.
    known: Vec<Known>,
    /// Whether every annotation here was removed since.
    emptied: bool,
    /// How many annotations KOReader's own file had at the last exchange
    /// that read it (not the file for KOReader); 0 if not known.
    seen: u32,
}

/// What both sides had at the last exchange, a line each: kind, start,
/// end and the note's fingerprint (`b 3 120 3 120 5d2c…`). And whether
/// every annotation here was removed since ([`EMPTIED`]), and how many
/// KOReader's own file had ([`THEIRS`]).
fn load_known(storage: &dyn Storage, book: &str) -> Result<Last, Unreadable> {
    let Some(data) = load(storage, &known_file(book), MAX_KNOWN)? else {
        return Ok(Last::default());
    };
    let text = core::str::from_utf8(&data).map_err(|_| Unreadable)?;
    let emptied = text.lines().any(|line| line == EMPTIED);
    let seen = text
        .lines()
        .find_map(|line| line.strip_prefix(THEIRS)?.parse().ok())
        .unwrap_or(0);
    let known = text
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(' ');
            let kind = match fields.next()? {
                "b" => Kind::Bookmark,
                "h" => Kind::Highlight,
                _ => return None,
            };
            let mut number = || fields.next()?.parse::<u32>().ok();
            let start = Locator::new(number()?, number()?);
            let end = Locator::new(number()?, number()?);
            let note = fields
                .next()
                .and_then(|hex| u64::from_str_radix(hex, 16).ok());
            Some(Known {
                key: (kind, start, end),
                note,
            })
        })
        .collect();
    Ok(Last {
        known,
        emptied,
        seen,
    })
}

/// Whether there are no annotations here of the book at `book`.
fn none_here(storage: &dyn Storage, book: &str) -> bool {
    matches!(
        storage.size(&annotations::file(book)),
        Ok(0) | Err(StorageError::NotFound)
    )
}

/// The annotations here of the book at `book` changed (as the reader
/// tells): where none are left of what both had, the user removed them
/// all, which is kept for the next exchange to pass on. None here when a
/// book is opened is rather its data moved halfway.
fn note_emptied(storage: &dyn Storage, book: &str) {
    if !none_here(storage, book) {
        return;
    }
    let Ok(Last {
        known,
        emptied: false,
        ..
    }) = load_known(storage, book)
    else {
        return;
    };
    if known.is_empty() {
        return;
    }
    #[expect(
        clippy::let_underscore_must_use,
        reason = "not kept, the next exchange changes nothing on either side, as before the removal was told"
    )]
    let _ = storage.append(&known_file(book), format!("{EMPTIED}\n").as_bytes());
}

fn save_known(storage: &dyn Storage, book: &str, known: &[Known], seen: u32) {
    let mut out = String::new();
    if seen > 0 {
        out.push_str(&format!("{THEIRS}{seen}\n"));
    }
    for Known {
        key: (kind, start, end),
        note,
    } in known
    {
        let kind = if *kind == Kind::Bookmark { 'b' } else { 'h' };
        out.push_str(&format!(
            "{kind} {} {} {} {}",
            start.section, start.offset, end.section, end.offset
        ));
        if let Some(note) = note {
            out.push_str(&format!(" {note:016x}"));
        }
        out.push('\n');
    }
    #[expect(
        clippy::let_underscore_must_use,
        reason = "kept as it was when this check came in; decide whether the user should hear of a failure here when this is next changed"
    )]
    let _ = storage.write(&known_file(book), out.as_bytes());
}

/// Whether a folder has anything in it.
fn has_entries(storage: &dyn Storage, dir: &str) -> bool {
    let mut any = false;
    #[expect(
        clippy::let_underscore_must_use,
        reason = "a folder that cannot be listed has no entries here"
    )]
    let _ = storage.list(dir, &mut |_| any = true);
    any
}

/// The annotations of KOReader's file at `path`, read if it is at most
/// `limit` bytes, and whether the file has a list of them (also an empty
/// one); `None` if there is no such file, none if it keeps none.
fn read_items(
    storage: &dyn Storage,
    path: &str,
    limit: usize,
) -> Result<Option<(Vec<Table>, bool)>, Unreadable> {
    let Some(data) = load(storage, path, limit)? else {
        return Ok(None);
    };
    let text = core::str::from_utf8(&data).map_err(|_| Unreadable)?;
    let Some(Value::Table(mut root)) = lua::parse(text) else {
        return Err(Unreadable);
    };
    drop(data);
    let at = root
        .fields
        .iter()
        .position(|(key, _)| matches!(key, Value::Str(s) if s == "annotations"));
    let Some(at) = at else {
        // Older KOReaders kept them apart, in a form not read here.
        return if root.get("bookmarks").is_some() || root.get("highlight").is_some() {
            Err(Unreadable)
        } else {
            Ok(Some((Vec::new(), false)))
        };
    };
    let Value::Table(items) = root.fields.swap_remove(at).1 else {
        return Err(Unreadable);
    };
    drop(root);
    // A list of tables, nothing else: what is not understood is not
    // passed on, so it must not be there.
    let mut items = items
        .fields
        .into_iter()
        .map(|field| match field {
            (Value::Number(n), Value::Table(item)) => Some((n.parse::<u64>().ok()?, item)),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(Unreadable)?;
    items.sort_by_key(|(n, _)| *n);
    Ok(Some((
        items.into_iter().map(|(_, item)| item).collect(),
        true,
    )))
}

/// KOReader's side of an exchange.
#[derive(Debug)]
pub struct Theirs {
    /// What KOReader has, or, while it has not taken over the last
    /// exchange, what it will have.
    pub items: Vec<Table>,
    /// Whether the last exchange still waits for KOReader.
    pub pending: bool,
    /// Whether KOReader's file has a list of annotations, also an empty
    /// one: none in it is then everything removed there.
    pub listed: bool,
}

/// KOReader's side for the book at `book`, from files of at most `limit`
/// bytes. `None` while KOReader keeps no file for it, also while a sync
/// has brought its folder but not yet the file in it: then there is
/// nothing to exchange with, and what both had stays known.
pub fn read_theirs(
    storage: &dyn Storage,
    book: &str,
    limit: usize,
) -> Result<Option<Theirs>, Unreadable> {
    let pending = storage.exists(&exchange_file(book));
    let path = if pending {
        exchange_file(book)
    } else {
        metadata_file(book)
    };
    Ok(
        read_items(storage, &path, limit)?.map(|(items, listed)| Theirs {
            items,
            pending,
            listed,
        }),
    )
}

/// The annotations here of the book at `book`. Lines not understood are
/// left out, so a file that is there but gave none is taken as one that
/// cannot be read too.
fn load_ours(storage: &dyn Storage, book: &str) -> Result<Vec<Annotation>, Unreadable> {
    let ours = annotations::load_for_update(storage, book).map_err(|_| Unreadable)?;
    if ours.is_empty() && !none_here(storage, book) {
        Err(Unreadable)
    } else {
        Ok(ours)
    }
}

/// What an exchange works with.
pub struct Exchange<'a> {
    pub storage: &'a dyn Storage,
    /// What to do where KOReader's file has none any more of what both
    /// had.
    pub removal: Removal,
    /// The book, opened.
    pub doc: &'a dyn Document,
    /// Seconds since 1970 (UTC) now, 0 if not known.
    pub now: i64,
    /// Local seconds since 1970 of seconds since 1970 (UTC).
    pub local: &'a dyn Fn(i64) -> i64,
    /// How new highlights look in KOReader: `lighten` or `underscore`.
    pub drawer: &'a str,
}

impl core::fmt::Debug for Exchange<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Exchange")
            .field("now", &self.now)
            .field("drawer", &self.drawer)
            .finish_non_exhaustive()
    }
}

/// KOReader's times: local, `2026-10-16 14:05:00`.
fn datetime(x: &Exchange<'_>, unix: i64) -> String {
    let t = LocalTime::from_seconds((x.local)(unix));
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        t.year, t.month, t.day, t.hour, t.minute, t.second
    )
}

/// Seconds since 1970 (UTC) of one of KOReader's times; 0 if none.
fn unix_of(x: &Exchange<'_>, text: &str) -> i64 {
    let number = |range: core::ops::Range<usize>| text.get(range)?.parse::<i64>().ok();
    let (Some(year), Some(month), Some(day)) = (number(0..4), number(5..7), number(8..10)) else {
        return 0;
    };
    let (hour, minute, second) = (
        number(11..13).unwrap_or(0),
        number(14..16).unwrap_or(0),
        number(17..19).unwrap_or(0),
    );
    // Days since 1970 of the date (proleptic Gregorian).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let local = days * 86_400 + hour * 3600 + minute * 60 + second;
    // The zone's offset then, near enough.
    local - ((x.local)(local) - local)
}

/// The text of a book from `start` to `end`, paragraphs on lines of
/// their own.
fn text_between(doc: &dyn Document, storage: &dyn Storage, start: Locator, end: Locator) -> String {
    let mut out = String::new();
    let mut at = start;
    for _ in 0..64 {
        if at >= end || out.len() >= MAX_TEXT {
            break;
        }
        let Ok(Some(block)) = doc.block_at(storage, at) else {
            if at.section + 1 >= doc.sections() {
                break;
            }
            at = Locator::new(at.section + 1, 0);
            continue;
        };
        let from = block.text_index(at.offset.max(block.start));
        let to = if at.section == end.section && end.offset < block.end {
            block.text_index(end.offset)
        } else {
            block.text.len()
        };
        if let Some(text) = block.text.get(from..to.max(from)) {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(text);
        }
        at = Locator::new(at.section, block.end.max(at.offset + 1));
    }
    out
}

/// The title of the chapter `at` is in.
fn chapter(doc: &dyn Document, storage: &dyn Storage, at: Locator) -> Option<String> {
    let mut title = None;
    for i in 0..doc.toc_len(storage) {
        let entry = doc.toc_entry(storage, i)?;
        if entry.at > at {
            break;
        }
        title = Some(entry.title);
    }
    title
}

/// Where KOReader's annotation `item` is here; `None` where it cannot be
/// placed (it then stays as it is).
fn locate(x: &Exchange<'_>, item: &Table) -> Option<Key> {
    let (doc, storage) = (x.doc, x.storage);
    if item.get("drawer").is_some() {
        let start = doc.locate_xpointer(storage, item.str("pos0")?)?;
        let end = doc.locate_xpointer(storage, item.str("pos1")?)?;
        (start < end).then_some((Kind::Highlight, start, end))
    } else {
        let at = doc.locate_xpointer(storage, item.str("page")?)?;
        Some((Kind::Bookmark, at, at))
    }
}

/// KOReader's annotation `item` as one here, at `key`.
fn annotation_of(x: &Exchange<'_>, item: &Table, (kind, start, end): Key) -> Annotation {
    let (doc, storage) = (x.doc, x.storage);
    let note = note_of(item);
    let text = match kind {
        Kind::Highlight => item
            .str("text")
            .map(String::from)
            .unwrap_or_else(|| text_between(doc, storage, start, end)),
        // Bookmarks show the beginning of their page.
        Kind::Bookmark => {
            let mut text = text_between(doc, storage, start, Locator::new(start.section, u32::MAX));
            text.truncate(text.char_indices().nth(120).map_or(text.len(), |(i, _)| i));
            text
        }
        _ => String::new(),
    };
    Annotation::new(
        Mark::new(kind, start, end, !note.is_empty()),
        (doc.progress(start) * 100.0 + 0.5) as u8,
        unix_of(x, item.str("datetime").unwrap_or_default()),
        unix_of(x, item.str("datetime_updated").unwrap_or_default()),
        annotations::snippet(&text),
        note,
    )
}

/// Annotation `a` as KOReader keeps it; `None` where it cannot be placed
/// there, or while neither it nor the clock tells when it was made
/// (KOReader knows annotations by that time).
fn item_of(x: &Exchange<'_>, a: &Annotation) -> Option<Value> {
    let (doc, storage) = (x.doc, x.storage);
    let created = known_time([a.created, x.now])?;
    let page = doc.xpointer(storage, a.mark.start)?;
    let optional = |v: Option<String>| v.map_or(Value::Nil, |v| lua::string(&v));
    let note = optional((!a.note.is_empty()).then(|| a.note.clone()));
    let updated = optional((a.updated > 0).then(|| datetime(x, a.updated)));
    let chapter = optional(chapter(doc, storage, a.mark.start));
    Some(match a.mark.kind {
        Kind::Bookmark => lua::table([
            ("chapter", chapter),
            ("datetime", lua::string(&datetime(x, created))),
            ("datetime_updated", updated),
            ("note", note),
            ("page", lua::string(&page)),
        ]),
        Kind::Highlight => {
            let end = doc.xpointer(storage, a.mark.end)?;
            let text = text_between(doc, storage, a.mark.start, a.mark.end);
            let text = if text.is_empty() {
                a.text.clone()
            } else {
                text
            };
            lua::table([
                ("chapter", chapter),
                ("datetime", lua::string(&datetime(x, created))),
                ("datetime_updated", updated),
                ("drawer", lua::string(x.drawer)),
                ("note", note),
                ("page", lua::string(&page)),
                ("pos0", lua::string(&page)),
                ("pos1", lua::string(&end)),
                ("text", lua::string(&text)),
            ])
        }
        _ => return None,
    })
}

/// The first of `times` that is known (seconds since 1970 over 0).
fn known_time(times: impl IntoIterator<Item = i64>) -> Option<i64> {
    times.into_iter().find(|&t| t > 0)
}

/// Exchanges the annotations of the book at `book` with KOReader's side
/// `theirs`; `true` if those here changed. Nothing changes on either side
/// if a file here cannot be read.
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
pub fn exchange(x: &Exchange<'_>, book: &str, theirs: Theirs) -> Result<bool, Unreadable> {
    let (doc, storage) = (x.doc, x.storage);
    // Only where KOReader's positions are understood.
    if doc.xpointer(storage, Locator::new(0, 0)).is_none() {
        return Ok(false);
    }
    let Theirs {
        items: theirs,
        pending,
        listed,
    } = theirs;
    let last = load_known(storage, book)?;
    let emptied = last.emptied;
    let mut ours = load_ours(storage, book)?;
    // KOReader writes an empty list for every book it opened: everything
    // removed there only where its own file had annotations at the last
    // exchange. Else (a fresh file of another device, say) what both had
    // is not taken as known there, and goes to KOReader again.
    let empty_there = listed && theirs.is_empty() && !last.known.is_empty();
    let removed_there = empty_there && !pending && last.seen > 0;
    // The reader decides, in the book: nothing changes before.
    if removed_there && !ours.is_empty() && x.removal == Removal::Ask {
        return Ok(false);
    }
    let removed_there = removed_there && x.removal == Removal::Remove;
    let known: &[Known] = if empty_there && !removed_there {
        &[]
    } else {
        &last.known
    };
    // None at all on one side of what both had is rather a move of book
    // data halfway or a sync not complete than everything removed there,
    // unless that side tells so.
    if !known.is_empty() && ((ours.is_empty() && !emptied) || (theirs.is_empty() && !listed)) {
        return Err(Unreadable);
    }
    // What KOReader's own file has now, for the next exchange.
    let seen = if pending {
        last.seen
    } else {
        u32::try_from(theirs.len()).unwrap_or(u32::MAX)
    };
    let known_of = |key: Key| known.iter().find(|k| k.key == key);
    let mut matched = alloc::vec![false; ours.len()];
    let (mut ours_changed, mut theirs_changed) = (false, false);
    let mut items: Vec<Value> = Vec::new();
    // What both have after this exchange.
    let mut shared: Vec<Known> = Vec::new();
    for mut item in theirs {
        let Some(key) = locate(x, &item) else {
            items.push(Value::Table(item));
            continue;
        };
        if let Some(i) = ours.iter().position(|a| key_of(&a.mark) == key) {
            matched[i] = true;
            let a = &mut ours[i];
            let note = note_of(&item);
            let mut same = true;
            if note != a.note {
                let changed = item
                    .str("datetime_updated")
                    .or(item.str("datetime"))
                    .unwrap_or_default();
                let their_time = unix_of(x, changed);
                let our_time = a.updated.max(a.created);
                // A note changed on one side since the last exchange goes
                // to the other; one changed on both, the later change,
                // where both times are known. Else both stay as they are.
                let before = known_of(key).and_then(|k| k.note);
                let theirs_wins = match before {
                    Some(n) if n == fingerprint(&a.note) => Some(true),
                    Some(n) if n == fingerprint(&note) => Some(false),
                    _ if their_time > 0 && our_time > 0 => Some(their_time > our_time),
                    _ => None,
                };
                match theirs_wins {
                    Some(true) => {
                        a.mark.noted = !note.is_empty();
                        a.note = note;
                        if their_time > 0 {
                            a.updated = their_time;
                        }
                        ours_changed = true;
                    }
                    Some(false) => {
                        let ours_note = (!a.note.is_empty()).then(|| lua::string(&a.note));
                        item.set("note", ours_note.unwrap_or(Value::Nil));
                        if let Some(time) = known_time([a.updated, a.created, x.now]) {
                            item.set("datetime_updated", lua::string(&datetime(x, time)));
                        }
                        theirs_changed = true;
                    }
                    None => same = false,
                }
            }
            shared.push(Known {
                key,
                note: same.then(|| fingerprint(&a.note)),
            });
            items.push(Value::Table(item));
        } else if known_of(key).is_some() {
            // Removed here: not passed on.
            theirs_changed = true;
        } else {
            let a = annotation_of(x, &item, key);
            shared.push(Known {
                key,
                note: Some(fingerprint(&a.note)),
            });
            ours.push(a);
            matched.push(true);
            ours_changed = true;
            items.push(Value::Table(item));
        }
    }
    // Ours that KOReader does not have: new here, or removed there.
    let mut kept = Vec::with_capacity(ours.len());
    for (a, matched) in ours.into_iter().zip(matched) {
        if matched {
            kept.push(a);
        } else if known_of(key_of(&a.mark)).is_some() && !pending {
            ours_changed = true;
        } else {
            // Those not passed on now are not known to both, so they are
            // offered again next time.
            if let Some(item) = item_of(x, &a) {
                items.push(item);
                theirs_changed = true;
                shared.push(Known {
                    key: key_of(&a.mark),
                    note: Some(fingerprint(&a.note)),
                });
            }
            kept.push(a);
        }
    }
    // Everything removed in KOReader: kept aside first, never removed
    // here without a copy.
    if removed_there && ours_changed && !keep_aside(storage, book) {
        return Err(Unreadable);
    }
    // Not kept here, what came from KOReader must not count as known:
    // the next exchange would take it for removed here.
    if ours_changed && annotations::save(storage, book, &kept).is_err() {
        return Err(Unreadable);
    }
    let mut passed = true;
    if theirs_changed {
        let at = known_time([x.now]).map(|now| lua::string(&datetime(x, now)));
        let file = lua::table([
            ("annotations", Value::Table(Table::from_list(items))),
            ("datetime", at.unwrap_or(Value::Nil)),
            ("device_id", lua::string(DEVICE)),
        ]);
        passed = storage
            .write(&exchange_file(book), lua::write(&file).as_bytes())
            .is_ok();
    }
    // Not passed on, what was offered must not count as known: the next
    // exchange would take it for removed in KOReader. What is known from
    // before stays true, and it all goes again next time.
    if passed && (shared != last.known || emptied || seen != last.seen) {
        save_known(storage, book, &shared, seen);
    }
    Ok(ours_changed)
}

/// What an exchange does where KOReader's file, seen with annotations
/// before, has none any more: removed there, or a fresh file synced in
/// from another device, which cannot be told apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Removal {
    /// Nothing changes on either side (the reader is asked:
    /// [`removed_there`]).
    Ask,
    /// Removed here too, a copy kept aside.
    Remove,
    /// Kept here, and offered to KOReader again.
    Keep,
}

/// Whether KOReader's side `theirs` has none any more of what both had,
/// while there are annotations here: the reader is asked before anything
/// changes ([`Removal`]).
pub fn removed_there(
    storage: &dyn Storage,
    book: &str,
    theirs: &Theirs,
) -> Result<bool, Unreadable> {
    let last = load_known(storage, book)?;
    let empty_there = theirs.listed && theirs.items.is_empty() && !last.known.is_empty();
    Ok(empty_there && !theirs.pending && last.seen > 0 && !load_ours(storage, book)?.is_empty())
}

/// Keeps a copy of the annotations here of the book at `book` beside
/// them (`<key>.txt.removed`, numbered where there is one), never over
/// another; `false` where it could not be made.
fn keep_aside(storage: &dyn Storage, book: &str) -> bool {
    let from = annotations::file(book);
    let (dir, name) = from.rsplit_once('/').unwrap_or(("", &from));
    let to = paperpaw_sdk::files::free_path(storage, dir, &format!("{name}.removed"));
    !storage.exists(&to) && paperpaw_sdk::files::copy(storage, &from, &to).is_ok()
}

/// What an exchange of a book came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exchanged {
    Same,
    Changed,
    /// KOReader's file has none any more of what both had: the reader is
    /// to say whether to remove them here too.
    Ask,
}

/// Exchanges the annotations of the book at `book` with KOReader's, if
/// KOReader keeps any for it.
fn exchange_book(env: &Env<'_>, book: &str, removal: Removal) -> Exchanged {
    // KOReader's side first: most books have none, and then the book
    // need not be opened.
    let limit = max_file(env.device().memory);
    let Ok(Some(theirs)) = read_theirs(env.storage(), book, limit) else {
        return Exchanged::Same;
    };
    if removal == Removal::Ask && removed_there(env.storage(), book, &theirs) == Ok(true) {
        return Exchanged::Ask;
    }
    // As the reader opens it: the places of marks are those of its
    // conversion of the book.
    let options = paperpaw_sdk::reading::open_options(env);
    let Ok(doc) = env.formats().open_with(env.storage(), book, &options) else {
        return Exchanged::Same;
    };
    // Ours at their places in this conversion first, where KOReader's are
    // found: made in another (the book's own styles switched since, say),
    // they would be taken for others there, and ours for removed.
    let conversion = paperpaw_sdk::reading::conversion(env);
    if annotations::relocate(env.storage(), book, conversion, doc.as_ref()).is_err() {
        return Exchanged::Same;
    }
    let local = |unix| time::local_seconds(env, unix);
    let x = Exchange {
        storage: env.storage(),
        removal,
        doc: doc.as_ref(),
        now: time::unix_seconds(env).unwrap_or(0),
        local: &local,
        // As highlights look here: a line under them, else a marker.
        drawer: if paperpaw_sdk::decorations::Decoration::of_role(
            env,
            &paperpaw_sdk::builtin::roles::HIGHLIGHT,
        )
        .line
        .is_some()
        {
            "underscore"
        } else {
            "lighten"
        },
    };
    let changed = exchange(&x, book, theirs).unwrap_or(false);
    if changed {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "those taken over keep their XPointers; where that cannot be written now, the reader takes them as of its conversion when it shows them"
        )]
        let _ = annotations::relocate(env.storage(), book, conversion, x.doc);
        return Exchanged::Changed;
    }
    Exchanged::Same
}

/// The reader's answer whether to remove here what KOReader's file has
/// no more, with the book as its `path`.
const REMOVAL_ANSWER: paperpaw_sdk::event::Event<paperpaw_sdk::replies::Confirmed> =
    paperpaw_sdk::event::Event::new("koreader.removal-answer");
const PATH: &str = "path";

/// Exchanges `book`'s annotations as `removal` says; where the reader is
/// to decide, asks once in this run (the question's *Back* keeps them).
fn exchange_or_ask(
    env: &Env<'_>,
    book: &str,
    removal: Removal,
    asked: &RefCell<Vec<String>>,
    announced: &Announced,
    out: &mut paperpaw_sdk::Commands,
) {
    match exchange_book(env, book, removal) {
        Exchanged::Changed => {
            announced.set(book);
            out.push(annotations::changed(book));
        }
        Exchanged::Ask if !asked.borrow().iter().any(|b| b == book) => {
            asked.borrow_mut().push(String::from(book));
            let i18n = env.i18n();
            out.push(
                paperpaw_sdk::ask::confirm(i18n.tr("koreader.removed-there"), &REMOVAL_ANSWER)
                    .text(i18n.tr("koreader.removed-there.text"))
                    .yes(i18n.tr("koreader.removed-there.remove"))
                    .no(i18n.tr("koreader.removed-there.keep"))
                    .with(PATH, book)
                    .command(),
            );
        }
        Exchanged::Ask | Exchanged::Same => {}
    }
}

/// The book whose annotations this plugin announced as changed last: the
/// announcement comes back to it, and needs no exchange.
#[derive(Debug, Default)]
pub struct Announced(RefCell<Option<String>>);

impl Announced {
    /// Remembers that the change of `book`'s annotations is announced.
    pub fn set(&self, book: &str) {
        *self.0.borrow_mut() = Some(String::from(book));
    }

    /// Whether the change of `book`'s annotations is the one announced
    /// (once).
    pub fn is_own(&self, book: &str) -> bool {
        let mut announced = self.0.borrow_mut();
        let own = announced.as_deref() == Some(book);
        if own {
            *announced = None;
        }
        own
    }
}

/// Whether a book at `book` shares KOReader's folder with another format
/// of it beside it (`Book.epub` and `Book.pdf`).
fn shares_sidecar(storage: &dyn Storage, book: &str) -> bool {
    let stem = |name: &str| name.rsplit_once('.').map(|(stem, _)| String::from(stem));
    let own = stem(paperpaw_sdk::files::name(book));
    let Ok(entries) = paperpaw_sdk::files::list(storage, paperpaw_sdk::files::parent(book), true)
    else {
        return false;
    };
    entries
        .iter()
        .any(|e| !e.is_dir && stem(&e.name) == own && e.name != paperpaw_sdk::files::name(book))
}

/// A book moved: KOReader's folder goes along (what is known moves as
/// book data), or only the book's own files in it where another format
/// of the book shares it.
fn moved_book(storage: &dyn Storage, from: &str, to: &str) {
    if has_entries(storage, &sidecar(from)) && shares_sidecar(storage, from) {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "there already, or not made: then the moves below fail and the files stay where KOReader finds them with the other format"
        )]
        let _ = storage.create_dir(&sidecar(to));
        for (old, new) in [
            (metadata_file(from), metadata_file(to)),
            (exchange_file(from), exchange_file(to)),
        ] {
            #[expect(
                clippy::let_underscore_must_use,
                reason = "kept as it was when this check came in; decide whether the user should hear of a failure here when this is next changed"
            )]
            let _ = storage.rename(&old, &new);
        }
    } else if has_entries(storage, &sidecar(from)) {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "kept as it was when this check came in; decide whether the user should hear of a failure here when this is next changed"
        )]
        let _ = storage.rename(&sidecar(from), &sidecar(to));
        let old = format!(
            "{}/{}.annotations.lua",
            sidecar(to),
            paperpaw_sdk::files::name(from)
        );
        #[expect(
            clippy::let_underscore_must_use,
            reason = "kept as it was when this check came in; decide whether the user should hear of a failure here when this is next changed"
        )]
        let _ = storage.rename(&old, &exchange_file(to));
    }
}

/// The KOReader plugin.
#[derive(Debug, Default)]
pub struct KoReader;

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl Plugin for KoReader {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            PLUGIN,
            "KOReader highlights",
            paperpaw_sdk::package_version!(),
        )
        .description("Shares bookmarks and highlights with KOReader for books read in both.")
        .how(&HOW)
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        // What both sides had at the last exchange, by book: moved and
        // removed with it.
        r.add_book_data(Env::plugin_dir(PLUGIN.as_str()), "")?;
        let announced = Rc::new(Announced::default());
        // Books whose reader was asked in this run whether to remove what
        // KOReader's file has no more.
        let asked = Rc::new(RefCell::new(Vec::new()));
        let (opened, opened_asked) = (Rc::clone(&announced), Rc::clone(&asked));
        r.on(events::BOOK_OPENED, move |env, reading, out| {
            let book = &reading.path;
            exchange_or_ask(env, book, Removal::Ask, &opened_asked, &opened, out);
        })?;
        let (answered, answered_asked) = (Rc::clone(&announced), Rc::clone(&asked));
        r.on(REMOVAL_ANSWER, move |env, answer, out| {
            let Some(book) = answer.context.get(PATH) else {
                return;
            };
            let removal = if answer.yes {
                Removal::Remove
            } else {
                Removal::Keep
            };
            let book = String::from(book);
            exchange_or_ask(env, &book, removal, &answered_asked, &answered, out);
        })?;
        r.on(events::ANNOTATIONS_CHANGED, move |env, changed, out| {
            if announced.is_own(&changed.path) {
                return;
            }
            note_emptied(env.storage(), &changed.path);
            exchange_or_ask(env, &changed.path, Removal::Ask, &asked, &announced, out);
        })?;
        r.on(events::BOOK_MOVED, |env, moved, _| {
            moved_book(env.storage(), &moved.from, &moved.path);
        })?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
