//! A native plugin's book scope (`command.set_scope`, `registry.md` section
//! 4.1; #257): while its screen is on top, the settings declared `scoped` take
//! the values kept for one book, else the global ones, and the settings
//! editor's "Only this book" writes there.
//!
//! The plugin names the book by its root-1 path; the host picks the file,
//! never the plugin: `<the plugin's Data>/books/<book key>/settings.ini`, with
//! the host's book key (`library.md` section 6). That is where Beta 3's reader
//! kept a book's own settings, so they carry over as they are (user data). The
//! file follows its book in the same steps as `book_data.spec` folders
//! (`book_data_steps`): moved to the new key, removed with the book, for every
//! plugin that has one, found on the card (also after a restart). A
//! `book_data.spec` folder is `books/<name>/<key>/`, and a `name` that could be
//! a key is refused ([`could_be_key`]), so the two never meet.
use super::book_data_steps::{Progress, permanent};
use super::storage::status;
use alloc::{string::String, vec::Vec};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY};
use paperpaw_platform::{Storage, StorageError};
use paperpaw_sdk::library::{KEY_LEN, key_into};

/// The folder of a plugin's Data that holds its book scopes.
const BOOKS: &str = "books";
/// A book scope's file in the book's folder.
const FILE: &str = "settings.ini";

/// `parts` joined into one string, its room reserved fallibly.
fn joined(parts: &[&str]) -> Result<String, i32> {
    let length = parts.iter().map(|part| part.len()).sum();
    let mut text = String::new();
    text.try_reserve_exact(length).map_err(|_| ERR_NO_MEMORY)?;
    for part in parts {
        text.push_str(part);
    }
    Ok(text)
}

/// Plugins' folders looked into for one book at most, in the card's order:
/// more than a device keeps installed. Folders past it (left by removed
/// plugins) are not looked into; their files stay under the old key.
const PLUGINS_MAX: usize = 64;

/// `<Data of plugin>/books/<key>`: the folder of one book's scope.
fn book_folder(plugin: &str, key: &str) -> Result<String, i32> {
    joined(&[
        paperpaw_sdk::data::DIR,
        "/plugins/",
        plugin,
        "/",
        BOOKS,
        "/",
        key,
    ])
}

