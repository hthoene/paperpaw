//! The writer of version 3 of a book's annotation file, for the host's
//! staged transactions (`core.library` ops 17-21): the file is rewritten
//! **line by line** into a temporary file and renamed over the old one, so
//! a power cut leaves the old file whole, and no more than a line (at most
//! [`LINE_MAX`] bytes) and the texts of one old row are held at a time.
//!
//! - **Rows** are written in reading order: the old rows keep the order of
//!   the file and the host's new rows (adds, and the rows that replace
//!   one) are merged in by their place ([`Edits::next_new`]).
//! - **Lines that are no row** (the book's `# book` line, a kind a later
//!   build added, damage) are copied byte for byte, where they were; the
//!   header lines `# format`, `# next-id` and `# generation` are written
//!   anew.
//! - **Version 1 and 2** files are converted on their first write: every
//!   row gets the 1-based place it had among the rows as its id,
//!   `# next-id` is the row count plus 1, and the generation is 2 before
//!   the change, 3 after it ([`next_generation`]).
//! - A row is one line: tabs and line breaks of a text become spaces
//!   ([`one_line`]), a note is cut as [`stored_note`] says, and a line over
//!   [`LINE_MAX`] bytes is refused ([`StorageError::TooLarge`]): the reader
//!   would take it for no row.
use alloc::string::String;
use alloc::vec::Vec;
use core::ops::ControlFlow;

use super::read::{Captured, head_of, parse_line};
use super::{
    FILE_VERSION, KEY_MAX, Kind, LINE_MAX, Window, Windows, letter, next_generation, one_line,
    stored_note,
};
use crate::data::{Error, MARK};
use crate::document::Locator;
use crate::{Storage, StorageError};

/// The file's own numbers before a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Info {
    /// The version the file is of (1 for none).
    pub version: u32,
    /// The generation callers compare ([`super::generation_of`]).
    pub generation: u32,
    /// The id the next added row gets.
    pub next_id: u32,
    /// Rows the file has.
    pub rows: u32,
}

/// What a row of the old file is, for [`Edits::keeps`].
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct Existing<'a> {
    pub id: u32,
    pub start: Locator,
    pub end: Locator,
    /// The client key (empty for none).
    pub key: &'a str,
}

/// One row to be written.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct Spec<'a> {
    pub id: u32,
    pub kind: Kind,
    pub start: Locator,
    pub end: Locator,
    pub percent: u8,
    pub created: i64,
    pub updated: i64,
    pub conversion: Option<u64>,
    pub start_xpointer: &'a str,
    pub end_xpointer: &'a str,
    pub text: &'a str,
    pub note: &'a str,
    pub key: &'a str,
}

impl<'a> Spec<'a> {
    /// A row of `kind` between `start` and `end` with id `id` and nothing
    /// else; the other fields are set as they are known.
    #[must_use]
    pub fn new(id: u32, kind: Kind, start: Locator, end: Locator) -> Self {
        Self {
            id,
            kind,
            start,
            end,
            percent: 0,
            created: 0,
            updated: 0,
            conversion: None,
            start_xpointer: "",
            end_xpointer: "",
            text: "",
            note: "",
            key: "",
        }
    }
}

