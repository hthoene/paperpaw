//! One catalog line seen without holding it: a book's path and numbers read
//! into fixed buffers on the stack (a line can be 16 KiB), with the whole
//! lengths of its texts and the counts of the tags and fields; a text
//! itself (title, author, series, a tag, a field) is read from a byte on
//! into the caller's buffer by [`try_text_at`], so no frame here holds
//! more than the path and one chunk (about 0.9 KiB). The memory error is
//! [`Error::OutOfMemory`].

use super::fallible::{each_head, in_library, path_of, try_changes};
use super::{Changes, State, Status};
use crate::data::Error;
use crate::{Storage, StorageError};
use paperpaw_digest::fnv1a64;

/// The longest path of a book; a longer one is no book of the library.
pub const VIEW_PATH: usize = 512;
/// Bytes of a number in the series kept.
pub const VIEW_NUMBER: usize = 32;
/// Bytes of a catalog line read at most (a value is a kilobyte at most);
/// a longer line is no book.
pub const LINE_MAX: usize = 16 * 1024;

/// A book of the catalog as noted last. The number is cut at a character
/// boundary at [`VIEW_NUMBER`]; the `_bytes` fields are the whole lengths
/// of the texts [`try_text_at`] reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct View<'a> {
    pub path: &'a str,
    pub number: &'a str,
    pub author_bytes: u32,
    pub title_bytes: u32,
    pub series_bytes: u32,
    /// How many tags and fields the book has.
    pub tags: u32,
    pub fields: u32,
    pub status: Status,
    pub added: Option<u32>,
    pub size: Option<u64>,
}

/// A column being read: its bytes up to a cap, and its whole length.
struct Column<const N: usize> {
    bytes: [u8; N],
    kept: usize,
    whole: u32,
}

impl<const N: usize> Column<N> {
    const fn new() -> Self {
        Self {
            bytes: [0; N],
            kept: 0,
            whole: 0,
        }
    }

    fn push(&mut self, byte: u8) {
        if self.kept < N {
            self.bytes[self.kept] = byte;
            self.kept += 1;
        }
        self.whole = self.whole.saturating_add(1);
    }

    /// The text kept: valid UTF-8 only, cut at a character boundary.
    fn text(&self) -> &str {
        let bytes = &self.bytes[..self.kept];
        match core::str::from_utf8(bytes) {
            Ok(text) => text,
            Err(e) => core::str::from_utf8(&bytes[..e.valid_up_to()]).unwrap_or_default(),
        }
    }

    /// The whole text, if it is valid UTF-8 and was not cut.
    fn strict(&self) -> Option<&str> {
        if self.whole as usize > N {
            return None;
        }
        core::str::from_utf8(&self.bytes[..self.kept]).ok()
    }

    fn number<T: core::str::FromStr>(&self) -> Option<T> {
        self.text().parse().ok()
    }
}

/// A long text being read into the caller's buffer: its bytes up to the
/// buffer's size, and its whole length.
struct Sink<'a> {
    out: &'a mut [u8],
    filled: usize,
    whole: u32,
}

impl<'a> Sink<'a> {
    fn new(out: &'a mut [u8]) -> Self {
        Self {
            out,
            filled: 0,
            whole: 0,
        }
    }

    fn push(&mut self, byte: u8) {
        if let Some(slot) = self.out.get_mut(self.filled) {
            *slot = byte;
            self.filled += 1;
        }
        self.whole = self.whole.saturating_add(1);
    }
}

/// Buffers for the three long texts of a book's line, read in the same scan
/// as the line's numbers ([`try_book_at_with`]); a text longer than its
/// buffer is cut (its whole length is in [`View`]).
#[derive(Debug)]
#[non_exhaustive]
pub struct BookTexts<'a> {
    pub title: &'a mut [u8],
    pub author: &'a mut [u8],
    pub series: &'a mut [u8],
}

