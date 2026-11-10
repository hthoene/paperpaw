//! [`try_moved`] and [`try_removed`]: what the library knows of a book
//! carried along or forgotten, with every card error the caller can
//! retry returned instead of dropped, for a host step that must fail and
//! run again (a power cut, a full card). An error no retry changes (a
//! path the card refuses, damaged data) does not stop the other parts:
//! they are still carried, and that error is returned at the end.

use alloc::collections::TryReserveError;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::{
    MAX_FAVOURITES, MAX_OPENED, book_file, cover_file, favourites_file, key, old_book_file,
    old_cover_variant_file, opened_file, try_book, write_details,
};
use crate::data::{Error, Format};
use crate::{Storage, StorageError};

/// [`moved`](super::moved) that returns the first card error instead of
/// going on without it: the details and the cover kept for the book
/// (also from where an earlier version kept them), the cover path the
/// details name, and its place among the recently opened and the
/// favourites. Call it once the book itself is at `to`: the cover path is
/// rewritten in the details of the book found there, and nothing is
/// rewritten where none is.
///
/// What is not there counts as done ([`StorageError::NotFound`] is never
/// returned), so a call repeated after an error, or after a power cut,
/// finishes the move. A list that is there but cannot be read (written by
/// a newer PaperPaw, or damaged) is not written over and is left out, as
/// [`moved`](super::moved) does: no retry would change it; so are details
/// of a newer PaperPaw. The lists and the details are read and written
/// without an infallible allocation (a list is up to 500 paths), so a
/// device short of memory gets [`StorageError::OutOfMemory`] to retry;
/// only paths of a few hundred bytes are allocated as everywhere.
///
/// # Errors
/// The first rename, remove, read or write error that a retry may cure
/// ([`StorageError::Io`], [`StorageError::Full`],
/// [`StorageError::OutOfMemory`] or one this version does not know): the
/// call stops there, and what was done before it stays done. Else the
/// first error no retry changes ([`StorageError::InvalidPath`],
/// [`StorageError::Unsupported`], [`StorageError::InvalidData`],
/// [`StorageError::TooLarge`]), returned once every other part was
/// carried: the part it hit stays where it was. [`StorageError::NotFound`]
/// is never returned.
pub fn try_moved(storage: &dyn Storage, from: &str, to: &str) -> Result<(), StorageError> {
    let mut left = Left::default();
    for (file, old) in [
        (
            book_file as fn(&str) -> String,
            old_book_file as fn(&str) -> String,
        ),
        (cover_file, |path| old_cover_variant_file(path, "")),
    ] {
        // The old place is tried only where nothing is at the current one;
        // after a permanent error there an older copy must not be moved
        // over `to`.
        let moved = match left.step(done(storage.rename(&file(from), &file(to))))? {
            Some(true) => true,
            Some(false) => left.step(done(storage.rename(&old(from), &file(to))))? == Some(true),
            None => false,
        };
        // What is left at the old place goes once the file is at `to`; where
        // nothing could be moved and nothing is at `to`, it is the only copy
        // and stays.
        if moved || storage.exists(&file(to)) {
            left.step(done(storage.remove(&old(from))))?;
        }
    }
    let book = match try_book(storage, to) {
        Ok(book) => book,
        // Details a newer PaperPaw wrote, or damaged ones: not written over.
        // `Other` is a data error of the format (not of the card), which no
        // retry changes either.
        Err(Error::Newer(_) | Error::Unreadable | Error::Other(_)) => None,
        Err(Error::Storage(StorageError::NotFound)) => None,
        Err(Error::Storage(e)) => {
            left.step(Err::<(), _>(e))?;
            None
        }
        Err(Error::OutOfMemory) => return Err(StorageError::OutOfMemory),
    };
    if let Some(mut book) = book
        && let Some(cover) = book.cover.as_mut()
        && cover.path.contains(&key(from))
    {
        cover.path = cover.path.replace(&key(from), &key(to));
        left.step(write_details(storage, to, &book).map_err(StorageError::from))?;
    }
    let replace = |list: &mut Vec<String>| {
        let mut changed = false;
        for p in list.iter_mut().filter(|p| *p == from) {
            *p = to.to_string();
            changed = true;
        }
        changed
    };
    left.step(try_update_list(
        storage,
        &opened_file(),
        MAX_OPENED,
        Unreadable::Leave,
        replace,
    ))?;
    left.step(try_update_list(
        storage,
        &favourites_file(),
        MAX_FAVOURITES,
        Unreadable::Leave,
        replace,
    ))?;
    left.end()
}

