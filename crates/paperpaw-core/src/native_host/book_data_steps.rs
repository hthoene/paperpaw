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
//! - **Bounded:** entries are found [`KEPT_MAX`] at a time, each folder listed
//!   once for a batch (and again only when a batch was full); a removal takes
//!   **pieces** of a folder (`Storage::remove_tree_step`: at most 64 removals
//!   and 256 reads each) up to the caller's budget and says there is more.
//! - **Stateless and repeatable:** nothing is kept between calls. What the
//!   card shows after a cut is all a repeat needs: a moved entry is no longer
//!   under the old key; a removed one is gone; a partly removed folder goes on
//!   from what is left. So a caller that is journalled (the library journal of
//!   a move, a delete) repeats a step after a cut and reaches the same end.
//! - **Never a replacement:** an entry whose new name is taken stays under the
//!   old key (the book already has data of its own there), and is cleaned up
//!   with the books no longer found; a move never overwrites.
//! - **Transient errors are said, permanent ones left:** a card error that may
//!   pass (`Io`, no memory, a full card, a cause this build doesn't know) is the
//!   call's status and the caller repeats the step; `NotFound` (already gone)
//!   is no error; an entry the card refuses for good (`InvalidPath`,
//!   `Unsupported`, `InvalidData`, `TooLarge`: a damaged or too deep entry, a
//!   name the card cannot make) stays where it is for the clean-up of books no
//!   longer found, and the step goes on with the others. So a book with such
//!   an entry, or with more entries than one batch, never stalls the library.
use super::storage::status;
use alloc::{string::String, vec::Vec};
use paperpaw_native::host_glue::ERR_NO_MEMORY;
use paperpaw_platform::{Storage, StorageError};
use paperpaw_sdk::library::{BookData, KEY_LEN, key_into};

/// Entries of one book in one folder found, and then changed, as one batch.
/// A book with more (per-page files) takes more batches.
pub(crate) const KEPT_MAX: usize = 64;

/// Batches of one folder a call takes at most (4,096 entries); what a still
/// larger book keeps stays for the clean-up.
const BATCHES: usize = 64;

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

/// Whether a card error is for good: the entry is refused (a damaged one, a
/// tree too deep, a name the card cannot make), and repeating changes
/// nothing. Others may pass (`Io`, no memory, a full card, an unknown cause).
pub(crate) const fn permanent(error: StorageError) -> bool {
    matches!(
        error,
        StorageError::InvalidPath
            | StorageError::Unsupported
            | StorageError::InvalidData
            | StorageError::TooLarge
    )
}

