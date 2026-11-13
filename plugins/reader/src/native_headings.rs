//! The headings scan (R6b): for a book whose own table of contents has at most
//! one entry (`native_toc::wants_scan`), the blocks are read for headings, a few
//! at a time, and the headings found are kept in the data folder so that the
//! next opening needs no scan. Beta 3's `toc.rs` is the reference.
//!
//! The scan is a background step model like [`crate::native_index::Counter`]:
//! the caller (`core.work`) calls [`Scan::step`] with a function that reads the
//! block at a place; the step reads at most [`STEP_BLOCKS`] blocks and says
//! whether more remain. Nothing here holds a host. Every allocation is fallible;
//! a scan that ran out of memory keeps nothing.
use crate::native_block::pieces;
use crate::native_document::{Next, read_block};
use crate::native_index::{KEPT, kept_with};
use crate::native_toc::{
    self, Entry, KeptReader, MAX_HEADINGS, MAX_KEPT, Place, STEP_BLOCKS, normalize, push_entry,
    title_of,
};
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;
use paperpaw_sdk::digest::fnv1a64;
use paperpaw_sdk::document::{Block, BlockKind};
use paperpaw_sdk::native::{
    Error, Host,
    document::{DocId, DocPosition, Documents},
};
use paperpaw_sdk::{Storage, StorageError};

/// The data file that lists the kept headings files, oldest first.
pub const KEPT_FILE: &str = "headings.list";
/// Headings files kept, so that the data folder does not fill with one per book.
pub const FILES_KEPT: usize = KEPT * 4;
/// The most of the list that is read.
const LIST_MAX: usize = 1024;
/// Bytes of the kept copy written at once.
const CHUNK: usize = 1024;

/// What one block of the book is, as the scan needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Seen {
    /// The heading level (1 the highest), `None` for any other block.
    pub heading: Option<u8>,
    /// The block's text (a block that is no heading may leave it empty).
    pub text: String,
    /// Where the block starts and ends in the source.
    pub start: u32,
    /// See `start`.
    pub end: u32,
}

impl Seen {
    /// A block read from the book (its text is moved, not copied).
    #[must_use]
    pub fn of(block: Block) -> Self {
        let heading = match block.kind {
            BlockKind::Heading(level) => Some(level),
            _ => None,
        };
        Self {
            heading,
            text: if heading.is_some() {
                block.text
            } else {
                String::new()
            },
            start: block.start,
            end: block.end,
        }
    }
}

/// The block of the book `id` at or after `at`, as the scan needs it: for the
/// caller's `read` function. `input` and `out` are the buffers of a block read
/// (`native_document::INPUT_BYTES` and `REPLY_BYTES`). `None` where the section
/// has no more blocks.
///
/// # Errors
/// What the document service refuses (`Busy` while the book prepares).
pub fn read_seen(
    (host, docs, id): (&Host<'_>, &Documents, DocId),
    (input, out): (&mut [u8], &mut [u8]),
    at: Place,
) -> Result<Option<Seen>, Error> {
    let place = DocPosition::new(at.section, at.offset);
    let read = read_block(|cursor, assembler| {
        let fragment = docs.block_at(host, id, place, cursor, input, out)?;
        let more = fragment.continuation();
        match assembler.push(pieces(&fragment), more.is_none())? {
            Some(block) => Ok(Next::Done(block)),
            None => more.map(Next::More).ok_or(Error::Invalid),
        }
    });
    match read {
        Ok(block) => Ok(Some(Seen::of(block))),
        Err(Error::NotFound) => Ok(None),
        Err(error) => Err(error),
    }
}

/// What one step did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// More of the book remains.
    More,
    /// The book is scanned (the headings are kept where the file takes them).
    Done,
    /// The scan cannot go on (out of memory, or the book is not ready); nothing
    /// is kept.
    Stopped,
}

/// The file of the kept headings of the book at `path` (the data folder is
/// flat: the path's hash tells the books apart, the edition inside it the
/// editions).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn file_name(path: &str) -> Result<String, Error> {
    let mut name = String::new();
    name.try_reserve_exact(30).map_err(|_| Error::NoMemory)?;
    write!(name, "headings-{:016x}.txt", fnv1a64(path)).map_err(|_| Error::NoMemory)?;
    Ok(name)
}

