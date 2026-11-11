//! The storage a `core.format` callback gives the parsers of this crate: the
//! book (and, for a `siblings` format, the files beside it) is root 3, and
//! everything below the plugin's own cache folder (`cache_prefix`, where the
//! interface-6 code keeps its conversions) is the document cache, root 6. The
//! host owns that cache per conversion, so the parsers' own stamps and
//! variants there are redundant but harmless.
use alloc::string::String;
use core::cmp::min;
use paperpaw_sdk::native::{
    Error, Host,
    storage::{self, Root},
};
use paperpaw_sdk::{DirEntry, Storage, StorageError};

/// The code a refused allocation travels as through `StorageError`.
pub(crate) const NO_MEMORY: u16 = 0x6e6d;

/// Roots 3 and 6 of the document being called, through the host.
pub struct NativeIo<'h, 'cb> {
    host: &'h Host<'cb>,
    prefix: String,
}

impl<'h, 'cb> NativeIo<'h, 'cb> {
    /// The roots of the call `host` belongs to; `cache_prefix` is the folder
    /// the parsers keep their conversions in.
    #[must_use]
    pub fn new(host: &'h Host<'cb>, cache_prefix: String) -> Self {
        Self {
            host,
            prefix: cache_prefix,
        }
    }
}

impl core::fmt::Debug for NativeIo<'_, '_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NativeIo").finish_non_exhaustive()
    }
}

/// Which root `path` is in, and its name there.
pub(crate) fn route<'a>(prefix: &str, path: &'a str) -> (Root, &'a str) {
    match path.strip_prefix(prefix) {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            (Root::DocumentCache, rest.trim_start_matches('/'))
        }
        _ => (Root::Book, path),
    }
}

pub(crate) fn storage_error(error: Error) -> StorageError {
    match error {
        Error::NotFound => StorageError::NotFound,
        Error::Full | Error::NoSpace => StorageError::Full,
        Error::Unsupported | Error::Denied => StorageError::Unsupported,
        Error::Invalid => StorageError::InvalidPath,
        // A refused allocation is not a damaged book: it keeps its name.
        Error::NoMemory => StorageError::Other(NO_MEMORY),
        _ => StorageError::Io,
    }
}

impl Storage for NativeIo<'_, '_> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let (root, name) = route(&self.prefix, path);
        let mut done = 0;
        while done < buf.len() {
            let end = min(buf.len(), done + storage::MAX_DATA);
            let Some(part) = buf.get_mut(done..end) else {
                break;
            };
            let at = offset.saturating_add(done as u64);
            let n = storage::read_at(self.host, root, name, at, part).map_err(storage_error)?;
            done += n;
            if n < part.len() {
                break;
            }
        }
        Ok(done)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        let (root, name) = route(&self.prefix, path);
        storage::size(self.host, root, name).map_err(storage_error)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let (root, name) = route(&self.prefix, path);
        if root == Root::Book {
            return Err(StorageError::Unsupported);
        }
        let mut pieces = data.chunks(storage::MAX_DATA);
        // The first piece makes the file new, the rest follow it.
        storage::write(self.host, root, name, pieces.next().unwrap_or_default())
            .map_err(storage_error)?;
        for piece in pieces {
            storage::append(self.host, root, name, piece).map_err(storage_error)?;
        }
        Ok(())
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let (root, name) = route(&self.prefix, path);
        if root == Root::Book {
            return Err(StorageError::Unsupported);
        }
        if data.is_empty() {
            return storage::append(self.host, root, name, data).map_err(storage_error);
        }
        for piece in data.chunks(storage::MAX_DATA) {
            storage::append(self.host, root, name, piece).map_err(storage_error)?;
        }
        Ok(())
    }

    /// Listing the book's folder needs `native::files` on root 3 (#384).
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        let (root, name) = route(&self.prefix, path);
        if root == Root::Book {
            return Err(StorageError::Unsupported);
        }
        storage::remove(self.host, root, name).map_err(storage_error)
    }

    /// The cache the host gives a conversion starts empty and is the host's to
    /// clear: what the interface-6 code removes to start a book over is not
    /// there, and the book's own root is read only.
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        match route(&self.prefix, path).0 {
            Root::Book => Err(StorageError::Unsupported),
            _ => Ok(()),
        }
    }

    /// Within the document cache (root 6, which a format may write); the
    /// book's own root is read only, and a move between roots is none.
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let (root, from) = route(&self.prefix, from);
        let (to_root, to) = route(&self.prefix, to);
        if root == Root::Book || to_root != root {
            return Err(StorageError::Unsupported);
        }
        storage::rename(self.host, root, from, to).map_err(storage_error)
    }
}

/// Answers one `core.format` call of `provider` over the roots of the call
/// `host` belongs to: what each native plugin of this family does in its
/// `event`. The cache folder the parsers use is copied fallibly.
///
/// # Errors
/// What [`FormatProvider::call`](crate::provider::FormatProvider::call) says;
/// `NoMemory` where the folder's name cannot be copied.
pub fn serve(
    provider: &mut crate::provider::FormatProvider,
    host: &Host<'_>,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, Error> {
    let mut prefix = String::new();
    prefix
        .try_reserve_exact(provider.cache().len())
        .map_err(|_| Error::NoMemory)?;
    prefix.push_str(provider.cache());
    provider.call(&NativeIo::new(host, prefix), input, out)
}
