//! [`try_moved`] and [`try_removed`]: what the library knows of a book
//! carried along or forgotten, with every card error the caller can
//! retry returned instead of dropped, for a host step that must fail and
//! run again (a power cut, a full card).

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::{
    MAX_FAVOURITES, MAX_OPENED, book, book_file, cover_file, favourites_file, key, old_book_file,
    old_cover_variant_file, opened_file, read_list, write_details,
};
use crate::{Storage, StorageError};

/// [`moved`](super::moved) that returns the first card error instead of
/// going on without it: the details and the cover kept for the book
/// (also from where an earlier version kept them), the cover path the
/// details name, and its place among the recently opened and the
/// favourites.
///
/// What is not there counts as done ([`StorageError::NotFound`] is never
/// returned), so a call repeated after an error, or after a power cut,
/// finishes the move. A list that is there but cannot be read (written by
/// a newer PaperPaw, or damaged) is not written over and is left out, as
/// [`moved`](super::moved) does: no retry would change it.
///
/// # Errors
/// The first rename, remove, read or write error other than
/// [`StorageError::NotFound`]; what was done before it stays done.
pub fn try_moved(storage: &dyn Storage, from: &str, to: &str) -> Result<(), StorageError> {
    for (file, old) in [
        (
            book_file as fn(&str) -> String,
            old_book_file as fn(&str) -> String,
        ),
        (cover_file, |path| old_cover_variant_file(path, "")),
    ] {
        let moved = done(storage.rename(&file(from), &file(to)))?
            || done(storage.rename(&old(from), &file(to)))?;
        // Not where neither could be moved and nothing is at `to`: the
        // only copy, read there still.
        if moved || storage.exists(&file(to)) {
            done(storage.remove(&old(from)))?;
        }
    }
    if let Some(mut book) = book(storage, to)
        && let Some(cover) = book.cover.as_mut()
        && cover.path.contains(&key(from))
    {
        cover.path = cover.path.replace(&key(from), &key(to));
        write_details(storage, to, &book).map_err(StorageError::from)?;
    }
    let replace = |list: &mut Vec<String>| {
        let mut changed = false;
        for p in list.iter_mut().filter(|p| *p == from) {
            *p = to.to_string();
            changed = true;
        }
        changed
    };
    try_update_list(storage, &opened_file(), MAX_OPENED, replace)?;
    try_update_list(storage, &favourites_file(), MAX_FAVOURITES, replace)
}

/// [`removed`](super::removed) that returns the first card error instead
/// of going on without it: the book's details and its cover (also from
/// where an earlier version kept them) and its place among the recently
/// opened and the favourites.
///
/// What is not there counts as done, so a repeated call finishes; a list
/// that cannot be read is left out, as for [`try_moved`].
///
/// # Errors
/// The first remove, read or write error other than
/// [`StorageError::NotFound`]; what was done before it stays done.
pub fn try_removed(storage: &dyn Storage, path: &str) -> Result<(), StorageError> {
    for file in [
        book_file(path),
        cover_file(path),
        old_book_file(path),
        old_cover_variant_file(path, ""),
    ] {
        done(storage.remove(&file))?;
    }
    let without = |list: &mut Vec<String>| {
        let before = list.len();
        list.retain(|p| p != path);
        list.len() != before
    };
    try_update_list(storage, &opened_file(), MAX_OPENED, without)?;
    try_update_list(storage, &favourites_file(), MAX_FAVOURITES, without)
}

/// Whether the step did something: `false` for a file that is not there.
fn done(result: Result<(), StorageError>) -> Result<bool, StorageError> {
    match result {
        Ok(()) => Ok(true),
        Err(StorageError::NotFound) => Ok(false),
        Err(e) => Err(e),
    }
}

/// [`update_list`](super::update_list) with its errors: a list that is
/// there but unreadable is skipped (not written over), any other read or
/// write error returned. The list is written only where `change` says it
/// changed it (no copy of a list of up to 500 paths to compare), so a
/// repeat writes nothing.
fn try_update_list(
    storage: &dyn Storage,
    path: &str,
    max: usize,
    change: impl FnOnce(&mut Vec<String>) -> bool,
) -> Result<(), StorageError> {
    let mut list = match read_list(storage, path, max) {
        Ok(list) => list,
        Err(StorageError::InvalidData | StorageError::NotFound) => return Ok(()),
        Err(e) => return Err(e),
    };
    if !change(&mut list) {
        return Ok(());
    }
    list.truncate(max);
    let mut text = list.join("\n");
    // As update_list: a first path that begins with a byte order mark
    // keeps it behind a mark of the file's own.
    if text.starts_with('\u{feff}') {
        text.insert(0, '\u{feff}');
    }
    storage.write(path, text.as_bytes())
}

#[cfg(test)]
#[path = "carry_tests.rs"]
mod tests;
