//! Storage backed by a directory of the host file system: the emulated SD
//! card of the desktop emulator, or an app's folder on Android.

// What goes wrong with the user's data is told, never dropped unseen:
// a result left unread says why (see CONTRIBUTING.md).
#![deny(clippy::let_underscore_must_use)]

use std::cell::{Cell, RefCell};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use paperpaw_platform::storage::is_valid_path;
use paperpaw_platform::tree::{DESCENT, MAX_READS, MAX_REMOVALS, TreeStep};
use paperpaw_platform::{DirEntry, Storage, StorageError};

/// Maps storage paths below a root directory.
#[derive(Debug)]
pub struct DirStorage {
    root: PathBuf,
    /// The file read last, kept open for sequential reads.
    open: RefCell<Option<(String, File)>>,
    /// The file system takes names in any case for the same entry.
    ignores_case: bool,
    /// What it says it has free instead of the file system's, where set.
    free: Option<u64>,
    /// When the root was last looked at for a read from the open file.
    looked: Cell<Option<Instant>>,
}

/// How long a read from the open file goes without looking at the root again.
/// The look is a `stat` (about 3 µs here, against 6 µs for a 4 KiB read; it
/// is more where a share is served by a helper process, as Android's shared
/// storage is), and a pulled volume is told a few reads later all the same.
const LOOK_EVERY: Duration = Duration::from_millis(50);

impl DirStorage {
    /// The directory the storage is rooted at.
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    pub fn new(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self {
            ignores_case: ignores_case(&root),
            root,
            open: RefCell::new(None),
            free: None,
            looked: Cell::new(None),
        })
    }

    /// The same, saying it has `bytes` free whatever the file system has:
    /// an emulated device's card, the same on every computer.
    pub fn with_free_space(mut self, bytes: u64) -> Self {
        self.free = Some(bytes);
        self
    }

    /// Whether the root folder is still there. A volume taken away (an SD
    /// card pulled, a share revoked) leaves its mount point gone: every
    /// operation then says [`StorageError::CARD_GONE`], not "not found",
    /// so that a book on it is never taken for deleted, and a write never
    /// makes the mount point again on the wrong disk.
    ///
    /// - A root that is missing, or is not a folder, is gone.
    /// - A root the system refuses to look at (`PermissionDenied`: an
    ///   access revoked, as a shared storage can be) is as unusable as a
    ///   missing one, and is gone too: the library then reports a read
    ///   error (`ERR_IO`), not an unsupported call.
    /// - Any other failure of the look is [`StorageError::Io`].
    ///
    /// The root is made once, by [`DirStorage::new`]. One deleted while the
    /// app runs is not made again until the storage is made again (the next
    /// start); a volume that comes back by itself (a card put in again)
    /// works again as soon as its folder is there.
    fn live(&self) -> Result<(), StorageError> {
        match fs::metadata(&self.root) {
            Ok(metadata) if metadata.is_dir() => Ok(()),
            Ok(_) => Err(StorageError::CARD_GONE),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
                ) =>
            {
                Err(StorageError::CARD_GONE)
            }
            Err(_) => Err(StorageError::Io),
        }
    }

    /// The folder `dir`, the root itself for `""` (to list or make it).
    fn resolve_dir(&self, dir: &str) -> Result<PathBuf, StorageError> {
        if dir.is_empty() {
            self.live()?;
            return self.start(0);
        }
        self.resolve(dir)
    }

    /// The root, in a path with room for `extra` more bytes (a path that
    /// cannot be made is `Io`, not a failed allocation).
    fn start(&self, extra: usize) -> Result<PathBuf, StorageError> {
        let mut full = PathBuf::new();
        reserve(&mut full, self.root.as_os_str().len() + extra + 1)?;
        full.push(&self.root);
        Ok(full)
    }

    /// The file or folder `path` inside the root: never the root itself,
    /// which no write, removal or move may reach.
    fn resolve(&self, path: &str) -> Result<PathBuf, StorageError> {
        if !is_valid_path(path) {
            return Err(StorageError::InvalidPath);
        }
        self.live()?;
        let mut full = self.start(path.len() + 1)?;
        for part in path.split('/') {
            if !is_own_name(&full, part) {
                return Err(StorageError::InvalidPath);
            }
            full.push(part);
        }
        Ok(full)
    }

    /// One piece of removing the tree at `root`, and what it cost.
    fn step(&self, root: &str, keep_root: bool) -> Result<(TreeStep, Budget), StorageError> {
        let mut target = self.resolve_dir(root)?;
        *self.open.borrow_mut() = None;
        let done = |removed| TreeStep::new(removed, true, 0);
        let mut budget = Budget::default();
        match fs::symlink_metadata(&target) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((done(0), budget)),
            Err(e) => return Err(error(e)),
            Ok(meta) if !meta.is_dir() => {
                fs::remove_file(&target).map_err(error)?;
                budget.removed = 1;
                return Ok((done(1), budget));
            }
            Ok(_) => {}
        }
        let more = |budget: &Budget| TreeStep::new(budget.removed, false, 1);
        if !sweep(&mut target, 0, &mut budget)? {
            return Ok((more(&budget), budget));
        }
        if !keep_root && !root.is_empty() {
            // The share of removals holds for the last folder too: the next
            // call finds it empty and takes it.
            if budget.removed >= MAX_REMOVALS {
                return Ok((more(&budget), budget));
            }
            fs::remove_dir(&target).map_err(error)?;
            budget.removed += 1;
        }
        Ok((done(budget.removed), budget))
    }

    /// Closes the cached handle if it refers to `path` (before writes).
    fn forget(&self, path: &str) {
        let mut open = self.open.borrow_mut();
        if open.as_ref().is_some_and(|(p, _)| p == path) {
            *open = None;
        }
    }
}

