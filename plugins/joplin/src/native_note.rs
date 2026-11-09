//! A note made from a notes file, and the files of a sync target read for
//! what matters: the same rules as interface 6 (`item.rs`, `markdown.rs`) over
//! [`Volume`]s, every step fallible, and no more held than a few pieces.
//!
//! - [`read_props`] reads an item's properties from the end of its file, at
//!   most [`TAIL`] bytes, and [`text_hash`] the MD5 of its text in pieces.
//! - [`head`] reads the title and author at the start of a notes file.
//! - [`write_note`] writes the note to a file: the title, the body copied
//!   piece by piece from the notes (all but the title line and the blank lines
//!   around, which a Joplin note has as its title), the properties; returns
//!   the MD5 of its text.
use alloc::{string::String, vec::Vec};

use paperpaw_sdk::digest::md5::Md5;
use paperpaw_sdk::native::{Error, storage::MAX_DATA};
use paperpaw_ui_kit::files::Volume;

use crate::native_joplin::{self as joplin, HEAD, Head, Note, Props, TAIL};

/// The pieces a body is copied in.
const PIECE: usize = 1024;
/// The most line breaks held back at the end of a piece, until text follows
/// them. A body that has more in a row at its end keeps the first of them
/// only (a note ends in text, not in thousands of blank lines).
const MAX_HELD: usize = 4096;

/// The properties of the item in `path`, from its end: only the end of the
/// file is read, however long its body.
///
/// # Errors
/// `NoMemory`; what the card says.
pub fn read_props(v: &dyn Volume, path: &str) -> Result<Props, Error> {
    let size = v.size(path)?;
    let from = size.saturating_sub(TAIL as u64);
    let mut tail = [0u8; TAIL];
    let n = v.read_at(path, from, &mut tail)?;
    joplin::item_props(size, tail.get(..n).unwrap_or(&[]))
}

/// The MD5 of an item's text, the first `end` bytes of the file `path` (see
/// [`Props::text_end`]), read in pieces: whether it was edited in Joplin since
/// PaperPaw wrote it, whatever Joplin did with its times.
///
/// # Errors
/// `NoMemory`; what the card says.
pub fn text_hash(v: &dyn Volume, path: &str, end: u64) -> Result<String, Error> {
    let mut md5 = Md5::new();
    let mut piece = [0u8; PIECE];
    let mut at = 0u64;
    while at < end {
        let want = usize::try_from(end - at).map_or(PIECE, |w| w.min(PIECE));
        let n = v.read_at(path, at, piece.get_mut(..want).unwrap_or(&mut []))?;
        if n == 0 {
            break;
        }
        md5.update(piece.get(..n).unwrap_or(&[]));
        at += n as u64;
    }
    let mut digits = [0u8; 32];
    paperpaw_ui_kit::text::copy(md5.hex_into(&mut digits))
}

/// The title and author at the start of the notes file `path`.
///
/// # Errors
/// `NoMemory`; what the card says.
pub fn head(v: &dyn Volume, path: &str) -> Result<Head, Error> {
    let mut start = [0u8; HEAD];
    let n = v.read_at(path, 0, &mut start)?;
    joplin::head_of(start.get(..n).unwrap_or(&[]))
}

/// Where a body being copied is.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Part {
    /// Before its first text: the title line and blank lines are left out.
    #[default]
    Start,
    /// After a `#` at the start: a title if a blank follows.
    Hash,
    /// In the title line.
    Title,
    Text,
}

/// A body copied piece by piece: what comes before is left out, the line
/// breaks at the end are held back until text follows them.
#[derive(Debug, Default)]
pub struct Body {
    part: Part,
    held: Vec<u8>,
    /// A `#` that turned out to be text, still to write.
    hash: bool,
    written: bool,
}

impl Body {
    /// Whether any of the body was handed on.
    #[must_use]
    pub const fn written(&self) -> bool {
        self.written
    }

