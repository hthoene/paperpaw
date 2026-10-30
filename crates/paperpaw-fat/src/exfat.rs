//! PaperPaw's storage on an exFAT file system: [`ExFatStorage`].

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};

use hadris_fat::exfat::sync::ExFatFs;
use hadris_fs::{
    Clock, DateTime, DirCursor, ErrorKind, FileType, MountOptions, Name, NodeId, OpenMode,
    RenameMode, SetAttr,
};
use hadris_storage::sync::BlockDevice;
use hadris_storage::{BlockIndex, BlockSize};
use paperpaw_platform::storage::is_valid_path;
use paperpaw_platform::{DirEntry, Storage, StorageError};

use crate::disk::{BLOCK, Blocks, DiskError, SharedDisk};
use crate::settle::{move_record, recorded_move};
use crate::{FOLDERS, JOURNAL, OPEN_FILES, hash, name, parent, same_path};

/// The disk as the exFAT driver reads and writes it, in blocks.
pub struct ExDisk<B: Blocks>(SharedDisk<B>);

impl<B: Blocks> ExDisk<B> {
    /// The disk `disk`, in blocks (e.g. to check the volume on it).
    pub fn new(disk: SharedDisk<B>) -> Self {
        Self(disk)
    }
}

impl<B: Blocks> core::fmt::Debug for ExDisk<B> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ExDisk").finish_non_exhaustive()
    }
}

impl<B: Blocks> hadris_io::ErrorType for ExDisk<B> {
    type Error = DiskError;
}

impl core::fmt::Display for DiskError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("the card failed to read or write a block")
    }
}

impl core::error::Error for DiskError {}

/// The size of a block, as the driver takes it.
const BLOCK_SIZE: BlockSize = match BlockSize::new(BLOCK as u32) {
    Some(size) => size,
    None => panic!("a block has a size"),
};

/// What the driver is told of a disk error.
fn failed(_: DiskError) -> hadris_io::Error<DiskError> {
    hadris_io::Error::device(DiskError, "card")
}

impl<B: Blocks> BlockDevice for ExDisk<B> {
    fn block_size(&self) -> BlockSize {
        BLOCK_SIZE
    }

    fn block_count(&self) -> u64 {
        self.0.blocks()
    }

    fn writable(&self) -> bool {
        true
    }

    fn read_blocks(
        &mut self,
        first: BlockIndex,
        buf: &mut [u8],
    ) -> Result<(), hadris_io::Error<DiskError>> {
        let at = first.get() * BLOCK as u64;
        self.0.read_at(at, buf).map_err(failed)
    }

    fn write_blocks(
        &mut self,
        first: BlockIndex,
        buf: &[u8],
    ) -> Result<(), hadris_io::Error<DiskError>> {
        let at = first.get() * BLOCK as u64;
        self.0.write_at(at, buf).map_err(failed)
    }

    fn flush(&mut self) -> Result<(), hadris_io::Error<DiskError>> {
        self.0.flush_all().map_err(failed)
    }
}

type Volume<B> = ExFatFs<ExDisk<B>>;

/// The time new entries are dated: none is known (see [`ExFatStorage`]).
struct NoClock;

impl Clock for NoClock {
    fn now(&self) -> DateTime {
        // 1980-01-01, the earliest exFAT dates.
        DateTime::from_unix_seconds(315_532_800).unwrap_or(DateTime::MIN)
    }
}

static NO_CLOCK: NoClock = NoClock;

/// Nodes the driver keeps pinned at most: the open files and the folders
/// kept, and those a call looks up on its way.
const NODES: usize = OPEN_FILES + FOLDERS + 16;

/// The ends of the names a write goes through (see [`ExFatStorage`]):
/// while it is written, and once it is whole. The second is preceded by
/// `~` and eight hex digits, the mark: the state of the file it replaces
/// when it was made whole (see [`ExFatStorage::state_of`]).
const TMP: &str = "~pp.tmp";
const NEW: &str = "~pp.new";
/// The hex digits of a mark in a name, with the `~` before them.
const MARK: usize = 8;

/// Longest name of an entry, in UTF-16 units (exFAT's limit).
const LONGEST: usize = 255;

/// Whether `name` is one a write goes through.
fn is_temp(name: &str) -> bool {
    temp_of(name).is_some()
}

/// What a name a write goes through is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Temp {
    /// The content, while written.
    Writing,
    /// The content, whole, with the mark of the file it is to replace (none
    /// where the name has none).
    Whole(Option<u32>),
}

