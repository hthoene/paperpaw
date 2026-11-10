//! A book's annotation file read a line at a time without holding it
//! (a line is up to 8 KiB; the file up to a megabyte): the host's reads
//! of `core.library` ops 15, 16 and 4 (kinds 5 and 6) use it, and no
//! allocation is made here. The caller lends the scratch the texts are
//! read into; a text, a note, a client key or an XPointer is read from
//! a byte on, up to a cap ([`Window`]), with its whole length.
//!
//! **What a row is** (one definition, shared with the writer of version 3):
//! a line that does not start with `# `, is at most [`LINE_MAX`] bytes,
//! starts with a kind letter (an ASCII letter: `b`, `h`, or another kind's
//! own) and whose numbers parse. Its columns are by their number: a line
//! of 13 fields (version 2) has the conversion, the start and end
//! XPointers, the text and the note after the times; of 10, the times, the
//! text and the note; of fewer, the text and the note after the progress.
//! Version 3 has exactly 15, the two more being the annotation's id (a
//! decimal `u32`, at least 1) and its client key (at most 64 bytes). Lines
//! that are not rows have no id and are never returned. In a file of
//! version 1 or 2 a row's id is its 1-based place among the rows in file
//! order; reads never write.
//!
//! **Generation:** the header line `# generation G` of a version 3 file; a
//! missing file, version 1 and 2 read as 1, and 0 never occurs
//! ([`generation_of`], [`next_generation`]): the number the service carries.

use crate::data::Error;
use crate::document::Locator;
use crate::{Storage, StorageError};
use core::ops::ControlFlow;

use super::Kind;

/// The longest line a row is read from; a longer line is no row.
pub const LINE_MAX: usize = 8 * 1024;
/// The version with ids, client keys and the generation.
pub const FILE_VERSION: u32 = 3;
/// Longest client key, in bytes.
pub const KEY_MAX: usize = 64;

const TAB: u8 = b'\t';
const MARK: &str = crate::data::MARK;

/// What the file says before its lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Head {
    /// The file's version: 1 where it has no mark, and for no file at all (a
    /// missing file reads as an empty one of version 1).
    pub version: u32,
    /// The committed changes of version 3 (`# generation`), 1 where it has
    /// none ([`generation_of`]).
    pub generation: u32,
}

/// The generation of a file whose header says `header` (`# generation G`,
/// `None` without one) and which is of `version`: a missing file, a file of
/// version 1 or 2 and a header of 0 or 1 all read as 1, the generation of a
/// file no version 3 writer ever wrote; 0 never occurs. It is the number the
/// service carries unchanged (`end.generation`, `edit_begin`, `lib_committed`).
pub const fn generation_of(version: u32, header: Option<u32>) -> u32 {
    match header {
        Some(generation) if version >= FILE_VERSION && generation >= 2 => generation,
        _ => 1,
    }
}

/// The generation after a committed change of a file of `generation`: one
/// more, past `u32::MAX` 2 again, so that neither 0 nor 1 is ever reached by
/// a change. The first write of a file of version 1 or 2 (generation 1) or of
/// no file stores 2 for the conversion or creation, and the change itself
/// makes it 3.
pub const fn next_generation(generation: u32) -> u32 {
    match generation.checked_add(1) {
        Some(next) if next >= 2 => next,
        _ => 2,
    }
}

/// How much of a column is read: `cap` bytes from byte `from` on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Window {
    pub from: u32,
    pub cap: u32,
}

impl Window {
    pub const fn new(from: u32, cap: u32) -> Self {
        Self { from, cap }
    }
    /// A column of which only the whole length is wanted.
    pub const NONE: Window = Window::new(0, 0);
}

/// The windows of the five texts of a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Windows {
    pub text: Window,
    pub note: Window,
    pub key: Window,
    pub start_xpointer: Window,
    pub end_xpointer: Window,
}

