//! Who imported a book (`docs/abi/library.md` 2.3): the plugin whose
//! `import` put the file into the library, kept so that a later `replace`
//! import of that name is allowed only for the same plugin and only while
//! nobody moved the book.
//!
//! One small file per book, `.paperpaw/library/imported/<key>.txt`, whose one
//! line is the plugin id. It is named by the book's **path** (the key is the
//! path's hash), so a move leaves nothing under the new path: the record is
//! cleared by construction. The move carry also calls [`clear`] for the old
//! path, so that a book moved away and back is not taken for the importer's
//! again; a delete removes it with the rest of the book's marks.
use super::{kept_file, storage_status};
use alloc::string::String;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY};
use paperpaw_platform::{Storage, StorageError};

/// The folder of the records.
const DIR: &str = ".paperpaw/library/imported";
/// The longest id a record holds (a plugin id).
const ID_MAX: usize = 96;

/// The record of the book at `path`: its file's name.
pub(crate) fn file(path: &str) -> Result<String, i32> {
    kept_file(DIR, path)
}

/// Keeps that `importer` imported the book at `path` (written whole, so a cut
/// leaves the old record or the new one).
pub(crate) fn set(storage: &dyn Storage, path: &str, importer: &str) -> Result<(), i32> {
    if importer.is_empty() || importer.len() > ID_MAX || importer.contains(['\n', '\r']) {
        return Err(ERR_INVALID);
    }
    let mut line = String::new();
    line.try_reserve_exact(importer.len() + 1)
        .map_err(|_| ERR_NO_MEMORY)?;
    line.push_str(importer);
    line.push('\n');
    storage
        .write(&file(path)?, line.as_bytes())
        .map_err(storage_status)
}

/// Whether the book at `path` was imported by `importer` and not moved since.
pub(crate) fn is_by(storage: &dyn Storage, path: &str, importer: &str) -> Result<bool, i32> {
    let mut buffer = [0_u8; ID_MAX + 2];
    let read = match storage.read_at(&file(path)?, 0, &mut buffer) {
        Ok(read) => read,
        Err(StorageError::NotFound) => return Ok(false),
        Err(error) => return Err(storage_status(error)),
    };
    // A record cut short or over the longest id is not one of ours.
    let Ok(text) = core::str::from_utf8(&buffer[..read]) else {
        return Ok(false);
    };
    Ok(read <= ID_MAX + 1 && text.trim_end_matches('\n') == importer)
}

/// Removes the record of the book at `path`, where there is one; the card's
/// error is the answer (the caller repeats the step).
pub(crate) fn remove(storage: &dyn Storage, path: &str) -> Result<(), i32> {
    storage.remove(&file(path)?).map_err(storage_status)
}

/// [`remove`] for the move carry, which has nothing to do with an error: a
/// record left behind names a path no book has, and nothing reads it as the
/// importer's of another book (the key is the path's hash).
#[expect(
    clippy::let_underscore_must_use,
    reason = "a record left behind belongs to no book"
)]
pub(crate) fn clear(storage: &dyn Storage, path: &str) {
    let _ = remove(storage, path);
}

#[cfg(test)]
#[path = "importer_tests.rs"]
mod tests;