/// What one piece of removing a tree has done.
#[derive(Debug, Default)]
struct Budget {
    removed: u32,
    /// Files shown by a listing, and the looks that a folder is empty.
    reads: u32,
    /// Folders shown on the way down, found again by every call.
    descent: u32,
}

/// Room for `extra` more bytes in `path`, or `Io`: the one allocation of the
/// walk that grows with the tree (std's own small ones, a listing's handle
/// and the name of an entry, are not fallible).
fn reserve(path: &mut PathBuf, extra: usize) -> Result<(), StorageError> {
    path.as_mut_os_string()
        .try_reserve(extra)
        .map_err(|_| StorageError::Io)
}

/// Takes the entries out of folder `dir` (`depth` levels below the tree's
/// root) until it is empty (true) or the call's share is used (false): at most
/// [`MAX_REMOVALS`] removals, files and looks at a folder numbering
/// [`MAX_READS`], and the folders passed on the way down apart
/// ([`DESCENT`]). The share is checked before every removal, also of the
/// folders left behind on the way back up. `dir` is the path of the folder
/// and is the same again on return.
fn sweep(dir: &mut PathBuf, depth: usize, budget: &mut Budget) -> Result<bool, StorageError> {
    let mut entries = fs::read_dir(&*dir).map_err(error)?;
    loop {
        // The share is checked before the next entry is fetched.
        if budget.removed >= MAX_REMOVALS || budget.reads >= MAX_READS {
            return Ok(false);
        }
        let Some(entry) = entries.next() else {
            break;
        };
        let entry = entry.map_err(error)?;
        let is_dir = entry.file_type().map_err(error)?.is_dir();
        let name = entry.file_name();
        if is_dir {
            // Going down is the descent: counted apart, as each call starts
            // at the root again.
            if depth + 1 > TREE_DEPTH {
                return Err(StorageError::InvalidData);
            }
            budget.descent += 1;
            if budget.descent > DESCENT {
                return Err(StorageError::Io);
            }
        } else {
            budget.reads += 1;
        }
        reserve(dir, 1 + name.len())?;
        dir.push(&name);
        let gone = remove_entry(dir, is_dir, depth, budget);
        dir.pop();
        if !gone? {
            return Ok(false);
        }
        budget.removed += 1;
    }
    // Entries removed while the folder was read may be skipped by the OS's
    // iterator: empty only once a new look finds none.
    if budget.reads >= MAX_READS {
        return Ok(false);
    }
    budget.reads += 1;
    Ok(fs::read_dir(&*dir).map_err(error)?.next().is_none())
}