impl Windows {
    /// No column read, only their whole lengths.
    pub const fn new() -> Self {
        let none = Window::NONE;
        Self {
            text: none,
            note: none,
            key: none,
            start_xpointer: none,
            end_xpointer: none,
        }
    }
    #[must_use]
    pub const fn text(mut self, window: Window) -> Self {
        self.text = window;
        self
    }
    #[must_use]
    pub const fn note(mut self, window: Window) -> Self {
        self.note = window;
        self
    }
    #[must_use]
    pub const fn key(mut self, window: Window) -> Self {
        self.key = window;
        self
    }
    /// The windows of the start and the end XPointer.
    #[must_use]
    pub const fn xpointers(mut self, start: Window, end: Window) -> Self {
        self.start_xpointer = start;
        self.end_xpointer = end;
        self
    }
    /// The bytes of scratch they need; `None` where the caps do not add up
    /// in a `usize` (32-bit targets), which no scratch can hold.
    #[must_use]
    pub fn scratch(&self) -> Option<usize> {
        checked_sum(self.caps().into_iter().map(|cap| cap as usize))
    }

    fn caps(&self) -> [u32; 5] {
        [
            self.text.cap,
            self.note.cap,
            self.key.cap,
            self.start_xpointer.cap,
            self.end_xpointer.cap,
        ]
    }
}

/// The sum of `sizes`, `None` on overflow.
fn checked_sum(mut sizes: impl Iterator<Item = usize>) -> Option<usize> {
    sizes.try_fold(0_usize, usize::checked_add)
}

/// A text read through a window: what was read (a character cut at its
/// end is left out) and the text's whole length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Part<'a> {
    pub text: &'a str,
    pub whole: u32,
}

/// One annotation of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Row<'a> {
    /// Its id (see the module's note for older files).
    pub id: u32,
    pub kind: Kind,
    pub start: Locator,
    pub end: Locator,
    pub percent: u8,
    pub created: i64,
    pub updated: i64,
    /// The conversion its offsets are in, where it is known.
    pub conversion: Option<u64>,
    pub text: Part<'a>,
    pub note: Part<'a>,
    pub key: Part<'a>,
    pub start_xpointer: Part<'a>,
    pub end_xpointer: Part<'a>,
}

/// A field of a line, for its number of fields.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Kind,
    Number(usize),
    Created,
    Updated,
    Conversion,
    StartXpointer,
    EndXpointer,
    Text,
    Note,
    Id,
    Key,
    Ignored,
}

/// Where the fields of a line of `fields` fields are, in a file of
/// `version`; `None` for a shape that is no row.
fn field_role(index: usize, fields: usize, version: u32) -> Option<Role> {
    if version >= FILE_VERSION && fields != 15 {
        return None;
    }
    Some(match (index, fields) {
        (0, _) => Role::Kind,
        (1..=4, _) => Role::Number(index - 1),
        (5, _) => Role::Number(4),
        (6, 10 | 13 | 15) => Role::Created,
        (7, 10 | 13 | 15) => Role::Updated,
        (8, 13 | 15) => Role::Conversion,
        (9, 13 | 15) => Role::StartXpointer,
        (10, 13 | 15) => Role::EndXpointer,
        (11, 13 | 15) | (8, 10) | (6, _) => Role::Text,
        (12, 13 | 15) | (9, 10) | (7, _) => Role::Note,
        (13, 15) => Role::Id,
        (14, 15) => Role::Key,
        _ => Role::Ignored,
    })
}

/// A number being read.
#[derive(Clone, Copy)]
struct Number {
    bytes: [u8; 24],
    len: usize,
    over: bool,
}