/// The scope file of `plugin` for the book with `key`.
fn file_at(plugin: &str, key: &str) -> Result<String, i32> {
    let mut path = book_folder(plugin, key)?;
    path.try_reserve_exact(1 + FILE.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    path.push('/');
    path.push_str(FILE);
    Ok(path)
}

/// Whether `path` may name a book: root 1's grammar (relative, at most 512
/// bytes) and not below a folder root 1 never reaches (`plugins`,
/// `.paperpaw`, `paperpaw.ini`).
pub(crate) fn book_path_ok(path: &str) -> bool {
    path.len() <= 512
        && paperpaw_platform::storage::is_valid_path(path)
        && !super::owner_paths::protected_first(path.split('/').next().unwrap_or_default())
}

/// The file of `plugin`'s scope for the book at `path`.
///
/// # Errors
/// `ERR_INVALID` for a path that names no book; `ERR_NO_MEMORY`.
pub(crate) fn file(plugin: &str, path: &str) -> Result<String, i32> {
    if !book_path_ok(path) {
        return Err(ERR_INVALID);
    }
    let mut digits = [0_u8; KEY_LEN];
    file_at(plugin, key_into(path, &mut digits))
}

/// Whether a `book_data.spec` name could be a book key (16 hex digits, in
/// either case: FAT does not tell `ABC` from `abc`): its folder
/// `books/<name>/` would meet a book's scope folder `books/<key>/`.
pub(crate) fn could_be_key(name: &str) -> bool {
    name.len() == KEY_LEN && name.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The plugins with a folder in the Data root (`.paperpaw/plugins/<id>`), at
/// most [`PLUGINS_MAX`]; none where the folder is not there.
fn plugins(storage: &dyn Storage) -> Result<Vec<String>, i32> {
    let dir = joined(&[paperpaw_sdk::data::DIR, "/plugins"])?;
    let mut found: Vec<String> = Vec::new();
    let mut failed = None;
    let listed = storage.list(&dir, &mut |entry| {
        if !entry.is_dir || failed.is_some() || found.len() >= PLUGINS_MAX {
            return;
        }
        let name = found
            .try_reserve(1)
            .map_err(|_| ERR_NO_MEMORY)
            .and_then(|()| joined(&[entry.name]));
        match name {
            Ok(name) => found.push(name),
            Err(error) => failed = Some(error),
        }
    });
    match (failed, listed) {
        (Some(error), _) => Err(error),
        (None, Ok(()) | Err(StorageError::NotFound)) => Ok(found),
        (None, Err(error)) => Err(status(error)),
    }
}

/// Whether the file at `path` is there. A transient card error is its status
/// (repeat the call), never "none", which would leave the file behind for
/// good; one for good counts as none (the file stays where it is).
fn there(storage: &dyn Storage, path: &str) -> Result<bool, i32> {
    match storage.size(path) {
        Ok(_) => Ok(true),
        Err(StorageError::NotFound) => Ok(false),
        Err(error) if permanent(error) => Ok(false),
        Err(error) => Err(status(error)),
    }
}

/// The book's folder of `plugin` goes when nothing is left in it (Beta 3 kept
/// more there: then it stays). Best effort: a folder left is harmless.
fn drop_if_empty(storage: &dyn Storage, plugin: &str, key: &str) {
    let Ok(folder) = book_folder(plugin, key) else {
        return;
    };
    let mut empty = true;
    if storage.list(&folder, &mut |_| empty = false).is_ok() && empty {
        let _ = storage.remove_dir_all(&folder);
    }
}

/// Moves every plugin's scope file of the book from key `old` to key `new`, in
/// the steps of the `book_data` folders (`book_data_steps::move_data`). A file
/// whose new name is taken, or that the card refuses for good, stays under the
/// old key; none is no error. Repeatable: a moved file is no longer there.
///
/// # Errors
/// `ERR_NO_MEMORY`; the card's status for a transient error (repeat the call).
pub(crate) fn move_scopes(storage: &dyn Storage, old: &str, new: &str) -> Result<(), i32> {
    for plugin in plugins(storage)? {
        let from = file_at(&plugin, old)?;
        if !there(storage, &from)? {
            continue;
        }
        let folder = book_folder(&plugin, new)?;
        let to = file_at(&plugin, new)?;
        // Never a replacement: the book under the new key has its own. A
        // folder in the file's place, or a file in its folder's, is taken too.
        if there(storage, &to)?
            || paperpaw_sdk::files::is_dir(storage, &to)
            || there(storage, &folder)?
        {
            continue;
        }
        // The new key's folder may not be there yet, and a card renames into
        // none (FAT): made first, as `book_data_steps` does; a failure shows
        // in the rename.
        let _ = storage.create_dir(&folder);
        match storage.rename(&from, &to) {
            Ok(()) => drop_if_empty(storage, &plugin, old),
            // Moved by an earlier, cut call, or refused: only a file gone
            // from the old key is moved.
            Err(StorageError::NotFound) => {
                if !there(storage, &from)? {
                    drop_if_empty(storage, &plugin, old);
                }
            }
            Err(error) if permanent(error) => {}
            Err(error) => return Err(status(error)),
        }
    }
    Ok(())
}

/// Removes every plugin's scope file of the book with `key`, one piece of
/// `budget` each (`book_data_steps::remove_data_within`). `More`: the budget
/// ran out first. None is no error; one the card refuses for good stays.
///
/// # Errors
/// `ERR_NO_MEMORY`; the card's status for a transient error (repeat the call).
pub(crate) fn remove_scopes(
    storage: &dyn Storage,
    key: &str,
    budget: &mut u32,
) -> Result<Progress, i32> {
    for plugin in plugins(storage)? {
        let path = file_at(&plugin, key)?;
        if !there(storage, &path)? {
            continue;
        }
        if *budget == 0 {
            return Ok(Progress::More);
        }
        *budget -= 1;
        match storage.remove(&path) {
            Ok(()) | Err(StorageError::NotFound) => drop_if_empty(storage, &plugin, key),
            Err(error) if permanent(error) => {}
            Err(error) => return Err(status(error)),
        }
    }
    Ok(Progress::Done)
}

#[cfg(test)]
#[path = "book_scope_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "book_scope_fat_tests.rs"]
mod fat_tests;