/// Removes the entry at `path`: a file, or a folder once it was emptied.
/// False where the call's share ran out before.
fn remove_entry(
    path: &mut PathBuf,
    is_dir: bool,
    depth: usize,
    budget: &mut Budget,
) -> Result<bool, StorageError> {
    if !is_dir {
        fs::remove_file(&*path).map_err(error)?;
        return Ok(true);
    }
    if !sweep(path, depth + 1, budget)? || budget.removed >= MAX_REMOVALS {
        return Ok(false);
    }
    fs::remove_dir(&*path).map_err(error)?;
    Ok(true)
}

/// Deepest folder below the root of a tree the removal goes into.
const TREE_DEPTH: usize = 256;

fn error(e: io::Error) -> StorageError {
    match e.kind() {
        io::ErrorKind::NotFound => StorageError::NotFound,
        io::ErrorKind::StorageFull => StorageError::Full,
        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => {
            StorageError::Unsupported
        }
        _ => StorageError::Io,
    }
}

/// Puts the folder of `path` on the disk, so a file renamed into it is
/// there after a power cut; where folders cannot be opened (Windows),
/// the system does it.
fn sync_dir(path: &Path) {
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        #[expect(
            clippy::let_underscore_must_use,
            reason = "a file system that cannot sync a folder keeps the file all the same; only a power cut right after may lose the rename"
        )]
        let _ = File::open(dir).and_then(|dir| dir.sync_all());
    }
    #[cfg(not(unix))]
    let _ = path;
}

fn create_parent(path: &Path) -> Result<(), StorageError> {
    match path.parent() {
        Some(parent) => fs::create_dir_all(parent).map_err(error),
        None => Ok(()),
    }
}

/// Whether `part` names what is in `dir` by its own name, not by another
/// the system takes for it: FAT and NTFS keep a short name beside a long
/// one (`PAPERP~1` beside `.paperpaw`), and Windows drops dots and spaces
/// at the end of a name (`.paperpaw.` is `.paperpaw` there). Such a name
/// reaches an entry by another spelling, e.g. PaperPaw's own data by a
/// path that does not look hidden.
fn is_own_name(dir: &Path, part: &str) -> bool {
    if cfg!(windows) && part.ends_with(['.', ' ']) {
        return false;
    }
    // Short names have a `~`: one is fine where nothing answers to it, or
    // where the folder lists an entry by it.
    !part.contains('~')
        || fs::symlink_metadata(dir.join(part)).is_err()
        || fs::read_dir(dir).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case(part))
            })
        })
}

/// Whether the file system at `root` takes a name in another case for the
/// same entry (NTFS, APFS, FAT; not ext4): asked of the root or the
/// nearest folder above it whose name has letters, in the other case.
fn ignores_case(root: &Path) -> bool {
    let probe = root.ancestors().find_map(|dir| {
        let name = dir.file_name()?.to_str()?;
        let other: String = name
            .chars()
            .map(|c| {
                if c.is_lowercase() {
                    c.to_ascii_uppercase()
                } else {
                    c.to_ascii_lowercase()
                }
            })
            .collect();
        (other != name).then(|| (dir, dir.with_file_name(other)))
    });
    match probe {
        Some((dir, other)) => same_entry(dir, &other),
        None => cfg!(any(windows, target_os = "macos")),
    }
}

