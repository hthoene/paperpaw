//! Interface-7 storage slots 20-26 over [`Storage`]: root, path and
//! buffers in, `ERR_*` out. One adapter serves one plugin; nothing here is
//! a wire record, a pointer or a table entry.
//!
//! Root 0 is the plugin's data folder `.paperpaw/plugins/<id>/`. A path is
//! validated as a relative path first and joined under that folder after,
//! so it cannot name the install folder `plugins/<id>/` or anything else
//! outside (S5). Root 1 is the user's files from the card's top, for a
//! plugin the slot let in (`uses.storage`, `files.md` 1.1); root 2 the
//! plugin's install folder `plugins/<id>/`, read only; roots 3, 4 and 6 are a
//! format call's document; the others are reserved.
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

use super::files_service::{Access, FilesHost};
use super::format_scope::{BookKind, FormatBook};
use alloc::borrow::Cow;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_DENIED, ERR_FULL, ERR_INVALID, ERR_IO, ERR_NO_MEMORY, ERR_NO_SPACE,
    ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED, ERR_UTF8,
};
use paperpaw_platform::tree::{Kind, kind};
use paperpaw_platform::{Storage, StorageError};
use paperpaw_sdk::Id;

/// Root 0: the plugin's own data folder.
pub const ROOT_PLUGIN: u32 = 0;
/// Root 1: the user's files from the card's top (`uses.storage`).
pub(crate) const ROOT_CARD: u32 = 1;
/// Root 2: the plugin's install folder, read only.
pub(crate) const ROOT_BUNDLE: u32 = 2;
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
    /// Root 1 for this call (the slot checked `uses.storage`); `None`: no root 1.
    card: Option<Card<'a>>,
    /// Root 2 for this call: `Some(built_in)`, where a plugin built into the
    /// firmware has an empty install folder; `None`: no root 2.
    bundle: Option<bool>,
}

