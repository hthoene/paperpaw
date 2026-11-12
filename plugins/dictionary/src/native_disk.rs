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

/// What a dictionary's block indexes are made from: the sizes of the files
/// still to be indexed. A dictionary that could not be indexed is tried again
/// only once they change.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn stamp(builders: &[paperpaw_stardict::Builder]) -> Result<String, Error> {
    let mut out = String::new();
    for (at, builder) in builders.iter().enumerate() {
        let mut digits = [0u8; 20];
        let mut size = builder.progress().1;
        let mut from = digits.len();
        loop {
            from -= 1;
            digits[from] = b'0' + (size % 10) as u8;
            size /= 10;
            if size == 0 {
                break;
            }
        }
        out.try_reserve(digits.len() - from + 1)
            .map_err(|_| Error::NoMemory)?;
        if at > 0 {
            out.push(' ');
        }
        for digit in &digits[from..] {
            out.push(char::from(*digit));
        }
    }
    Ok(out)
}

/// The file in a dictionary's cache that says it could not be indexed, and from
/// what ([`stamp`]).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn failed_file(cache: &str) -> Result<String, Error> {
    paperpaw_ui_kit::text::concat(&[cache, "/failed"])
}

/// Whether the dictionary with cache `cache` could not be indexed before, from
/// the files `stamp` names.
pub fn failed_before(storage: &dyn Storage, cache: &str, stamp: &str) -> bool {
    failed_file(cache).is_ok_and(|path| {
        paperpaw_sdk::read_file(storage, &path, 256).is_ok_and(|kept| kept == stamp.as_bytes())
    })
}

/// Whether the folder `name` of the data folder is the cache of a dictionary
/// that `kept` (the caches of those found, as [`cache_of`] names them) does not
/// hold: a name of 16 hexadecimal digits, as [`cache_of`] makes them, that no
/// dictionary found has.
#[must_use]
pub fn is_stale_cache(name: &str, kept: &[String]) -> bool {
    let ours = name.len() == 16 && name.bytes().all(|b| b.is_ascii_hexdigit());
    ours && !kept
        .iter()
        .any(|path| path.strip_prefix(CACHE) == Some(name))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;
    use alloc::{string::ToString, vec};

    #[test]
    fn only_the_caches_of_dictionaries_gone_are_stale() {
        let kept = vec![cache_of("d/a.ifo").unwrap()];
        let held = kept[0].strip_prefix(CACHE).unwrap().to_string();
        assert!(!is_stale_cache(&held, &kept));
        assert!(is_stale_cache("0123456789abcdef", &kept));
        // Names that are not a cache's are never the plugin's to remove.
        assert!(!is_stale_cache("settings", &kept));
        assert!(!is_stale_cache("0123456789abcdeg", &kept));
        assert!(!is_stale_cache("0123456789abcde", &kept));
        assert!(is_stale_cache(&held, &[]));
    }
}