/// [`removed`](super::removed) that returns the first card error instead
/// of going on without it: the book's details and its cover (also from
/// where an earlier version kept them) and its place among the recently
/// opened and the favourites.
///
/// What is not there counts as done, so a repeated call finishes. A list
/// that is there but cannot be read (a newer version's, damaged, no text)
/// is not written over and is a permanent error, as the book would stay
/// listed: the other parts are still forgotten, then
/// [`StorageError::InvalidData`] is returned. ([`try_moved`] leaves such a
/// list out.)
///
/// # Errors
/// As [`try_moved`]: the first error a retry may cure stops the call;
/// else the first one no retry changes (an unreadable list included) is
/// returned once every other part was forgotten.
pub fn try_removed(storage: &dyn Storage, path: &str) -> Result<(), StorageError> {
    let mut left = Left::default();
    for file in [
        book_file(path),
        cover_file(path),
        old_book_file(path),
        old_cover_variant_file(path, ""),
    ] {
        left.step(done(storage.remove(&file)))?;
    }
    let without = |list: &mut Vec<String>| {
        let before = list.len();
        list.retain(|p| p != path);
        list.len() != before
    };
    left.step(try_update_list(
        storage,
        &opened_file(),
        MAX_OPENED,
        Unreadable::Fail,
        without,
    ))?;
    left.step(try_update_list(
        storage,
        &favourites_file(),
        MAX_FAVOURITES,
        Unreadable::Fail,
        without,
    ))?;
    left.end()
}

/// Whether no retry changes `error`: the card refuses the path or the
/// call, or the data is damaged or too large. Everything else (a card
/// error, a full card, no memory, an error this version does not know)
/// may pass. A variant added to [`StorageError`] later is transient until
/// it is listed here, so it must be classified here when it is added.
const fn is_permanent(error: StorageError) -> bool {
    matches!(
        error,
        StorageError::InvalidPath
            | StorageError::Unsupported
            | StorageError::InvalidData
            | StorageError::TooLarge
    )
}

/// The first permanent error of a carry, kept while the other parts go on.
#[derive(Default)]
struct Left(Option<StorageError>);

impl Left {
    /// The step's value; `None` for a permanent error (kept, the carry goes
    /// on); a transient one is returned to stop the carry.
    fn step<T>(&mut self, result: Result<T, StorageError>) -> Result<Option<T>, StorageError> {
        match result {
            Ok(value) => Ok(Some(value)),
            Err(error) if is_permanent(error) => {
                self.0.get_or_insert(error);
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// The first permanent error, if a step had one.
    fn end(self) -> Result<(), StorageError> {
        self.0.map_or(Ok(()), Err)
    }
}

/// Whether the step did something: `false` for a file that is not there.
fn done(result: Result<(), StorageError>) -> Result<bool, StorageError> {
    match result {
        Ok(()) => Ok(true),
        Err(StorageError::NotFound) => Ok(false),
        Err(e) => Err(e),
    }
}

/// What a list that is there but cannot be read (a newer version's, damaged,
/// no text) does to the call: it is never written over either way.
#[derive(Clone, Copy)]
enum Unreadable {
    /// Left as it is; the call goes on (a move).
    Leave,
    /// [`StorageError::InvalidData`], a permanent error (a removal): the
    /// book would stay listed.
    Fail,
}

/// [`update_list`](super::update_list) with its errors: a list that is
/// there but unreadable is not written over and is left or fails the call as
/// `unreadable` says, any other read or write error is returned. The list is
/// written only where `change` says it changed it (no copy of a list of up to
/// 500 paths to compare), so a repeat writes nothing.
fn try_update_list(
    storage: &dyn Storage,
    path: &str,
    max: usize,
    unreadable: Unreadable,
    change: impl FnOnce(&mut Vec<String>) -> bool,
) -> Result<(), StorageError> {
    let text = match Format::FIRST.try_load(storage, path, max * 512) {
        Ok(Some(text)) => text,
        Ok(None) | Err(Error::Storage(StorageError::NotFound)) => return Ok(()),
        Err(Error::Newer(_) | Error::Unreadable | Error::Other(_)) => {
            return match unreadable {
                Unreadable::Leave => Ok(()),
                Unreadable::Fail => Err(StorageError::InvalidData),
            };
        }
        Err(Error::Storage(e)) => return Err(e),
        Err(Error::OutOfMemory) => return Err(StorageError::OutOfMemory),
    };
    let mut list = try_lines(&text).map_err(|_| StorageError::OutOfMemory)?;
    drop(text);
    if !change(&mut list) {
        return Ok(());
    }
    list.truncate(max);
    let text = try_joined(&list).map_err(|_| StorageError::OutOfMemory)?;
    storage.write(path, text.as_bytes())
}

/// The non-empty lines of `text`, as `read_list` keeps them, without an
/// infallible allocation.
fn try_lines(text: &str) -> Result<Vec<String>, TryReserveError> {
    let mut list = Vec::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let mut path = String::new();
        path.try_reserve_exact(line.len())?;
        path.push_str(line);
        list.try_reserve(1)?;
        list.push(path);
    }
    Ok(list)
}

/// The list's text as `update_list` writes it (one path a line; a first
/// path that begins with a byte order mark keeps it behind a mark of the
/// file's own), without an infallible allocation.
fn try_joined(list: &[String]) -> Result<String, TryReserveError> {
    let mark = list
        .first()
        .is_some_and(|first| first.starts_with('\u{feff}'));
    let size = list.iter().map(|p| p.len() + 1).sum::<usize>() + if mark { 3 } else { 0 };
    let mut text = String::new();
    text.try_reserve_exact(size)?;
    if mark {
        text.push('\u{feff}');
    }
    for (n, p) in list.iter().enumerate() {
        if n > 0 {
            text.push('\n');
        }
        text.push_str(p);
    }
    Ok(text)
}

#[cfg(test)]
#[path = "carry_tests.rs"]
mod tests;