/// Whether paths `a` and `b` lead to the same entry.
#[cfg(unix)]
fn same_entry(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

/// Whether paths `a` and `b` lead to the same entry.
#[cfg(not(unix))]
fn same_entry(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Whether `a` and `b` are the same entry of one folder: see
/// [`Storage::same_entry`] for `DirStorage`.
#[cfg(unix)]
fn same_directory_entry(a: &Path, b: &Path, ignores_case: bool) -> Option<bool> {
    use std::os::unix::fs::MetadataExt;
    /// Device and inode, `Some(None)` where nothing is there.
    fn identity(look: io::Result<fs::Metadata>) -> Option<Option<(u64, u64)>> {
        match look {
            Ok(m) => Some(Some((m.dev(), m.ino()))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Some(None),
            Err(_) => None,
        }
    }
    let (name_a, name_b) = (a.file_name()?, b.file_name()?);
    let same_name = name_a == name_b
        || (ignores_case
            && name_a
                .to_str()
                .zip(name_b.to_str())
                .is_some_and(|(x, y)| x.to_lowercase() == y.to_lowercase()));
    if !same_name {
        return Some(false);
    }
    // The entry itself, not what a link leads to.
    let (entry_a, entry_b) = (
        identity(fs::symlink_metadata(a))?,
        identity(fs::symlink_metadata(b))?,
    );
    // The folders are followed: a folder reached by a link is the folder.
    let (dir_a, dir_b) = (
        identity(fs::metadata(a.parent()?))?,
        identity(fs::metadata(b.parent()?))?,
    );
    Some(entry_a.is_some() && entry_a == entry_b && dir_a.is_some() && dir_a == dir_b)
}

/// Not answered where this has not been run.
#[cfg(not(unix))]
fn same_directory_entry(_: &Path, _: &Path, _: bool) -> Option<bool> {
    None
}

impl Storage for DirStorage {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let mut open = self.open.borrow_mut();
        // Also for the file read last, kept open: a volume pulled since says
        // so, it is not read from a handle that may still answer. A new
        // handle looks at the root in `resolve`; one in use, at most every
        // `LOOK_EVERY`.
        if open.as_ref().is_some_and(|(p, _)| p == path)
            && self
                .looked
                .get()
                .is_none_or(|at| at.elapsed() >= LOOK_EVERY)
        {
            if let Err(gone) = self.live() {
                *open = None;
                self.looked.set(None);
                return Err(gone);
            }
            self.looked.set(Some(Instant::now()));
        }
        if open.as_ref().is_none_or(|(p, _)| p != path) {
            let file = File::open(self.resolve(path)?).map_err(error)?;
            *open = Some((path.to_string(), file));
            self.looked.set(Some(Instant::now()));
        }
        let Some((_, file)) = open.as_mut() else {
            return Err(StorageError::Io);
        };
        // Past where any file can reach (and a seek can go): its end.
        if offset > i64::MAX as u64 {
            return Ok(0);
        }
        file.seek(SeekFrom::Start(offset)).map_err(error)?;
        let mut filled = 0;
        while filled < buf.len() {
            match file.read(&mut buf[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(error(e)),
            }
        }
        Ok(filled)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        let metadata = fs::metadata(self.resolve(path)?).map_err(error)?;
        if metadata.is_file() {
            Ok(metadata.len())
        } else {
            Err(StorageError::NotFound)
        }
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let target = self.resolve(path)?;
        self.forget(path);
        create_parent(&target)?;
        // Write a temporary file beside it, on the disk before it is
        // renamed over the file: never a half-written file, also after a
        // power cut. Named after the whole name, so `a.x` and `a.y` do
        // not share one.
        let mut temp = target.clone().into_os_string();
        temp.push(".paperpaw-tmp");
        let temp = PathBuf::from(temp);
        let written = File::create(&temp).and_then(|mut file| {
            file.write_all(data)?;
            file.sync_all()
        });
        if let Err(e) = written.and_then(|()| fs::rename(&temp, &target)) {
            #[expect(
                clippy::let_underscore_must_use,
                reason = "cleaning up after a failed write, whose own error is the one told"
            )]
            let _ = fs::remove_file(&temp);
            return Err(error(e));
        }
        sync_dir(&target);
        Ok(())
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let target = self.resolve(path)?;
        self.forget(path);
        create_parent(&target)?;
        // To the end by seeking, not with `O_APPEND`: Android's shared
        // storage (FUSE) does not always honour it and writes at the
        // start instead, which broke uploads of more than one part.
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&target)
            .and_then(|mut file| {
                file.seek(SeekFrom::End(0))?;
                file.write_all(data)
            })
            .map_err(error)
    }

    fn write_at(&self, path: &str, offset: u64, data: &[u8]) -> Result<(), StorageError> {
        let target = self.resolve(path)?;
        self.forget(path);
        OpenOptions::new()
            .write(true)
            .open(&target)
            .and_then(|mut file| {
                file.seek(SeekFrom::Start(offset))?;
                file.write_all(data)
            })
            .map_err(error)
    }

    fn truncate(&self, path: &str, len: u64) -> Result<(), StorageError> {
        let target = self.resolve(path)?;
        self.forget(path);
        OpenOptions::new()
            .write(true)
            .open(&target)
            .and_then(|file| file.set_len(len))
            .map_err(error)
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        // By name: the order `read_dir` gives differs between systems
        // (sorted on Windows, as stored on Linux), and with it what is
        // listed first, e.g. the order of books of the same title.
        let mut entries = Vec::new();
        for entry in fs::read_dir(self.resolve_dir(dir)?).map_err(error)? {
            let entry = entry.map_err(error)?;
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let size = if metadata.is_file() {
                metadata.len()
            } else {
                0
            };
            entries.push((name, metadata.is_dir(), size, seconds(&metadata)));
        }
        paperpaw_sort::sort_by(&mut entries, |a, b| a.0.cmp(&b.0));
        for (name, is_dir, size, modified) in &entries {
            visit(DirEntry::new(name, *is_dir, *size).with_modified(*modified));
        }
        Ok(())
    }

    fn modified(&self, path: &str) -> Option<i64> {
        seconds(&fs::metadata(self.resolve(path).ok()?).ok()?)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        let target = self.resolve(path)?;
        self.forget(path);
        match fs::remove_file(target) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(error(e)),
            _ => Ok(()),
        }
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let (source, target) = (self.resolve(from)?, self.resolve(to)?);
        // Files kept open must not block the move (Windows).
        *self.open.borrow_mut() = None;
        create_parent(&target)?;
        fs::rename(source, target).map_err(error)
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        fs::create_dir_all(self.resolve_dir(path)?).map_err(error)
    }

    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        let target = self.resolve(path)?;
        *self.open.borrow_mut() = None;
        match fs::remove_dir_all(target) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(error(e)),
            _ => Ok(()),
        }
    }

    /// A tree removed a piece at a time on the host's own file system: at
    /// most 64 removals and 256 entries read a call (the folders passed on the
    /// way down counted apart), a folder deeper than 256 levels `InvalidData`,
    /// the rest the OS's limits (`Io`). The path is built fallibly. The place
    /// is not needed: each call starts at the root and what it removed is
    /// gone, so it is only ever a nonzero token.
    fn remove_tree_step(
        &self,
        root: &str,
        keep_root: bool,
        place: u64,
    ) -> Result<TreeStep, StorageError> {
        let _ = place;
        self.step(root, keep_root).map(|(step, _)| step)
    }

    fn ignores_case(&self) -> bool {
        self.ignores_case
    }

    /// Whether `a` and `b` are the same directory entry: the same name (in
    /// another case, where the file system ignores case) in the same
    /// folder. Nothing is followed at the last name, so a hard link, or a
    /// symlink and what it points at, are two entries, each with its own
    /// name; a folder reached by two ways (a symlink to it) is one folder.
    /// A path that is refused (it leaves the root, or is a short alias this
    /// storage does not take) and one that names nothing are
    /// `Some(false)`; the root gone, or a look that failed, are `None`.
    /// Windows has no answer until its path is run there: `None`.
    fn same_entry(&self, a: &str, b: &str) -> Option<bool> {
        let (a, b) = match (self.resolve(a), self.resolve(b)) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(StorageError::InvalidPath), Ok(_) | Err(StorageError::InvalidPath))
            | (Ok(_), Err(StorageError::InvalidPath)) => return Some(false),
            _ => return None,
        };
        same_directory_entry(&a, &b, self.ignores_case)
    }

    fn free_space(&self) -> Option<u64> {
        self.free
            .or_else(|| space(&self.root).map(|(free, _)| free))
    }

    /// The file system's size; none for an emulated card told its free
    /// space ([`DirStorage::with_free_space`]).
    fn total_space(&self) -> Option<u64> {
        match self.free {
            Some(_) => None,
            None => space(&self.root).map(|(_, total)| total),
        }
    }

    fn local_path(&self, path: &str, with: &mut dyn FnMut(&str)) {
        if let Some(real) = self.resolve(path).ok().as_deref().and_then(Path::to_str) {
            with(real);
        }
    }
}

