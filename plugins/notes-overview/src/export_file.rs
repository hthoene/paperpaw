//! The `My Clippings.txt` file written to the card: a part file beside it,
//! appended to in pieces, renamed into place when whole. A refusal anywhere
//! (memory, a full card) removes the part and leaves the old file as it was.
//!
//! [`PartFile`] is a [`Sink`] over the platform [`Storage`] (the native shell
//! gives it the `core.files` adapter), so the export of any number of books
//! needs the piece in hand and nothing more. Every allocation is fallible.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::Error;
use paperpaw_sdk::{Storage, StorageError};

use crate::model::{FLUSH, Sink};

/// What was said of a storage failure: a full card is told as such, any
/// other failure as an I/O error.
fn storage_error(error: StorageError) -> Error {
    match error {
        StorageError::Full => Error::Full,
        _ => Error::Io,
    }
}

/// A file being written beside its place, and put in place by
/// [`PartFile::finish`]. Dropped without it, the part is removed.
pub struct PartFile<'a> {
    storage: &'a dyn Storage,
    path: String,
    part: String,
    buffer: Vec<u8>,
    begun: bool,
    done: bool,
}

impl core::fmt::Debug for PartFile<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PartFile")
            .field("path", &self.path)
            .field("held", &self.buffer.len())
            .finish()
    }
}

impl<'a> PartFile<'a> {
    /// A file `name` in `folder` (made if it is not there), to be written.
    ///
    /// # Errors
    /// [`Error::NoMemory`] for the names and the piece; the storage's
    /// refusal to make the folder.
    pub fn new(storage: &'a dyn Storage, folder: &str, name: &str) -> Result<Self, Error> {
        let mut path = String::new();
        join(&mut path, &[folder, "/", name])?;
        let mut part = String::new();
        join(&mut part, &[&path, ".part"])?;
        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(FLUSH)
            .map_err(|_| Error::NoMemory)?;
        storage.create_dir(folder).map_err(storage_error)?;
        Ok(Self {
            storage,
            path,
            part,
            buffer,
            begun: false,
            done: false,
        })
    }

    /// Appends what is held to the part (the first time it is made anew:
    /// what a cut-off export left is not used).
    fn flush(&mut self) -> Result<(), Error> {
        let sent = if self.begun {
            self.storage.append(&self.part, &self.buffer)
        } else {
            self.storage.write(&self.part, &self.buffer)
        };
        sent.map_err(storage_error)?;
        self.begun = true;
        self.buffer.clear();
        Ok(())
    }

    /// Writes what is left and puts the file in place of the old one: what
    /// was there stays until the new one is whole.
    ///
    /// # Errors
    /// The storage's refusal; the part is removed and the old file kept
    /// (unless the failed rename took it, as FAT may: then the part is all
    /// there is, and stays).
    pub fn finish(mut self) -> Result<(), Error> {
        self.flush()?;
        let had_old = self.storage.exists(&self.path);
        let moved = self.storage.rename(&self.part, &self.path);
        if moved.is_ok() || !had_old || self.storage.exists(&self.path) {
            // Nothing for Drop to remove where the part is where it belongs,
            // and a part beside a kept old file is not wanted.
            self.done = moved.is_ok();
        } else {
            // The old file went and the part did not arrive: keep the part.
            self.done = true;
        }
        moved.map_err(storage_error)
    }
}

impl Sink for PartFile<'_> {
    fn put(&mut self, text: &str) -> Result<(), Error> {
        for piece in text.as_bytes().chunks(FLUSH) {
            if self.buffer.len() + piece.len() > FLUSH {
                self.flush()?;
            }
            // Within what was reserved: no allocation.
            self.buffer.extend_from_slice(piece);
        }
        Ok(())
    }
}

impl Drop for PartFile<'_> {
    fn drop(&mut self) {
        if !self.done {
            // A leftover is overwritten by the next export; nothing to tell.
            let _ = self.storage.remove(&self.part);
        }
    }
}

/// `out` made of `pieces`, with fallible allocation.
fn join(out: &mut String, pieces: &[&str]) -> Result<(), Error> {
    for piece in pieces {
        out.try_reserve(piece.len()).map_err(|_| Error::NoMemory)?;
        out.push_str(piece);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
