//! What the library knows of one book, read without an infallible
//! allocation: every `String` and `Vec` here is reserved with
//! `try_reserve`, so a device short of memory gets
//! [`Error::OutOfMemory`](crate::data::Error::OutOfMemory) instead of an
//! abort, and nothing is returned half read. The plain forms
//! ([`book`](super::book), [`key`](super::key)) are these forms with the
//! error left out (or, for the key, an abort as before).

use alloc::collections::TryReserveError;
use alloc::string::String;
use alloc::vec::Vec;

use super::{Book, DETAILS, DETAILS_DIR, DIR, MAX_DETAILS, OLD_DETAILS_DIR, config, fnv1a64};
use crate::Storage;
use crate::data::Error;
use crate::document::ImageRef;

/// The digits of a [`key`](super::key).
pub const KEY_LEN: usize = 16;

/// The book key of `path` (16 hexadecimal digits, [`key`](super::key))
/// written into `out`, without allocating.
pub fn key_into<'o>(path: &str, out: &'o mut [u8; KEY_LEN]) -> &'o str {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let hash = fnv1a64(path);
    for (i, digit) in out.iter_mut().enumerate() {
        *digit = HEX[((hash >> (60 - 4 * i)) & 0xf) as usize];
    }
    // The digits are ASCII.
    core::str::from_utf8(out).unwrap_or_default()
}

/// [`key`](super::key) with the memory error instead of an abort.
///
/// # Errors
/// [`TryReserveError`] where the 16 bytes cannot be had.
pub fn try_key(path: &str) -> Result<String, TryReserveError> {
    let mut digits = [0_u8; KEY_LEN];
    copy(key_into(path, &mut digits))
}

/// The result of a `try_` read for the plain forms, which have always
/// stopped the program where there is no memory: the book, `None` where
/// the details could not be read, and the allocation failure handler
/// (`handle_alloc_error`) where memory was short, never `None`: a caller
/// takes `None` for a book the library does not know and may write or
/// extract it again.
pub(super) fn surviving<T>(result: Result<T, Error>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(Error::OutOfMemory | Error::Storage(crate::StorageError::OutOfMemory)) => {
            alloc::alloc::handle_alloc_error(core::alloc::Layout::new::<Book>())
        }
        Err(_) => None,
    }
}

/// `text` as a new `String`, reserved fallibly.
fn copy(text: &str) -> Result<String, TryReserveError> {
    join(&[text])
}

/// `parts` one after the other as a new `String`, reserved fallibly once.
fn join(parts: &[&str]) -> Result<String, TryReserveError> {
    let mut out = String::new();
    out.try_reserve_exact(parts.iter().map(|part| part.len()).sum())?;
    for part in parts {
        out.push_str(part);
    }
    Ok(out)
}

/// Where the details of the book at `path` are kept (`book_file`).
pub(super) fn try_book_file(path: &str) -> Result<String, TryReserveError> {
    let mut digits = [0_u8; KEY_LEN];
    let key = key_into(path, &mut digits);
    let part = key.get(..2).unwrap_or("00");
    join(&[DIR, "/", DETAILS_DIR, "/", part, "/", key, ".ini"])
}

/// Where earlier versions kept them (`old_book_file`).
pub(super) fn try_old_book_file(path: &str) -> Result<String, TryReserveError> {
    let mut digits = [0_u8; KEY_LEN];
    join(&[
        DIR,
        "/",
        OLD_DETAILS_DIR,
        "/",
        key_into(path, &mut digits),
        ".ini",
    ])
}

/// The details file text of the book at `path`: the file of this
/// version's place, else the old one.
pub(super) fn try_load_details(storage: &dyn Storage, path: &str) -> Result<Option<String>, Error> {
    let format = crate::data::Format::FIRST;
    let file = try_book_file(path).map_err(|_| Error::OutOfMemory)?;
    match format.try_load(storage, &file, MAX_DETAILS)? {
        Some(details) => Ok(Some(details)),
        None => {
            drop(file);
            let old = try_old_book_file(path).map_err(|_| Error::OutOfMemory)?;
            format.try_load(storage, &old, MAX_DETAILS)
        }
    }
}

/// `text` owned: what the entry's value is, with escapes resolved.
fn own(value: &config::Value<'_>) -> Result<String, TryReserveError> {
    match value.try_as_str()? {
        alloc::borrow::Cow::Owned(text) => Ok(text),
        alloc::borrow::Cow::Borrowed(text) => copy(text),
    }
}

fn push<T>(list: &mut Vec<T>, item: T) -> Result<(), TryReserveError> {
    list.try_reserve(1)?;
    list.push(item);
    Ok(())
}