/// Bytes the file system of `dir` has free for this user, and in all.
#[cfg(unix)]
fn space(dir: &Path) -> Option<(u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    // SAFETY: a valid C string, and a buffer statvfs fills.
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: as above; nothing is kept past the call.
    if unsafe { libc::statvfs(path.as_ptr(), &mut stats) } != 0 {
        return None;
    }
    #[allow(
        clippy::unnecessary_cast,
        reason = "the fields' types differ by platform: a cast on one is none on another"
    )]
    Some((
        stats.f_bavail as u64 * stats.f_frsize as u64,
        stats.f_blocks as u64 * stats.f_frsize as u64,
    ))
}

/// Bytes the file system of `dir` has free for this user, and in all.
#[cfg(windows)]
fn space(dir: &Path) -> Option<(u64, u64)> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetDiskFreeSpaceExW(
            directory: *const u16,
            free_to_caller: *mut u64,
            total: *mut u64,
            total_free: *mut u64,
        ) -> i32;
    }
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain([0]).collect();
    let (mut free, mut total) = (0u64, 0u64);
    // SAFETY: a NUL-terminated path, and numbers the call fills; the
    // last may be null.
    let done =
        unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, std::ptr::null_mut()) };
    (done != 0).then_some((free, total))
}

#[cfg(not(any(unix, windows)))]
fn space(_: &Path) -> Option<(u64, u64)> {
    None
}