/// Up to `max` entries of `folder` kept for the book with `key`, in the
/// folder itself and in the part of its key, after the first `skip` of them,
/// appended to `out`. `skip` is the entries an earlier batch left behind: they
/// stay first in the listing, in their order, while moved ones are gone.
///
/// # Errors
/// `ERR_NO_MEMORY`; the card's status for a folder that cannot be listed (one
/// that is not there keeps nothing).
fn find(
    storage: &dyn Storage,
    folder: &BookData,
    key: &str,
    skip: usize,
    max: usize,
    out: &mut Vec<Kept>,
) -> Result<(), i32> {
    let part = key.get(..2).unwrap_or("00");
    let mut seen = 0_usize;
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
            if entry_key != key || failed.is_some() || out.len() >= max {
                return;
            }
            seen += 1;
            if seen <= skip {
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
/// key where it was in a part. An entry whose new name is taken, or that the
/// card refuses for good, stays under the old key (and is cleaned up with the
/// books no longer found); the others move, in batches of [`KEPT_MAX`].
///
/// # Errors
/// `ERR_NO_MEMORY` (nothing more is changed); the card's status for a
/// transient error (the entries before are moved: repeat the call).
pub(crate) fn move_data(
    storage: &dyn Storage,
    data: &[BookData],
    from: &str,
    to: &str,
) -> Result<(), i32> {
    let (mut old_digits, mut new_digits) = ([0_u8; KEY_LEN], [0_u8; KEY_LEN]);
    let old = key_into(from, &mut old_digits);
    let new = key_into(to, &mut new_digits);
    if old == new {
        return Ok(());
    }
    for folder in data {
        let mut left = 0;
        for _ in 0..BATCHES {
            let mut kept = Vec::new();
            find(storage, folder, old, left, KEPT_MAX, &mut kept)?;
            for entry in &kept {
                if !move_entry(storage, folder, new, entry)? {
                    left += 1;
                }
            }
            if kept.len() < KEPT_MAX {
                break;
            }
        }
    }
    Ok(())
}

/// One entry to the new key. `false`: it stays (its new name is taken, or the
/// card refuses it for good).
fn move_entry(
    storage: &dyn Storage,
    folder: &BookData,
    new: &str,
    entry: &Kept,
) -> Result<bool, i32> {
    let named = concat(&[&folder.prefix, new, &entry.tail])?;
    let dir = match entry.split {
        true => joined(&[&folder.folder, new.get(..2).unwrap_or("00")])?,
        false => alloc_string(&folder.folder)?,
    };
    let target = joined(&[dir.as_str(), named.as_str()])?;
    // `exists` knows files only: a folder taken counts too.
    if storage.exists(&target) || paperpaw_sdk::files::is_dir(storage, &target) {
        return Ok(false);
    }
    if entry.split {
        // The part of the new key may not be there yet; a failure shows in
        // the rename below.
        let _ = storage.create_dir(&dir);
    }
    match storage.rename(&entry.path, &target) {
        Ok(()) | Err(StorageError::NotFound) => Ok(true),
        Err(error) if permanent(error) => Ok(false),
        Err(error) => Err(status(error)),
    }
}

/// How one piece of removing an entry went.
enum Removed {
    /// The entry is gone.
    Gone,
    /// A folder was cut down but is not gone yet.
    More,
    /// The card refuses it for good, or took nothing: it stays.
    Left,
}

/// One piece of removing an entry. A folder goes on from `place` (where the
/// last piece of it ended; 0 at first), which a piece that did not finish
/// moves on.
fn remove_entry(storage: &dyn Storage, entry: &Kept, place: &mut u64) -> Result<Removed, i32> {
    let done = match entry.is_dir {
        true => storage
            .remove_tree_step(&entry.path, false, *place)
            .map(|step| match (step.done, step.removed) {
                (true, _) => Removed::Gone,
                (false, 0) => Removed::Left,
                (false, _) => {
                    *place = step.next;
                    Removed::More
                }
            }),
        false => storage.remove(&entry.path).map(|()| Removed::Gone),
    };
    match done {
        Ok(step) => Ok(step),
        Err(StorageError::NotFound) => Ok(Removed::Gone),
        Err(error) if permanent(error) => Ok(Removed::Left),
        Err(error) => Err(status(error)),
    }
}

/// The folder of all document caches (`.paperpaw/cache/<book key>/...`).
const DOC_CACHE_ROOT: &str = ".paperpaw/cache";

/// Most folders of one level of a cache `unstamp` looks into (format plugins of
/// a book, conversions of a plugin); more than the card keeps (two conversions).
const STAMPS_LISTED: usize = 16;

/// The names of the folders in `dir`, at most [`STAMPS_LISTED`]; `None` where
/// `dir` is not there.
fn subfolders(storage: &dyn Storage, dir: &str) -> Result<Option<Vec<String>>, i32> {
    let mut names: Vec<String> = Vec::new();
    let mut failed = None;
    let listed = storage.list(dir, &mut |entry| {
        if !entry.is_dir || failed.is_some() || names.len() >= STAMPS_LISTED {
            return;
        }
        match gate()
            .and_then(|()| names.try_reserve(1).map_err(|_| ERR_NO_MEMORY))
            .and_then(|()| alloc_string(entry.name))
        {
            Ok(name) => names.push(name),
            Err(error) => failed = Some(error),
        }
    });
    match (failed, listed) {
        (Some(error), _) => Err(error),
        (None, Ok(())) => Ok(Some(names)),
        (None, Err(StorageError::NotFound)) => Ok(None),
        (None, Err(error)) => Err(status(error)),
    }
}

/// Takes the stamps `.complete` away from every conversion of the cache at
/// `folder` (`<format plugin>/<conversion>/.host/.complete`): a cache without
/// its stamp is not used, so no cut, no error and no budget that ends a
/// removal in the middle leaves pages that an open would take for a whole
/// cache. `false`: there is no cache.
///
/// # Errors
/// `ERR_NO_MEMORY`; the card's status for a transient error.
fn unstamp(storage: &dyn Storage, folder: &str) -> Result<bool, i32> {
    let Some(plugins) = subfolders(storage, folder)? else {
        return Ok(false);
    };
    for plugin in &plugins {
        let plugin_dir = joined(&[folder, plugin])?;
        for conversion in subfolders(storage, &plugin_dir)?.iter().flatten() {
            let conversion_dir = joined(&[&plugin_dir, conversion])?;
            for stamp in [".host/.complete", ".complete"] {
                match storage.remove(&joined(&[&conversion_dir, stamp])?) {
                    Ok(()) | Err(StorageError::NotFound) => {}
                    Err(error) if permanent(error) => {}
                    Err(error) => return Err(status(error)),
                }
            }
        }
    }
    Ok(true)
}

/// Removes what the host keeps to show the document at `path`
/// (`.paperpaw/cache/<book key>/`, `docs/abi/document.md`): the folder's name
/// holds the book key, so it is never renamed to a book's new key (the new
/// key's is made at the next open). **Stamps first** ([`unstamp`]), then the
/// tree in bounded `remove_tree_step` pieces of the shared `budget`; what an
/// exhausted budget or a refusing card leaves is a cache nobody uses and is
/// `Done` or `More` for the caller. A book that is open is busy for a move, so
/// no document holds the folder here.
///
/// # Errors
/// `ERR_NO_MEMORY`; the card's status for a transient error.
pub(crate) fn remove_doc_cache(
    storage: &dyn Storage,
    path: &str,
    budget: &mut u32,
) -> Result<Progress, i32> {
    let mut digits = [0_u8; KEY_LEN];
    let key = key_into(path, &mut digits);
    let folder = joined(&[DOC_CACHE_ROOT, key])?;
    if !unstamp(storage, &folder)? {
        return Ok(Progress::Done);
    }
    let mut place = 0;
    loop {
        if *budget == 0 {
            return Ok(Progress::More);
        }
        *budget -= 1;
        match storage.remove_tree_step(&folder, false, place) {
            Ok(step) if step.done => return Ok(Progress::Done),
            // Took nothing: the card refuses it; the clean-up of books no
            // longer found is for what stays.
            Ok(step) if step.removed == 0 => return Ok(Progress::Done),
            Ok(step) => place = step.next,
            Err(StorageError::NotFound) => return Ok(Progress::Done),
            Err(error) if permanent(error) => return Ok(Progress::Done),
            Err(error) => return Err(status(error)),
        }
    }
}

/// What a `book_moved` event of the `core` plugin does for the data kept for
/// the book: the library's own steps moved it already, this catches a move that
/// came another way. A card error leaves the rest under the old key, where the
/// next move or the clean-up finds it.
pub(crate) fn follow_move(storage: &dyn Storage, data: &[BookData], from: &str, to: &str) {
    let _ = move_data(storage, data, from, to);
    let mut budget = REMOVE_PIECES;
    let _ = remove_doc_cache(storage, from, &mut budget);
}

/// What a `book_removed` event does: at most [`REMOVE_PIECES`] bounded pieces
/// for the book in all; what a larger folder leaves is a cache nobody uses
/// (stamps go first) or data for the clean-up of books no longer found.
pub(crate) fn follow_removal(storage: &dyn Storage, data: &[BookData], path: &str) {
    let mut budget = REMOVE_PIECES;
    let _ = remove_data_within(storage, data, path, &mut budget);
    let _ = remove_doc_cache(storage, path, &mut budget);
}

/// Removes what the plugins keep for the book at `path` in folders `data`,
/// at most `pieces` pieces (a file, or one bounded step of a folder). `More`:
/// the book still has data a repeat may remove; `Done`: nothing is left but
/// what the card refuses for good (the clean-up of books no longer found takes
/// that).
///
/// # Errors
/// `ERR_NO_MEMORY`; the card's status for a transient error (repeat the call).
#[cfg(test)]
pub(crate) fn remove_data(
    storage: &dyn Storage,
    data: &[BookData],
    path: &str,
    pieces: u32,
) -> Result<Progress, i32> {
    let mut budget = pieces;
    remove_data_within(storage, data, path, &mut budget)
}

/// [`remove_data`] taking its pieces from `budget`, which others share.
///
/// # Errors
/// As [`remove_data`].
pub(crate) fn remove_data_within(
    storage: &dyn Storage,
    data: &[BookData],
    path: &str,
    budget: &mut u32,
) -> Result<Progress, i32> {
    let mut digits = [0_u8; KEY_LEN];
    let key = key_into(path, &mut digits);
    for folder in data {
        let mut left = 0;
        for _ in 0..BATCHES {
            let mut kept = Vec::new();
            find(storage, folder, key, left, KEPT_MAX, &mut kept)?;
            for entry in &kept {
                let mut place = 0;
                loop {
                    if *budget == 0 {
                        return Ok(Progress::More);
                    }
                    *budget -= 1;
                    match remove_entry(storage, entry, &mut place)? {
                        Removed::More => {}
                        Removed::Gone => break,
                        Removed::Left => {
                            left += 1;
                            break;
                        }
                    }
                }
            }
            if kept.len() < KEPT_MAX {
                break;
            }
        }
    }
    Ok(Progress::Done)
}

#[cfg(test)]
#[path = "book_data_steps_tests.rs"]
mod tests;
