//! One catalog line seen without holding it: a book's columns read into
//! fixed buffers on the stack (a line can be 16 KiB, a title is cut at
//! 512 bytes), with the full lengths of what was cut and the counts of the
//! tags and fields. The memory error is [`Error::OutOfMemory`].

use super::fallible::{each_head, in_library, path_of, try_changes};
use super::{Changes, State, Status};
use crate::data::Error;
use crate::{Storage, StorageError};
use paperpaw_digest::fnv1a64;

/// The longest path, title and author, series, number kept by a [`View`].
pub const VIEW_PATH: usize = 512;
/// Bytes of a title or an author kept.
pub const VIEW_TEXT: usize = 512;
/// Bytes of a series kept.
pub const VIEW_SERIES: usize = 256;
/// Bytes of a number in the series kept.
pub const VIEW_NUMBER: usize = 32;

/// A book of the catalog as noted last. Texts are cut at a character
/// boundary at their caps; the `_bytes` fields are the whole lengths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct View<'a> {
    pub path: &'a str,
    pub author: &'a str,
    pub title: &'a str,
    pub series: &'a str,
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

    fn number<T: core::str::FromStr>(&self) -> Option<T> {
        self.text().parse().ok()
    }
}

/// The columns of a line, in the order the catalog writes them.
struct Columns {
    path: Column<VIEW_PATH>,
    author: Column<VIEW_TEXT>,
    title: Column<VIEW_TEXT>,
    series: Column<VIEW_SERIES>,
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

impl Columns {
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
                    *count += u32::from(core::mem::take(open));
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
        self.tags += u32::from(core::mem::take(&mut self.tag_open));
        self.fields += u32::from(core::mem::take(&mut self.field_open));
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
    let changes = try_changes(storage)?;
    let file = in_library("catalog.txt")?;
    let mut columns = Columns {
        path: Column::new(),
        author: Column::new(),
        title: Column::new(),
        series: Column::new(),
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
    let (mut index, mut at, mut done) = (0_usize, offset, false);
    let mut chunk = [0_u8; 256];
    while !done {
        let n = match storage.read_at(&file, at, &mut chunk) {
            Ok(n) => n,
            Err(StorageError::NotFound) => return Ok(false),
            Err(e) => return Err(e.into()),
        };
        if n == 0 {
            break;
        }
        at += n as u64;
        for &byte in &chunk[..n] {
            match byte {
                b'\n' => {
                    done = true;
                    break;
                }
                b'\t' => index += 1,
                byte => columns.take(index, byte),
            }
        }
    }
    columns.finish();
    // A path cut at its cap is no path of the library.
    if columns.path.whole as usize > VIEW_PATH || columns.path.kept == 0 {
        return Ok(false);
    }
    let path = columns.path.text();
    let hash = fnv1a64(path);
    if changes.removed(hash) {
        return Ok(false);
    }
    let mut status = Status::parse(columns.progress.text(), columns.flags.text());
    if let Some(noted) = changes.status(hash) {
        status = noted;
    }
    visit(&View {
        path,
        author: columns.author.text(),
        title: columns.title.text(),
        series: columns.series.text(),
        number: columns.number.text(),
        author_bytes: columns.author.whole,
        title_bytes: columns.title.whole,
        series_bytes: columns.series.whole,
        tags: columns.tags,
        fields: columns.fields,
        status,
        added: columns.added.number(),
        size: columns.size.number(),
    });
    Ok(true)
}

/// Where the line of the book at `path` starts in the catalog, if the
/// catalog has it and it was not removed.
///
/// # Errors
/// As [`try_view_at`].
pub fn try_offset_of(storage: &dyn Storage, path: &str) -> Result<Option<u64>, Error> {
    let changes: Changes = try_changes(storage)?;
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
