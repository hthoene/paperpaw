//! The card as the file manager's model sees it: [`Storage`] over `core.files`
//! (folders, information, moves, removal) and the plugin storage calls on the
//! card root (reads, writes in pieces).
//!
//! The host calls are behind [`Card`] so that the translation (pages of a
//! folder put together, errors told as storage errors, a move that makes the
//! folder it goes into) is tested without a host; [`HostCard`] is the thin
//! implementation over the SDK.

use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_sdk::native::files::{self, Files};
use paperpaw_sdk::native::storage::{self, MAX_DATA, Root};
use paperpaw_sdk::native::{Error, Host};
use paperpaw_sdk::{DirEntry, Storage, StorageError};

/// What the card says of one file or folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Meta {
    pub is_dir: bool,
    pub size: u64,
    /// Seconds since 1970, where the card says.
    pub modified: Option<u64>,
}

/// One entry of a folder as a page gives it.
pub(crate) type Visit<'a> = dyn FnMut(&str, Meta) + 'a;

/// The host's calls on the card, one bounded piece of work each.
pub(crate) trait Card {
    /// Visits the entries of the page of folder `dir` at `cursor` (0 starts);
    /// the cursor of the next page, if there is one.
    fn list(&self, dir: &str, cursor: u64, visit: &mut Visit<'_>) -> Result<Option<u64>, Error>;
    fn stat(&self, path: &str) -> Result<Meta, Error>;
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, Error>;
    /// Replaces the file with `data` (at most [`MAX_DATA`]).
    fn write(&self, path: &str, data: &[u8]) -> Result<(), Error>;
    /// Appends `data` (at most [`MAX_DATA`]); creates the file.
    fn append(&self, path: &str, data: &[u8]) -> Result<(), Error>;
    fn remove(&self, path: &str) -> Result<(), Error>;
    fn make_dir(&self, path: &str) -> Result<(), Error>;
    fn remove_tree(&self, path: &str) -> Result<(), Error>;
    fn move_entry(&self, from: &str, to: &str, replace: bool) -> Result<(), Error>;
    fn sync(&self) -> Result<(), Error>;
    fn ignores_case(&self) -> bool;
}

/// A host error as the storage's.
pub(crate) fn storage_error(error: Error) -> StorageError {
    match error {
        Error::NotFound => StorageError::NotFound,
        Error::Full => StorageError::Full,
        // A path or a name over the cap.
        Error::NoSpace => StorageError::TooLarge,
        Error::Unsupported | Error::Denied => StorageError::Unsupported,
        Error::Invalid | Error::Utf8 => StorageError::InvalidPath,
        _ => StorageError::Io,
    }
}

/// [`Storage`] over a [`Card`], for the file manager's model.
pub(crate) struct CardStorage<C>(pub C);

impl<C> core::fmt::Debug for CardStorage<C> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CardStorage").finish_non_exhaustive()
    }
}

impl<C: Card> CardStorage<C> {
    fn pieces(
        data: &[u8],
        mut each: impl FnMut(&[u8]) -> Result<(), Error>,
    ) -> Result<(), StorageError> {
        for piece in data.chunks(MAX_DATA) {
            each(piece).map_err(storage_error)?;
        }
        Ok(())
    }
}

impl<C: Card> Storage for CardStorage<C> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let mut done = 0;
        while done < buf.len() {
            let end = buf.len().min(done + MAX_DATA);
            let Some(part) = buf.get_mut(done..end) else {
                break;
            };
            let at = offset.saturating_add(done as u64);
            let n = self.0.read_at(path, at, part).map_err(storage_error)?;
            done += n;
            // A short read is the end.
            if n < part.len() {
                break;
            }
        }
        Ok(done)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        // A folder is no file with a size: `is_dir` of the model asks
        // for "no file by that name".
        match self.0.stat(path).map_err(storage_error)? {
            Meta { is_dir: true, .. } => Err(StorageError::NotFound),
            meta => Ok(meta.size),
        }
    }

    /// Whole files of up to one piece only: the model writes nothing bigger
    /// (it creates empty files and appends), and a bigger write could not be
    /// made atomic here.
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if data.len() > MAX_DATA {
            return Err(StorageError::TooLarge);
        }
        self.0.write(path, data).map_err(storage_error)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if data.is_empty() {
            return self.0.append(path, data).map_err(storage_error);
        }
        Self::pieces(data, |piece| self.0.append(path, piece))
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        let mut cursor = 0;
        loop {
            let next = self
                .0
                .list(dir, cursor, &mut |name, meta| {
                    let entry = DirEntry::new(name, meta.is_dir, meta.size)
                        .with_modified(meta.modified.and_then(|t| i64::try_from(t).ok()));
                    visit(entry);
                })
                .map_err(storage_error)?;
            match next {
                Some(next) => cursor = next,
                None => return Ok(()),
            }
        }
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        match self.0.remove(path) {
            Ok(()) | Err(Error::NotFound) => Ok(()),
            Err(error) => Err(storage_error(error)),
        }
    }

    fn modified(&self, path: &str) -> Option<i64> {
        let meta = self.0.stat(path).ok()?;
        meta.modified.and_then(|t| i64::try_from(t).ok())
    }

    /// Replaces a file there; a folder is never replaced. The folder `to` goes
    /// into is made where it is not there.
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        match self.0.move_entry(from, to, true) {
            Err(Error::NotFound) if self.0.stat(from).is_ok() => {
                // The source is there: the folder above `to` is not.
                if let Some((parent, _)) = to.rsplit_once('/') {
                    self.0.make_dir(parent).map_err(storage_error)?;
                }
                self.0.move_entry(from, to, true).map_err(storage_error)
            }
            result => result.map_err(storage_error),
        }
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.make_dir(path).map_err(storage_error)
    }

    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        match self.0.remove_tree(path) {
            Ok(()) | Err(Error::NotFound) => Ok(()),
            Err(error) => Err(storage_error(error)),
        }
    }

    fn sync(&self) -> Result<(), StorageError> {
        self.0.sync().map_err(storage_error)
    }

    fn ignores_case(&self) -> bool {
        self.0.ignores_case()
    }
}

