//! What the file manager decides and does without a host: paths and names, a
//! folder's entries a page at a time in their order, and the changes (rename,
//! move, copy, delete) over the platform [`Storage`](paperpaw_sdk::Storage).
//!
//! The storage is the card as the host gives it (the native shell hands the
//! `core.files` adapter, whose folder reads are page by page), so nothing here
//! holds more than a page of entries, one path or one copy buffer, however big
//! a folder or a tree is. Every allocation is fallible: a refusal comes back as
//! [`Error::NoMemory`], never a panic, and a change that stops halfway leaves
//! the storage as `Storage` calls leave it (a copy is whole or not there).

pub mod folder;
pub mod ops;
pub mod path;

#[cfg(test)]
pub(crate) mod testing;

use paperpaw_sdk::StorageError;
pub use paperpaw_sdk::native::Error;

/// A storage failure, told as the host's errors are.
#[must_use]
pub fn error_of(error: StorageError) -> Error {
    match error {
        StorageError::NotFound => Error::NotFound,
        StorageError::InvalidPath | StorageError::InvalidData => Error::Invalid,
        StorageError::Unsupported => Error::Unsupported,
        StorageError::Full => Error::Full,
        StorageError::TooLarge => Error::NoSpace,
        _ => Error::Io,
    }
}
