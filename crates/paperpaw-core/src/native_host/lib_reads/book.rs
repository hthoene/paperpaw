//! Ops 3 `book` and 4 `text`: one book's line, whole texts by offsets.
use super::{CatalogReads, fail, valid_prefix, zeroed};
use crate::native_host::lib_service::{BookRef, One};
use crate::native_host::registrations::{self as wire, Record};
use paperpaw_native::host_glue::{
    ERR_INVALID, ERR_IO, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_UNSUPPORTED,
};
use paperpaw_platform::StorageError;
use paperpaw_sdk::library::{self, catalog};

/// Bytes of the long forms `lib_book` carries.
const PATH: usize = 512;
const TITLE: usize = 512;
const AUTHOR: usize = 512;
const SERIES: usize = 256;
const NUMBER: usize = 32;
const FORMAT: usize = 128;
/// Bytes of a `lib_text`.
const TEXT: usize = 1024;
/// A book is new for this many days after it was added.
const NEW_DAYS: u32 = 30;
/// `lib_book.progress` of a book never opened.
const NEVER_OPENED: u32 = 255;

mod flag {
    pub(super) const FAVOURITE: u32 = 1;
    pub(super) const OPENED: u32 = 1 << 1;
    pub(super) const FINISHED: u32 = 1 << 2;
    pub(super) const NEW: u32 = 1 << 3;
    pub(super) const INDEXED: u32 = 1 << 4;
    /// Title, author, series cut: bits 5-7.
    pub(super) const CUT: [u32; 3] = [1 << 5, 1 << 6, 1 << 7];
    /// The book's file is not on the card now (`lib_book` only).
    pub(super) const MISSING: u32 = 1 << 8;
}

/// What of a line `book` keeps beyond the path and the number (copied into
/// the buffer): the numbers.
#[derive(Clone, Copy, Default)]
struct Line {
    path: usize,
    number: usize,
    author_bytes: u32,
    title_bytes: u32,
    series_bytes: u32,
    tags: u32,
    fields: u32,
    status: catalog::Status,
    added: Option<u32>,
    size: Option<u64>,
}

/// The line of the book, its path and number in `path` and `number`.
fn line_of(
    reads: &CatalogReads,
    offset: u64,
    path: &mut [u8],
    number: &mut [u8],
) -> Result<Line, i32> {
    let mut line = None;
    let seen = catalog::try_view_at(&*reads.storage, offset, &mut |view| {
        let (p, n) = (view.path.as_bytes(), view.number.as_bytes());
        if p.len() <= path.len() && n.len() <= number.len() {
            path[..p.len()].copy_from_slice(p);
            number[..n.len()].copy_from_slice(n);
            line = Some(Line {
                path: p.len(),
                number: n.len(),
                author_bytes: view.author_bytes,
                title_bytes: view.title_bytes,
                series_bytes: view.series_bytes,
                tags: view.tags,
                fields: view.fields,
                status: view.status,
                added: view.added,
                size: view.size,
            });
        }
    })
    .map_err(fail)?;
    if seen {
        line.ok_or(ERR_NO_SPACE)
    } else {
        Err(ERR_NOT_FOUND)
    }
}

/// A text of the line read into `out`: its valid prefix and whole length.
fn text_of<'o>(
    reads: &CatalogReads,
    offset: u64,
    column: catalog::TextColumn,
    out: &'o mut [u8],
) -> Result<&'o str, i32> {
    let read = catalog::try_text_at(&*reads.storage, offset, column, 0, 0, out)
        .map_err(fail)?
        .ok_or(ERR_NOT_FOUND)?;
    Ok(valid_prefix(&out[..read.filled]))
}

/// The length of `text` as the wire counts.
fn bytes(text: &str) -> u32 {
    u32::try_from(text.len()).unwrap_or(u32::MAX)
}

