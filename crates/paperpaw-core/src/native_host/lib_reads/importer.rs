//! Who imported a book (`docs/abi/library.md` 2.3): the plugin whose
//! `import` put the file into the library, kept so that a later `replace`
//! import of that name is allowed only for the same plugin, only while nobody
//! moved the book, and only while the file is still the one that was
//! imported.
//!
//! One small file per book, `.paperpaw/library/imported/<key>.txt`: the plugin
//! id on the first line and the fingerprint of the imported file
//! (`<size> <partial md5>`, as KOReader's) on the second. It is named by the
//! book's **path** (the key is the path's hash), so a move leaves nothing under
//! the new path: the record is cleared by construction. The fingerprint is
//! what stops a plugin overwriting a file the user put at that name since
//! (deleted on a computer and copied there again): the record then no longer
//! matches. The move carry also calls [`clear`] for the old path, so that a
//! book moved away and back is not taken for the importer's again; a delete
//! removes it with the rest of the book's marks.
use super::{carry_status, kept_file, storage_status};
use alloc::string::String;
use core::fmt::Write as _;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_IO, ERR_NO_MEMORY};
use paperpaw_platform::Storage;
use paperpaw_sdk::library::Fingerprint;

/// The folder of the records.
const DIR: &str = ".paperpaw/library/imported";
/// The longest id a record holds (a plugin id).
const ID_MAX: usize = 96;
/// The most a record holds: the id, a line break, the fingerprint (a size of
/// 20 digits, a space, 32 hex digits) and a line break, with room to spare.
const RECORD_MAX: usize = ID_MAX + 1 + 20 + 1 + 32 + 1 + 8;

/// The record of the book at `path`: its file's name.
pub(crate) fn file(path: &str) -> Result<String, i32> {
    kept_file(DIR, path)
}

/// Keeps that `importer` imported the book at `path`, the file as it is now
/// (written whole, so a cut leaves the old record or the new one). A file
/// that cannot be read is `ERR_IO`: there is nothing to tell it by.
pub(crate) fn set(storage: &dyn Storage, path: &str, importer: &str) -> Result<(), i32> {
    if importer.is_empty() || importer.len() > ID_MAX || importer.contains(['\n', '\r']) {
        return Err(ERR_INVALID);
    }
    let fingerprint = Fingerprint::of(storage, path).ok_or(ERR_IO)?;
    let mut record = String::new();
    record
        .try_reserve_exact(RECORD_MAX)
        .map_err(|_| ERR_NO_MEMORY)?;
    let _ = writeln!(record, "{importer}");
    let _ = writeln!(record, "{}", fingerprint.text());
    storage
        .write(&file(path)?, record.as_bytes())
        .map_err(carry_status)
}

/// Whether the book at `path` was imported by `importer`, was not moved since
/// and is still the file that was imported: the same size and the same
/// partial MD5. A record that is not ours, or no file, is not.
pub(crate) fn is_by(storage: &dyn Storage, path: &str, importer: &str) -> Result<bool, i32> {
    let mut buffer = [0_u8; RECORD_MAX + 1];
    let read = match storage.read_at(&file(path)?, 0, &mut buffer) {
        Ok(read) => read,
        Err(paperpaw_platform::StorageError::NotFound) => return Ok(false),
        Err(error) => return Err(storage_status(error)),
    };
    // A record over the most there is, or no text, is not one of ours.
    if read > RECORD_MAX {
        return Ok(false);
    }
    let Ok(text) = core::str::from_utf8(&buffer[..read]) else {
        return Ok(false);
    };
    let mut lines = text.lines();
    let (Some(id), Some(kept)) = (lines.next(), lines.next()) else {
        return Ok(false);
    };
    if id != importer {
        return Ok(false);
    }
    let (Some(kept), Some(now)) = (Fingerprint::parse(kept), Fingerprint::of(storage, path)) else {
        return Ok(false);
    };
    Ok(kept == now)
}

/// Removes the record of the book at `path`, where there is one; the card's
/// error is the answer (the caller repeats the step).
pub(crate) fn remove(storage: &dyn Storage, path: &str) -> Result<(), i32> {
    storage.remove(&file(path)?).map_err(carry_status)
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
