//! Interface-7 storage slots 20-26 over [`Storage`]: root, path and
//! buffers in, `ERR_*` out. One adapter serves one plugin; nothing here is
//! a wire record, a pointer or a table entry.
//!
//! Root 0 is the plugin's data folder `.paperpaw/plugins/<id>/`. A path is
//! validated as a relative path first and joined under that folder after,
//! so it cannot name the install folder `plugins/<id>/` or anything else
//! outside (S5). Root 1 and later are reserved.
//!
//! The reserve (S4) trusts [`Storage::free_space`] and
//! [`Storage::total_space`]: `free_space` never says more than is free
//! (the FAT storage tells no count before it has counted its FAT, never
//! the FAT32 FSInfo count, which after a power cut may say more, #238).
//!
//! [`Storage`]: paperpaw_platform::Storage
//! [`Storage::free_space`]: paperpaw_platform::Storage::free_space
//! [`Storage::total_space`]: paperpaw_platform::Storage::total_space
//! This adapter only compares what it is told and never makes a count up.
//! Where a storage cannot say how much is free (yet), nothing is refused
//! here and the storage's own `Full` becomes `ERR_FULL`.
//!
//! Counting (S7) is the `Metered` storage the adapter is given: bytes read,
//! bytes written and `sync` calls of the plugin whose code runs.

use paperpaw_native::host_glue::{
    ERR_FULL, ERR_INVALID, ERR_IO, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_UNSUPPORTED, ERR_UTF8,
};
use paperpaw_platform::{Storage, StorageError};
use paperpaw_sdk::Id;

/// Root 0: the plugin's own data folder.
pub const ROOT_PLUGIN: u32 = 0;
/// Longest path a plugin passes, in bytes.
pub const MAX_PATH: usize = 512;
/// Longest segment of a path, in bytes.
pub const MAX_SEGMENT: usize = 255;
/// Most bytes one `write` or `append` takes, and one `read_at` gives.
pub const MAX_DATA: usize = 4096;
/// Longest joined path, in bytes.
const MAX_JOINED: usize = 640;
/// The smallest free-space reserve: 256 KiB.
pub const MIN_RESERVE: u64 = 256 * 1024;
const PREFIX: &str = ".paperpaw/plugins/";

/// Storage slots 20-26 for one plugin.
pub struct StorageAdapter<'a> {
    storage: &'a dyn Storage,
    /// `.paperpaw/plugins/<id>/`, kept; the one allocation of the adapter.
    folder: alloc::string::String,
}

impl core::fmt::Debug for StorageAdapter<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StorageAdapter")
            .field("folder", &self.folder)
            .finish_non_exhaustive()
    }
}

impl<'a> StorageAdapter<'a> {
    /// The adapter of plugin `id` over `storage` (hand it the `Metered`
    /// one to have the plugin's use counted). `None` for an invalid id.
    #[must_use]
    pub fn new(storage: &'a dyn Storage, id: &str) -> Option<Self> {
        Id::parse(id).ok()?;
        let mut folder = alloc::string::String::with_capacity(PREFIX.len() + id.len() + 1);
        folder.push_str(PREFIX);
        folder.push_str(id);
        folder.push('/');
        (folder.len() + MAX_PATH <= MAX_JOINED).then_some(Self { storage, folder })
    }

    /// The reserve to keep free: the larger of 256 KiB and 1 % of the card.
    fn reserve(&self) -> u64 {
        let one_percent = self.storage.total_space().map_or(0, |total| total / 100);
        one_percent.max(MIN_RESERVE)
    }

    /// `ERR_FULL` when `len` more bytes would leave less than the reserve.
    fn check_room(&self, len: usize) -> Result<(), i32> {
        match self.storage.free_space() {
            Some(free) if free.saturating_sub(len as u64) < self.reserve() => Err(ERR_FULL),
            _ => Ok(()),
        }
    }

    /// Validates `path` (root first, then size, UTF-8 and grammar) and
    /// calls `with` with it joined under the data folder.
    fn resolve<R>(
        &self,
        root: u32,
        path: &[u8],
        with: impl FnOnce(&str) -> Result<R, i32>,
    ) -> Result<R, i32> {
        if root != ROOT_PLUGIN {
            return Err(ERR_UNSUPPORTED);
        }
        let relative = relative_path(path)?;
        let mut buf = [0u8; MAX_JOINED];
        let (head, tail) = buf.split_at_mut(self.folder.len());
        head.copy_from_slice(self.folder.as_bytes());
        let tail = tail.get_mut(..relative.len()).ok_or(ERR_NO_SPACE)?;
        tail.copy_from_slice(relative.as_bytes());
        let joined = &buf[..self.folder.len() + relative.len()];
        let joined = core::str::from_utf8(joined).map_err(|_| ERR_UTF8)?;
        // The joined path is checked again (S5): it stays below the folder.
        if !joined.starts_with(self.folder.as_str())
            || !paperpaw_platform::storage::is_valid_path(joined)
        {
            return Err(ERR_INVALID);
        }
        with(joined)
    }

    /// Runs `act` on `joined`; where the storage says the folder above it is
    /// not there (`NotFound`), makes the folders and runs it once more. So the
    /// common case, a file in a folder that exists, costs no directory lookup.
    fn with_parents(
        &self,
        joined: &str,
        act: impl Fn(&str) -> Result<(), StorageError>,
    ) -> Result<(), i32> {
        match act(joined) {
            Err(StorageError::NotFound) => {
                self.make_parents(joined)?;
                act(joined).map_err(status)
            }
            other => other.map_err(status),
        }
    }

