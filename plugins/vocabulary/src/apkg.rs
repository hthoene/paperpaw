//! Anki's `.apkg` decks from the card (`/Flashcards/deck.apkg`): a ZIP with
//! an SQLite database of the notes, read here without Anki.
//!
//! The deck's database (`collection.anki21`, or `collection.anki2` of an
//! export for older Anki) is inflated to a file of the plugin's folder, a
//! piece at a time; its `notes` table is read page by page; each note's
//! first three fields (front, back, example) are written as plain text to a
//! tab separated file, which is then merged as every other file of the
//! folder is. Nothing of the database is kept in memory but one page and
//! one note, so the heap is the same as for a text file. Scheduling, media
//! and the other note types' templates are left out (ticket #26, Anki's own
//! sync, is not this).

use alloc::string::String;

use paperpaw_sdk::{Storage, StorageError};
use paperpaw_sqlite::{Database, Error as SqlError, Row, Value};
use paperpaw_zip::{ZipArchive, ZipError};

use crate::{cards, lines, table};

/// The deck's database inflated: at most this large. Inflating it and
/// reading it are a step each; on the X4 (an SD card on the SPI bus) a step
/// must end well within the watchdog's 20 s. A deck exported without
/// scheduling is far smaller than one with.
pub(crate) const MAX_DB: u32 = 4 * 1024 * 1024;
/// A ZIP of at most this many files (the media of a deck are files too):
/// its index takes 8 bytes of memory for each.
const MAX_FILES: u16 = 4000;
/// The notes of a deck, as many as the cards a deck holds.
const MAX_NOTES: usize = cards::MAX_CARDS;
/// A note whose fields are longer than this (in bytes, together) is left
/// out, unread: its pictures are in it as text. A card holds 500 letters
/// on each side, so nothing a card can hold is lost.
const MAX_NOTE: usize = table::MAX_RECORD;
/// A note's row (all its columns, as stored) larger than this is not read:
/// a row read takes about twice its size while it is decoded.
const MAX_PAYLOAD: usize = 16 * 1024;
/// How far back from the end of a ZIP its end record is looked for: a
/// comment is at most 64 KiB.
const COMMENT: u64 = 0xffff;
/// What the text of notes is collected in before it is written.
const BUFFER: usize = 2048 + 1024;

/// Why a deck could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fault {
    /// Not an Anki deck: no ZIP, no database, no notes.
    NoNotes,
    /// Made by a newer Anki, which keeps its notes where this cannot read.
    NewerAnki,
    /// More files, a larger database or more notes than are read.
    TooLarge,
    /// No room for it in memory.
    Memory,
    /// The card could not be read or written.
    Storage,
}

impl From<StorageError> for Fault {
    fn from(_: StorageError) -> Self {
        Fault::Storage
    }
}

impl From<ZipError> for Fault {
    fn from(e: ZipError) -> Self {
        match e {
            ZipError::OutOfMemory => Fault::Memory,
            ZipError::TooLarge => Fault::TooLarge,
            ZipError::Storage(_) => Fault::Storage,
            _ => Fault::NoNotes,
        }
    }
}

impl From<SqlError> for Fault {
    fn from(e: SqlError) -> Self {
        match e {
            SqlError::OutOfMemory => Fault::Memory,
            SqlError::Storage(_) => Fault::Storage,
            _ => Fault::NoNotes,
        }
    }
}

/// Whether `name` is a deck of Anki's.
pub(crate) fn is_apkg(name: &str) -> bool {
    name.len() > 5
        && name.is_char_boundary(name.len() - 5)
        && name
            .get(name.len() - 5..)
            .is_some_and(|ext| ext.eq_ignore_ascii_case(".apkg"))
}

/// Where the database is inflated to.
const DATABASE: &str = ".paperpaw/plugins/vocabulary/cards/apkg.db";
/// Where the notes are written to as text.
const TEXT: &str = ".paperpaw/plugins/vocabulary/cards/apkg.tsv";

/// Where the notes are written to as text, to be read as a file of the
/// folder (until the job is done with it).
pub(crate) const fn text_path() -> &'static str {
    TEXT
}

/// Removes what a deck left in the plugin's folder.
pub(crate) fn tidy(storage: &dyn Storage) {
    // The names are constants: tidying allocates nothing, so it runs
    // whatever the heap says.
    for path in [DATABASE, TEXT] {
        if storage.exists(path) {
            // A leftover is overwritten by the next deck; nothing to tell.
            let _ = storage.remove(path);
        }
    }
}