    /// Hands on what of `piece` goes into the note: `out` gets it in one or
    /// more parts.
    ///
    /// # Errors
    /// `NoMemory`, or what `out` says.
    pub fn feed(
        &mut self,
        mut piece: &[u8],
        out: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
    ) -> Result<(), Error> {
        while self.part != Part::Text {
            let Some((&first, rest)) = piece.split_first() else {
                return Ok(());
            };
            match (self.part, first) {
                (Part::Start, b'\n' | b'\r' | b' ' | b'\t') => piece = rest,
                (Part::Start, b'#') => {
                    self.part = Part::Hash;
                    piece = rest;
                }
                (Part::Start, _) => self.part = Part::Text,
                (Part::Hash, b' ') => self.part = Part::Title,
                (Part::Hash, _) => {
                    // Text that starts with `#`, e.g. a `##` heading.
                    self.part = Part::Text;
                    self.hash = true;
                }
                (Part::Title, b'\n') => {
                    self.part = Part::Start;
                    piece = rest;
                }
                (_, _) => piece = rest,
            }
        }
        let text = piece
            .iter()
            .rposition(|b| !matches!(b, b'\n' | b'\r'))
            .map_or(0, |last| last + 1);
        if core::mem::take(&mut self.hash) {
            out(b"#")?;
            self.written = true;
        }
        if text > 0 {
            if !self.held.is_empty() {
                out(&self.held)?;
                self.held.clear();
            }
            out(piece.get(..text).unwrap_or(&[]))?;
            self.written = true;
        }
        let tail = piece.get(text..).unwrap_or(&[]);
        let room = MAX_HELD.saturating_sub(self.held.len()).min(tail.len());
        self.held.try_reserve(room).map_err(|_| Error::NoMemory)?;
        self.held.extend_from_slice(tail.get(..room).unwrap_or(&[]));
        Ok(())
    }
}

/// Hands on the body of the notes file `from` in pieces (see [`Body`]).
/// Returns whether there was any.
///
/// # Errors
/// `NoMemory`; what the card says, or `out`.
pub fn copy_body(
    notes: &dyn Volume,
    from: &str,
    out: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
) -> Result<bool, Error> {
    let mut piece = [0u8; PIECE];
    let mut at = 0u64;
    let mut body = Body::default();
    loop {
        let n = notes.read_at(from, at, &mut piece)?;
        if n == 0 {
            return Ok(body.written);
        }
        at += n as u64;
        body.feed(piece.get(..n).unwrap_or(&[]), out)?;
    }
}

/// Appends `bytes` to `to` in calls of at most [`MAX_DATA`].
fn append(data: &dyn Volume, to: &str, bytes: &[u8]) -> Result<(), Error> {
    for part in bytes.chunks(MAX_DATA) {
        data.append(to, part)?;
    }
    Ok(())
}

/// Writes the note to the file `to` in the data folder, its body the reader's
/// notes in the file `from` on the card (see [`copy_body`]), copied piece by
/// piece. Returns the MD5 of its text (see [`text_hash`]).
///
/// # Errors
/// `NoMemory`; what the card says (`to` is left as far as it was written).
pub fn write_note(
    note: &Note<'_>,
    notes: &dyn Volume,
    from: &str,
    data: &dyn Volume,
    to: &str,
) -> Result<String, Error> {
    let mut title = String::new();
    joplin::one_line_into(&mut title, note.title)?;
    let mut md5 = Md5::new();
    md5.update(title.as_bytes());
    let mut start = String::new();
    start
        .try_reserve_exact(title.len() + 2)
        .map_err(|_| Error::NoMemory)?;
    start.push_str(&title);
    start.push_str("\n\n");
    if start.len() > MAX_DATA {
        return Err(Error::NoSpace);
    }
    data.write(to, start.as_bytes())?;
    let mut first = true;
    let body = copy_body(notes, from, &mut |piece| {
        if core::mem::take(&mut first) {
            md5.update(b"\n\n");
        }
        md5.update(piece);
        append(data, to, piece)
    })?;
    if body {
        data.append(to, b"\n\n")?;
    }
    append(data, to, note.props()?.as_bytes())?;
    let mut digits = [0u8; 32];
    paperpaw_ui_kit::text::copy(md5.hex_into(&mut digits))
}