/// The file `name` is written for, and what it is, if it is a name a
/// write goes through.
fn temp_of(name: &str) -> Option<(&str, Temp)> {
    let end_of = |end: &str| name.len().checked_sub(end.len());
    if let Some((file, end)) = end_of(TMP).and_then(|at| name.split_at_checked(at))
        && !file.is_empty()
        && end.eq_ignore_ascii_case(TMP)
    {
        return Some((file, Temp::Writing));
    }
    let (rest, end) = name.split_at_checked(end_of(NEW)?)?;
    if !end.eq_ignore_ascii_case(NEW) {
        return None;
    }
    // `<file>~<mark>`: the mark's digits are the last ones before the end.
    let marked = rest
        .len()
        .checked_sub(1 + MARK)
        .and_then(|at| rest.split_at_checked(at))
        .and_then(|(file, mark)| {
            let digits = mark.strip_prefix('~')?;
            let mark = u32::from_str_radix(digits, 16).ok()?;
            (!file.is_empty() && digits.bytes().all(|b| b.is_ascii_hexdigit()))
                .then_some((file, Some(mark)))
        });
    let (file, mark) = marked.unwrap_or((rest, None));
    (!file.is_empty()).then_some((file, Temp::Whole(mark)))
}

/// The name of the content of `file`, whole, with its `mark`.
fn whole_name(file: &str, mark: u32) -> String {
    format!("{file}~{mark:08x}{NEW}")
}

/// Refuses a path PaperPaw does not take, or one of the storage's own.
fn check(path: &str) -> Result<(), StorageError> {
    let own = same_path(path, JOURNAL) || path.split('/').any(is_temp);
    if is_valid_path(path) && !own {
        Ok(())
    } else {
        Err(StorageError::InvalidPath)
    }
}

fn error(e: hadris_fs::Error<DiskError>) -> StorageError {
    match e.kind() {
        ErrorKind::NotFound => StorageError::NotFound,
        ErrorKind::NoSpace => StorageError::Full,
        ErrorKind::InvalidInput | ErrorKind::NameTooLong => StorageError::InvalidPath,
        ErrorKind::FileTooLarge => StorageError::TooLarge,
        ErrorKind::ReadOnly => StorageError::Unsupported,
        _ => StorageError::Io,
    }
}

type Result2<T> = Result<T, StorageError>;

/// A file kept open.
struct Open {
    path: String,
    node: NodeId,
    len: u64,
    /// Opened for writing (an append): its size is the driver's to write
    /// back, on close.
    write: bool,
}

/// PaperPaw's storage on an exFAT file system, e.g. a card of 64 GB or
/// more as sold, over the same [`SharedDisk`] as
/// [`FatStorage`](crate::FatStorage), its blocks kept and written back in
/// the order they were changed (see [`BlockDisk`](crate::BlockDisk)).
///
/// The file system is hadris-fat's exFAT driver (`ExFatFs`, as patched
/// for PaperPaw): it marks clusters in the FAT and the allocation bitmap
/// before an entry set names them and frees them only after the set is
/// gone, sets `VolumeDirty` before its first write after a mount or a sync
/// and clears it on [`Storage::sync`], and streams the bitmap and the FAT
/// through one block: nothing of the card's size is kept in memory.
///
/// What it leaves to a cut, this storage settles as `FatStorage` does:
///
/// - A file is written whole under a name of the moment (`<name>~pp.tmp`),
///   synced, renamed to a second one (`<name>~<mark>~pp.new`) once whole,
///   and only then renamed over the file it replaces. Cut off at any
///   point, the file reads its old content or its new one, whole. The
///   mark is the state of the file it replaces (first cluster, size and
///   time of change): where the card is changed elsewhere after a cut (a
///   computer writes the file anew) and the file is no longer in that
///   state, the change is the newer and the whole content is let go, not
///   put over it. A folder with entry sets a cut left torn (sealed again,
///   see below) settles as before: the replacement takes the place, as the
///   state of a torn file tells nothing. The names of
///   the moment hold the file's own name, so a replacement cut off where
///   the old entry set was overwritten in part (one that crosses a block)
///   still finds the name it was for.
/// - A rename in exFAT writes the new entry set before it removes the old
///   one: cut off between, two names hold the same clusters. Such a pair
///   is told by its first cluster and the stale name taken away without
///   freeing them (`ExFatFs::unlink_entry`, added for PaperPaw).
/// - What a cut left in a folder is settled when the folder is first
///   looked at after a mount, before any file in it is opened: an entry
///   set written across two blocks and cut off between is sealed again
///   as it stands (its clusters were taken before it named them), the
///   entries of one cut off in its removal are let go
///   (`ExFatFs::sweep`, added for PaperPaw); a whole new content takes
///   its file's place, a part of one goes. A folder counts as settled
///   only when this is done: after an error (a read or write failing) it
///   is not kept as looked at, and its next look settles it anew.
/// - A move is recorded first in [`JOURNAL`](crate::JOURNAL) at the root,
///   and finished at the next mount before anything else; as the folders,
///   it is marked finished only when it is, and tried again after an
///   error.
///
/// The names of the moment are neither listed nor taken by name. Times
/// are not kept (the X4 has no clock of its own): new entries are dated
/// 1980-01-01, as on FAT.
pub struct ExFatStorage<B: Blocks + 'static> {
    fs: RefCell<Volume<B>>,
    disk: SharedDisk<B>,
    /// The files used last, the latest last.
    open: RefCell<Vec<Open>>,
    /// The folders looked up last, the latest last, by their paths.
    folders: RefCell<Vec<(String, NodeId)>>,
    /// The folders settled since the mount, by a hash of their paths.
    settled: RefCell<Vec<u64>>,
    /// The journal of moves was looked at since the mount.
    moves_settled: Cell<bool>,
}