fn storage_error(error: StorageError) -> Error {
    match error {
        StorageError::OutOfMemory => Error::NoMemory,
        _ => Error::Io,
    }
}

/// The headings kept for the book at `path` in its `edition` (the document's
/// conversion, which moves when the book or what reads it changes): `None`
/// where there are none, they were made for another edition or cannot be read.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn load(storage: &dyn Storage, path: &str, edition: u64) -> Result<Option<Vec<Entry>>, Error> {
    let name = file_name(path)?;
    // Read [`CHUNK`] bytes at a time and a line at a time: the copy is never
    // held whole (#248).
    let mut chunk = Vec::new();
    chunk
        .try_reserve_exact(CHUNK)
        .map_err(|_| Error::NoMemory)?;
    chunk.resize(CHUNK, 0);
    let mut line = Vec::new();
    line.try_reserve_exact(LINE_MAX)
        .map_err(|_| Error::NoMemory)?;
    let (mut reader, mut at, mut long) = (KeptReader::new(edition), 0_u64, false);
    'file: loop {
        let n = match storage.read_at(&name, at, &mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(StorageError::OutOfMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(None),
        };
        at = at.saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
        if at > u64::try_from(MAX_KEPT).unwrap_or(u64::MAX) {
            return Ok(None);
        }
        for &byte in &chunk[..n] {
            if byte != b'\n' {
                // A line longer than any written is not an entry.
                long |= line.len() == LINE_MAX;
                if !long {
                    line.push(byte);
                }
                continue;
            }
            if !long && !take_line(&mut reader, &line)? {
                break 'file;
            }
            long = false;
            line.clear();
        }
    }
    if !long && !line.is_empty() {
        take_line(&mut reader, &line)?;
    }
    Ok(reader.finish())
}

/// The longest line of a kept copy: a title of [`native_toc::MAX_TITLE`]
/// characters and its numbers.
const LINE_MAX: usize = native_toc::MAX_TITLE * 4 + 64;

/// Gives `line` to `reader`; one that is no UTF-8 is left out.
fn take_line(reader: &mut KeptReader, line: &[u8]) -> Result<bool, Error> {
    match core::str::from_utf8(line) {
        Ok(text) => reader.line(text),
        Err(_) => Ok(true),
    }
}

/// Writes the kept copy of `headings` to `name` in pieces (a temporary file,
/// then renamed: a cut leaves the old copy), so the whole text is never held.
fn write_kept(
    storage: &dyn Storage,
    name: &str,
    edition: u64,
    headings: &[Entry],
) -> Result<(), Error> {
    let mut temporary = String::new();
    temporary
        .try_reserve_exact(name.len() + 4)
        .map_err(|_| Error::NoMemory)?;
    temporary.push_str(name);
    temporary.push_str(".new");
    storage.remove(&temporary).map_err(storage_error)?;
    let mut piece = String::new();
    piece
        .try_reserve_exact(CHUNK + 2 * native_toc::MAX_TITLE * 4 + 64)
        .map_err(|_| Error::NoMemory)?;
    let result = (|| {
        writeln!(piece, "size {edition}").map_err(|_| Error::NoMemory)?;
        storage.append(&temporary, b"").map_err(storage_error)?;
        for h in headings {
            writeln!(
                piece,
                "{}\t{}\t{}\t{}",
                h.at.section, h.at.offset, h.level, h.title
            )
            .map_err(|_| Error::NoMemory)?;
            if piece.len() >= CHUNK {
                storage
                    .append(&temporary, piece.as_bytes())
                    .map_err(storage_error)?;
                piece.clear();
            }
        }
        if !piece.is_empty() {
            storage
                .append(&temporary, piece.as_bytes())
                .map_err(storage_error)?;
        }
        storage.rename(&temporary, name).map_err(storage_error)
    })();
    if result.is_err() {
        storage.remove(&temporary).ok();
    }
    result
}