impl Number {
    const EMPTY: Number = Number {
        bytes: [0; 24],
        len: 0,
        over: false,
    };
    fn push(&mut self, byte: u8) {
        if self.len < self.bytes.len() {
            self.bytes[self.len] = byte;
            self.len += 1;
        } else {
            self.over = true;
        }
    }
    fn text(&self) -> Option<&str> {
        if self.over {
            return None;
        }
        core::str::from_utf8(&self.bytes[..self.len]).ok()
    }
    fn parse<T: core::str::FromStr>(&self) -> Option<T> {
        self.text()?.parse().ok()
    }
}

/// A text column being read through its window.
#[derive(Clone, Copy, Default)]
pub(super) struct Slot {
    window: Window,
    /// Where its bytes lie in the scratch.
    at: usize,
    filled: usize,
    whole: u32,
}

impl Slot {
    fn push(&mut self, byte: u8, scratch: &mut [u8]) {
        if self.whole >= self.window.from && self.filled < self.window.cap as usize {
            scratch[self.at + self.filled] = byte;
            self.filled += 1;
        }
        self.whole = self.whole.saturating_add(1);
    }
    pub(super) fn part<'a>(&self, scratch: &'a [u8]) -> Part<'a> {
        let bytes = &scratch[self.at..self.at + self.filled];
        let text = match core::str::from_utf8(bytes) {
            Ok(text) => text,
            Err(e) => core::str::from_utf8(&bytes[..e.valid_up_to()]).unwrap_or_default(),
        };
        Part {
            text,
            whole: self.whole,
        }
    }
}

/// The file's chunk reader.
struct Reader<'s> {
    storage: &'s dyn Storage,
    file: &'s str,
    chunk: [u8; 256],
    at: u64,
    len: usize,
    used: usize,
    done: bool,
}

impl<'s> Reader<'s> {
    fn new(storage: &'s dyn Storage, file: &'s str, at: u64) -> Self {
        Self {
            storage,
            file,
            chunk: [0; 256],
            at,
            len: 0,
            used: 0,
            done: false,
        }
    }

    /// The next byte of the file, `None` at its end.
    fn next(&mut self) -> Result<Option<u8>, Error> {
        if self.used == self.len {
            if self.done {
                return Ok(None);
            }
            let n = match self.storage.read_at(self.file, self.at, &mut self.chunk) {
                Ok(n) => n,
                Err(StorageError::NotFound) => 0,
                Err(StorageError::OutOfMemory) => return Err(Error::OutOfMemory),
                Err(e) => return Err(e.into()),
            };
            if n == 0 {
                self.done = true;
                return Ok(None);
            }
            self.at += n as u64;
            self.len = n;
            self.used = 0;
        }
        let byte = self.chunk[self.used];
        self.used += 1;
        Ok(Some(byte))
    }

    /// Where the next byte is in the file.
    fn position(&self) -> u64 {
        self.at - (self.len - self.used) as u64
    }
}

pub(super) fn file_of(book: &str) -> Result<alloc::string::String, Error> {
    let mut digits = [0_u8; crate::library::KEY_LEN];
    let key = crate::library::key_into(book, &mut digits);
    let mut name = alloc::string::String::new();
    name.try_reserve_exact(
        crate::data::DIR.len() + "/annotations/".len() + key.len() + ".txt".len(),
    )
    .map_err(|_| Error::OutOfMemory)?;
    for part in [crate::data::DIR, "/annotations/", key, ".txt"] {
        name.push_str(part);
    }
    Ok(name)
}

/// What a header line `# ...` says: the version and the generation.
fn header(line: &[u8], head: &mut Head, generation: &mut Option<u32>) {
    let Ok(line) = core::str::from_utf8(line) else {
        return;
    };
    if let Some(n) = line.strip_prefix(MARK) {
        head.version = n.trim().parse().unwrap_or(1);
    } else if let Some(g) = line.strip_prefix("# generation ") {
        *generation = g.trim().parse().ok();
    }
}