/// How many files the ZIP at `path` holds, from its end record, looked for
/// in the last 64 KiB (the record is followed by a comment of up to that
/// size), a piece at a time; `None` where it is not found.
fn file_count(storage: &dyn Storage, path: &str) -> Result<Option<u16>, StorageError> {
    const PIECE: usize = 1024;
    let size = storage.size(path)?;
    let mut piece = [0u8; PIECE + 21];
    // The piece that ends at `end`, from the end of the file back.
    let mut end = size;
    while size - end <= COMMENT + 22 && end >= 22 {
        let start = end
            .saturating_sub(PIECE as u64 + 21)
            .max(size.saturating_sub(COMMENT + 22));
        let got = usize::try_from(end - start).unwrap_or(0);
        if got < 22 || storage.read_at(path, start, &mut piece[..got])? != got {
            return Ok(None);
        }
        for at in (0..=got - 22).rev() {
            let record = &piece[at..at + 22];
            let comment = u64::from(u16::from_le_bytes([record[20], record[21]]));
            // The record, and a comment that ends where the file does.
            if record[..4] == [0x50, 0x4b, 5, 6] && start + at as u64 + 22 + comment == size {
                return Ok(Some(u16::from_le_bytes([record[10], record[11]])));
            }
        }
        if start == 0 || start <= size.saturating_sub(COMMENT + 22) {
            break;
        }
        // The next piece ends where this one starts, and 21 bytes more:
        // a record that lay across the edge is whole in one of them.
        end = start + 21;
    }
    Ok(None)
}

/// What was made of a deck's notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Notes {
    /// The notes written as cards' text.
    pub count: usize,
    /// The notes left out as too long.
    pub skipped: usize,
}

/// The deck at `path`'s database inflated to the plugin's folder, in one
/// step (see [`MAX_DB`]); [`notes`] reads it in the next.
pub(crate) fn inflate(storage: &dyn Storage, path: &str) -> Result<(), Fault> {
    tidy(storage);
    let result = inflate_database(storage, path);
    if result.is_err() {
        tidy(storage);
    }
    result
}

fn inflate_database(storage: &dyn Storage, path: &str) -> Result<(), Fault> {
    if file_count(storage, path)?.is_some_and(|n| n > MAX_FILES) {
        return Err(Fault::TooLarge);
    }
    let zip = ZipArchive::open(storage, path)?;
    let named = |name: &str| zip.entry(storage, name);
    // Newer Anki puts its notes in `collection.anki21b` (zstd) and leaves
    // an old one with a note that says so: only a deck exported for older
    // versions is read.
    let entry = match (named("collection.anki21")?, named("collection.anki21b")?) {
        (Some(entry), _) => entry,
        (None, Some(_)) => return Err(Fault::NewerAnki),
        (None, None) => named("collection.anki2")?.ok_or(Fault::NoNotes)?,
    };
    if entry.size > MAX_DB {
        return Err(Fault::TooLarge);
    }
    let db = DATABASE;
    storage.write(db, &[])?;
    let mut failed = None;
    zip.stream(
        storage,
        &entry,
        &mut |chunk| match storage.append(db, chunk) {
            Ok(()) => true,
            Err(e) => {
                failed = Some(e);
                false
            }
        },
    )?;
    match failed {
        Some(e) => Err(e.into()),
        None => Ok(()),
    }
}

/// The notes of the inflated database written as text to [`text_path`], in
/// one step; the database is removed after it.
pub(crate) fn notes(storage: &dyn Storage) -> Result<Notes, Fault> {
    let result = write_notes(storage, DATABASE);
    // The database is only needed while it is read.
    let _ = storage.remove(DATABASE);
    result
}

/// [`inflate`] and [`notes`] in a row.
#[cfg(test)]
pub(crate) fn convert(storage: &dyn Storage, path: &str) -> Result<Notes, Fault> {
    inflate(storage, path)?;
    notes(storage)
}

