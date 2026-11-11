//! Op 12 `import` before the card is touched (`docs/abi/library.md` 2.3): the
//! name the book gets and whether it may be taken. The names are checked
//! already (the service); this decides, on the card, what `to` is and whether
//! the existing file there may be replaced, and refuses before anything is
//! changed. The journalled move itself is Files'
//! ([`LibraryMoves::begin_import`](super::lib_moves_service::LibraryMoves::begin_import)).
//!
//! - **Source.** The caller's own root 0: `.paperpaw/plugins/<plugin>/<from>`,
//!   `ERR_NOT_FOUND` where the file is not there.
//! - **Name.** `<library folder>/<folder>/<name>`; an existing file is
//!   `ERR_STATE` unless `replace` or `free_name` says what to do. `free_name`
//!   takes `name (2).ext`, `(3)` and so on, the first that is free.
//! - **Replace.** Only a book this plugin imported and nobody moved or
//!   changed since ([`importer::is_by`](super::lib_reads::importer_is_by)): any
//!   other file is `ERR_DENIED`. A file that was never imported by anyone
//!   is the user's.
use super::lib_reads::importer_is_by;
use super::lib_service::ImportRequest;
use super::storage::{PREFIX, relative_path};
use alloc::string::String;
use core::fmt::Write as _;
use paperpaw_native::host_glue::{
    ERR_DENIED, ERR_IO, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE,
};
use paperpaw_platform::{Storage, StorageError};

/// Names `name (2)` to `name (99)` are tried; past that nothing is free.
const FREE_NAMES: u32 = 99;
/// The longest root-1 path (`lib_result.path`).
const PATH_MAX: usize = 512;

/// What the card will be asked.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Plan {
    /// The caller's file on the card.
    pub(crate) from: String,
    /// The path the book gets, final.
    pub(crate) to: String,
    /// An existing book of the caller's is replaced.
    pub(crate) replace: bool,
}

/// The card path of the caller's `from`.
fn source(storage: &dyn Storage, plugin: &str, from: &str) -> Result<String, i32> {
    let relative = relative_path(from.as_bytes())?;
    let mut path = String::new();
    path.try_reserve_exact(PREFIX.len() + plugin.len() + 1 + relative.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    for part in [PREFIX, plugin, "/", relative] {
        path.push_str(part);
    }
    match storage.size(&path) {
        Ok(_) => Ok(path),
        Err(StorageError::NotFound) => Err(ERR_NOT_FOUND),
        Err(StorageError::OutOfMemory) => Err(ERR_NO_MEMORY),
        Err(_) => Err(ERR_IO),
    }
}

/// `<library>/<folder>/<stem><suffix><ext>` without a leading or double slash.
fn join(library: &str, folder: &str, name: &str) -> Result<String, i32> {
    let mut path = String::new();
    path.try_reserve_exact(library.len() + folder.len() + name.len() + 16)
        .map_err(|_| ERR_NO_MEMORY)?;
    for part in [library, folder] {
        if !part.is_empty() {
            path.push_str(part.trim_matches('/'));
            path.push('/');
        }
    }
    path.push_str(name);
    Ok(path)
}

/// `name (n).ext`: the number goes before the last dot (not a leading one).
fn numbered(name: &str, n: u32) -> Result<String, i32> {
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 => name.split_at(dot),
        _ => (name, ""),
    };
    let mut numbered = String::new();
    numbered
        .try_reserve_exact(name.len() + 8)
        .map_err(|_| ERR_NO_MEMORY)?;
    let _ = write!(numbered, "{stem} ({n}){extension}");
    Ok(numbered)
}

/// Decides an import: the source, the final path and whether a book is
/// replaced. `busy` says a path is in use (`ERR_BUSY` is the caller's: this
/// only plans). Nothing is changed.
///
/// # Errors
/// `ERR_NOT_FOUND` for a source that is not there; `ERR_STATE` for a name that
/// exists without `replace` or `free_name`, or where none is free;
/// `ERR_DENIED` for a `replace` of a file the plugin did not import (or that
/// changed since); `ERR_NO_SPACE` where the path would be over 512 bytes; the
/// card's and memory's.
pub(crate) fn plan(
    storage: &dyn Storage,
    plugin: &str,
    library: &str,
    request: &ImportRequest<'_>,
) -> Result<Plan, i32> {
    let from = source(storage, plugin, request.from)?;
    let to = join(library, request.folder, request.name)?;
    if to.len() > PATH_MAX {
        return Err(ERR_NO_SPACE);
    }
    if !storage.exists(&to) {
        return Ok(Plan {
            from,
            to,
            replace: false,
        });
    }
    if request.replace {
        // Only the plugin's own import, still the file it imported.
        return if importer_is_by(storage, &to, plugin)? {
            Ok(Plan {
                from,
                to,
                replace: true,
            })
        } else {
            Err(ERR_DENIED)
        };
    }
    if !request.free_name {
        return Err(ERR_STATE);
    }
    for n in 2..=FREE_NAMES {
        let candidate = join(library, request.folder, &numbered(request.name, n)?)?;
        if candidate.len() > PATH_MAX {
            return Err(ERR_NO_SPACE);
        }
        if !storage.exists(&candidate) {
            return Ok(Plan {
                from,
                to: candidate,
                replace: false,
            });
        }
    }
    Err(ERR_STATE)
}

#[cfg(test)]
#[path = "lib_import_tests.rs"]
mod tests;
