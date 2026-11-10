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

use super::format_scope::{BookKind, FormatBook};
use alloc::borrow::Cow;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_DENIED, ERR_FULL, ERR_INVALID, ERR_IO, ERR_NO_MEMORY, ERR_NO_SPACE,
    ERR_NOT_FOUND, ERR_UNSUPPORTED, ERR_UTF8,
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
pub(super) const MAX_JOINED: usize = 640;
/// Longest plugin folder that leaves room for every permitted path.
pub(super) const MAX_FOLDER: usize = MAX_JOINED - MAX_PATH;
/// The smallest free-space reserve: 256 KiB.
pub const MIN_RESERVE: u64 = 256 * 1024;
pub(super) const PREFIX: &str = ".paperpaw/plugins/";

/// Storage slots 20-26 for one plugin.
pub struct StorageAdapter<'a> {
    storage: &'a dyn Storage,
    /// Owned for the public constructor, borrowed for a scoped raw callback.
    folder: Cow<'a, str>,
    /// The paths the host's own writers hold (`core.net` saves, `core.sockets`
    /// reads to a file, with their staged files): a write to one is
    /// `ERR_BUSY`, so the plugin's own `name.pp-staged` recipe can never meet
    /// the host's temporary file.
    in_use: Option<&'a super::paths_in_use::PathsInUse>,
    /// The document of the format call the plugin is in, for roots 3, 4 and 6
    /// (`document.md` 3, Roots); `None` outside such a call.
    format: Option<&'a FormatBook<'a>>,
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
        (folder.len() <= MAX_FOLDER).then_some(Self {
            storage,
            folder: Cow::Owned(folder),
            in_use: None,
            format: None,
        })
    }

    /// Borrow a stack-built plugin folder for one synchronous host callback.
    /// Validate the route without parsing an allocating SDK Id.
    pub(crate) fn borrowed_folder(
        storage: &'a dyn Storage,
        folder: &'a str,
        in_use: Option<&'a super::paths_in_use::PathsInUse>,
    ) -> Option<Self> {
        let owner = folder.strip_prefix(PREFIX)?.strip_suffix('/')?;
        if folder.len() > MAX_FOLDER
            || owner.contains('/')
            || relative_path(owner.as_bytes()).is_err()
        {
            return None;
        }
        Some(Self {
            storage,
            folder: Cow::Borrowed(folder),
            in_use,
            format: None,
        })
    }

    /// The adapter for a call of a format plugin: roots 3, 4 and 6 are the
    /// document of `book`.
    pub(crate) fn with_format(mut self, book: &'a FormatBook<'a>) -> Self {
        self.format = Some(book);
        self
    }

    /// `ERR_FULL` when `len` more bytes would leave less than the reserve.
    fn check_room(&self, len: usize) -> Result<(), i32> {
        check_room(self.storage, len)
    }

    /// Validates `path` (root first, then size, UTF-8 and grammar) and
    /// calls `with` with it joined under the data folder.
    fn resolve<R>(
        &self,
        root: u32,
        path: &[u8],
        write: bool,
        with: impl FnOnce(&str) -> Result<R, i32>,
    ) -> Result<R, i32> {
        if root != ROOT_PLUGIN {
            return self.resolve_format(root, path, write, with);
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
        if !joined.starts_with(self.folder.as_ref())
            || !paperpaw_platform::storage::is_valid_path(joined)
        {
            return Err(ERR_INVALID);
        }
        with(joined)
    }

    /// Roots 3 (the book), 4 (the user's CSS file) and 6 (the document's
    /// cache), for a format plugin inside a call for a document. In order:
    /// the root (`ERR_UNSUPPORTED` for any other, and for these outside such a
    /// call or where the call has none: a document without a cache folder, a
    /// call that does not read the CSS file), `ERR_DENIED` for a change of
    /// root 3 or 4, the path rules, then what the root names. Root 6 keeps
    /// `.complete` and `.host/` for the host: a change is `ERR_DENIED`, a read
    /// `ERR_NOT_FOUND`.
    #[inline(never)]
    fn resolve_format<R>(
        &self,
        root: u32,
        path: &[u8],
        write: bool,
        with: impl FnOnce(&str) -> Result<R, i32>,
    ) -> Result<R, i32> {
        const BOOK: u32 = 3;
        const CSS: u32 = 4;
        const CACHE: u32 = 6;
        if !matches!(root, BOOK | CSS | CACHE) {
            return Err(ERR_UNSUPPORTED);
        }
        let book = self.format.ok_or(ERR_UNSUPPORTED)?;
        let (base, single) = match root {
            BOOK => match book.kind {
                BookKind::File => (book.path, true),
                _ => (book.folder().unwrap_or(""), false),
            },
            CSS if book.css.is_empty() => return Err(ERR_UNSUPPORTED),
            CSS => (book.css, true),
            _ if book.cache.is_empty() => return Err(ERR_UNSUPPORTED),
            _ => (book.cache, false),
        };
        if write && root != CACHE {
            return Err(ERR_DENIED);
        }
        // The empty path names the book's file or the CSS file; a cache has no
        // file at its top (a change of it would be the cache folder's).
        let relative = if path.is_empty() && root != CACHE {
            ""
        } else {
            relative_path(path)?
        };
        // A call without the file it is for (a book with no path) has no root.
        if single && base.is_empty() {
            return Err(ERR_UNSUPPORTED);
        }
        if single {
            // One file: its name is the path "".
            return if relative.is_empty() {
                with(base)
            } else {
                Err(ERR_NOT_FOUND)
            };
        }
        let first = relative.split('/').next().unwrap_or_default();
        if root == CACHE && reserved_in_cache(first, relative) {
            return Err(if write { ERR_DENIED } else { ERR_NOT_FOUND });
        }
        // A book at the top of the card must not reach the kernel's folders.
        if root == BOOK && base.is_empty() && !first.is_empty() && reaches_kernel(first) {
            return Err(ERR_INVALID);
        }
        let mut buf = [0u8; MAX_JOINED];
        let slash = usize::from(!base.is_empty() && !relative.is_empty());
        let end = base.len() + slash + relative.len();
        let target = buf.get_mut(..end).ok_or(ERR_NO_SPACE)?;
        target[..base.len()].copy_from_slice(base.as_bytes());
        if slash == 1 {
            target[base.len()] = b'/';
        }
        target[base.len() + slash..].copy_from_slice(relative.as_bytes());
        let joined = core::str::from_utf8(target).map_err(|_| ERR_UTF8)?;
        if !joined.is_empty() && !paperpaw_platform::storage::is_valid_path(joined) {
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

    /// `ERR_BUSY` when a host writer holds `joined` (a file it is saving to, or
    /// its staged file).
    fn free_of_host_writers(&self, joined: &str) -> Result<(), i32> {
        match self.in_use {
            Some(table) if table.holds(joined, self.storage.ignores_case()) => Err(ERR_BUSY),
            _ => Ok(()),
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
        self.resolve(root, path, false, |p| self.storage.size(p).map_err(status))
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
        self.resolve(root, path, false, |p| {
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
        self.put(root, path, data, true, |p| self.storage.write(p, data))
    }

    /// Slot 23: appends `data` (at most 4096 bytes), kept back until
    /// [`sync`](Self::sync). The reserve is checked first (S4), but after
    /// `ERR_FULL` or `ERR_IO` a partial tail may remain in the file.
    ///
    /// # Errors
    /// An `ERR_*` status.
    pub fn append(&self, root: u32, path: &[u8], data: &[u8]) -> Result<(), i32> {
        self.put(root, path, data, false, |p| self.storage.append(p, data))
    }

    fn put(
        &self,
        root: u32,
        path: &[u8],
        data: &[u8],
        replaces: bool,
        act: impl Fn(&str) -> Result<(), StorageError>,
    ) -> Result<(), i32> {
        if data.len() > MAX_DATA {
            return Err(ERR_NO_SPACE);
        }
        self.resolve(root, path, true, |p| {
            self.free_of_host_writers(p)?;
            self.check_room(data.len())?;
            // A write replaces the file: what it held is not counted twice.
            let credit = if replaces && root == 6 {
                self.size_of(p)
            } else {
                0
            };
            self.charge(root, data.len() as u64, credit)?;
            let done = self.with_parents(p, &act);
            if done.is_err() {
                // The write failed: the cache holds what it held (a write is
                // whole or not at all), so the charge goes back.
                let _ = self.charge(root, credit, data.len() as u64);
            }
            done
        })
    }

    /// What was taken from the cache stops counting; a count that cannot be
    /// had changes nothing (it only overcounts).
    fn credit(&self, root: u32, bytes: u64) {
        let _ = self.charge(root, 0, bytes);
    }

    /// The bytes of the file at `p`, 0 where there is none.
    fn size_of(&self, p: &str) -> u64 {
        self.storage.size(p).unwrap_or(0)
    }

    /// The cache's count (root 6) of the call's document: `add` bytes more,
    /// `credit` fewer (`ERR_NO_SPACE` over the cap). The other roots are not
    /// counted here.
    fn charge(&self, root: u32, add: u64, credit: u64) -> Result<(), i32> {
        let (6, Some(book)) = (root, self.format) else {
            return Ok(());
        };
        match book.quota {
            Some(scope) => scope.charge(book.cache, add, credit, book.cache_cap),
            None => Ok(()),
        }
    }

    /// Slot 24: renames `from` to `to`, replacing a file there.
    /// `ERR_NOT_FOUND` if `from` is none. No reserve check where no folder
    /// is made: a rename takes no space, and on a full card the S1 recipe
    /// still publishes its temp file.
    ///
    /// # Errors
    /// An `ERR_*` status.
    pub fn rename(&self, root: u32, from: &[u8], to: &[u8]) -> Result<(), i32> {
        self.resolve(root, from, true, |f| {
            self.resolve(root, to, true, |t| {
                // Neither end may be a file a host writer holds.
                self.free_of_host_writers(f)?;
                self.free_of_host_writers(t)?;
                // What the rename replaces is not in the cache any more.
                let credit = if root == 6 { self.size_of(t) } else { 0 };
                let moved = match self.storage.rename(f, t) {
                    // A missing `from` is NotFound too: folders above `to` are
                    // made only where `from` is there.
                    Err(StorageError::NotFound) if self.storage.exists(f) => {
                        self.make_parents(t)?;
                        self.storage.rename(f, t).map_err(status)
                    }
                    other => other.map_err(status),
                };
                if moved.is_ok() {
                    self.credit(root, credit);
                }
                moved
            })
        })
    }

    /// Slot 25: removes the file; a missing one is fine.
    ///
    /// # Errors
    /// An `ERR_*` status.
    pub fn remove(&self, root: u32, path: &[u8]) -> Result<(), i32> {
        self.resolve(root, path, true, |p| {
            self.free_of_host_writers(p)?;
            let credit = if root == 6 { self.size_of(p) } else { 0 };
            match self.storage.remove(p) {
                Ok(()) => {
                    self.credit(root, credit);
                    Ok(())
                }
                Err(StorageError::NotFound) => Ok(()),
                Err(e) => Err(status(e)),
            }
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

/// Whether `first` (the first segment of `relative`) or the whole path names
/// what the host keeps in a document's cache: the stamp `.complete` at the
/// top, anything under `.host`. A name with an 8.3 tail (`~`) at the top of the
/// cache might be another spelling of either on a FAT card, so it is refused
/// too (a cache file of a format named so is made under another name).
fn reserved_in_cache(first: &str, relative: &str) -> bool {
    let same = super::files_service::same_fold;
    same(first, ".host")
        || same(relative, ".complete")
        || (!relative.contains('/') && first.contains('~'))
}

/// Whether the first segment of a card path is one of the kernel's folders,
/// in any spelling the card takes for one.
fn reaches_kernel(first: &str) -> bool {
    let same = super::files_service::same_fold;
    same(first, "plugins") || same(first, ".paperpaw")
}

/// The reserve to keep free: the larger of 256 KiB and 1 % of the card.
pub(super) fn reserve(storage: &dyn Storage) -> u64 {
    let one_percent = storage.total_space().map_or(0, |total| total / 100);
    one_percent.max(MIN_RESERVE)
}

/// Whether `len` more bytes keep the free-space reserve (a storage that does not tell its
/// free space has room): `check_room` as a plain answer, for the host writers.
pub(super) fn has_room(storage: &dyn Storage, len: u64) -> bool {
    storage
        .free_space()
        .is_none_or(|free| free.saturating_sub(len) >= reserve(storage))
}

/// `ERR_FULL` when `len` more bytes would leave less than the reserve.
pub(super) fn check_room(storage: &dyn Storage, len: usize) -> Result<(), i32> {
    match storage.free_space() {
        Some(free) if free.saturating_sub(len as u64) < reserve(storage) => Err(ERR_FULL),
        _ => Ok(()),
    }
}

/// `path` as a relative path: sizes, UTF-8 and grammar, nothing joined.
pub(super) fn relative_path(path: &[u8]) -> Result<&str, i32> {
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
pub(super) fn status(error: StorageError) -> i32 {
    match error {
        StorageError::NotFound => ERR_NOT_FOUND,
        StorageError::InvalidPath => ERR_INVALID,
        StorageError::Unsupported => ERR_UNSUPPORTED,
        StorageError::Full => ERR_FULL,
        StorageError::TooLarge => ERR_NO_SPACE,
        StorageError::OutOfMemory => ERR_NO_MEMORY,
        _ => ERR_IO,
    }
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