/// What the host asks of the writer while it merges.
pub trait Edits {
    /// Whether the row stays: `Ok(false)` drops it (removed, or replaced by a
    /// row [`Self::next_new`] gives).
    ///
    /// # Errors
    /// Stops the rewrite; nothing is changed.
    fn keeps(&mut self, row: &Existing<'_>) -> Result<bool, Error>;

    /// The next new row (in reading order) that starts at or before `limit`
    /// (`None`: whatever is left), given to `write`; `Ok(false)` where there
    /// is none.
    ///
    /// # Errors
    /// Stops the rewrite; nothing is changed.
    fn next_new(
        &mut self,
        limit: Option<(Locator, Locator)>,
        write: &mut dyn FnMut(&Spec<'_>) -> Result<(), Error>,
    ) -> Result<bool, Error>;
}

/// The annotation file of the book whose key is `key` ([`crate::library::key`]
/// as a number): the file [`super::file`] names for its path, without the path.
///
/// # Errors
/// [`Error::OutOfMemory`].
pub fn file_of_key(key: u64) -> Result<String, Error> {
    let mut name = String::new();
    name.try_reserve_exact(crate::data::DIR.len() + "/annotations/".len() + 16 + ".txt".len())
        .map_err(|_| Error::OutOfMemory)?;
    use core::fmt::Write;
    let _ = write!(name, "{}/annotations/{key:016x}.txt", crate::data::DIR);
    Ok(name)
}

/// The numbers of the annotation file `file` as a write starts from them.
///
/// # Errors
/// [`Error::Newer`] for a later version; the storage's.
pub fn try_info(storage: &dyn Storage, file: &str) -> Result<Info, Error> {
    let head = head_of(storage, file)?;
    let mut rows = 0_u32;
    let mut next_id = None;
    let mut max_id = 0_u32;
    let mut scratch = [0_u8; 0];
    let keys = Windows::new();
    each_line(storage, file, &mut |line| {
        if let Some(n) = line.strip_prefix(b"# next-id ") {
            next_id = core::str::from_utf8(n)
                .ok()
                .and_then(|n| n.trim().parse::<u32>().ok());
        } else if let Some(row) = parse_line(line, head.version, &keys, &mut scratch) {
            rows = rows.saturating_add(1);
            max_id = max_id.max(row.id);
        }
        Ok(ControlFlow::Continue(()))
    })?;
    // A file of an older version numbers its rows by place.
    let next_id = if head.version >= FILE_VERSION {
        next_id.unwrap_or(0).max(max_id.saturating_add(1)).max(1)
    } else {
        rows.saturating_add(1)
    };
    Ok(Info {
        version: head.version,
        generation: head.generation,
        next_id,
        rows,
    })
}

/// Calls `visit` with each row of the annotation file `file`, in file order
/// (the id of a row of an older version is its place), with the key it has.
///
/// # Errors
/// [`Error::Newer`]; the storage's; [`Error::OutOfMemory`].
pub fn each_existing(
    storage: &dyn Storage,
    file: &str,
    visit: &mut dyn FnMut(&Existing<'_>) -> Result<(), Error>,
) -> Result<(), Error> {
    let head = head_of(storage, file)?;
    let windows = Windows::new().key(Window::new(0, KEY_MAX as u32 + 1));
    let mut scratch = Vec::new();
    scratch
        .try_reserve_exact(KEY_MAX + 1)
        .map_err(|_| Error::OutOfMemory)?;
    scratch.resize(KEY_MAX + 1, 0);
    let mut place = 0_u32;
    each_line(storage, file, &mut |line| {
        if let Some(row) = parse_line(line, head.version, &windows, &mut scratch) {
            place = place.saturating_add(1);
            let key = if head.version >= FILE_VERSION {
                row.slots[2].part(&scratch).text
            } else {
                ""
            };
            visit(&Existing {
                id: if head.version >= FILE_VERSION {
                    row.id
                } else {
                    place
                },
                start: row.start,
                end: row.end,
                key,
            })?;
        }
        Ok(ControlFlow::Continue(()))
    })
}

/// The generation a write of `info` leaves the file at.
#[must_use]
pub fn generation_after(info: &Info) -> u32 {
    next_generation(if info.version < FILE_VERSION {
        2
    } else {
        info.generation
    })
}

/// A row as its line (no newline), appended to `line`.
///
/// # Errors
/// [`Error::OutOfMemory`]; [`StorageError::TooLarge`] for a line over
/// [`LINE_MAX`] bytes, or a client key over [`KEY_MAX`], and
/// [`Error::Unreadable`] for a kind that has no letter.
pub fn format_row(spec: &Spec<'_>, line: &mut Vec<u8>) -> Result<(), Error> {
    let kind = letter(spec.kind).ok_or(Error::Unreadable)?;
    if spec.key.len() > KEY_MAX || spec.key.contains(['\t', '\n', '\r']) || spec.id == 0 {
        return Err(Error::Unreadable);
    }
    let text = one_line(spec.text);
    let note = stored_note(spec.note);
    let (start, end) = (one_line(spec.start_xpointer), one_line(spec.end_xpointer));
    let numbers = [
        spec.start.section,
        spec.start.offset,
        spec.end.section,
        spec.end.offset,
    ];
    let mut head = String::new();
    head.try_reserve(80).map_err(|_| Error::OutOfMemory)?;
    use core::fmt::Write;
    let _ = write!(
        head,
        "{kind}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t",
        numbers[0], numbers[1], numbers[2], numbers[3], spec.percent, spec.created, spec.updated
    );
    if let Some(conversion) = spec.conversion {
        let _ = write!(head, "{conversion:016x}");
    }
    let mut tail = String::new();
    tail.try_reserve(24).map_err(|_| Error::OutOfMemory)?;
    let _ = write!(tail, "\t{}\t", spec.id);
    let total = [
        &head, "\t", &start, "\t", &end, "\t", &text, "\t", &note, &tail, spec.key,
    ]
    .iter()
    .map(|part| part.len())
    .sum::<usize>();
    if total > LINE_MAX {
        return Err(StorageError::TooLarge.into());
    }
    line.try_reserve(total).map_err(|_| Error::OutOfMemory)?;
    for part in [
        head.as_str(),
        "\t",
        &start,
        "\t",
        &end,
        "\t",
        &text,
        "\t",
        &note,
        &tail,
        spec.key,
    ] {
        line.extend_from_slice(part.as_bytes());
    }
    Ok(())
}

/// The columns an old row is read through when it is converted to
/// version 3: a text up to a whole line, a note up to its cap.
fn convert_windows() -> Windows {
    Windows::new()
        .text(Window::new(0, LINE_MAX as u32))
        .note(Window::new(0, 3 * 1024 + 4))
        .key(Window::new(0, KEY_MAX as u32 + 1))
        .xpointers(Window::new(0, 1024), Window::new(0, 1024))
}

/// What a line of the file is given to.
type LineVisit<'a> = dyn FnMut(&[u8]) -> Result<ControlFlow<()>, Error> + 'a;
/// Where the bytes of a line over [`LINE_MAX`] go (`None`: its end).
type LongSink<'a> = dyn FnMut(Option<&[u8]>) -> Result<(), Error> + 'a;

/// [`lines`] without a use for the lines that are too long.
fn each_line(storage: &dyn Storage, file: &str, visit: &mut LineVisit<'_>) -> Result<(), Error> {
    let mut sink = |_: Option<&[u8]>| Ok(());
    lines(storage, file, visit, &mut sink)
}

/// Calls `visit` with each line of `file` (without its newline), at most
/// [`LINE_MAX`] bytes of it; a longer line gives `visit` nothing but is
/// passed to `long` (its bytes, in pieces, then `None` at its end).
fn lines(
    storage: &dyn Storage,
    file: &str,
    visit: &mut LineVisit<'_>,
    long: &mut LongSink<'_>,
) -> Result<(), Error> {
    let mut line = Vec::new();
    line.try_reserve_exact(LINE_MAX + 1)
        .map_err(|_| Error::OutOfMemory)?;
    let mut chunk = [0_u8; 512];
    let (mut at, mut over) = (0_u64, false);
    loop {
        let n = match storage.read_at(file, at, &mut chunk) {
            Ok(n) => n,
            Err(StorageError::NotFound) => 0,
            Err(e) => return Err(e.into()),
        };
        if n == 0 {
            break;
        }
        at += n as u64;
        for piece in chunk[..n].split_inclusive(|b| *b == b'\n') {
            let ended = piece.last() == Some(&b'\n');
            let body = if ended {
                &piece[..piece.len() - 1]
            } else {
                piece
            };
            if over {
                long(Some(body))?;
                if ended {
                    long(None)?;
                    over = false;
                }
                continue;
            }
            if line.len() + body.len() > LINE_MAX {
                over = true;
                long(Some(&line))?;
                long(Some(body))?;
                line.clear();
                if ended {
                    long(None)?;
                    over = false;
                }
                continue;
            }
            line.extend_from_slice(body);
            if ended {
                if visit(&line)?.is_break() {
                    return Ok(());
                }
                line.clear();
            }
        }
    }
    if over {
        long(None)?;
    } else if !line.is_empty() {
        // The last line has no newline.
        let _ = visit(&line)?;
    }
    Ok(())
}

/// The output file, written in chunks of at most 4 KiB.
struct Out<'s> {
    storage: &'s dyn Storage,
    file: &'s str,
    chunk: Vec<u8>,
}

impl Out<'_> {
    fn put_bytes(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if self.chunk.len() + bytes.len() > 4096 {
            self.flush()?;
        }
        if bytes.len() >= 4096 {
            return self.storage.append(self.file, bytes).map_err(Error::from);
        }
        self.chunk.extend_from_slice(bytes);
        Ok(())
    }
    fn line(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.put_bytes(bytes)?;
        self.put_bytes(b"\n")
    }
    fn flush(&mut self) -> Result<(), Error> {
        if !self.chunk.is_empty() {
            self.storage.append(self.file, &self.chunk)?;
            self.chunk.clear();
        }
        Ok(())
    }
}

/// Rewrites the annotation file `file` as version 3 into `tmp` and renames it
/// over the file: the old rows in file order, those `edits` drops left out,
/// its new rows merged in by their place. `adds` is how many ids the new
/// rows take from `info.next_id` on (the header says `next-id` before any
/// row). Returns the generation written. Where anything fails `tmp` is
/// removed and the old file is as it was.
///
/// # Errors
/// [`Error::Newer`]; the storage's (including a card without `rename`);
/// [`Error::OutOfMemory`]; what `edits` gives.
pub fn rewrite(
    storage: &dyn Storage,
    file: &str,
    tmp: &str,
    (info, adds): (&Info, u32),
    edits: &mut dyn Edits,
) -> Result<u32, Error> {
    storage.remove(tmp)?;
    let made = merge(storage, file, tmp, (info, adds), edits);
    let done = made.and_then(|generation| {
        // Before the rename: a name must not point at data still kept back.
        storage.sync()?;
        storage.rename(tmp, file)?;
        Ok(generation)
    });
    if done.is_err() {
        let _ = storage.remove(tmp);
    }
    done
}

fn merge(
    storage: &dyn Storage,
    file: &str,
    tmp: &str,
    (info, adds): (&Info, u32),
    edits: &mut dyn Edits,
) -> Result<u32, Error> {
    let generation = generation_after(info);
    let next_id = info.next_id.checked_add(adds).ok_or(Error::Unreadable)?;
    let mut out = Out {
        storage,
        file: tmp,
        chunk: Vec::new(),
    };
    out.chunk
        .try_reserve_exact(4096)
        .map_err(|_| Error::OutOfMemory)?;
    let mut header = String::new();
    header.try_reserve(64).map_err(|_| Error::OutOfMemory)?;
    use core::fmt::Write;
    let _ = write!(
        header,
        "{MARK}{FILE_VERSION}\n# next-id {next_id}\n# generation {generation}\n"
    );
    out.put_bytes(header.as_bytes())?;
    let convert = info.version < FILE_VERSION;
    let windows = if convert {
        convert_windows()
    } else {
        Windows::new().key(Window::new(0, KEY_MAX as u32 + 1))
    };
    let mut scratch = Vec::new();
    scratch
        .try_reserve_exact(windows.scratch().ok_or(Error::Unreadable)?)
        .map_err(|_| Error::OutOfMemory)?;
    scratch.resize(windows.scratch().unwrap_or(0), 0);
    let mut buffer = Vec::new();
    let mut place = 0_u32;
    let out = core::cell::RefCell::new(out);
    // A new row is written as its line.
    let write_new = |spec: &Spec<'_>| -> Result<(), Error> {
        let mut line = Vec::new();
        format_row(spec, &mut line)?;
        out.borrow_mut().line(&line)
    };
    let mut visit = |line: &[u8]| -> Result<ControlFlow<()>, Error> {
        if line.starts_with(b"# ") {
            // The header lines are written anew.
            let header = [MARK, "# next-id ", "# generation "];
            if header.iter().any(|h| line.starts_with(h.as_bytes())) {
                return Ok(ControlFlow::Continue(()));
            }
        }
        let Some(row) = parse_line(line, info.version, &windows, &mut scratch) else {
            // No row: kept as it is, where it was.
            out.borrow_mut().line(line)?;
            return Ok(ControlFlow::Continue(()));
        };
        place = place.saturating_add(1);
        let id = if convert { place } else { row.id };
        let key = if convert {
            ""
        } else {
            row.slots[2].part(&scratch).text
        };
        let existing = Existing {
            id,
            start: row.start,
            end: row.end,
            key,
        };
        // New rows that come before this one are written first.
        while edits.next_new(Some((row.start, row.end)), &mut |s| write_new(s))? {}
        if !edits.keeps(&existing)? {
            return Ok(ControlFlow::Continue(()));
        }
        if convert {
            buffer.clear();
            let spec = converted(id, &row, &scratch);
            format_row(&spec, &mut buffer)?;
            out.borrow_mut().line(&buffer)?;
        } else {
            out.borrow_mut().line(line)?;
        }
        Ok(ControlFlow::Continue(()))
    };
    let mut passed = |piece: Option<&[u8]>| -> Result<(), Error> {
        match piece {
            Some(bytes) => out.borrow_mut().put_bytes(bytes),
            None => out.borrow_mut().put_bytes(b"\n"),
        }
    };
    lines(storage, file, &mut visit, &mut passed)?;
    while edits.next_new(None, &mut |s| write_new(s))? {}
    out.borrow_mut().flush()?;
    Ok(generation)
}

/// The row of an old file as version 3 says it.
fn converted<'a>(id: u32, row: &Captured, scratch: &'a [u8]) -> Spec<'a> {
    let mut spec = Spec::new(id, row.kind, row.start, row.end);
    spec.percent = row.percent;
    spec.created = row.created;
    spec.updated = row.updated;
    spec.conversion = row.conversion;
    spec.start_xpointer = row.slots[3].part(scratch).text;
    spec.end_xpointer = row.slots[4].part(scratch).text;
    spec.text = row.slots[0].part(scratch).text;
    spec.note = row.slots[1].part(scratch).text;
    spec
}

#[cfg(test)]
#[path = "write_tests.rs"]
mod tests;