/// When `metadata`'s file was last changed, in seconds since 1970.
fn seconds(metadata: &fs::Metadata) -> Option<i64> {
    let since = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    i64::try_from(since.as_secs()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A chain of 256 folders with leaves, taken away piece by piece: every
    /// call within its removals and its reads (the folders passed on the way
    /// down counted apart), also the one that unwinds the chain and takes the
    /// root; the work is never more than the shares allow.
    #[test]
    fn a_deep_chain_is_removed_within_the_shares_of_every_call() {
        let root = std::env::temp_dir().join(format!("paperpaw-deep-{}", std::process::id()));
        fs::remove_dir_all(&root).unwrap_or_default();
        let storage = DirStorage::new(&root).unwrap();
        let mut chain = String::from("T");
        for _ in 0..TREE_DEPTH {
            chain.push_str("/a");
        }
        for n in 0..70 {
            storage.write(&format!("{chain}/f{n}"), b"x").unwrap();
        }
        // Files on the way down, so that they are removed before the chain.
        storage.write("T/a/side", b"x").unwrap();
        storage.write("Keep/k", b"keep").unwrap();
        let (mut calls, mut removed) = (0, 0);
        loop {
            let (step, budget) = storage.step("T", false).unwrap();
            calls += 1;
            removed += step.removed;
            assert!(budget.removed <= MAX_REMOVALS, "{}", budget.removed);
            assert!(budget.reads <= MAX_READS, "{}", budget.reads);
            assert!(budget.descent <= DESCENT, "{}", budget.descent);
            assert_eq!(step.removed, budget.removed);
            if step.done {
                break;
            }
            assert!(calls < 40, "it does not end");
        }
        // 70 + 1 files and 256 folders, and the root.
        assert_eq!(removed, 70 + 1 + 256 + 1);
        assert!(calls >= 328 / 64, "{calls}");
        assert!(!root.join("T").exists());
        assert_eq!(storage.size("Keep/k"), Ok(4));
        fs::remove_dir_all(root).unwrap();
    }

    /// Files of one name with other extensions are written apart, and
    /// nothing is left beside them.
    #[test]
    fn written_whole_by_their_own_temporary_files() {
        let root = std::env::temp_dir().join(format!("paperpaw-temp-{}", std::process::id()));
        let storage = DirStorage::new(&root).unwrap();
        storage.write("a.x", b"x").unwrap();
        // The temporary file of `a.y` was `a.paperpaw-tmp`, as `a.x`'s.
        fs::write(root.join("a.paperpaw-tmp"), b"someone's").unwrap();
        storage.write("a.y", b"y").unwrap();
        assert_eq!(fs::read(root.join("a.paperpaw-tmp")).unwrap(), b"someone's");
        let mut names = Vec::new();
        storage
            .list("", &mut |e| names.push(e.name.to_string()))
            .unwrap();
        names.sort();
        assert_eq!(names, ["a.paperpaw-tmp", "a.x", "a.y"]);
        fs::remove_dir_all(root).unwrap();
    }

    /// A file written in place and cut, as an emulator's saved memory.
    #[test]
    fn written_in_place_and_cut() {
        let root = std::env::temp_dir().join(format!("paperpaw-at-{}", std::process::id()));
        let storage = DirStorage::new(&root).unwrap();
        storage.write("save.bin", b"abcdef").unwrap();
        storage.write_at("save.bin", 2, b"XY").unwrap();
        assert_eq!(fs::read(root.join("save.bin")).unwrap(), b"abXYef");
        storage.truncate("save.bin", 3).unwrap();
        assert_eq!(fs::read(root.join("save.bin")).unwrap(), b"abX");
        fs::remove_dir_all(root).unwrap();
    }

    /// The file system says how much it has free, and in all.
    #[test]
    fn free_space_is_told() {
        let root = std::env::temp_dir().join(format!("paperpaw-free-{}", std::process::id()));
        let storage = DirStorage::new(&root).unwrap();
        let free = storage.free_space().unwrap();
        assert!(free > 0);
        assert!(storage.total_space().unwrap() >= free);
        // Or as much as it is told to say, of a size not told.
        let storage = DirStorage::new(&root).unwrap().with_free_space(1234);
        assert_eq!(storage.free_space(), Some(1234));
        assert_eq!(storage.total_space(), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn round_trips_files_below_the_root() {
        let root = std::env::temp_dir().join(format!("paperpaw-storage-{}", std::process::id()));
        let storage = DirStorage::new(&root).unwrap();
        storage.write("a/b.txt", b"hello").unwrap();
        storage.append("a/b.txt", b" world").unwrap();
        let mut buf = [0u8; 5];
        assert_eq!(storage.read_at("a/b.txt", 6, &mut buf), Ok(5));
        assert_eq!(&buf, b"world");
        assert_eq!(storage.size("a/b.txt"), Ok(11));
        let mut names = Vec::new();
        storage
            .list("a", &mut |e| names.push(e.name.to_string()))
            .unwrap();
        assert_eq!(names, ["b.txt"]);
        assert_eq!(storage.size("../outside"), Err(StorageError::InvalidPath));
        storage.remove("a/b.txt").unwrap();
        assert_eq!(storage.size("a/b.txt"), Err(StorageError::NotFound));
        fs::remove_dir_all(root).unwrap();
    }

    /// A folder is listed by name on every system (what is listed first
    /// decides e.g. the order of books of the same title).
    #[test]
    fn folders_are_listed_by_name() {
        let root = std::env::temp_dir().join(format!("paperpaw-listed-{}", std::process::id()));
        let storage = DirStorage::new(&root).unwrap();
        for name in ["m.txt", "b.epub", "z.txt", "b.txt", "a"] {
            storage.write(&format!("d/{name}"), b"x").unwrap();
        }
        let mut names = Vec::new();
        storage
            .list("d", &mut |e| names.push(e.name.to_string()))
            .unwrap();
        assert_eq!(names, ["a", "b.epub", "b.txt", "m.txt", "z.txt"]);
        fs::remove_dir_all(root).unwrap();
    }

    /// Whether the storage ignores case is asked of the file system.
    #[test]
    fn knows_whether_case_counts() {
        let root = std::env::temp_dir().join(format!("paperpaw-case-{}", std::process::id()));
        let storage = DirStorage::new(&root).unwrap();
        storage.write("Mixed.txt", b"x").unwrap();
        let other = storage.resolve("mixed.txt").map(|p| p.exists());
        assert_eq!(storage.ignores_case(), other == Ok(true));
        fs::remove_dir_all(root).unwrap();
    }

    /// What is there is not reached by another spelling the system takes
    /// for it (where it keeps short names, as Windows mostly does); names
    /// with `~` of their own are.
    #[test]
    fn other_spellings_reach_nothing() {
        let root = std::env::temp_dir().join(format!("paperpaw-spellings-{}", std::process::id()));
        let storage = DirStorage::new(&root).unwrap();
        storage.write(".paperpaw/secrets.txt", b"password").unwrap();
        storage.write("Books/A long title.epub", b"a book").unwrap();
        for (other, own) in [
            ("PAPERP~1/secrets.txt", ".paperpaw/secrets.txt"),
            ("paperp~1/secrets.txt", ".paperpaw/secrets.txt"),
            ("Books/ALONGT~1.EPU", "Books/A long title.epub"),
        ] {
            let mut buf = [0u8; 8];
            let read = storage.read_at(other, 0, &mut buf);
            assert!(read.is_err(), "{other}: {read:?}");
            // Made anew where the system keeps no short names, refused
            // where it does: what is there stays as it was.
            #[expect(
                clippy::let_underscore_must_use,
                reason = "the attempt is what is tested; whether it works depends on the system"
            )]
            let _ = storage.write(other, b"stolen");
            #[expect(
                clippy::let_underscore_must_use,
                reason = "the attempt is what is tested; whether it works depends on the system"
            )]
            let _ = storage.append(other, b"stolen");
            assert_ne!(storage.size(own), Ok(6 + 6), "{other}");
            let n = storage.read_at(own, 0, &mut buf).unwrap();
            assert_ne!(&buf[..n.min(6)], b"stolen", "{other}");
        }
        assert_eq!(storage.size(".paperpaw/secrets.txt"), Ok(8));
        assert_eq!(storage.size("Books/A long title.epub"), Ok(6));
        if cfg!(windows) {
            for other in [
                ".paperpaw./secrets.txt",
                ".paperpaw /secrets.txt",
                ".paperpaw/secrets.txt.",
            ] {
                assert_eq!(
                    storage.size(other),
                    Err(StorageError::InvalidPath),
                    "{other}"
                );
            }
        }
        storage.write("Books/Vol~1.txt", b"one").unwrap();
        assert_eq!(storage.size("Books/Vol~1.txt"), Ok(3));
        storage.remove("Books/Vol~1.txt").unwrap();
        assert_eq!(storage.size(".paperpaw/secrets.txt"), Ok(8));
        fs::remove_dir_all(root).unwrap();
    }
}
