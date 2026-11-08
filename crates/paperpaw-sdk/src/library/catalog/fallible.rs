//! The catalog and its log read without an infallible allocation and in
//! about 1.5 KiB of stack at most (`each_head`: a 512-byte chunk and 520
//! bytes of line head; `try_path_at` adds one more head). Lines are looked
//! at by their first bytes only (a catalog line starts with the book's
//! path, at most 512 bytes; a log line is short), so no line is ever held
//! whole. A device short of memory gets [`Error::OutOfMemory`] instead of
//! an abort, and nothing is returned half read.

use alloc::string::String;

use super::{Changes, MAX_CHANGES, parse_change};
use crate::data::Error;
use crate::library::DIR;
use crate::{Storage, StorageError};
use paperpaw_digest::fnv1a64;

/// The bytes of a line looked at: the longest path (512) and a tab.
const HEAD: usize = 520;

/// Calls `visit` with where each line starts and its first [`HEAD`]
/// bytes, until it says `false`. A file that is not there has no lines.
pub(crate) fn each_head(
    storage: &dyn Storage,
    path: &str,
    from: u64,
    visit: &mut dyn FnMut(u64, &[u8]) -> bool,
) -> Result<(), StorageError> {
    let mut chunk = [0_u8; 512];
    let mut head = [0_u8; HEAD];
    let (mut kept, mut seen) = (0, 0_u64);
    let (mut start, mut offset) = (from, from);
    loop {
        let n = match storage.read_at(path, offset, &mut chunk) {
            Ok(n) => n,
            Err(StorageError::NotFound) if offset == from => return Ok(()),
            Err(e) => return Err(e),
        };
        if n == 0 {
            break;
        }
        offset += n as u64;
        for &byte in &chunk[..n] {
            seen += 1;
            if byte == b'\n' {
                if !visit(start, &head[..kept]) {
                    return Ok(());
                }
                (kept, start) = (0, from + seen);
            } else if kept < HEAD {
                head[kept] = byte;
                kept += 1;
            }
        }
    }
    if kept > 0 {
        visit(start, &head[..kept]);
    }
    Ok(())
}

/// The path column of a catalog line head: none for the header, a line
/// that is not text, or a path that is cut (longer than a path may be).
fn path_of(head: &[u8]) -> Option<&str> {
    let end = head.iter().position(|&b| b == b'\t').unwrap_or(head.len());
    let path = core::str::from_utf8(head.get(..end)?).ok()?;
    (!path.is_empty() && end < HEAD).then_some(path)
}

/// [`super::changes`] with the memory error: the log of status changes
/// noted beside the catalog; none where there is no log.
///
/// # Errors
/// [`Error::OutOfMemory`], or the storage's error where the log is there
/// but cannot be read.
pub fn try_changes(storage: &dyn Storage) -> Result<Changes, Error> {
    let log = log_path_try()?;
    let mut changes = Changes::default();
    let mut count = 0_usize;
    each_head(storage, &log, 0, &mut |_, _| {
        count += 1;
        true
    })?;
    changes
        .entries
        .try_reserve_exact(count.min(MAX_CHANGES))
        .map_err(|_| Error::OutOfMemory)?;
    each_head(storage, &log, 0, &mut |_, head| {
        if let Some((hash, change)) = core::str::from_utf8(head)
            .ok()
            .and_then(|line| parse_change(line.trim_end()))
        {
            changes.note(hash, change);
        }
        true
    })?;
    Ok(changes)
}

/// `<DIR>/<name>`, reserved fallibly (`log_path`, `catalog_path` allocate
/// without a way to say no).
fn in_library(name: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(DIR.len() + 1 + name.len())
        .map_err(|_| Error::OutOfMemory)?;
    out.push_str(DIR);
    out.push('/');
    out.push_str(name);
    Ok(out)
}

fn log_path_try() -> Result<String, Error> {
    in_library("status.log")
}

fn catalog_path_try() -> Result<String, Error> {
    in_library("catalog.txt")
}

/// `text` as a new `String`, reserved fallibly.
fn copy(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(text.len())
        .map_err(|_| Error::OutOfMemory)?;
    out.push_str(text);
    Ok(out)
}

/// How many books the catalog holds, as noted last (those removed left
/// out): [`each_at`](super::each_at)'s books counted, none held.
///
/// # Errors
/// As [`try_changes`], and the storage's error where the catalog cannot
/// be read.
pub fn try_count(storage: &dyn Storage) -> Result<u32, Error> {
    let changes = try_changes(storage)?;
    let file = catalog_path_try()?;
    let mut books = 0_u32;
    each_head(storage, &file, 0, &mut |_, head| {
        if let Some(path) = path_of(head)
            && !changes.removed(fnv1a64(path))
        {
            books = books.saturating_add(1);
        }
        true
    })?;
    Ok(books)
}

/// The path of the book whose line starts at `offset` in the catalog (as
/// [`each_at`](super::each_at) told): `None` for no line there or a book
/// removed.
///
/// # Errors
/// As [`try_count`].
pub fn try_path_at(storage: &dyn Storage, offset: u64) -> Result<Option<String>, Error> {
    let changes = try_changes(storage)?;
    let file = catalog_path_try()?;
    let mut found: Option<&str> = None;
    let mut line = [0_u8; HEAD];
    let mut length = 0;
    each_head(storage, &file, offset, &mut |_, head| {
        line[..head.len()].copy_from_slice(head);
        length = head.len();
        false
    })?;
    if let Some(path) = path_of(&line[..length])
        && !changes.removed(fnv1a64(path))
    {
        found = Some(path);
    }
    found.map(copy).transpose()
}