impl<'a> BookTexts<'a> {
    /// The buffers of the title, the author and the series.
    pub fn new(title: &'a mut [u8], author: &'a mut [u8], series: &'a mut [u8]) -> Self {
        Self {
            title,
            author,
            series,
        }
    }
}

/// How many bytes of each text are in its buffer after [`try_book_at_with`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Filled {
    pub title: usize,
    pub author: usize,
    pub series: usize,
}

/// The columns of a line, in the order the catalog writes them.
struct Columns<'a> {
    path: Column<VIEW_PATH>,
    author: Sink<'a>,
    title: Sink<'a>,
    series: Sink<'a>,
    number: Column<VIEW_NUMBER>,
    progress: Column<4>,
    flags: Column<4>,
    added: Column<12>,
    size: Column<24>,
    tags: u32,
    fields: u32,
    // Whether the tags (fields) part being read has a byte.
    tag_open: bool,
    field_open: bool,
}

impl Columns<'_> {
    /// Takes the byte of column `index` (a tab ends one).
    fn take(&mut self, index: usize, byte: u8) {
        const SEPARATOR: u8 = 0x1f;
        match index {
            0 => self.path.push(byte),
            1 => self.author.push(byte),
            2 => self.title.push(byte),
            3 => self.series.push(byte),
            4 => self.number.push(byte),
            5 | 9 => {
                let (open, count) = if index == 5 {
                    (&mut self.tag_open, &mut self.tags)
                } else {
                    (&mut self.field_open, &mut self.fields)
                };
                if byte == SEPARATOR {
                    *count = count.saturating_add(u32::from(core::mem::take(open)));
                } else {
                    *open = true;
                }
            }
            6 => self.progress.push(byte),
            7 => self.flags.push(byte),
            8 => self.added.push(byte),
            10 => self.size.push(byte),
            _ => {}
        }
    }

    fn finish(&mut self) {
        self.tags = self
            .tags
            .saturating_add(u32::from(core::mem::take(&mut self.tag_open)));
        self.fields = self
            .fields
            .saturating_add(u32::from(core::mem::take(&mut self.field_open)));
    }
}

/// Calls `take` with the column and the byte of each byte of the line that
/// starts at `offset` (a tab ends a column): `false` where the catalog is
/// not there.
fn scan(
    storage: &dyn Storage,
    file: &str,
    offset: u64,
    take: &mut dyn FnMut(usize, u8),
) -> Result<bool, Error> {
    let (mut index, mut at) = (0_usize, offset);
    let mut chunk = [0_u8; 256];
    loop {
        if at - offset > LINE_MAX as u64 {
            // No catalog line is this long: not a book.
            return Ok(false);
        }
        let n = match storage.read_at(file, at, &mut chunk) {
            Ok(n) => n,
            Err(StorageError::NotFound) => return Ok(false),
            Err(e) => return Err(e.into()),
        };
        if n == 0 {
            return Ok(true);
        }
        at += n as u64;
        for &byte in &chunk[..n] {
            match byte {
                b'\n' => return Ok(true),
                b'\t' => index += 1,
                byte => take(index, byte),
            }
        }
    }
}