/// Reads the head of the file: its version and generation. A file of a later
/// version than this build reads is [`Error::Newer`].
///
/// # Errors
/// [`Error::Newer`], [`Error::OutOfMemory`] (the file's name) or the
/// storage's.
pub fn try_head(storage: &dyn Storage, book: &str) -> Result<Head, Error> {
    head_of(storage, &file_of(book)?)
}

pub(super) fn head_of(storage: &dyn Storage, file: &str) -> Result<Head, Error> {
    let mut head = Head {
        version: 0,
        generation: 1,
    };
    let mut generation = None;
    let mut reader = Reader::new(storage, file, 0);
    // The header lines come first; at most this many are looked at.
    for _ in 0..8 {
        let (mut line, mut len, mut any) = ([0_u8; 48], 0_usize, false);
        loop {
            match reader.next()? {
                Some(b'\n') | None => break,
                Some(byte) => {
                    any = true;
                    if len < line.len() {
                        line[len] = byte;
                        len += 1;
                    }
                }
            }
        }
        if head.version == 0 && (any || reader.done) {
            // A file without a mark is version 1.
            head.version = 1;
        }
        let text = line[..len]
            .strip_prefix(b"\xEF\xBB\xBF")
            .unwrap_or(&line[..len]);
        if !text.starts_with(b"# ") {
            break;
        }
        header(text, &mut head, &mut generation);
    }
    if head.version > FILE_VERSION {
        return Err(Error::Newer(head.version));
    }
    head.generation = generation_of(head.version, generation);
    Ok(head)
}