impl<B: Blocks + 'static> core::fmt::Debug for ExFatStorage<B> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ExFatStorage").finish_non_exhaustive()
    }
}

impl<B: Blocks + 'static> ExFatStorage<B> {
    /// The exFAT volume on `disk` (one [`BlockDisk::find`](crate::BlockDisk::find)
    /// found), mounted. Never formats anything.
    pub fn mount(disk: SharedDisk<B>) -> Result<Self, StorageError> {
        let options = MountOptions::new()
            .with_clock(&NO_CLOCK)
            .with_node_limit(NODES);
        let fs = ExFatFs::mount(ExDisk(disk.clone()), options).map_err(|_| StorageError::Io)?;
        Ok(Self {
            fs: RefCell::new(fs),
            disk,
            open: RefCell::new(Vec::new()),
            folders: RefCell::new(Vec::new()),
            settled: RefCell::new(Vec::new()),
            moves_settled: Cell::new(false),
        })
    }

    /// Writes back the disk's changed blocks.
    fn write_back(&self) -> Result2<()> {
        self.disk.flush_all().map_err(|_| StorageError::Io)
    }

    /// `result`, the disk's changed blocks written back after it.
    fn written<T>(&self, result: Result2<T>) -> Result2<T> {
        let back = self.write_back();
        let value = result?;
        back.map(|()| value)
    }

    // Nodes and folders.

    /// Drops the pin of `node`, unless it is the root.
    fn forget(&self, node: NodeId) {
        let mut fs = self.fs.borrow_mut();
        if node != fs.root() {
            fs.forget(node, 1);
        }
    }

    /// The folder at `path` (`""` for the root), pinned by the folders
    /// kept: looked up there, else from its parent's, settled when first
    /// looked at. With `make`, missing folders are made.
    fn dir(&self, path: &str, make: bool) -> Result2<NodeId> {
        if path.is_empty() {
            let root = self.fs.borrow().root();
            self.settle_folder("", root)?;
            return Ok(root);
        }
        {
            let mut folders = self.folders.borrow_mut();
            if let Some(i) = folders.iter().position(|(p, _)| same_path(p, path)) {
                let kept = folders.remove(i);
                let node = kept.1;
                folders.push(kept);
                return Ok(node);
            }
        }
        let up = self.dir(parent(path), make)?;
        let name = Name::new(name(path));
        let found = self.fs.borrow_mut().lookup(up, name);
        let node = match found {
            Ok(node) => node,
            Err(e) if make && e.kind() == ErrorKind::NotFound => self
                .fs
                .borrow_mut()
                .mkdir(up, name, &SetAttr::new())
                .map_err(error)?,
            Err(e) => return Err(error(e)),
        };
        let is_dir = self
            .fs
            .borrow_mut()
            .stat(node)
            .map(|m| m.file_type() == FileType::Dir);
        if !matches!(is_dir, Ok(true)) {
            self.forget(node);
            return Err(StorageError::NotFound);
        }
        let mut folders = self.folders.borrow_mut();
        if folders.len() >= FOLDERS {
            let (_, old) = folders.remove(0);
            drop(folders);
            self.forget(old);
            folders = self.folders.borrow_mut();
        }
        folders.push((path.to_string(), node));
        drop(folders);
        if let Err(e) = self.settle_folder(path, node) {
            // Not kept unsettled: the next look at the folder settles it
            // again (a kept folder is not looked at anew).
            self.folders
                .borrow_mut()
                .retain(|(p, _)| !same_path(p, path));
            self.forget(node);
            return Err(e);
        }
        Ok(node)
    }

    /// Lets go of the folders kept at or below `path` (all where it is
    /// the root): they are moved or removed.
    fn drop_folders(&self, path: &str) {
        let gone: Vec<NodeId> = {
            let mut folders = self.folders.borrow_mut();
            let mut gone = Vec::new();
            folders.retain(|(p, node)| {
                let below = path.is_empty() || crate::is_within(p, path);
                if below {
                    gone.push(*node);
                }
                !below
            });
            gone
        };
        for node in gone {
            self.forget(node);
        }
    }

    /// The node named `name` in `dir`, pinned, if there is one.
    fn find(&self, dir: NodeId, name: &str) -> Result2<Option<NodeId>> {
        match self.fs.borrow_mut().lookup(dir, Name::new(name)) {
            Ok(node) => Ok(Some(node)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(error(e)),
        }
    }

    /// The first cluster of `node`'s data, if it has any.
    fn first_cluster(&self, node: NodeId) -> Result2<Option<u64>> {
        let mut runs = [hadris_fs::Extent::new(0, 0); 1];
        let n = self
            .fs
            .borrow_mut()
            .extents(node, 0, &mut runs)
            .map_err(error)?;
        Ok((n > 0).then(|| runs[0].offset()))
    }

    /// Whether `a` and `b` in `dir` hold the same clusters.
    fn share(&self, dir: NodeId, a: &str, b: &str) -> Result2<bool> {
        let (Some(x), Some(y)) = (self.find(dir, a)?, self.find(dir, b)?) else {
            return Ok(false);
        };
        let shared = match (self.first_cluster(x), self.first_cluster(y)) {
            (Ok(Some(p)), Ok(Some(q))) => Ok(p == q),
            (Ok(_), Ok(_)) => Ok(false),
            (Err(e), _) | (_, Err(e)) => Err(e),
        };
        self.forget(x);
        self.forget(y);
        shared
    }

    /// What `node`, a file, is now: a digest of its first cluster, size
    /// and modification time. A name of the content made whole holds the
    /// state of the file it is to replace when it was made, and the file
    /// is changed since if its state is another: on a computer, say,
    /// where the card was taken after a cut. (Not by times alone: the X4
    /// dates its writes 1980 and keeps none, but any other writer sets
    /// one.)
    fn state_of(&self, node: NodeId) -> Result2<u32> {
        let cluster = self.first_cluster(node)?.unwrap_or(0);
        let meta = self.fs.borrow_mut().stat(node).map_err(error)?;
        let modified = meta.modified().map_or(0, |t| t.unix_seconds());
        let bytes = cluster
            .to_le_bytes()
            .into_iter()
            .chain(meta.len().to_le_bytes())
            .chain(modified.to_le_bytes());
        Ok(crate::fnv(bytes) as u32)
    }

    // Open files.

    /// Runs `f` on the file at `path`, opened (for writing where `write`,
    /// made where missing then), kept open after.
    fn with_open<R>(
        &self,
        path: &str,
        write: bool,
        f: impl FnOnce(&mut Volume<B>, &mut Open) -> Result2<R>,
    ) -> Result2<R> {
        check(path)?;
        let found = self
            .open
            .borrow()
            .iter()
            .position(|o| same_path(&o.path, path));
        let mut kept = match found {
            Some(i) if write && !self.open.borrow()[i].write => {
                let o = self.open.borrow_mut().remove(i);
                self.close_open(o)?;
                self.open_file(path, write)?
            }
            Some(i) => self.open.borrow_mut().remove(i),
            None => self.open_file(path, write)?,
        };
        let result = f(&mut self.fs.borrow_mut(), &mut kept);
        let mut open = self.open.borrow_mut();
        if open.len() >= OPEN_FILES {
            let old = open.remove(0);
            drop(open);
            self.close_open(old)?;
            open = self.open.borrow_mut();
        }
        open.push(kept);
        result
    }

    /// The file at `path`, opened.
    fn open_file(&self, path: &str, write: bool) -> Result2<Open> {
        let dir = self.dir(parent(path), write)?;
        let name = name(path);
        let node = match self.find(dir, name)? {
            Some(node) => node,
            None if write => self
                .fs
                .borrow_mut()
                .create(dir, Name::new(name), &SetAttr::new())
                .map_err(error)?,
            None => return Err(StorageError::NotFound),
        };
        let opened = (|| {
            let mut fs = self.fs.borrow_mut();
            let meta = fs.stat(node).map_err(error)?;
            if meta.file_type() != FileType::File {
                return Err(StorageError::NotFound);
            }
            let mode = if write {
                OpenMode::Write
            } else {
                OpenMode::Read
            };
            fs.open(node, mode).map_err(error)?;
            Ok(meta.len())
        })();
        match opened {
            Ok(len) => Ok(Open {
                path: path.to_string(),
                node,
                len,
                write,
            }),
            Err(e) => {
                self.forget(node);
                Err(e)
            }
        }
    }

    /// Closes `o`: its size written back where it was written to.
    fn close_open(&self, o: Open) -> Result2<()> {
        let closed = self.fs.borrow_mut().close(o.node).map_err(error);
        self.forget(o.node);
        closed
    }

    /// Closes the file at `path` (and all below it where `tree`), if open.
    fn close(&self, path: &str, tree: bool) -> Result2<()> {
        let gone: Vec<Open> = {
            let mut open = self.open.borrow_mut();
            let mut gone = Vec::new();
            let mut i = 0;
            while i < open.len() {
                let o = &open[i];
                let hit = path.is_empty()
                    || same_path(&o.path, path)
                    || (tree && crate::is_within(&o.path, path));
                if hit {
                    gone.push(open.remove(i));
                } else {
                    i += 1;
                }
            }
            gone
        };
        let mut result = Ok(());
        for o in gone {
            if let Err(e) = self.close_open(o) {
                result = Err(e);
            }
        }
        result
    }

    // Settling what a cut left.

    /// Settles what a cut left in the folder at `path` (`node`), once a
    /// mount (see [`ExFatStorage`]): marked settled only when it is, so that
    /// a failure (a read error, a card taken out) is tried again at the next
    /// look at the folder, and the folder is not worked in meanwhile.
    fn settle_folder(&self, path: &str, node: NodeId) -> Result2<()> {
        let key = hash(path);
        if self.settled.borrow().contains(&key) {
            return Ok(());
        }
        self.settle_names(node)?;
        self.settled.borrow_mut().push(key);
        Ok(())
    }

    /// The work of [`settle_folder`](Self::settle_folder): idempotent, so
    /// that it can be done again from the start after a failure.
    fn settle_names(&self, node: NodeId) -> Result2<()> {
        // Entry sets a cut left torn, sealed or let go (see
        // `ExFatFs::sweep`, added for PaperPaw).
        let swept = self.fs.borrow_mut().sweep(node).map_err(error)?;
        let mut files: Vec<String> = Vec::new();
        self.each(node, &mut |name, _, _| {
            if let Some((file, _)) = temp_of(name)
                && !files.iter().any(|f| same_path(f, file))
            {
                files.push(file.to_string());
            }
        })?;
        for file in files {
            self.settle(node, &file, swept > 0)?;
        }
        self.write_back()
    }

    /// Settles the names a write of `file` in `dir` went through. `torn`:
    /// the folder had entry sets a cut left torn (and sealed again).
    fn settle(&self, dir: NodeId, file: &str, torn: bool) -> Result2<()> {
        let tmp = format!("{file}{TMP}");
        let fs_unlink = |name: &str, keep: bool| -> Result2<()> {
            let mut fs = self.fs.borrow_mut();
            let done = if keep {
                fs.unlink_entry(dir, Name::new(name))
            } else {
                fs.unlink(dir, Name::new(name))
            };
            match done {
                Err(e) if e.kind() != ErrorKind::NotFound => Err(error(e)),
                _ => Ok(()),
            }
        };
        // The contents of the file made whole, with their marks.
        let mut wholes: Vec<(String, Option<u32>)> = Vec::new();
        self.each(dir, &mut |name, _, _| {
            if let Some((of, Temp::Whole(mark))) = temp_of(name)
                && same_path(of, file)
            {
                wholes.push((name.to_string(), mark));
            }
        })?;
        if let Some(node) = self.find(dir, &tmp)? {
            self.forget(node);
            // Renamed already, cut off before its old name went: its
            // clusters are the other name's.
            let mut renamed = self.share(dir, &tmp, file)?;
            for (whole, _) in &wholes {
                renamed = renamed || self.share(dir, &tmp, whole)?;
            }
            fs_unlink(&tmp, renamed)?;
        }
        for (whole, mark) in wholes {
            let Some(node) = self.find(dir, &whole)? else {
                continue;
            };
            self.forget(node);
            if self.share(dir, &whole, file)? {
                fs_unlink(&whole, true)?;
                continue;
            }
            let target = self.find(dir, file)?;
            // Whole, it takes its file's place, unless the file was changed
            // since (on a computer, say) while its replacement was cut off:
            // that change is the newer, and the replacement let go. A file
            // torn by the replacement itself (sealed again by the sweep
            // above) is no such change; nor is one whose state the mark
            // cannot tell (a name without it).
            let changed = match target {
                Some(t) => {
                    let now = self.state_of(t);
                    self.forget(t);
                    let now = now?;
                    !torn && mark.is_some_and(|m| m != now)
                }
                None => false,
            };
            if changed {
                fs_unlink(&whole, false)?;
            } else {
                self.fs
                    .borrow_mut()
                    .rename(
                        dir,
                        Name::new(&whole),
                        dir,
                        Name::new(file),
                        RenameMode::Replace,
                    )
                    .map_err(error)?;
            }
        }
        Ok(())
    }

    /// Finishes a move the journal holds, once a mount.
    fn settle_moves(&self) -> Result2<()> {
        if self.moves_settled.get() {
            return Ok(());
        }
        self.finish_moves()?;
        // Only when it is done: a failure (a read error, a card taken
        // out) is tried again at the next call, before anything else is
        // changed. What it did before is done again from its start, as it
        // finds it.
        self.moves_settled.set(true);
        Ok(())
    }

    /// The work of [`settle_moves`](Self::settle_moves).
    fn finish_moves(&self) -> Result2<()> {
        let root = self.fs.borrow().root();
        let Some(journal) = self.find(root, JOURNAL)? else {
            return Ok(());
        };
        let record = self.read_node(journal, 64 * 1024);
        self.forget(journal);
        let record = record?;
        let Some((from, to)) = recorded_move(&record) else {
            return Ok(());
        };
        let (from, to) = (from.to_string(), to.to_string());
        let (from_dir, to_dir) = (self.dir(parent(&from), false), self.dir(parent(&to), true));
        if let (Ok(from_dir), Ok(to_dir)) = (from_dir, to_dir) {
            let source = self.find(from_dir, name(&from))?;
            let target = self.find(to_dir, name(&to))?;
            let same = match (source, target) {
                (Some(s), Some(t)) => {
                    let same = matches!(
                        (self.first_cluster(s)?, self.first_cluster(t)?),
                        (Some(p), Some(q)) if p == q
                    ) || s == t;
                    self.forget(s);
                    self.forget(t);
                    same
                }
                (Some(s), None) => {
                    self.forget(s);
                    false
                }
                (None, Some(t)) => {
                    self.forget(t);
                    true
                }
                (None, None) => true,
            };
            let mut fs = self.fs.borrow_mut();
            if source.is_some() && same && !same_path(&from, &to) {
                // Moved, its old name left: taken away, the clusters kept.
                match fs.unlink_entry(from_dir, Name::new(name(&from))) {
                    Err(e) if e.kind() == ErrorKind::IsADirectory => {
                        // A folder's: its entries are the moved one's.
                        fs.rename(
                            from_dir,
                            Name::new(name(&from)),
                            to_dir,
                            Name::new(name(&to)),
                            RenameMode::NoReplace,
                        )
                        .ok();
                    }
                    Err(e) if e.kind() != ErrorKind::NotFound => return Err(error(e)),
                    _ => {}
                }
            } else if source.is_some() {
                fs.rename(
                    from_dir,
                    Name::new(name(&from)),
                    to_dir,
                    Name::new(name(&to)),
                    RenameMode::Replace,
                )
                .map_err(error)?;
            }
        }
        self.clear_journal()?;
        self.write_back()
    }

    /// Up to `max` bytes of the file `node`.
    fn read_node(&self, node: NodeId, max: usize) -> Result2<Vec<u8>> {
        let mut fs = self.fs.borrow_mut();
        let len = fs.stat(node).map_err(error)?.len();
        let len = usize::try_from(len).unwrap_or(usize::MAX).min(max);
        let mut data = alloc::vec![0u8; len];
        fs.open(node, OpenMode::Read).map_err(error)?;
        let mut at = 0;
        let read = loop {
            if at == len {
                break Ok(());
            }
            match fs.read(node, at as u64, &mut data[at..]) {
                Ok(0) => break Err(StorageError::Io),
                Ok(n) => at += n,
                Err(e) => break Err(error(e)),
            }
        };
        let closed = fs.close(node).map_err(error);
        read.and(closed)?;
        Ok(data)
    }

    /// Writes `data` at the start of the journal, made where missing, and
    /// syncs it.
    fn write_journal(&self, data: &[u8]) -> Result2<()> {
        let root = self.fs.borrow().root();
        let node = match self.find(root, JOURNAL)? {
            Some(node) => node,
            None => self
                .fs
                .borrow_mut()
                .create(root, Name::new(JOURNAL), &SetAttr::new())
                .map_err(error)?,
        };
        let result = (|| {
            let mut fs = self.fs.borrow_mut();
            fs.open(node, OpenMode::Write).map_err(error)?;
            let written = write_all(&mut fs, node, 0, data);
            let closed = fs.close(node).map_err(error);
            written.and(closed)?;
            fs.fsync(node).map_err(error)
        })();
        self.forget(node);
        result
    }

    /// Clears the journal: its head becomes zeros.
    fn clear_journal(&self) -> Result2<()> {
        let root = self.fs.borrow().root();
        match self.find(root, JOURNAL)? {
            Some(node) => {
                self.forget(node);
                self.write_journal(&[0u8; crate::settle::MOVE_HEAD])
            }
            None => Ok(()),
        }
    }

    // What the calls do.

    /// Calls `visit` with the name, whether a folder, and the size of each
    /// entry of `dir`, the storage's own too.
    fn each(
        &self,
        dir: NodeId,
        visit: &mut dyn FnMut(&str, bool, &hadris_fs::Metadata),
    ) -> Result2<()> {
        let mut cursor = DirCursor::START;
        // Bounded: a folder whose clusters loop (a damaged card) ends.
        for _ in 0..crate::ENTRIES_MAX {
            let entry = self.fs.borrow_mut().readdir(dir, cursor).map_err(error)?;
            let Some(entry) = entry else {
                return Ok(());
            };
            cursor = entry.next_cursor();
            let name = core::str::from_utf8(entry.name().as_bytes()).unwrap_or("");
            visit(name, entry.file_type() == FileType::Dir, entry.metadata());
        }
        Err(StorageError::Io)
    }

    /// [`Storage::write`], its blocks not yet written back.
    fn write_whole(&self, path: &str, data: &[u8]) -> Result2<()> {
        check(path)?;
        let name = name(path);
        if name.encode_utf16().count() + 1 + MARK + NEW.len() > LONGEST {
            return Err(StorageError::InvalidPath);
        }
        self.close(path, false)?;
        let dir = self.dir(parent(path), true)?;
        // The state of the file replaced, which its replacement carries in
        // its name (see `state_of`).
        let there = match self.find(dir, name)? {
            Some(node) => {
                let is_dir = self
                    .fs
                    .borrow_mut()
                    .stat(node)
                    .map(|m| m.file_type() == FileType::Dir);
                let state = self.state_of(node);
                self.forget(node);
                if is_dir.map_err(error)? {
                    return Err(StorageError::InvalidPath);
                }
                Some(state?)
            }
            None => None,
        };
        let tmp = format!("{name}{TMP}");
        let new = whole_name(name, there.unwrap_or(0));
        let node = self
            .fs
            .borrow_mut()
            .create(dir, Name::new(&tmp), &SetAttr::new())
            .map_err(error)?;
        let written = (|| {
            let mut fs = self.fs.borrow_mut();
            fs.open(node, OpenMode::Write).map_err(error)?;
            let written = write_all(&mut fs, node, 0, data);
            let closed = fs.close(node).map_err(error);
            written.and(closed)?;
            fs.fsync(node).map_err(error)
        })();
        self.forget(node);
        if let Err(e) = written {
            // A card that is full keeps no part of the new content.
            #[expect(
                clippy::let_underscore_must_use,
                reason = "cleaning up after a failed write, whose own error is the one told"
            )]
            let _ = self.fs.borrow_mut().unlink(dir, Name::new(&tmp));
            return Err(e);
        }
        let mut fs = self.fs.borrow_mut();
        let (tmp, new, file) = (Name::new(&tmp), Name::new(&new), Name::new(name));
        if there.is_some() {
            fs.rename(dir, tmp, dir, new, RenameMode::NoReplace)
                .map_err(error)?;
            fs.rename(dir, new, dir, file, RenameMode::Replace)
                .map_err(error)
        } else {
            fs.rename(dir, tmp, dir, file, RenameMode::NoReplace)
                .map_err(error)
        }
    }

    /// [`Storage::remove`], its blocks not yet written back.
    fn remove_file(&self, path: &str) -> Result2<()> {
        check(path)?;
        self.close(path, false)?;
        let dir = match self.dir(parent(path), false) {
            Ok(dir) => dir,
            Err(StorageError::NotFound) => return Ok(()),
            Err(e) => return Err(e),
        };
        let mut fs = self.fs.borrow_mut();
        match fs.unlink(dir, Name::new(name(path))) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) if e.kind() == ErrorKind::IsADirectory => {
                match fs.rmdir(dir, Name::new(name(path))) {
                    Ok(()) => Ok(()),
                    Err(e) => Err(error(e)),
                }
            }
            Err(e) => Err(error(e)),
        }
    }

    /// [`Storage::rename`], its blocks not yet written back.
    fn move_entry(&self, from: &str, to: &str) -> Result2<()> {
        check(from)?;
        check(to)?;
        if same_path(from, to) && from == to {
            return Ok(());
        }
        if crate::is_within(to, from) && !same_path(from, to) {
            return Err(StorageError::InvalidPath);
        }
        self.close(from, true)?;
        self.close(to, true)?;
        let from_dir = self.dir(parent(from), false)?;
        let source = self
            .find(from_dir, name(from))?
            .ok_or(StorageError::NotFound)?;
        self.forget(source);
        let to_dir = self.dir(parent(to), true)?;
        if let Some(target) = self.find(to_dir, name(to))? {
            let is_dir = self
                .fs
                .borrow_mut()
                .stat(target)
                .map(|m| m.file_type() == FileType::Dir);
            self.forget(target);
            if is_dir.map_err(error)? && !same_path(from, to) {
                return Err(StorageError::InvalidPath);
            }
        }
        self.drop_folders(from);
        // Recorded first: cut off, it is finished at the next mount.
        self.write_journal(&move_record(from, to)?)?;
        // The folders may have been let go above.
        let from_dir = self.dir(parent(from), false)?;
        let to_dir = self.dir(parent(to), true)?;
        let moved = self
            .fs
            .borrow_mut()
            .rename(
                from_dir,
                Name::new(name(from)),
                to_dir,
                Name::new(name(to)),
                RenameMode::Replace,
            )
            .map_err(error);
        self.write_back()?;
        let cleared = self.clear_journal();
        moved.and(cleared)
    }

    /// [`Storage::remove_dir_all`], its blocks not yet written back.
    fn remove_tree(&self, path: &str) -> Result2<()> {
        let dir = match self.dir(path, false) {
            Ok(dir) => dir,
            Err(StorageError::NotFound) => return Ok(()),
            Err(e) => return Err(e),
        };
        let mut entries: Vec<(String, bool)> = Vec::new();
        self.each(dir, &mut |name, is_dir, _| {
            entries.push((name.to_string(), is_dir))
        })?;
        for (entry, is_dir) in entries {
            let below = format!("{path}/{entry}");
            if is_dir {
                self.remove_tree(&below)?;
            } else {
                let dir = self.dir(path, false)?;
                match self.fs.borrow_mut().unlink(dir, Name::new(&entry)) {
                    Err(e) if e.kind() != ErrorKind::NotFound => return Err(error(e)),
                    _ => {}
                }
            }
        }
        self.drop_folders(path);
        let up = self.dir(parent(path), false)?;
        match self.fs.borrow_mut().rmdir(up, Name::new(name(path))) {
            Err(e) if e.kind() != ErrorKind::NotFound => Err(error(e)),
            _ => Ok(()),
        }
    }
}