pub(super) fn book(reads: &CatalogReads, book: BookRef<'_>, visit: One<'_>) -> Result<(), i32> {
    let offset = reads.locate(book)?;
    let key = u32::try_from(offset).map_err(|_| ERR_NO_SPACE)?;
    let generation = reads.generation()?;
    let mut buffer = zeroed(PATH + TITLE + AUTHOR + SERIES + NUMBER + FORMAT)?;
    let (path_b, rest) = buffer.split_at_mut(PATH);
    let (title_b, rest) = rest.split_at_mut(TITLE);
    let (author_b, rest) = rest.split_at_mut(AUTHOR);
    let (series_b, rest) = rest.split_at_mut(SERIES);
    let (number_b, format_b) = rest.split_at_mut(NUMBER);
    let line = line_of(reads, offset, path_b, number_b)?;
    let path = core::str::from_utf8(&path_b[..line.path]).map_err(|_| ERR_INVALID)?;
    let number = core::str::from_utf8(&number_b[..line.number]).map_err(|_| ERR_INVALID)?;
    let title = text_of(reads, offset, catalog::TextColumn::Title, title_b)?;
    let author = text_of(reads, offset, catalog::TextColumn::Author, author_b)?;
    let series = text_of(reads, offset, catalog::TextColumn::Series, series_b)?;
    let mut format_len = None;
    reads.env.format_id(path, &mut |id| {
        if id.len() <= format_b.len() {
            format_b[..id.len()].copy_from_slice(id.as_bytes());
            format_len = Some(id.len());
        }
    });
    let format = match format_len {
        Some(n) => core::str::from_utf8(&format_b[..n]).map_err(|_| ERR_INVALID)?,
        None => "",
    };
    let indexed = indexed(reads, path, line.size)?;
    let missing = file_is_gone(reads, path)?;
    let status = line.status;
    let opened = status.opened || status.progress.is_some();
    let mut flags = 0;
    for (set, bit) in [
        (status.favourite, flag::FAVOURITE),
        (opened, flag::OPENED),
        (status.progress == Some(100), flag::FINISHED),
        (is_new(reads, line.added, opened), flag::NEW),
        (indexed, flag::INDEXED),
        (missing, flag::MISSING),
        (bytes(title) < line.title_bytes, flag::CUT[0]),
        (bytes(author) < line.author_bytes, flag::CUT[1]),
        (bytes(series) < line.series_bytes, flag::CUT[2]),
    ] {
        if set {
            flags |= bit;
        }
    }
    visit(Record::ServiceLibBook(wire::ServiceLibBook {
        key,
        generation,
        path,
        title,
        author,
        series,
        number,
        progress: status.progress.map_or(NEVER_OPENED, u32::from),
        flags,
        added: line.added.unwrap_or(0),
        size: line.size.unwrap_or(0),
        format,
        tags: line.tags,
        fields: line.fields,
        title_bytes: line.title_bytes,
        author_bytes: line.author_bytes,
        series_bytes: line.series_bytes,
    }));
    Ok(())
}