/// Notes `name` as the newest kept file and removes the files beyond
/// [`FILES_KEPT`]. A list that cannot be handled leaves the files alone: they
/// take room, not data.
fn keep(storage: &dyn Storage, name: &str) {
    let Ok(mut buffer) = paperpaw_ui_kit::mem::buffer(LIST_MAX) else {
        return;
    };
    let read = storage.read_at(KEPT_FILE, 0, &mut buffer).unwrap_or(0);
    let list = core::str::from_utf8(buffer.get(..read).unwrap_or(&[])).unwrap_or("");
    let mut evicted = Vec::new();
    let Ok(updated) = kept_with(list, name, FILES_KEPT, &mut evicted) else {
        return;
    };
    if storage.write(KEPT_FILE, updated.as_bytes()).is_err() {
        return;
    }
    for old in &evicted {
        storage.remove(old).ok();
    }
}

/// The scan of one book.
#[derive(Debug)]
pub struct Scan {
    name: String,
    /// What tells the book's editions apart (see [`load`]).
    edition: u64,
    sections: u32,
    next: Place,
    found: Vec<Entry>,
    /// What the headings found cost the list ([`native_toc::row_cost`]): the
    /// scan stops one heading past [`native_toc::TOC_BYTES`], so the list knows
    /// it left some out.
    bytes: usize,
    /// `Some` once the scan ended: `true` where it ran to the end.
    end: Option<bool>,
}

impl Scan {
    /// A scan of the book at `path` in its `edition` (see [`load`]) with
    /// `sections` sections, from its start.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn new(path: &str, edition: u64, sections: u32) -> Result<Self, Error> {
        Ok(Self {
            name: file_name(path)?,
            edition,
            sections,
            next: Place::default(),
            found: Vec::new(),
            bytes: 0,
            end: None,
        })
    }

    /// How far the scan is, in percent of the sections (100 once done).
    #[must_use]
    pub fn percent(&self) -> u32 {
        if self.end == Some(true) || self.sections == 0 {
            return 100;
        }
        let done = u64::from(self.next.section.min(self.sections)) * 100;
        u32::try_from(done / u64::from(self.sections)).unwrap_or(100)
    }

    /// The headings found so far (levels counted from the highest once done).
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.found
    }

    /// The headings found, given away (the scan is spent).
    #[must_use]
    pub fn into_entries(self) -> Vec<Entry> {
        self.found
    }

    /// Reads up to [`STEP_BLOCKS`] blocks with `read` (the block at or after a
    /// place; `None` where the section has no more). A section that cannot be
    /// read is passed over; one that is not ready or a refused allocation stops
    /// the scan. At the end the headings are kept in `storage`, best effort.
    pub fn step(
        &mut self,
        storage: &dyn Storage,
        read: &mut dyn FnMut(Place) -> Result<Option<Seen>, Error>,
    ) -> Step {
        match self.end {
            Some(true) => return Step::Done,
            Some(false) => return Step::Stopped,
            None => {}
        }
        for _ in 0..STEP_BLOCKS {
            if self.next.section >= self.sections
                || self.found.len() >= MAX_HEADINGS
                || self.bytes > native_toc::TOC_BYTES
            {
                return self.finish(storage);
            }
            match read(self.next) {
                Ok(Some(block)) => {
                    if self.take(&block).is_err() {
                        self.end = Some(false);
                        return Step::Stopped;
                    }
                    self.next = native_toc::next_place(self.next, block.end);
                }
                Ok(None) => self.next = Place::new(self.next.section.saturating_add(1), 0),
                // The book closed (or is going): nothing is kept.
                Err(
                    Error::NoMemory | Error::Busy | Error::Cancelled | Error::Denied | Error::State,
                ) => {
                    self.end = Some(false);
                    return Step::Stopped;
                }
                // A section the book cannot give is passed.
                Err(_) => self.next = Place::new(self.next.section.saturating_add(1), 0),
            }
        }
        Step::More
    }

    fn take(&mut self, block: &Seen) -> Result<(), Error> {
        let Some(level) = block.heading else {
            return Ok(());
        };
        let Some(title) = title_of(&block.text)? else {
            return Ok(());
        };
        self.bytes = self.bytes.saturating_add(native_toc::row_cost(&title));
        push_entry(
            &mut self.found,
            Entry::new(title, level, Place::new(self.next.section, block.start)),
        )
    }

    fn finish(&mut self, storage: &dyn Storage) -> Step {
        normalize(&mut self.found);
        self.end = Some(true);
        if write_kept(storage, &self.name, self.edition, &self.found).is_ok() {
            keep(storage, &self.name);
        }
        Step::Done
    }
}

#[cfg(test)]
mod tests;
