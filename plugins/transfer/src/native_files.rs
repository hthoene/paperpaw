//! The card as the page's requests see it: `core.files` root 1 (the user's
//! files), and the request bodies of the data folder (root 0).
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error, Host,
    files::{self, Files},
    storage::{self, Root},
};
use paperpaw_ui_kit::mem::buffer;

use crate::native_web::Card;

/// Pages of a folder read at most (32 entries each): well over a folder of
/// thousands of books.
const MOST_PAGES: usize = 4096;

/// A [`Card`] on the host, with its two buffers for the calls of one request.
pub struct HostCard<'a, 'h> {
    host: &'a Host<'h>,
    files: &'a Files,
    input: Vec<u8>,
    out: Vec<u8>,
}

impl<'a, 'h> HostCard<'a, 'h> {
    /// # Errors
    /// `NoMemory`.
    pub fn new(host: &'a Host<'h>, files: &'a Files) -> Result<Self, Error> {
        Ok(Self {
            host,
            files,
            input: buffer(files::INPUT_BYTES)?,
            out: buffer(files::OUTPUT_BYTES)?,
        })
    }
}

impl Card for HostCard<'_, '_> {
    fn stat(&mut self, path: &str) -> Result<Option<(bool, u64)>, Error> {
        let found = self
            .files
            .stat(self.host, Root::Files, path, &mut self.input, &mut self.out);
        match found {
            Ok(entry) => Ok(Some((entry.is_folder(), entry.size()))),
            Err(Error::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn list(
        &mut self,
        dir: &str,
        take: &mut dyn FnMut(&str, bool, u64) -> Result<bool, Error>,
    ) -> Result<(), Error> {
        let mut cursor = 0;
        for _ in 0..MOST_PAGES {
            let page = self.files.list(
                self.host,
                Root::Files,
                dir,
                cursor,
                &mut self.input,
                &mut self.out,
            )?;
            for entry in page.entries() {
                // Named in no path: not reachable by any request.
                if !entry.unaddressable() && !take(entry.name(), entry.is_folder(), entry.size())? {
                    return Ok(());
                }
            }
            match page.next() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        Ok(())
    }

    fn make_dir(&mut self, path: &str) -> Result<(), Error> {
        self.files
            .make_dir(self.host, Root::Files, path, &mut self.input, &mut self.out)
    }

    fn remove(&mut self, path: &str) -> Result<(), Error> {
        self.files
            .remove_tree(self.host, Root::Files, path, &mut self.input, &mut self.out)
            .map(|_| ())
    }

    fn free(&mut self) -> Result<Option<u64>, Error> {
        let space = self
            .files
            .space(self.host, Root::Files, &mut self.input, &mut self.out)?;
        Ok(space.free())
    }

    fn put(&mut self, body: Option<&str>, path: &str) -> Result<(), Error> {
        match body {
            // From the data folder to the card, never over what is there.
            Some(body) => self.files.move_entry(
                self.host,
                Root::Data,
                body,
                Root::Files,
                path,
                false,
                &mut self.input,
                &mut self.out,
            ),
            None => storage::write(self.host, Root::Files, path, &[]),
        }
    }
}
