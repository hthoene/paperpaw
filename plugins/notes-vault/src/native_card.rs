//! The vault on the card, read only: [`Storage`] over `core.files` (folder
//! listings, information) and the card root of the storage calls (reads).
//!
//! The vault is never changed (`PORTING-7.md`), so writing, moving and removal
//! answer [`StorageError::Unsupported`]. A refused allocation is remembered
//! (`Storage` has no word for it, so it says [`StorageError::Io`]) and told as
//! [`Error::NoMemory`] by [`with_card`]. This is a small copy of what the file
//! manager's card adapter does for writing too; it moves into the kit when
//! that one does.
use alloc::vec::Vec;
use core::cell::{Cell, RefCell, RefMut};

use paperpaw_sdk::native::files::{self, Files};
use paperpaw_sdk::native::storage::{self, MAX_DATA, Root};
use paperpaw_sdk::native::{Error, Host};
use paperpaw_sdk::{DirEntry, Storage, StorageError};

/// A host error as the storage's.
fn storage_error(error: Error) -> StorageError {
    match error {
        Error::NotFound => StorageError::NotFound,
        Error::Full => StorageError::Full,
        Error::NoSpace => StorageError::TooLarge,
        Error::Unsupported | Error::Denied => StorageError::Unsupported,
        Error::Invalid | Error::Utf8 => StorageError::InvalidPath,
        // Not the card's fault, and not a missing card either.
        _ => StorageError::Io,
    }
}

/// The card through `core.files` for one callback.
struct Card<'h, 'cb> {
    host: &'h Host<'cb>,
    files: Files,
    input: RefCell<Vec<u8>>,
    output: RefCell<Vec<u8>>,
    ignores_case: bool,
    out_of_memory: Cell<bool>,
}

type Buffers<'a> = (RefMut<'a, Vec<u8>>, RefMut<'a, Vec<u8>>);

impl<'h, 'cb> Card<'h, 'cb> {
    fn open(host: &'h Host<'cb>) -> Result<Self, Error> {
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
            // The card is FAT unless it says: names that differ in case only
            // are then one.
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
                out_of_memory: Cell::new(false),
            }),
            Err(error) => {
                files.release(host).ok();
                Err(error)
            }
        }
    }

    /// Both buffers of a call; [`Error::Busy`] where a call is already using
    /// them (a nested call must not panic).
    fn buffers(&self) -> Result<Buffers<'_>, Error> {
        match (self.input.try_borrow_mut(), self.output.try_borrow_mut()) {
            (Ok(input), Ok(output)) => Ok((input, output)),
            _ => Err(Error::Busy),
        }
    }

    fn noted<T>(&self, result: Result<T, Error>) -> Result<T, StorageError> {
        result.map_err(|error| {
            if error == Error::NoMemory {
                self.out_of_memory.set(true);
            }
            storage_error(error)
        })
    }
}

impl Storage for Card<'_, '_> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let mut done = 0;
        while done < buf.len() {
            let end = buf.len().min(done + MAX_DATA);
            let Some(part) = buf.get_mut(done..end) else {
                break;
            };
            let at = offset.saturating_add(done as u64);
            let n = self.noted(storage::read_at(self.host, Root::Files, path, at, part))?;
            done += n;
            // A short read is the end.
            if n < part.len() {
                break;
            }
        }
        Ok(done)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        let (mut input, mut output) = self.noted(self.buffers())?;
        let entry =
            self.noted(
                self.files
                    .stat(self.host, Root::Files, path, &mut input, &mut output),
            )?;
        // A folder is no file with a size.
        if entry.is_folder() {
            Err(StorageError::NotFound)
        } else {
            Ok(entry.size())
        }
    }

    fn write(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }

    fn append(&self, _: &str, _: &[u8]) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        let mut cursor = 0;
        loop {
            let (mut input, mut output) = self.noted(self.buffers())?;
            let listing = self.noted(self.files.list(
                self.host,
                Root::Files,
                dir,
                cursor,
                &mut input,
                &mut output,
            ))?;
            for entry in listing.entries() {
                // A name the host cannot address again is not shown.
                if entry.unaddressable() {
                    continue;
                }
                visit(
                    DirEntry::new(entry.name(), entry.is_folder(), entry.size())
                        .with_modified(entry.modified().and_then(|t| i64::try_from(t).ok())),
                );
            }
            match listing.next() {
                Some(next) => cursor = next,
                None => return Ok(()),
            }
        }
    }

    fn remove(&self, _: &str) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }

    fn modified(&self, path: &str) -> Option<i64> {
        let (mut input, mut output) = self.buffers().ok()?;
        let entry = self
            .noted(
                self.files
                    .stat(self.host, Root::Files, path, &mut input, &mut output),
            )
            .ok()?;
        entry.modified().and_then(|t| i64::try_from(t).ok())
    }

    fn ignores_case(&self) -> bool {
        self.ignores_case
    }
}

/// Runs `work` on the card as a read-only [`Storage`]; the service is given
/// back after, whatever `work` says.
///
/// The model keeps a read that failed as "nothing there" (a card taken out is
/// an empty folder), so a refused allocation, which `Storage` cannot word, is
/// told here whatever `work` returned.
///
/// # Errors
/// [`Error::NoMemory`] where the host ran out of memory on the way; what
/// opening, `work` or closing says, the first.
pub fn with_card<T>(
    host: &Host<'_>,
    work: impl FnOnce(&dyn Storage) -> Result<T, Error>,
) -> Result<T, Error> {
    let card = Card::open(host)?;
    let done = work(&card);
    let memory = card.out_of_memory.get();
    let Card { files, .. } = card;
    let closed = files.release(host);
    if memory {
        return Err(Error::NoMemory);
    }
    let value = done?;
    closed?;
    Ok(value)
}
