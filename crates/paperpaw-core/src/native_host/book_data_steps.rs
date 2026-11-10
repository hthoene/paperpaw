//! What the plugins keep for a book follows it (`library.md` 1 and 4): the
//! folders of `book_data.spec` (`books/<name>/<book key>/` in a plugin's Data,
//! and the interface 6 folders the kernel registers) are moved to the new key
//! when the book moves, and removed when the book is replaced or deleted.
//!
//! The SDK's `library::move_data` and `remove_data` do this for an interface 6
//! plugin: they drop every error, allocate without checking, and remove a
//! folder in one call whatever its size. The host's library work goes by these
//! instead:
//!
//! - **Fallible:** every list is built with `try_reserve`; no memory is
//!   `ERR_NO_MEMORY`, and nothing is changed by the call that said so.
//! - **Bounded:** one call finds at most [`KEPT_MAX`] entries of the book and
//!   changes them; a removal takes **one piece** of a folder
//!   (`Storage::remove_tree_step`: at most 64 removals and 256 reads) and says
//!   there is more.
//! - **Stateless and repeatable:** nothing is kept between calls. What the
//!   card shows after a cut is all a repeat needs: a moved entry is no longer
//!   under the old key; a removed one is gone; a partly removed folder goes on
//!   from what is left. So a caller that is journalled (the library journal of
//!   a move, a delete) repeats a step after a cut and reaches the same end.
//! - **Never a replacement:** an entry whose new name is taken stays under the
//!   old key (the book already has data of its own there), and is cleaned up
//!   with the books no longer found; a move never overwrites.
//! - **Errors are said:** a card error is the call's status and the step is
//!   repeated by the caller; only `NotFound` (already gone) is no error.
use super::storage::status;
use alloc::{string::String, vec::Vec};
use paperpaw_native::host_glue::{ERR_FULL, ERR_NO_MEMORY};
use paperpaw_platform::{Storage, StorageError};
use paperpaw_sdk::library::{self, BookData};

/// Most entries of one book, in all folders, one call finds. Folders are at
/// most four a plugin and a book keeps an entry and a few variants in each.
pub(crate) const KEPT_MAX: usize = 64;

/// Pieces a caller that cannot repeat its call (an event handler) takes at
/// most for one book: 64 removals each.
pub(crate) const REMOVE_PIECES: u32 = 64;

/// Whether a removal has more to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Progress {
    More,
    Done,
}

/// One entry kept for a book.
struct Kept {
    /// Its path.
    path: String,
    is_dir: bool,
    /// What follows the key in its name (empty, or `.` and more).
    tail: String,
    /// In the part of its folder named by the first two digits of the key.
    split: bool,
}

#[cfg(test)]
use super::net_service::gate;
#[cfg(not(test))]
#[inline]
const fn gate() -> Result<(), i32> {
    Ok(())
}