/// The book whose line starts at `offset` in the catalog (as
/// [`each_at`](super::each_at) told), as noted last, seen by `visit`:
/// `Ok(false)` for no line there, a header, or a book removed.
///
/// # Errors
/// [`Error::OutOfMemory`] (the log of changes is held while the line is
/// read) or the storage's error.
pub fn try_view_at(
    storage: &dyn Storage,
    offset: u64,
    visit: &mut dyn FnMut(&View<'_>),
) -> Result<bool, Error> {
    try_view_at_with(storage, &try_changes(storage)?, offset, visit)
}

/// [`try_view_at`] with the log of changes the caller already read
/// ([`try_changes`]), so that a call that looks at one book several times
/// reads the log once.
///
/// # Errors
/// As [`try_view_at`], less the log.
pub fn try_view_at_with(
    storage: &dyn Storage,
    changes: &Changes,
    offset: u64,
    visit: &mut dyn FnMut(&View<'_>),
) -> Result<bool, Error> {
    try_book_at_with(
        storage,
        changes,
        offset,
        BookTexts::new(&mut [], &mut [], &mut []),
        &mut |view, _| visit(view),
    )
}

/// [`try_view_at_with`] that also reads the title, the author and the series
/// into `texts` in the same scan of the line (a line is read from the card
/// once, not once for the numbers and once for each text). `visit` also gets
/// how much of each text is in its buffer; the bytes may end inside a
/// character, which the caller cuts.
///
/// # Errors
/// As [`try_view_at_with`].
pub fn try_book_at_with(
    storage: &dyn Storage,
    changes: &Changes,
    offset: u64,
    texts: BookTexts<'_>,
    visit: &mut dyn FnMut(&View<'_>, Filled),
) -> Result<bool, Error> {
    let file = in_library("catalog.txt")?;
    let mut columns = Columns {
        path: Column::new(),
        author: Sink::new(texts.author),
        title: Sink::new(texts.title),
        series: Sink::new(texts.series),
        number: Column::new(),
        progress: Column::new(),
        flags: Column::new(),
        added: Column::new(),
        size: Column::new(),
        tags: 0,
        fields: 0,
        tag_open: false,
        field_open: false,
    };
    if !scan(storage, &file, offset, &mut |index, byte| {
        columns.take(index, byte)
    })? {
        return Ok(false);
    }
    columns.finish();
    // A path cut at its cap, or not text, is no path of the library: it is
    // refused, never cut and passed.
    let Some(path) = columns.path.strict().filter(|path| !path.is_empty()) else {
        return Ok(false);
    };
    let hash = fnv1a64(path);
    if changes.removed(hash) {
        return Ok(false);
    }
    let mut status = Status::parse(columns.progress.text(), columns.flags.text());
    if let Some(noted) = changes.status(hash) {
        status = noted;
    }
    let filled = Filled {
        title: columns.title.filled,
        author: columns.author.filled,
        series: columns.series.filled,
    };
    visit(
        &View {
            path,
            number: columns.number.text(),
            author_bytes: columns.author.whole,
            title_bytes: columns.title.whole,
            series_bytes: columns.series.whole,
            tags: columns.tags,
            fields: columns.fields,
            status,
            added: columns.added.number(),
            size: columns.size.number(),
        },
        filled,
    );
    Ok(true)
}

/// Where the line of the book at `path` starts in the catalog, if the
/// catalog has it and it was not removed.
///
/// # Errors
/// As [`try_view_at`].
pub fn try_offset_of(storage: &dyn Storage, path: &str) -> Result<Option<u64>, Error> {
    try_offset_of_with(storage, &try_changes(storage)?, path)
}

/// [`try_offset_of`] with the log of changes the caller already read.
///
/// # Errors
/// As [`try_offset_of`], less the log.
pub fn try_offset_of_with(
    storage: &dyn Storage,
    changes: &Changes,
    path: &str,
) -> Result<Option<u64>, Error> {
    if changes.removed(fnv1a64(path)) {
        return Ok(None);
    }
    let file = in_library("catalog.txt")?;
    let mut found = None;
    each_head(storage, &file, 0, &mut |offset, head| {
        if path_of(head) == Some(path) {
            found = Some(offset);
            return false;
        }
        true
    })?;
    Ok(found)
}

/// [`state`](super::state) with the memory error.
///
/// # Errors
/// [`Error::OutOfMemory`].
pub fn try_state(storage: &dyn Storage) -> Result<State, Error> {
    let (catalog, log) = (in_library("catalog.txt")?, in_library("status.log")?);
    match storage.size(&catalog) {
        Ok(size) => {
            let log = storage.size(&log).unwrap_or(0);
            Ok(State::Ready {
                stamp: (size << 20) ^ log,
            })
        }
        Err(_) => {
            let building = in_library("catalog.tmp")?;
            Ok(if storage.exists(&building) {
                State::Building
            } else {
                State::Missing
            })
        }
    }
}

/// [`generation`](super::generation) with the memory error: `None`
/// without a catalog.
///
/// # Errors
/// [`Error::OutOfMemory`].
pub fn try_generation(storage: &dyn Storage) -> Result<Option<u64>, Error> {
    let file = in_library("catalog.txt")?;
    let mut buf = [0_u8; 48];
    let Ok(n) = storage.read_at(&file, 0, &mut buf) else {
        return Ok(None);
    };
    let line = &buf[..n];
    let line = &line[..line.iter().position(|&b| b == b'\n').unwrap_or(n)];
    let text = core::str::from_utf8(line).unwrap_or("");
    let Some(rest) = text.strip_prefix(super::HEADER) else {
        return Ok(Some(0));
    };
    Ok(Some(
        rest.trim_start_matches('\t').trim().parse().unwrap_or(0),
    ))
}

/// A text of a book's catalog line that can be longer than a [`View`]
/// keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextColumn {
    Author,
    Title,
    Series,
    /// The `part`-th tag.
    Tag,
    /// The `part`-th in-catalog field (`id=value`).
    Field,
}

/// What [`try_text_at`] read: the text's whole length and how many bytes
/// of it are in the buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Text {
    pub total: u32,
    pub filled: usize,
}

/// The text `column` (its `part`-th item for a tag or a field, else 0) of
/// the book whose line starts at `offset`, from byte `from` on into `out`:
/// `None` where there is no such book or item. The bytes may begin or end
/// inside a character; the caller cuts them.
///
/// # Errors
/// As [`try_view_at`].
pub fn try_text_at(
    storage: &dyn Storage,
    offset: u64,
    column: TextColumn,
    part: u32,
    from: u32,
    out: &mut [u8],
) -> Result<Option<Text>, Error> {
    try_text_at_with(
        storage,
        &try_changes(storage)?,
        offset,
        column,
        part,
        from,
        out,
    )
}

/// [`try_text_at`] with the log of changes the caller already read.
///
/// # Errors
/// As [`try_text_at`], less the log.
pub fn try_text_at_with(
    storage: &dyn Storage,
    changes: &Changes,
    offset: u64,
    column: TextColumn,
    part: u32,
    from: u32,
    out: &mut [u8],
) -> Result<Option<Text>, Error> {
    const SEPARATOR: u8 = 0x1f;
    let file = in_library("catalog.txt")?;
    let wanted = match column {
        TextColumn::Author => 1,
        TextColumn::Title => 2,
        TextColumn::Series => 3,
        TextColumn::Tag => 5,
        TextColumn::Field => 9,
    };
    let list = wanted == 5 || wanted == 9;
    if !list && part != 0 {
        return Ok(None);
    }
    let mut path = Column::<VIEW_PATH>::new();
    // Non-empty items started in the wanted column, and bytes of the item.
    let (mut items, mut open) = (0_u32, false);
    let (mut total, mut filled) = (0_u32, 0_usize);
    let mut exists = !list;
    let found = scan(storage, &file, offset, &mut |index, byte| {
        if index == 0 {
            path.push(byte);
        } else if index == wanted {
            if list {
                if byte == SEPARATOR {
                    open = false;
                    return;
                }
                if !open {
                    open = true;
                    items = items.saturating_add(1);
                }
                if items != part.saturating_add(1) {
                    return;
                }
                exists = true;
            }
            if total >= from && filled < out.len() {
                out[filled] = byte;
                filled += 1;
            }
            total = total.saturating_add(1);
        }
    })?;
    let Some(path) = path.strict().filter(|path| !path.is_empty()) else {
        return Ok(None);
    };
    if !found || !exists || changes.removed(fnv1a64(path)) {
        return Ok(None);
    }
    Ok(Some(Text { total, filled }))
}