/// Whether the book's file is gone from the card (`lib_book.flags` bit 8).
///
/// In turn, stopping at the first that finds it:
/// 1. `size` of the path (a file);
/// 2. `modified` of the path, which the FAT storage answers for a directory
///    too, by looking up its entry and without walking the folder (a folder
///    book of thousands of pictures costs one lookup); `exists` is
///    `size().is_ok()` by default and cannot see a directory;
/// 3. only where a storage does not tell (`None`), a `list_from` that stops
///    at the first entry. FAT reads a folder whole in `list_from` (only the
///    visitor stops), so this is the fallback, not the check.
///
/// A path the storage cannot name (`InvalidPath`) is a book that is not
/// there. A storage that fails (a card not mounted, a failing card) is
/// `ERR_IO`, never "gone". Before a book is called gone the storage must
/// show a root (`list_from("")`): where it does not (the books' storage is
/// `NoStorage`, the catalog being on another storage that still answers) the
/// answer is `ERR_IO` (`ERR_NO_MEMORY` if the probe itself is refused
/// memory), or every book would read as removed. Limit: a router
/// that shows a live root over an empty mount cannot be told from a deleted
/// top folder through `Storage`.
///
/// Allocation: the host's own is none. A FAT storage's directory walk (the
/// `size` lookup's included) allocates inside rust-fatfs, which the host
/// cannot refuse.
///
/// Cost: one lookup per `book` call (two for a folder), three or four for a
/// book that is gone; unmeasured on the X4. A list page does not call this
/// per row (it has `lib_entry` rows; op 3 is for the one book the plugin
/// shows).
fn file_is_gone(reads: &CatalogReads, path: &str) -> Result<bool, i32> {
    let nothing =
        |error: StorageError| matches!(error, StorageError::NotFound | StorageError::InvalidPath);
    match reads.storage.size(path) {
        Ok(_) => return Ok(false),
        Err(error) if nothing(error) => {}
        Err(error) => return Err(fail(error.into())),
    }
    if reads.storage.modified(path).is_some() {
        return Ok(false);
    }
    match reads.storage.list_from(path, 0, &mut |_| false) {
        Ok(_) => return Ok(false),
        Err(error) if nothing(error) => {}
        Err(error) => return Err(fail(error.into())),
    }
    // Not found by any: gone, but only on a storage that is there at all.
    // No root is `ERR_IO`; a probe that ran out of memory is not that.
    match reads.storage.list_from("", 0, &mut |_| false) {
        Ok(_) => Ok(true),
        Err(StorageError::OutOfMemory) => Err(ERR_NO_MEMORY),
        Err(_) => Err(ERR_IO),
    }
}

/// Whether the library read title, author and cover from the book (its
/// details say so); a file the details do not belong to is not indexed.
fn indexed(reads: &CatalogReads, path: &str, size: Option<u64>) -> Result<bool, i32> {
    let size = match size {
        Some(size) => size,
        None => match reads.storage.size(path) {
            Ok(size) => size,
            Err(_) => return Ok(false),
        },
    };
    library::try_indexed(&*reads.storage, path, size).map_err(fail)
}

/// Added within the last [`NEW_DAYS`] days and not opened.
fn is_new(reads: &CatalogReads, added: Option<u32>, opened: bool) -> bool {
    match (added, reads.env.today()) {
        (Some(added), Some(today)) => !opened && today >= added && today - added <= NEW_DAYS,
        _ => false,
    }
}

pub(super) fn text(
    reads: &CatalogReads,
    book: BookRef<'_>,
    at: &wire::ServiceLibTextAt,
    visit: One<'_>,
) -> Result<(), i32> {
    use catalog::TextColumn::{Author, Field, Series, Tag, Title};
    let column = match at.kind {
        0 => Title,
        1 => Author,
        2 => Series,
        3 => Tag,
        4 => Field,
        // The annotation's own text and note: its file.
        5 | 6 => {
            return reads.with_path(book, |path| {
                super::annotations::text(&*reads.storage, path, at, visit)
            });
        }
        _ => return Err(ERR_UNSUPPORTED),
    };
    let offset = reads.locate(book)?;
    let mut buffer = zeroed(TEXT).map_err(|_| ERR_NO_MEMORY)?;
    let read = catalog::try_text_at(
        &*reads.storage,
        offset,
        column,
        at.index,
        at.offset,
        &mut buffer,
    )
    .map_err(fail)?
    .ok_or(ERR_NOT_FOUND)?;
    if at.offset > read.total {
        return Err(ERR_INVALID);
    }
    let found = &buffer[..read.filled];
    // The text starts at a character: an offset inside one is the caller's.
    if found.first().is_some_and(|b| b & 0xc0 == 0x80) {
        return Err(ERR_INVALID);
    }
    let shown = valid_prefix(found);
    // Stored bytes that are not text, where text is left: an empty answer
    // would send the caller on from the same offset for ever.
    if shown.is_empty() && read.total > at.offset {
        return Err(ERR_INVALID);
    }
    visit(Record::ServiceLibText(wire::ServiceLibText {
        kind: at.kind,
        index: at.index,
        total: read.total,
        text: shown,
    }));
    Ok(())
}
