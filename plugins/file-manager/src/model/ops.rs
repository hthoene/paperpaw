//! The changes the file manager makes: rename, move, copy and delete, with
//! fallible allocation. A copy of a file goes to a part beside it and is put
//! in place when whole, so a cut short (a full card, a refused allocation)
//! leaves no half file that looks whole and what was at the target stays.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{Storage, StorageError};

use super::folder::{Entry, Folder};
use super::path::{self, copy as copy_text, is_dir, is_within, join, same_path};
use super::{Error, error_of};

/// What one read of a copy takes.
const CHUNK: usize = 4 * 1024;
/// Folders deep a copy goes: each level holds a page of entries and two paths.
pub const MAX_DEPTH: usize = 16;
/// Entries of a folder taken at a time by a copy.
const PAGE: usize = 8;

/// Moves `from` to `to`: renamed where the storage can, else copied and the
/// old one deleted. A folder is never moved into itself.
///
/// # Errors
/// The storage's refusal; [`Error::Invalid`] for a folder moved into itself.
pub fn move_to(storage: &dyn Storage, from: &str, to: &str) -> Result<(), Error> {
    if is_within(storage, to, from) && !same_path(storage, to, from) {
        return Err(Error::Invalid);
    }
    match storage.rename(from, to) {
        Err(StorageError::Unsupported) => {
            // Copied over itself it would be emptied, and then deleted.
            if same_path(storage, from, to) {
                return Err(Error::Invalid);
            }
            copy(storage, from, to)?;
            delete(storage, from)
        }
        result => result.map_err(error_of),
    }
}

/// Deletes the file or the folder and everything in it.
///
/// # Errors
/// The storage's refusal.
pub fn delete(storage: &dyn Storage, path: &str) -> Result<(), Error> {
    if is_dir(storage, path) {
        storage.remove_dir_all(path)
    } else {
        storage.remove(path)
    }
    .map_err(error_of)
}

/// Copies the file `from` to `to`, or the folder with everything in it.
///
/// # Errors
/// The storage's refusal; [`Error::Invalid`] for a folder copied into itself;
/// [`Error::NoSpace`] for folders nested deeper than [`MAX_DEPTH`].
pub fn copy(storage: &dyn Storage, from: &str, to: &str) -> Result<(), Error> {
    copy_at(storage, from, to, 0)
}

fn copy_at(storage: &dyn Storage, from: &str, to: &str, depth: usize) -> Result<(), Error> {
    if !is_dir(storage, from) {
        return copy_file(storage, from, to);
    }
    if is_within(storage, to, from) {
        // Into itself: it would never end.
        return Err(Error::Invalid);
    }
    if depth >= MAX_DEPTH {
        return Err(Error::NoSpace);
    }
    storage.create_dir(to).map_err(error_of)?;
    // A page of entries at a time, found by name (the folder is not listed
    // while a copy is made: a storage may not take calls from inside its
    // listing), so a folder of any size takes one page and the depth's worth.
    let folder = Folder {
        storage,
        dir: from,
        hidden: true,
    };
    let mut last: Option<Entry> = None;
    loop {
        let page = folder.after(last.as_ref(), PAGE)?;
        for entry in &page {
            let source = join(from, &entry.name)?;
            let target = join(to, &entry.name)?;
            copy_at(storage, &source, &target, depth + 1)?;
        }
        if page.len() < PAGE {
            return Ok(());
        }
        last = page.into_iter().next_back();
    }
}

/// `path` with `.part` after it.
fn part_of(path: &str) -> Result<String, Error> {
    let mut part = copy_text(path)?;
    path::push(&mut part, ".part")?;
    Ok(part)
}

fn copy_file(storage: &dyn Storage, from: &str, to: &str) -> Result<(), Error> {
    // The copy is begun by emptying its target: over itself it would be gone.
    if same_path(storage, from, to) {
        return Err(Error::Invalid);
    }
    let part = part_of(to)?;
    let copied = copy_bytes(storage, from, &part);
    let had_old = storage.exists(to);
    match copied {
        // Renamed directly, not by `move_to`: where the storage cannot
        // rename, that would copy the part again through its own part.
        Ok(()) => match storage.rename(&part, to) {
            Err(StorageError::Unsupported) => {
                let _ = storage.remove(&part);
                copy_bytes(storage, from, to)
            }
            Err(e) => {
                // Kept where it is the only copy (FAT removes the old one first).
                if !had_old || storage.exists(to) {
                    let _ = storage.remove(&part);
                }
                Err(error_of(e))
            }
            Ok(()) => Ok(()),
        },
        Err(e) => {
            let _ = storage.remove(&part);
            Err(e)
        }
    }
}

fn copy_bytes(storage: &dyn Storage, from: &str, to: &str) -> Result<(), Error> {
    let mut chunk: Vec<u8> = Vec::new();
    chunk
        .try_reserve_exact(CHUNK)
        .map_err(|_| Error::NoMemory)?;
    chunk.resize(CHUNK, 0);
    storage.write(to, &[]).map_err(error_of)?;
    let mut at = 0u64;
    loop {
        let n = storage.read_at(from, at, &mut chunk).map_err(error_of)?;
        if n == 0 {
            return Ok(());
        }
        storage
            .append(to, chunk.get(..n).ok_or(Error::Io)?)
            .map_err(error_of)?;
        at += n as u64;
    }
}

/// Renames `path` to `name` in its folder: where another entry has that name,
/// to a free one beside it (`name (2)`). A name that differs in case only is
/// the same one on storages that ignore case (the memory card): there it is
/// renamed through a free name between, as such a storage takes both names for
/// one. The path it has now, or why not.
///
/// # Errors
/// The storage's refusal; [`Error::NoMemory`].
pub fn rename(storage: &dyn Storage, path: &str, name: &str) -> Result<String, Error> {
    let dir = path::parent(path);
    let own = path::name(path);
    let mut other = false;
    storage
        .list(dir, &mut |e| other |= e.name == name && e.name != own)
        .map_err(error_of)?;
    let alike = own
        .chars()
        .flat_map(char::to_lowercase)
        .eq(name.chars().flat_map(char::to_lowercase));
    if other || !alike {
        let target = path::free_path(storage, dir, name)?;
        move_to(storage, path, &target)?;
        return Ok(target);
    }
    let target = join(dir, name)?;
    let mut hidden = String::new();
    path::push(&mut hidden, ".")?;
    path::push(&mut hidden, own)?;
    path::push(&mut hidden, ".renaming")?;
    let between = path::free_path(storage, dir, &hidden)?;
    move_to(storage, path, &between)?;
    if let Err(e) = move_to(storage, &between, &target) {
        // Back as it was.
        let _ = move_to(storage, &between, path);
        return Err(e);
    }
    Ok(target)
}

#[cfg(test)]
mod tests;