/// Writes all of `data` to the open file `node` from `at` on.
fn write_all<B: Blocks>(fs: &mut Volume<B>, node: NodeId, at: u64, data: &[u8]) -> Result2<()> {
    let mut done = 0;
    while done < data.len() {
        match fs.write(node, at + done as u64, &data[done..]) {
            Ok(0) => return Err(StorageError::Io),
            Ok(n) => done += n,
            Err(e) => return Err(error(e)),
        }
    }
    Ok(())
}

impl<B: Blocks + 'static> Storage for ExFatStorage<B> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.settle_moves()?;
        self.with_open(path, false, |fs, o| {
            if offset >= o.len {
                return Ok(0);
            }
            let want = buf
                .len()
                .min(usize::try_from(o.len - offset).unwrap_or(usize::MAX));
            let mut read = 0;
            while read < want {
                match fs.read(o.node, offset + read as u64, &mut buf[read..want]) {
                    Ok(0) => return Err(StorageError::Io),
                    Ok(n) => read += n,
                    Err(e) => return Err(error(e)),
                }
            }
            Ok(read)
        })
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.settle_moves()?;
        self.with_open(path, false, |_, o| Ok(o.len))
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.settle_moves()?;
        self.written(self.write_whole(path, data))
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.settle_moves()?;
        self.with_open(path, true, |fs, o| {
            let written = write_all(fs, o.node, o.len, data);
            // As far as it came, where the card is full.
            o.len = fs.stat(o.node).map_err(error)?.len();
            written
        })
    }

    fn sync(&self) -> Result<(), StorageError> {
        let closed = self.close("", true);
        let synced = self.fs.borrow_mut().sync().map_err(error);
        let back = self.write_back();
        closed.and(synced).and(back)
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.settle_moves()?;
        if !dir.is_empty() {
            check(dir)?;
        }
        // The sizes of files appended to, as appended.
        self.close("", true)?;
        let node = self.dir(dir, false)?;
        self.each(node, &mut |name, is_dir, meta| {
            let own = is_temp(name) || (dir.is_empty() && same_path(name, JOURNAL));
            if !own {
                let modified = meta.modified().map(|t| t.unix_seconds());
                visit(
                    DirEntry::new(name, is_dir, if is_dir { 0 } else { meta.len() })
                        .with_modified(modified),
                );
            }
        })
    }

    fn modified(&self, path: &str) -> Option<i64> {
        let path = path.trim_matches('/');
        check(path).ok()?;
        self.settle_moves().ok()?;
        let dir = self.dir(parent(path), false).ok()?;
        let node = self.find(dir, name(path)).ok()??;
        let meta = self.fs.borrow_mut().stat(node);
        self.forget(node);
        meta.ok()?.modified().map(|t| t.unix_seconds())
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.settle_moves()?;
        self.written(self.remove_file(path))
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.settle_moves()?;
        self.written(self.move_entry(from, to))
    }

    /// exFAT names entries without regard to case, through the volume's
    /// up-case table.
    fn ignores_case(&self) -> bool {
        true
    }

    /// Counted from the allocation bitmap once a mount (a block at a
    /// time), then followed as clusters are taken and freed.
    fn free_space(&self) -> Option<u64> {
        let stats = self.fs.borrow_mut().statfs().ok()?;
        Some(stats.free_blocks() * u64::from(stats.block_size()))
    }

    fn total_space(&self) -> Option<u64> {
        let fs = self.fs.borrow();
        let info = fs.info();
        Some(u64::from(info.cluster_count()) * info.cluster_size())
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.settle_moves()?;
        check(path)?;
        let made = self.dir(path, true).map(|_| ());
        self.written(made)
    }

    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.settle_moves()?;
        check(path)?;
        self.close(path, true)?;
        let removed = self.remove_tree(path);
        self.written(removed)
    }
}