/// The details in `source` as a [`Book`]; the memory error leaves no
/// book.
pub(super) fn try_parse_details(source: &str) -> Result<Book, TryReserveError> {
    let mut book = Book::default();
    let (mut cover_path, mut cover_width, mut cover_height) = (None, None, None);
    let mut version = 1;
    for entry in config::parse(source).filter_map(Result::ok) {
        match (entry.section, entry.key) {
            ("field", key) => push(&mut book.fields, (copy(key)?, own(&entry.value)?))?,
            (_, "size") => book.size = entry.value.raw().parse().unwrap_or_default(),
            (_, "indexed") => book.indexed = entry.value.as_bool().unwrap_or_default(),
            (_, "title") => book.title = Some(own(&entry.value)?),
            (_, "author") => book.author = Some(own(&entry.value)?),
            (_, "series") => book.series = Some(own(&entry.value)?),
            (_, "series-index") => book.series_index = Some(own(&entry.value)?),
            (_, key) if key.starts_with("tag.") => push(&mut book.tags, own(&entry.value)?)?,
            (_, "version") => version = entry.value.as_u32().unwrap_or(1),
            (_, "cover") => cover_path = Some(own(&entry.value)?),
            (_, "cover-width") => cover_width = entry.value.as_u32(),
            (_, "cover-height") => cover_height = entry.value.as_u32(),
            (_, "progress") => book.progress = entry.value.as_u32().map(|p| p.min(100) as u8),
            (_, "added") => book.added = entry.value.as_u32(),
            _ => {}
        }
    }
    if let (Some(path), Some(width), Some(height)) = (cover_path, cover_width, cover_height) {
        book.cover = Some(ImageRef {
            path,
            width,
            height,
        });
    }
    // Details of an older kind are read again.
    book.indexed &= version == DETAILS;
    Ok(book)
}

/// What the library knows of the book at `path`, if it belongs to this
/// file ([`book`](super::book)), with the memory error.
///
/// # Errors
/// [`Error::OutOfMemory`] where the details or the book could not be had
/// in memory; the other [`Error`]s where the details could not be read.
pub fn try_book(storage: &dyn Storage, path: &str) -> Result<Option<Book>, Error> {
    let Ok(size) = storage.size(path) else {
        return Ok(None);
    };
    try_book_sized(storage, path, size)
}

/// [`try_book`] for a file whose size is known
/// ([`book_sized`](super::book_sized)).
///
/// # Errors
/// As [`try_book`].
pub fn try_book_sized(storage: &dyn Storage, path: &str, size: u64) -> Result<Option<Book>, Error> {
    let Some(source) = try_load_details(storage, path)? else {
        return Ok(None);
    };
    let book = try_parse_details(&source).map_err(|_| Error::OutOfMemory)?;
    Ok((book.size == size).then_some(book))
}

/// Whether the list file `file` (the books opened, the favourites: one
/// path a line, at most `max`) holds `path`; the file is looked through a
/// piece at a time and none is held. `TooLarge` where it is longer than a
/// list of `max` paths can be.
fn list_has(storage: &dyn Storage, file: &str, max: usize, path: &str) -> Result<bool, Error> {
    match storage.size(file) {
        Ok(size) if size > (max * 512) as u64 => {
            return Err(Error::Storage(crate::StorageError::TooLarge));
        }
        Ok(_) => {}
        Err(crate::StorageError::NotFound) => return Ok(false),
        Err(e) => return Err(Error::Storage(e)),
    }
    let mut has = false;
    let mut first = true;
    super::catalog::each_head(storage, file, 0, &mut |_, head| {
        let head = if core::mem::take(&mut first) {
            head.strip_prefix("\u{feff}".as_bytes()).unwrap_or(head)
        } else {
            head
        };
        let head = head.strip_suffix(b"\r").unwrap_or(head);
        has = !head.is_empty() && head == path.as_bytes();
        !has
    })?;
    Ok(has)
}

/// The size and progress a details file `source` says, without a book.
fn progress_in(source: &str) -> (u64, Option<u8>) {
    let (mut size, mut progress) = (0, None);
    for entry in config::parse(source).filter_map(Result::ok) {
        match (entry.section, entry.key) {
            ("field", _) => {}
            (_, "size") => size = entry.value.raw().parse().unwrap_or_default(),
            (_, "progress") => progress = entry.value.as_u32().map(|p| p.min(100) as u8),
            _ => {}
        }
    }
    (size, progress)
}

/// [`status_of`](super::status_of) with the memory error: how far the book
/// at `path` is read (its details, when they are of this file), whether it
/// is a favourite and whether it was opened.
///
/// # Errors
/// [`Error::OutOfMemory`]; the storage's error where the book is gone or a
/// list or the details cannot be read.
pub fn try_status(storage: &dyn Storage, path: &str) -> Result<super::catalog::Status, Error> {
    let size = storage.size(path)?;
    let progress = match try_load_details(storage, path)? {
        Some(source) => {
            let (kept, progress) = progress_in(&source);
            if kept == size { progress } else { None }
        }
        None => None,
    };
    let mut file = String::new();
    file.try_reserve_exact(DIR.len() + "/favourites.txt".len())
        .map_err(|_| Error::OutOfMemory)?;
    file.push_str(DIR);
    file.push_str("/favourites.txt");
    let favourite = list_has(storage, &file, super::MAX_FAVOURITES, path)?;
    file.truncate(DIR.len());
    file.push_str("/opened.txt");
    let opened = list_has(storage, &file, super::MAX_OPENED, path)?;
    Ok(super::catalog::Status::new(progress, favourite, opened))
}
