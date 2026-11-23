//! What the file manager decides and does without a host: paths and names, a
//! folder's entries a page at a time in their order, and the changes (rename,
//! move, copy, delete) over the platform [`paperpaw_sdk::Storage`].
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

pub use paperpaw_sdk::native::Error;
use paperpaw_sdk::{Storage, StorageError};

/// The storage the file manager changes: a [`Storage`] that also says what a
/// path names.
pub trait Disk: Storage {
    /// Whether `path` names a folder (`true`) or a file (`false`). A storage
    /// that cannot ask for one entry lists the folder: what has no size and
    /// does not list is nothing.
    ///
    /// # Errors
    /// [`StorageError::NotFound`] where `path` names nothing; the storage's
    /// error where it cannot tell.
    fn is_folder(&self, path: &str) -> Result<bool, StorageError> {
        match self.size(path) {
            Ok(_) => Ok(false),
            Err(StorageError::NotFound) => self
                .list(path, &mut |_| {})
                .map(|()| true)
                .map_err(|_| StorageError::NotFound),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
impl Disk for paperpaw_sdk::MemoryStorage {}

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
