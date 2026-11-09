//! What the StarDict reader sees of the host's files: the user's dictionaries
//! (root 1, read only) and the indexes made from them (the plugin's data
//! folder, root 0), as one `Storage` whose paths say where they are.
use alloc::string::String;
use core::cmp::min;
use paperpaw_sdk::{
    DirEntry, Storage, StorageError,
    native::{
        Error, Host,
        storage::{self, Root},
    },
};

/// The code a refused allocation travels as through `StorageError`.
pub const NO_MEMORY: u16 = 0x6e6d;
/// A path that starts here is in the data folder; its characters cannot be in
/// a path of the card, so no dictionary is ever mistaken for an index.
pub const CACHE: &str = "\u{1}c/";

/// The error a host call gave, as the StarDict reader takes it.
#[must_use]
pub fn storage_error(error: Error) -> StorageError {
    match error {
        Error::NotFound => StorageError::NotFound,
        Error::Full | Error::NoSpace => StorageError::Full,
        Error::Unsupported | Error::Denied => StorageError::Unsupported,
        Error::Invalid => StorageError::InvalidPath,
        Error::NoMemory => StorageError::Other(NO_MEMORY),
        _ => StorageError::Io,
    }
}

/// The error of a failed read, as the model takes it: a refused allocation
/// keeps its name.
#[must_use]
pub fn model_error(error: &paperpaw_stardict::Error) -> Error {
    match error {
        paperpaw_stardict::Error::OutOfMemory
        | paperpaw_stardict::Error::Storage(StorageError::Other(NO_MEMORY)) => Error::NoMemory,
        paperpaw_stardict::Error::Storage(StorageError::NotFound) => Error::NotFound,
        _ => Error::Invalid,
    }
}

/// One root of the host's storage.
pub struct HostRoot<'h, 'cb> {
    host: &'h Host<'cb>,
    root: Root,
}

impl<'h, 'cb> HostRoot<'h, 'cb> {
    /// The user's files on the card.
    #[must_use]
    pub const fn files(host: &'h Host<'cb>) -> Self {
        Self {
            host,
            root: Root::Files,
        }
    }

    /// The plugin's data folder.
    #[must_use]
    pub const fn data(host: &'h Host<'cb>) -> Self {
        Self {
            host,
            root: Root::Data,
        }
    }
}

impl core::fmt::Debug for HostRoot<'_, '_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HostRoot").finish_non_exhaustive()
    }
}

impl Storage for HostRoot<'_, '_> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let mut done = 0;
        while done < buf.len() {
            let end = min(buf.len(), done + storage::MAX_DATA);
            let Some(part) = buf.get_mut(done..end) else {
                break;
            };
            let at = offset.saturating_add(done as u64);
            let n =
                storage::read_at(self.host, self.root, path, at, part).map_err(storage_error)?;
            done += n;
            if n < part.len() {
                break;
            }
        }
        Ok(done)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        storage::size(self.host, self.root, path).map_err(storage_error)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if self.root == Root::Files {
            return Err(StorageError::Unsupported);
        }
        let mut pieces = data.chunks(storage::MAX_DATA);
        storage::write(
            self.host,
            self.root,
            path,
            pieces.next().unwrap_or_default(),
        )
        .map_err(storage_error)?;
        for piece in pieces {
            storage::append(self.host, self.root, path, piece).map_err(storage_error)?;
        }
        Ok(())
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if self.root == Root::Files {
            return Err(StorageError::Unsupported);
        }
        if data.is_empty() {
            return storage::append(self.host, self.root, path, data).map_err(storage_error);
        }
        for piece in data.chunks(storage::MAX_DATA) {
            storage::append(self.host, self.root, path, piece).map_err(storage_error)?;
        }
        Ok(())
    }

    /// Listing goes through `core.files` ([`crate::native_folders`]).
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        if self.root == Root::Files {
            return Err(StorageError::Unsupported);
        }
        storage::remove(self.host, self.root, path).map_err(storage_error)
    }
}

/// The dictionaries' files and the indexes' folder as one storage: a path that
/// starts with [`CACHE`] is in `data`, any other in `files`.
pub struct Routed<'a> {
    /// Where the dictionaries are.
    pub files: &'a dyn Storage,
    /// Where the indexes are made.
    pub data: &'a dyn Storage,
}

impl core::fmt::Debug for Routed<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Routed").finish_non_exhaustive()
    }
}

impl Routed<'_> {
    fn pick<'p>(&self, path: &'p str) -> (&dyn Storage, &'p str) {
        match path.strip_prefix(CACHE) {
            Some(name) => (self.data, name),
            None => (self.files, path),
        }
    }
}

impl Storage for Routed<'_> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let (side, name) = self.pick(path);
        side.read_at(name, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        let (side, name) = self.pick(path);
        side.size(name)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let (side, name) = self.pick(path);
        side.write(name, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let (side, name) = self.pick(path);
        side.append(name, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        let (side, name) = self.pick(dir);
        side.list(name, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        let (side, name) = self.pick(path);
        side.remove(name)
    }
}

/// The folder of the indexes of the dictionary `ifo`: named by a hash of its
/// path, in the data folder.
///
/// # Errors
/// [`Error::NoMemory`] when the name cannot be made.
pub fn cache_of(ifo: &str) -> Result<String, Error> {
    let hash = paperpaw_sdk::digest::fnv1a64(ifo.as_bytes());
    let mut name = String::new();
    name.try_reserve_exact(CACHE.len() + 16)
        .map_err(|_| Error::NoMemory)?;
    name.push_str(CACHE);
    for shift in (0..16).rev() {
        let nibble = ((hash >> (shift * 4)) & 0xf) as u8;
        name.push(char::from(b"0123456789abcdef"[usize::from(nibble)]));
    }
    Ok(name)
}