/// The card through `core.files` and the card root of the storage calls.
/// Opened for a callback and closed with [`HostCard::close`].
pub(crate) struct HostCard<'h, 'cb> {
    host: &'h Host<'cb>,
    files: Files,
    input: RefCell<Vec<u8>>,
    output: RefCell<Vec<u8>>,
    ignores_case: bool,
}

impl<'h, 'cb> HostCard<'h, 'cb> {
    /// Opens the service and takes its buffers (one allocation each).
    ///
    /// # Errors
    /// The host's refusal; [`Error::NoMemory`] for the buffers.
    pub(crate) fn open(host: &'h Host<'cb>) -> Result<Self, Error> {
        let files = Files::open(host)?;
        let made = (|| {
            let mut input = Vec::new();
            input
                .try_reserve_exact(files::INPUT_BYTES)
                .map_err(|_| Error::NoMemory)?;
            input.resize(files::INPUT_BYTES, 0);
            let mut output = Vec::new();
            output
                .try_reserve_exact(files::OUTPUT_BYTES)
                .map_err(|_| Error::NoMemory)?;
            output.resize(files::OUTPUT_BYTES, 0);
            // The card is FAT unless it says: names that differ in case
            // only are then one.
            let ignores_case = files
                .space(host, Root::Files, &mut input, &mut output)
                .map_or(true, |space| space.ignores_case());
            Ok((input, output, ignores_case))
        })();
        match made {
            Ok((input, output, ignores_case)) => Ok(Self {
                host,
                files,
                input: RefCell::new(input),
                output: RefCell::new(output),
                ignores_case,
            }),
            Err(error) => {
                files.release(host).ok();
                Err(error)
            }
        }
    }

    /// Gives the service back.
    ///
    /// # Errors
    /// The host's refusal.
    pub(crate) fn close(self) -> Result<(), Error> {
        self.files.release(self.host)
    }
}

/// Runs `work` on the card as a [`Storage`]; the service is given back after,
/// whatever `work` says.
///
/// # Errors
/// What opening, `work` or closing says; the first.
pub(crate) fn with_card<T>(
    host: &Host<'_>,
    work: impl FnOnce(&dyn Storage) -> Result<T, Error>,
) -> Result<T, Error> {
    let card = CardStorage(HostCard::open(host)?);
    let done = work(&card);
    let closed = card.0.close();
    let value = done?;
    closed?;
    Ok(value)
}

impl Card for HostCard<'_, '_> {
    fn list(&self, dir: &str, cursor: u64, visit: &mut Visit<'_>) -> Result<Option<u64>, Error> {
        let (mut input, mut output) = (self.input.borrow_mut(), self.output.borrow_mut());
        let listing =
            self.files
                .list(self.host, Root::Files, dir, cursor, &mut input, &mut output)?;
        for entry in listing.entries() {
            // A name the host cannot address again is not shown.
            if entry.unaddressable() {
                continue;
            }
            let meta = Meta {
                is_dir: entry.is_folder(),
                size: entry.size(),
                modified: entry.modified(),
            };
            visit(entry.name(), meta);
        }
        Ok(listing.next())
    }

    fn stat(&self, path: &str) -> Result<Meta, Error> {
        let (mut input, mut output) = (self.input.borrow_mut(), self.output.borrow_mut());
        let entry = self
            .files
            .stat(self.host, Root::Files, path, &mut input, &mut output)?;
        Ok(Meta {
            is_dir: entry.is_folder(),
            size: entry.size(),
            modified: entry.modified(),
        })
    }

    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, Error> {
        storage::read_at(self.host, Root::Files, path, offset, buf)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), Error> {
        storage::write(self.host, Root::Files, path, data)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), Error> {
        storage::append(self.host, Root::Files, path, data)
    }

    fn remove(&self, path: &str) -> Result<(), Error> {
        storage::remove(self.host, Root::Files, path)
    }

    fn make_dir(&self, path: &str) -> Result<(), Error> {
        let (mut input, mut output) = (self.input.borrow_mut(), self.output.borrow_mut());
        self.files
            .make_dir(self.host, Root::Files, path, &mut input, &mut output)
    }

    fn remove_tree(&self, path: &str) -> Result<(), Error> {
        let (mut input, mut output) = (self.input.borrow_mut(), self.output.borrow_mut());
        self.files
            .remove_tree(self.host, Root::Files, path, &mut input, &mut output)
            .map(|_| ())
    }

    fn move_entry(&self, from: &str, to: &str, replace: bool) -> Result<(), Error> {
        let (mut input, mut output) = (self.input.borrow_mut(), self.output.borrow_mut());
        self.files.move_entry(
            self.host,
            Root::Files,
            from,
            Root::Files,
            to,
            replace,
            &mut input,
            &mut output,
        )
    }

    fn sync(&self) -> Result<(), Error> {
        storage::sync(self.host)
    }

    fn ignores_case(&self) -> bool {
        self.ignores_case
    }
}

#[cfg(test)]
pub(crate) mod tests;
