//! The file the export writes: staged in the plugin's own data folder in
//! pieces of at most [`MAX_DATA`] bytes, then moved onto the card (replacing
//! the old file) when whole. A refusal anywhere (memory, a full card) removes
//! the staged file and leaves the old one as it was.
use crate::model::Sink;
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error, Host,
    files::Files,
    storage::{self, MAX_DATA, Root},
};

/// The staged file's name in the data folder (its own; `.pp-staged` is the
/// storage's).
pub const STAGED: &str = "export.part";

/// A file being staged. Dropped without [`Staged::put_in_place`], the staged
/// file is removed.
#[derive(Debug)]
pub struct Staged<'a, 'h> {
    host: &'a Host<'h>,
    buffer: Vec<u8>,
    begun: bool,
    done: bool,
}

impl<'a, 'h> Staged<'a, 'h> {
    /// A file to be written; what an earlier cut-off export left is not used.
    ///
    /// # Errors
    /// `NoMemory` for the piece.
    pub fn new(host: &'a Host<'h>) -> Result<Self, Error> {
        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(MAX_DATA)
            .map_err(|_| Error::NoMemory)?;
        Ok(Self {
            host,
            buffer,
            begun: false,
            done: false,
        })
    }

    /// Writes what is held to the staged file (the first time it is made
    /// anew).
    fn flush(&mut self) -> Result<(), Error> {
        if self.begun {
            storage::append(self.host, Root::Data, STAGED, &self.buffer)?;
        } else {
            storage::write(self.host, Root::Data, STAGED, &self.buffer)?;
        }
        self.begun = true;
        self.buffer.clear();
        Ok(())
    }

    /// Writes what is left and moves the file to `path` on the card in place
    /// of the old one, making `folder` first.
    ///
    /// # Errors
    /// The host's refusal; the staged file is removed then and the old one
    /// kept.
    pub fn put_in_place(
        mut self,
        files: &Files,
        folder: &str,
        path: &str,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        self.flush()?;
        storage::sync(self.host)?;
        files.make_dir(self.host, Root::Files, folder, input, out)?;
        files.move_entry(
            self.host,
            Root::Data,
            STAGED,
            Root::Files,
            path,
            true,
            input,
            out,
        )?;
        // Moved: nothing is left to remove.
        self.done = true;
        Ok(())
    }

    /// Removes the staged file (an export with nothing in it).
    pub fn discard(self) {}
}

impl Sink for Staged<'_, '_> {
    fn put(&mut self, text: &str) -> Result<(), Error> {
        for piece in text.as_bytes().chunks(MAX_DATA) {
            if self.buffer.len() + piece.len() > MAX_DATA {
                self.flush()?;
            }
            // Within what was reserved: no allocation.
            self.buffer.extend_from_slice(piece);
        }
        Ok(())
    }
}

impl Drop for Staged<'_, '_> {
    fn drop(&mut self) {
        if !self.done {
            // A leftover is written anew by the next export; nothing to tell.
            let _ = storage::remove(self.host, Root::Data, STAGED);
        }
    }
}