    /// Makes the folders above `joined`, where the storage needs them made.
    /// A folder takes a cluster: the reserve is checked first (S4).
    fn make_parents(&self, joined: &str) -> Result<(), i32> {
        let Some((parent, _)) = joined.rsplit_once('/') else {
            return Ok(());
        };
        self.check_room(0)?;
        match self.storage.create_dir(parent) {
            Ok(()) | Err(StorageError::Unsupported) => Ok(()),
            Err(e) => Err(status(e)),
        }
    }

    /// Slot 20: the size of the file at `path`. `ERR_NOT_FOUND` if none.
    ///
    /// # Errors
    /// An `ERR_*` status; nothing is touched.
    pub fn size(&self, root: u32, path: &[u8]) -> Result<u64, i32> {
        self.resolve(root, path, |p| self.storage.size(p).map_err(status))
    }

    /// Slot 21: reads into `out` from `offset`; the count of bytes copied,
    /// 0 at the end of the file. `out` longer than 4096 bytes is
    /// `ERR_NO_SPACE`.
    ///
    /// # Errors
    /// An `ERR_*` status; nothing is touched.
    pub fn read_at(
        &self,
        root: u32,
        path: &[u8],
        offset: u64,
        out: &mut [u8],
    ) -> Result<usize, i32> {
        if out.len() > MAX_DATA {
            return Err(ERR_NO_SPACE);
        }
        self.resolve(root, path, |p| {
            self.storage.read_at(p, offset, out).map_err(status)
        })
    }

    /// Slot 22: replaces the file with `data` (at most 4096 bytes), whole
    /// or not at all. The reserve is checked first (S4): `ERR_FULL`
    /// writes nothing.
    ///
    /// # Errors
    /// An `ERR_*` status.
    pub fn write(&self, root: u32, path: &[u8], data: &[u8]) -> Result<(), i32> {
        self.put(root, path, data, |p| self.storage.write(p, data))
    }

    /// Slot 23: appends `data` (at most 4096 bytes), kept back until
    /// [`sync`](Self::sync). The reserve is checked first (S4), but after
    /// `ERR_FULL` or `ERR_IO` a partial tail may remain in the file.
    ///
    /// # Errors
    /// An `ERR_*` status.
    pub fn append(&self, root: u32, path: &[u8], data: &[u8]) -> Result<(), i32> {
        self.put(root, path, data, |p| self.storage.append(p, data))
    }

    fn put(
        &self,
        root: u32,
        path: &[u8],
        data: &[u8],
        act: impl Fn(&str) -> Result<(), StorageError>,
    ) -> Result<(), i32> {
        if data.len() > MAX_DATA {
            return Err(ERR_NO_SPACE);
        }
        self.resolve(root, path, |p| {
            self.check_room(data.len())?;
            self.with_parents(p, &act)
        })
    }

    /// Slot 24: renames `from` to `to`, replacing a file there.
    /// `ERR_NOT_FOUND` if `from` is none. No reserve check where no folder
    /// is made: a rename takes no space, and on a full card the S1 recipe
    /// still publishes its temp file.
    ///
    /// # Errors
    /// An `ERR_*` status.
    pub fn rename(&self, root: u32, from: &[u8], to: &[u8]) -> Result<(), i32> {
        self.resolve(root, from, |f| {
            self.resolve(root, to, |t| match self.storage.rename(f, t) {
                // A missing `from` is NotFound too: folders above `to` are
                // made only where `from` is there.
                Err(StorageError::NotFound) if self.storage.exists(f) => {
                    self.make_parents(t)?;
                    self.storage.rename(f, t).map_err(status)
                }
                other => other.map_err(status),
            })
        })
    }

    /// Slot 25: removes the file; a missing one is fine.
    ///
    /// # Errors
    /// An `ERR_*` status.
    pub fn remove(&self, root: u32, path: &[u8]) -> Result<(), i32> {
        self.resolve(root, path, |p| match self.storage.remove(p) {
            Ok(()) | Err(StorageError::NotFound) => Ok(()),
            Err(e) => Err(status(e)),
        })
    }

    /// Slot 26: writes back everything kept back, of the whole device.
    ///
    /// # Errors
    /// An `ERR_*` status.
    pub fn sync(&self) -> Result<(), i32> {
        self.storage.sync().map_err(status)
    }
}

/// `path` as a relative path: sizes, UTF-8 and grammar, nothing joined.
fn relative_path(path: &[u8]) -> Result<&str, i32> {
    if path.len() > MAX_PATH {
        return Err(ERR_NO_SPACE);
    }
    let text = core::str::from_utf8(path).map_err(|_| ERR_UTF8)?;
    if text.split('/').any(|s| s.len() > MAX_SEGMENT) {
        return Err(ERR_NO_SPACE);
    }
    if !paperpaw_platform::storage::is_valid_path(text) {
        return Err(ERR_INVALID);
    }
    Ok(text)
}

/// The status of a storage failure.
fn status(error: StorageError) -> i32 {
    match error {
        StorageError::NotFound => ERR_NOT_FOUND,
        StorageError::InvalidPath => ERR_INVALID,
        StorageError::Unsupported => ERR_UNSUPPORTED,
        StorageError::Full => ERR_FULL,
        StorageError::TooLarge => ERR_NO_SPACE,
        _ => ERR_IO,
    }
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