/// The notes table of the database at `db` written as text.
fn write_notes(storage: &dyn Storage, db: &str) -> Result<Notes, Fault> {
    let database = Database::open(storage, db)?;
    let (columns, root) = database.table("notes").map_err(|_| Fault::NoNotes)?;
    let flds = columns
        .iter()
        .position(|c| c.eq_ignore_ascii_case("flds"))
        .ok_or(Fault::NoNotes)?;
    let out = text_path();
    storage.write(out, b"#separator:tab\n")?;
    let (mut count, mut skipped, mut buffer) = (0usize, 0usize, String::new());
    // What stopped the scan: a refused allocation or a write that failed.
    let failed: core::cell::Cell<Option<Fault>> = core::cell::Cell::new(None);
    // A buffer for a few notes is reserved at once: the pushes below take
    // from it, and a note larger than what is left grows it fallibly.
    buffer
        .try_reserve_exact(BUFFER)
        .map_err(|_| Fault::Memory)?;
    let put = |buffer: &mut String| match storage.append(out, buffer.as_bytes()) {
        Ok(()) => {
            buffer.clear();
            true
        }
        Err(e) => {
            failed.set(Some(e.into()));
            false
        }
    };
    let mut too_many = false;
    database.scan_bounded(root, MAX_PAYLOAD, &mut |_, row| {
        // Too large to hold (pictures in it as text, say): passed over by
        // the reader, nothing of it allocated.
        let Row::Values(values) = row else {
            skipped += 1;
            return true;
        };
        let Some(Value::Text(text)) = values.get(flds) else {
            return true;
        };
        // Nothing of a long note is copied: its pictures are text in it.
        if text.len() > MAX_NOTE {
            skipped += 1;
            return true;
        }
        let mut fields = text.split('\x1f').map(note_text);
        let (Ok(front), Ok(back), Ok(example)) = (
            fields.next().unwrap_or(Ok(String::new())),
            fields.next().unwrap_or(Ok(String::new())),
            fields.next().unwrap_or(Ok(String::new())),
        ) else {
            failed.set(Some(Fault::Memory));
            return false;
        };
        if front.is_empty() {
            return true;
        }
        if count >= MAX_NOTES {
            too_many = true;
            return false;
        }
        count += 1;
        let mut line = Ok(());
        for (i, cell) in [front, back, example].iter().enumerate() {
            if line.is_ok() && i > 0 {
                line = lines::push(&mut buffer, "\t");
            }
            if line.is_ok() {
                line = quote(&mut buffer, cell);
            }
        }
        if line.and_then(|()| lines::push(&mut buffer, "\n")).is_err() {
            failed.set(Some(Fault::Memory));
            return false;
        }
        buffer.len() < 2048 || put(&mut buffer)
    })?;
    if too_many {
        return Err(Fault::TooLarge);
    }
    if !buffer.is_empty() {
        put(&mut buffer);
    }
    match failed.get() {
        Some(e) => Err(e),
        None if count == 0 => Err(Fault::NoNotes),
        None => Ok(Notes { count, skipped }),
    }
}

/// A field of Anki's as plain text: markup and sounds out, pictures that
/// are text (`data:` addresses) out, cut at what a card's text may be.
fn note_text(field: &str) -> Result<String, StorageError> {
    let mut text = String::new();
    // Never longer than the field: one reservation.
    text.try_reserve_exact(field.len())
        .map_err(|_| StorageError::TooLarge)?;
    let mut rest = field;
    while let Some((at, end)) = next_hole(rest) {
        text.push_str(rest.get(..at).unwrap_or(""));
        rest = rest.get(end..).unwrap_or("");
    }
    text.push_str(rest);
    table::plain(cut(&text, table::MAX_FIELD))
}

/// Where the next sound (`[sound:…]`) or `data:` address (`data:image/png;…`)
/// of `text` is, from where it begins to where it ends.
fn next_hole(text: &str) -> Option<(usize, usize)> {
    let sound = text
        .find("[sound:")
        .map(|at| (at, text[at..].find(']').map_or(text.len(), |e| at + e + 1)));
    let mut data = None;
    let mut from = 0;
    while let Some(found) = text[from..].find("data:") {
        let at = from + found;
        let after = &text[at + 5..];
        let kind = after
            .find([';', ',', ' ', '"', '\''])
            .map_or(after, |e| &after[..e]);
        if kind.contains('/') {
            let end = text[at..]
                .find(['"', '\'', ' ', ')', '<', '>', '\n'])
                .map_or(text.len(), |e| at + e);
            data = Some((at, end));
            break;
        }
        from = at + 5;
    }
    match (sound, data) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (a, b) => a.or(b),
    }
}

/// `text` cut to at most `most` bytes, at a character.
fn cut(text: &str, most: usize) -> &str {
    let mut end = most.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// `cell` as a field of a tab separated file, quoted where it has to be.
fn quote(out: &mut String, cell: &str) -> Result<(), StorageError> {
    if !cell.contains(['\t', '"', '\n', '\r']) {
        return lines::push(out, cell);
    }
    // At most a doubled quote for a byte, and the two around.
    out.try_reserve(cell.len().saturating_mul(2).saturating_add(2))
        .map_err(|_| StorageError::TooLarge)?;
    out.push('"');
    for c in cell.chars() {
        if c == '"' {
            out.push('"');
        }
        out.push(c);
    }
    out.push('"');
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;