/// Calls `visit` with each row of the file, in file order, with its place
/// (0-based among the rows) and the texts read through `windows` into
/// `scratch` (at least [`Windows::scratch`] bytes). `visit` may stop the read.
/// Returns how many rows were seen.
///
/// Nothing is allocated; a file that is not there has no rows.
///
/// # Errors
/// [`Error::Newer`] for a file of a later version; a scratch too small is
/// [`Error::Unreadable`]; [`Error::OutOfMemory`] (the file's name); the
/// storage's errors.
pub fn try_rows(
    storage: &dyn Storage,
    book: &str,
    windows: &Windows,
    scratch: &mut [u8],
    visit: &mut dyn FnMut(u32, &Row<'_>) -> ControlFlow<()>,
) -> Result<u32, Error> {
    // Windows no scratch can hold are refused before anything is read.
    if windows.scratch().is_none_or(|need| scratch.len() < need) {
        return Err(Error::Unreadable);
    }
    let file = file_of(book)?;
    let head = head_of(storage, &file)?;
    let mut reader = Reader::new(storage, &file, 0);
    let mut seen = 0_u32;
    // Lines are read twice: the first to count the fields (the role of a
    // field depends on how many there are), the second for the fields.
    loop {
        let start = reader.position();
        let Some(shape) = measure_shape(&mut reader, head.version)? else {
            break;
        };
        let Shape::Line { fields } = shape else {
            continue;
        };
        let mut again = Reader::new(storage, &file, start);
        let mut next = || again.next();
        let Some(row) = capture(&mut next, fields, head.version, windows, scratch)? else {
            continue;
        };
        seen = seen.saturating_add(1);
        let id = if head.version >= FILE_VERSION {
            row.id
        } else {
            seen
        };
        let (parts, slots) = (row.slots, &*scratch);
        let view = Row {
            id,
            kind: row.kind,
            start: row.start,
            end: row.end,
            percent: row.percent,
            created: row.created,
            updated: row.updated,
            conversion: row.conversion,
            text: parts[0].part(slots),
            note: parts[1].part(slots),
            key: parts[2].part(slots),
            start_xpointer: parts[3].part(slots),
            end_xpointer: parts[4].part(slots),
        };
        if visit(seen - 1, &view).is_break() {
            break;
        }
    }
    Ok(seen)
}

enum Shape {
    /// A line of this many fields (as the version splits it).
    Line { fields: usize },
    /// A header, an empty or too long line: no row.
    Other,
}

/// Reads one line: how many fields it has, or what it is instead. `None` at
/// the end of the file.
fn measure_shape(reader: &mut Reader<'_>, version: u32) -> Result<Option<Shape>, Error> {
    let most = match version {
        0..=1 => 10,
        2 => 13,
        _ => 15,
    };
    let (mut tabs, mut length, mut comment) = (0_usize, 0_usize, false);
    let mut head = [0_u8; 2];
    loop {
        let Some(byte) = reader.next()? else {
            return Ok((length > 0).then(|| line_shape(tabs, length, comment, most)));
        };
        if byte == b'\n' {
            return Ok(Some(line_shape(tabs, length, comment, most)));
        }
        if length < head.len() {
            head[length] = byte;
            comment = length + 1 == head.len() && head == *b"# ";
        }
        if byte == TAB {
            tabs += 1;
        }
        length += 1;
    }
}

fn line_shape(tabs: usize, length: usize, comment: bool, most: usize) -> Shape {
    if comment || length == 0 || length > LINE_MAX {
        return Shape::Other;
    }
    Shape::Line {
        fields: (tabs + 1).min(most),
    }
}

/// What `capture` keeps of a row besides the slots.
pub(super) struct Captured {
    pub(super) id: u32,
    pub(super) kind: Kind,
    pub(super) start: Locator,
    pub(super) end: Locator,
    pub(super) percent: u8,
    pub(super) created: i64,
    pub(super) updated: i64,
    pub(super) conversion: Option<u64>,
    pub(super) slots: [Slot; 5],
}

/// The fields of a line as they are taken, byte by byte.
struct Fields {
    kind: u8,
    kind_len: usize,
    numbers: [Number; 5],
    created: Number,
    updated: Number,
    conversion: Number,
    id: Number,
    slots: [Slot; 5],
}

impl Fields {
    /// The fields of a line to be read through `windows`; `None` for caps that
    /// do not add up in a `usize`.
    fn new(windows: &Windows) -> Option<Self> {
        let order = [
            windows.text,
            windows.note,
            windows.key,
            windows.start_xpointer,
            windows.end_xpointer,
        ];
        let mut slots = [Slot::default(); 5];
        let mut at = 0_usize;
        for (slot, window) in slots.iter_mut().zip(order) {
            *slot = Slot {
                window,
                at,
                filled: 0,
                whole: 0,
            };
            at = at.checked_add(window.cap as usize)?;
        }
        Some(Self {
            kind: 0,
            kind_len: 0,
            numbers: [Number::EMPTY; 5],
            created: Number::EMPTY,
            updated: Number::EMPTY,
            conversion: Number::EMPTY,
            id: Number::EMPTY,
            slots,
        })
    }

    fn take(&mut self, role: Role, byte: u8, scratch: &mut [u8]) {
        match role {
            Role::Kind => {
                self.kind = byte;
                self.kind_len += 1;
            }
            Role::Number(n) => self.numbers[n].push(byte),
            Role::Created => self.created.push(byte),
            Role::Updated => self.updated.push(byte),
            Role::Conversion => self.conversion.push(byte),
            Role::Id => self.id.push(byte),
            Role::Text => self.slots[0].push(byte, scratch),
            Role::Note => self.slots[1].push(byte, scratch),
            Role::Key => self.slots[2].push(byte, scratch),
            Role::StartXpointer => self.slots[3].push(byte, scratch),
            Role::EndXpointer => self.slots[4].push(byte, scratch),
            Role::Ignored => {}
        }
    }

    /// The row the fields make, if they make one.
    fn finish(self, fields: usize, version: u32) -> Option<Captured> {
        // A kind is one ASCII letter.
        if self.kind_len != 1 || !self.kind.is_ascii_alphabetic() {
            return None;
        }
        let kind = match self.kind {
            b'b' => Kind::Bookmark,
            b'h' => Kind::Highlight,
            letter => Kind::Other(u16::from(letter)),
        };
        let n = |i: usize| self.numbers[i].parse::<u32>();
        let (a, b, c, d) = (n(0)?, n(1)?, n(2)?, n(3)?);
        let percent = self.numbers[4].parse::<i64>()?;
        let (created, updated) = if matches!(fields, 10 | 13 | 15) {
            (self.created.parse::<i64>()?, self.updated.parse::<i64>()?)
        } else {
            (0, 0)
        };
        let id = if version >= FILE_VERSION {
            self.id.parse::<u32>().filter(|id| *id >= 1)?
        } else {
            0
        };
        // A key past its bound is no key; an XPointer end without a start is
        // damage.
        if self.slots[2].whole as usize > KEY_MAX
            || (self.slots[3].whole == 0 && self.slots[4].whole != 0)
        {
            return None;
        }
        let conversion = if matches!(fields, 13 | 15) {
            self.conversion
                .text()
                .filter(|c| c.len() == 16)
                .and_then(|c| u64::from_str_radix(c, 16).ok())
        } else {
            None
        };
        Some(Captured {
            id,
            kind,
            start: Locator::new(a, b),
            end: Locator::new(c, d),
            percent: percent.clamp(0, 100) as u8,
            created,
            updated,
            conversion,
            slots: self.slots,
        })
    }
}

/// Reads the line `next` is at the start of, of `fields` fields.
fn capture(
    next: &mut dyn FnMut() -> Result<Option<u8>, Error>,
    fields: usize,
    version: u32,
    windows: &Windows,
    scratch: &mut [u8],
) -> Result<Option<Captured>, Error> {
    let mut line = Fields::new(windows).ok_or(Error::Unreadable)?;
    let mut index = 0_usize;
    while let Some(byte) = next()? {
        if byte == b'\n' {
            break;
        }
        if byte == TAB && index + 1 < fields {
            index += 1;
            continue;
        }
        let Some(role) = field_role(index, fields, version) else {
            return Ok(None);
        };
        line.take(role, byte, scratch);
    }
    Ok(line.finish(fields, version))
}

/// The row `line` (without its newline) is in a file of `version`, its texts
/// read through `windows` into `scratch`; `None` where it is no row. The
/// writer of version 3 reads the lines it rewrites with this.
pub(super) fn parse_line(
    line: &[u8],
    version: u32,
    windows: &Windows,
    scratch: &mut [u8],
) -> Option<Captured> {
    let most = match version {
        0..=1 => 10,
        2 => 13,
        _ => 15,
    };
    let tabs = line.iter().filter(|b| **b == TAB).count();
    let comment = line.starts_with(b"# ");
    let Shape::Line { fields } = line_shape(tabs, line.len(), comment, most) else {
        return None;
    };
    let mut bytes = line.iter().copied();
    let mut next = || Ok(bytes.next());
    capture(&mut next, fields, version, windows, scratch)
        .ok()
        .flatten()
}

/// Whether `line` (without its newline) is a row in a file of `version`: the
/// one definition of a row. [`try_rows`] reads by it, and **the writer of
/// version 3 must call this** to decide which lines of a file it rewrites
/// carry an id (everything else it copies as it is), so that an id is
/// never given to a line the reader does not count.
#[must_use]
pub fn is_row(line: &[u8], version: u32) -> bool {
    let most = match version {
        0..=1 => 10,
        2 => 13,
        _ => 15,
    };
    let tabs = line.iter().filter(|b| **b == TAB).count();
    let comment = line.starts_with(b"# ");
    let Shape::Line { fields } = line_shape(tabs, line.len(), comment, most) else {
        return false;
    };
    let mut bytes = line.iter().copied();
    let mut next = || Ok(bytes.next());
    // No window: nothing is kept, only the shape and the numbers are judged.
    matches!(
        capture(&mut next, fields, version, &Windows::new(), &mut []),
        Ok(Some(_))
    )
}

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