fn alloc_string(text: &str) -> Result<String, i32> {
    gate()?;
    let mut owned = String::new();
    owned
        .try_reserve_exact(text.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    owned.push_str(text);
    Ok(owned)
}

fn concat(parts: &[&str]) -> Result<String, i32> {
    gate()?;
    let mut owned = String::new();
    owned
        .try_reserve_exact(parts.iter().map(|part| part.len()).sum())
        .map_err(|_| ERR_NO_MEMORY)?;
    for part in parts {
        owned.push_str(part);
    }
    Ok(owned)
}

fn joined(parts: &[&str]) -> Result<String, i32> {
    gate()?;
    let mut owned = String::new();
    owned
        .try_reserve_exact(parts.iter().map(|part| part.len() + 1).sum())
        .map_err(|_| ERR_NO_MEMORY)?;
    for part in parts.iter().filter(|part| !part.is_empty()) {
        if !owned.is_empty() {
            owned.push('/');
        }
        owned.push_str(part);
    }
    Ok(owned)
}

/// The entries of `folder` kept for the book with `key`, in the folder itself
/// and in the part of its key, appended to `out` (at most [`KEPT_MAX`] in all).
///
/// # Errors
/// `ERR_NO_MEMORY`; the card's status for a folder that cannot be listed (one
/// that is not there keeps nothing).
fn find(
    storage: &dyn Storage,
    folder: &BookData,
    key: &str,
    out: &mut Vec<Kept>,
) -> Result<(), i32> {
    let part = key.get(..2).unwrap_or("00");
    for split in [false, true] {
        let dir = match split {
            true => joined(&[&folder.folder, part])?,
            false => alloc_string(&folder.folder)?,
        };
        let dir_name = dir.as_str();
        let mut failed = None;
        let listed = storage.list(&dir, &mut |entry| {
            let Some((entry_key, tail)) = folder.key_of(entry.name) else {
                return;
            };
            if entry_key != key || failed.is_some() {
                return;
            }
            if out.len() >= KEPT_MAX {
                failed = Some(ERR_FULL);
                return;
            }
            let made = gate()
                .and_then(|()| out.try_reserve(1).map_err(|_| ERR_NO_MEMORY))
                .and_then(|()| {
                    Ok(Kept {
                        path: joined(&[dir_name, entry.name])?,
                        is_dir: entry.is_dir,
                        tail: alloc_string(tail)?,
                        split,
                    })
                });
            match made {
                Ok(kept) => out.push(kept),
                Err(error) => failed = Some(error),
            }
        });
        match (failed, listed) {
            (Some(error), _) => return Err(error),
            (None, Ok(()) | Err(StorageError::NotFound)) => {}
            (None, Err(error)) => return Err(status(error)),
        }
    }
    Ok(())
}

/// Moves what the plugins keep for the book at `from` in folders `data` to
/// the key of `to`: each entry is one rename, to the folder's part of the new
/// key where it was in a part. An entry whose new name is taken stays.
///
/// # Errors
/// `ERR_NO_MEMORY` (nothing was changed); the card's status for a rename that
/// failed (the entries before it are moved: repeat the call).
pub(crate) fn move_data(
    storage: &dyn Storage,
    data: &[BookData],
    from: &str,
    to: &str,
) -> Result<(), i32> {
    let (old, new) = (library::key(from), library::key(to));
    if old == new {
        return Ok(());
    }
    for folder in data {
        let mut kept = Vec::new();
        find(storage, folder, &old, &mut kept)?;
        for entry in &kept {
            let named = concat(&[&folder.prefix, &new, &entry.tail])?;
            let dir = match entry.split {
                true => joined(&[&folder.folder, new.get(..2).unwrap_or("00")])?,
                false => alloc_string(&folder.folder)?,
            };
            let target = joined(&[dir.as_str(), named.as_str()])?;
            // `exists` knows files only: a folder taken counts too.
            if storage.exists(&target) || paperpaw_sdk::files::is_dir(storage, &target) {
                continue;
            }
            if entry.split {
                // The part of the new key may not be there yet; a failure
                // shows in the rename below.
                let _ = storage.create_dir(&dir);
            }
            match storage.rename(&entry.path, &target) {
                Ok(()) | Err(StorageError::NotFound) => {}
                Err(error) => return Err(status(error)),
            }
        }
    }
    Ok(())
}

/// One piece of removing what the plugins keep for the book at `path` in
/// folders `data`: a file, or one bounded piece of a folder. `More`: call
/// again; `Done`: nothing of the book is left.
///
/// # Errors
/// `ERR_NO_MEMORY`; the card's status (repeat the call).
pub(crate) fn remove_data_piece(
    storage: &dyn Storage,
    data: &[BookData],
    path: &str,
) -> Result<Progress, i32> {
    let key = library::key(path);
    for folder in data {
        let mut kept = Vec::new();
        find(storage, folder, &key, &mut kept)?;
        let Some(entry) = kept.first() else {
            continue;
        };
        let done = match entry.is_dir {
            true => storage.remove_tree_step(&entry.path, false, 0).map(|_| ()),
            false => storage.remove(&entry.path),
        };
        return match done {
            Ok(()) | Err(StorageError::NotFound) => Ok(Progress::More),
            Err(error) => Err(status(error)),
        };
    }
    Ok(Progress::Done)
}

/// What a `book_moved` event of the `core` plugin does for the data kept for
/// the book: the library's own steps moved it already, this catches a move that
/// came another way. A card error leaves the rest under the old key, where the
/// next move or the clean-up finds it.
pub(crate) fn follow_move(storage: &dyn Storage, data: &[BookData], from: &str, to: &str) {
    let _ = move_data(storage, data, from, to);
}

/// What a `book_removed` event does: at most [`REMOVE_PIECES`] bounded pieces
/// for the book; what a larger folder leaves is removed with the books no
/// longer found.
pub(crate) fn follow_removal(storage: &dyn Storage, data: &[BookData], path: &str) {
    let _ = remove_data(storage, data, path, REMOVE_PIECES);
}

/// [`remove_data_piece`] up to `pieces` times; `More` if the book still has
/// data after them (a folder larger than that, or the call is to be repeated).
///
/// # Errors
/// As [`remove_data_piece`].
pub(crate) fn remove_data(
    storage: &dyn Storage,
    data: &[BookData],
    path: &str,
    pieces: u32,
) -> Result<Progress, i32> {
    for _ in 0..pieces {
        if remove_data_piece(storage, data, path)? == Progress::Done {
            return Ok(Progress::Done);
        }
    }
    Ok(Progress::More)
}

#[cfg(test)]
#[path = "book_data_steps_tests.rs"]
mod tests;