/// Root 1 of one slot call: what else holds the user's files, beyond the
/// host writers' paths (`in_use`). `files` is `core.files`'s host where it is
/// wired: a change of a path it says is busy (the library's move job, the
/// reader's book) is `ERR_BUSY`, as on `core.files` (`files.md` section 3).
#[derive(Clone, Copy)]
pub(crate) struct Card<'a> {
    /// `Some` wherever root 1 opens: a shell offers `uses.storage` only where
    /// it wires `core.files` (the desktop's `linked.rs`); `None` only in tests.
    pub(crate) files: Option<&'a dyn FilesHost>,
    /// The kernel's formats, where the callback's frame has them: what a
    /// book is, for the renames the library follows (`files.md` 2.6).
    pub(crate) formats: Option<&'a paperpaw_sdk::Formats>,
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
            card: None,
            bundle: None,
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
            card: None,
            bundle: None,
        })
    }

    /// The adapter for a call of a format plugin: roots 3, 4 and 6 are the
    /// document of `book`.
    pub(crate) fn with_format(mut self, book: &'a FormatBook<'a>) -> Self {
        self.format = Some(book);
        self
    }

    /// The adapter for a call on root 1, the user's files (`files.md` 1.1).
    pub(crate) fn with_card(mut self, card: Card<'a>) -> Self {
        self.card = Some(card);
        self
    }

    /// The adapter for a call on root 2, the plugin's install folder
    /// (`files.md` 1.1); `built_in`: the plugin is linked into the firmware.
    pub(crate) fn with_bundle(mut self, built_in: bool) -> Self {
        self.bundle = Some(built_in);
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
        if root == ROOT_CARD {
            return self.resolve_card(path, with);
        }
        if root == ROOT_BUNDLE {
            return self.resolve_bundle(path, write, with);
        }
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

    /// Root 1, the card's top: `ERR_UNSUPPORTED` where the call has no root 1,
    /// the path rules, then the root-1 exclusions `core.files` keeps
    /// (`plugins`, `.paperpaw`, `paperpaw.ini` as the first segment, in any
    /// spelling the card takes for one, also an 8.3 alias): `ERR_INVALID`. A
    /// running data plugin's folder stays refused here (`files.md` 1.1: only
    /// `stat`, `list` and `image_prepare` reach it).
    #[inline(never)]
    fn resolve_card<R>(
        &self,
        path: &[u8],
        with: impl FnOnce(&str) -> Result<R, i32>,
    ) -> Result<R, i32> {
        if self.card.is_none() {
            return Err(ERR_UNSUPPORTED);
        }
        let relative = relative_path(path)?;
        let first = relative.split('/').next().unwrap_or_default();
        if super::owner_paths::protected_first(first)
            || super::storage_files::excluded_on(self.storage, ROOT_CARD, "", first)
        {
            return Err(ERR_INVALID);
        }
        with(relative)
    }

    /// Root 2, the plugin's install folder `plugins/<id>/`, `<id>` stamped from
    /// the data folder as root 0's: `ERR_UNSUPPORTED` where the call has no
    /// root 2, `ERR_DENIED` for a change, the path rules, then `ERR_NOT_FOUND`
    /// for a plugin built into the firmware (it has no install folder: what
    /// the card holds there, a leftover of an older install, is never read).
    #[inline(never)]
    fn resolve_bundle<R>(
        &self,
        path: &[u8],
        write: bool,
        with: impl FnOnce(&str) -> Result<R, i32>,
    ) -> Result<R, i32> {
        let built_in = self.bundle.ok_or(ERR_UNSUPPORTED)?;
        if write {
            return Err(ERR_DENIED);
        }
        let relative = relative_path(path)?;
        if built_in {
            return Err(ERR_NOT_FOUND);
        }
        // `.paperpaw/plugins/<id>/` checked at construction: `plugins/<id>/`.
        let folder = self.folder.strip_prefix(".paperpaw/").ok_or(ERR_INVALID)?;
        let mut buf = [0u8; MAX_JOINED];
        let end = folder.len() + relative.len();
        let target = buf.get_mut(..end).ok_or(ERR_NO_SPACE)?;
        target[..folder.len()].copy_from_slice(folder.as_bytes());
        target[folder.len()..].copy_from_slice(relative.as_bytes());
        let joined = core::str::from_utf8(target).map_err(|_| ERR_UTF8)?;
        if !joined.starts_with(folder) || !paperpaw_platform::storage::is_valid_path(joined) {
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
    /// its staged file) or, on root 1, `core.files`'s host says `access` to it
    /// is busy ([`Card`]).
    fn free_of_host_writers(&self, joined: &str, access: Access) -> Result<(), i32> {
        let held = self
            .in_use
            .is_some_and(|table| table.holds(joined, self.storage.ignores_case()));
        let busy = self
            .card
            .and_then(|card| card.files)
            .is_some_and(|files| files.busy(joined, access));
        if held || busy { Err(ERR_BUSY) } else { Ok(()) }
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
            self.free_of_host_writers(p, Access::Write)?;
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
                self.free_of_host_writers(f, Access::Rename)?;
                self.free_of_host_writers(t, Access::Rename)?;
                // Root 1: the library follows (`files.md` 2.6).
                if let Some((card, files)) = self.card.and_then(|card| Some((card, card.files?))) {
                    return self.rename_card(files, card.formats, f, t);
                }
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

    /// Slot 24 on root 1, where `core.files`' host is wired: the user's files
    /// move as `core.files` moves them (`files.md` 2.6), its host's rename with
    /// the library following, so that a book or a folder of books keeps its
    /// position, annotations and plugins' book data: the journal entry, the
    /// rename, then the library's bounded job carries each book to its new
    /// path. `ERR_BUSY` while such a job runs (one at a time, device-wide).
    /// As on the plain rename, a missing `from` is `ERR_NOT_FOUND` and the
    /// folders above `to` are made where they are not there yet; the move job
    /// says the rest (`ERR_STATE` for a folder at `to`, `ERR_INVALID` for a
    /// folder into itself).
    ///
    /// A single file no format reads under either name has nothing for the
    /// library to carry ([`library_follows`](super::files_service::library_follows)):
    /// its move is the card's rename and starts no job, so a save of two
    /// renames in one callback is whole (#612).
    #[inline(never)]
    fn rename_card(
        &self,
        files: &dyn FilesHost,
        formats: Option<&paperpaw_sdk::Formats>,
        f: &str,
        t: &str,
    ) -> Result<(), i32> {
        if files.moving() {
            return Err(ERR_BUSY);
        }
        let there = |path: &str| {
            self.storage.exists(path) || self.storage.list_from(path, 0, &mut |_| false).is_ok()
        };
        if !there(f) {
            return Err(ERR_NOT_FOUND);
        }
        if let Some((parent, _)) = t.rsplit_once('/')
            && !there(parent)
        {
            self.make_parents(t)?;
        }
        // A single file no format reads is the card's rename alone (2.6),
        // refused where the job would: below itself, or onto a folder.
        let folder = !self.storage.exists(f);
        let library = super::files_service::library_follows(formats, folder, f, t);
        if !library {
            let below = files.within(self.storage, t, f);
            if below && !files.within(self.storage, f, t) {
                return Err(ERR_INVALID);
            }
            if kind(self.storage, t).map_err(status)? == Kind::Folder {
                return Err(ERR_STATE);
            }
        }
        files.rename(self.storage, f, t, true, library)
    }

    /// Slot 25: removes the file; a missing one is fine.
    ///
    /// # Errors
    /// An `ERR_*` status.
    pub fn remove(&self, root: u32, path: &[u8]) -> Result<(), i32> {
        self.resolve(root, path, true, |p| {
            self.free_of_host_writers(p, Access::Remove)?;
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
