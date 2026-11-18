//! Looking at one path, and removing a folder tree a piece at a time, over
//! any [`Storage`].
//!
//! [`Storage::remove_dir_all`] walks a whole tree in one call and, on the
//! storages that hold names on the heap while they do, needs room for every
//! name of a folder at once. [`remove_tree_step`] is the bounded form the
//! interface-7 file service (`docs/abi/files.md` 2.5) needs: one call shows
//! it at most [`MAX_READS`] folder entries (every entry a listing visits is
//! counted, the look-ahead and the checks that a folder is empty too;
//! finding the place's folder again is counted apart, at most [`DESCENT`])
//! and removes at most [`MAX_REMOVALS`] entries. It keeps nothing between
//! calls but a 64-bit place it hands back; the path it walks is the one
//! thing it holds on the heap, built fallibly (this is the walk for storages
//! without a step of their own: the memory card and the host's file system
//! have theirs).
//! Entries are removed one at a time, by their path or, for names no path can
//! hold, by their place ([`Storage::remove_listed`]); a folder is handed to
//! [`Storage::remove_dir_all`] only once a listing from its start has shown
//! it empty.
//!
//! The walk only ever goes **down** from the folder it is given, by names it
//! has just listed and checked ([`is_usable_name`]); it never follows a
//! folder's `..`. A place that names a folder it can no longer find is
//! **stale** and the walk goes on from the nearest folder it can find, so a
//! stale place costs time, never a removal outside the tree.
//!
//! The place holds the depth of the folder being emptied below the tree's
//! root and the storage's own place ([`Storage::list_from`]) in it. The
//! folder is found again by following, from the root, the first usable
//! subfolder of each folder down to that depth: what the walk leaves before
//! the folder it works in is only files it has removed since and names it
//! cannot use, so that first subfolder is the one it went into.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::storage::{DirEntry, Storage, StorageError, is_valid_path};

/// Most removals one [`remove_tree_step`] makes (a file, or a folder
/// that is empty).
pub const MAX_REMOVALS: u32 = 64;
/// Most folder entries one [`remove_tree_step`] has a listing show.
pub const MAX_READS: u32 = 256;
/// Deepest folder the walk goes into: 256 levels below the tree's root. A
/// folder below that ends the call with [`StorageError::InvalidData`]; what
/// was removed stays removed, nothing outside the tree is touched.
pub const MAX_DEPTH: usize = 256;
/// Most entries one call reads finding the folder of a place again: a look
/// at two entries a level.
pub const DESCENT: u32 = 2 * MAX_DEPTH as u32 + 8;
/// Most entries the removal of one entry by its place reads (the entry, the
/// one after it, and the check that a folder is empty).
const PLACE_READS: u32 = 3;
/// Longest name the walk uses, in bytes (a segment of a path).
pub const MAX_NAME: usize = 255;
/// Bits of a place that hold the storage's place; the rest (16) hold the depth,
/// which goes up to [`MAX_DEPTH`] (256, nine bits).
const POSITION_BITS: u32 = 48;
/// What a path names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Kind {
    /// There is none.
    Missing,
    /// A file.
    File,
    /// A folder.
    Folder,
}

/// What the storage says a path names. A path whose folders above are a file
/// or missing is [`Kind::Missing`].
///
/// # Errors
/// The storage's own (`Io`, a card that is gone), never `NotFound` or
/// `InvalidPath`.
pub fn kind<S: Storage + ?Sized>(storage: &S, path: &str) -> Result<Kind, StorageError> {
    match storage.size(path) {
        Ok(_) => return Ok(Kind::File),
        Err(StorageError::NotFound) => {}
        Err(StorageError::InvalidPath) => {}
        Err(other) => return Err(other),
    }
    // A folder is no file for `size`; listing tells whether it is one. The
    // first entry only is asked for.
    match storage.list_bounded(path, 0, &mut |_| false) {
        Ok(_) => Ok(Kind::Folder),
        Err(StorageError::NotFound | StorageError::InvalidPath) => Ok(Kind::Missing),
        Err(other) => Err(other),
    }
}

/// Whether a listed name can be used in a path of its own: one segment of
/// the path grammar of at most [`MAX_NAME`] bytes. Others are listed but
/// not reached by path.
#[must_use]
pub fn is_usable_name(name: &str) -> bool {
    name.len() <= MAX_NAME && !name.contains('/') && is_valid_path(name)
}

/// What one [`remove_tree_step`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TreeStep {
    /// Entries removed by this call.
    pub removed: u32,
    /// Whether the tree is gone (or, with `keep_root`, empty).
    pub done: bool,
    /// Where to go on (0 once `done`).
    pub next: u64,
}

impl TreeStep {
    /// A step that removed `removed` entries; `done` once the tree is gone,
    /// `next` the place to go on from (0 once done).
    #[must_use]
    pub const fn new(removed: u32, done: bool, next: u64) -> Self {
        Self {
            removed,
            done,
            next,
        }
    }
}

fn encode(depth: usize, position: u64) -> Result<u64, StorageError> {
    if position >> POSITION_BITS != 0 {
        return Err(StorageError::Io);
    }
    Ok((depth as u64) << POSITION_BITS | position)
}

/// The depth and the storage's place of `place`; a depth the walk does not
/// go to is stale (starts over).
fn decode(place: u64) -> (usize, u64) {
    let depth = (place >> POSITION_BITS) as usize;
    if depth > MAX_DEPTH {
        return (0, 0);
    }
    (depth, place & ((1 << POSITION_BITS) - 1))
}

/// The path being walked: the root, then `/name` per level. On the heap, built
/// fallibly (a failure is `Io`); the storage's own limits on a path are its
/// own to refuse.
struct Path {
    bytes: String,
    /// The length of the path at each depth.
    ends: Vec<usize>,
}

impl Path {
    fn new(root: &str) -> Result<Self, StorageError> {
        let mut bytes = String::new();
        bytes
            .try_reserve(root.len())
            .map_err(|_| StorageError::Io)?;
        bytes.push_str(root);
        let mut ends = Vec::new();
        ends.try_reserve(1).map_err(|_| StorageError::Io)?;
        ends.push(root.len());
        Ok(Self { bytes, ends })
    }

    fn at(&self, depth: usize) -> &str {
        self.bytes
            .get(..self.ends.get(depth).copied().unwrap_or(0))
            .unwrap_or_default()
    }

    /// The path of `name` in the folder at `depth`.
    fn child(&mut self, depth: usize, name: &str) -> Result<&str, StorageError> {
        let start = self.ends.get(depth).copied().ok_or(StorageError::Io)?;
        self.bytes.truncate(start);
        self.ends.truncate(depth + 1);
        self.bytes
            .try_reserve(1 + name.len())
            .map_err(|_| StorageError::Io)?;
        self.ends.try_reserve(1).map_err(|_| StorageError::Io)?;
        self.bytes.push('/');
        self.bytes.push_str(name);
        self.ends.push(self.bytes.len());
        Ok(&self.bytes)
    }
}

/// The entry being worked on, copied out of the listing.
struct Entry {
    name: [u8; MAX_NAME],
    len: usize,
    is_dir: bool,
    /// Whether its name is one the walk can use.
    usable: bool,
}

impl Entry {
    const EMPTY: Self = Self {
        name: [0; MAX_NAME],
        len: 0,
        is_dir: false,
        usable: false,
    };

    fn take(&mut self, entry: &DirEntry<'_>) {
        self.usable = is_usable_name(entry.name);
        self.is_dir = entry.is_dir;
        self.len = if self.usable { entry.name.len() } else { 0 };
        if self.usable {
            self.name[..self.len].copy_from_slice(entry.name.as_bytes());
        }
    }

    fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.len]).unwrap_or_default()
    }
}

/// Where the folder's listing stands after the entry just taken.
enum Next {
    /// No entry at all.
    Nothing,
    /// An entry, and it was the last.
    Last,
    /// An entry, and the place of the one after it.
    At(u64),
}

/// The entry at `position` of `dir` and the place after it. One listing call
/// asking for two entries: it stops at the second, whose place is wanted and
/// which is still there when the first is gone. Every entry shown is counted.
fn fetch<S: Storage + ?Sized>(
    storage: &S,
    dir: &str,
    position: u64,
    entry: &mut Entry,
    reads: &mut u32,
) -> Result<Next, StorageError> {
    let mut seen = 0u8;
    let stopped = storage.list_bounded(dir, position, &mut |found| {
        *reads += 1;
        if seen == 0 {
            entry.take(&found);
            seen = 1;
            true
        } else {
            seen = 2;
            false
        }
    })?;
    Ok(match (seen, stopped) {
        (0, _) => Next::Nothing,
        (2, Some(place)) => Next::At(place),
        _ => Next::Last,
    })
}

// With places that are counts of entries (`list_counted`: the default of
// storages without stable places), every removal shifts the entries after it,
// so the place after an entry skips one. The pass over a folder from its first
// entry before it is declared empty keeps this correct; it only costs extra
// passes.

/// The way down to the place's folder is longer than its share (folders added
/// above it between calls): after removals the call ends and goes on from the
/// place it was given; with none, `Io`, so that no call loops without progress.
fn out_of_descent(removed: u32, place: u64) -> Result<TreeStep, StorageError> {
    if removed == 0 {
        return Err(StorageError::Io);
    }
    Ok(TreeStep {
        removed,
        done: false,
        next: place,
    })
}

/// The step that is the whole answer where `root` is no folder: nothing there
/// (done), or a file (removed).
fn whole_answer<S: Storage + ?Sized>(
    storage: &S,
    root: &str,
) -> Result<Option<TreeStep>, StorageError> {
    let removed = match kind(storage, root)? {
        Kind::Missing => 0,
        Kind::File => {
            storage.remove(root)?;
            1
        }
        Kind::Folder => return Ok(None),
    };
    Ok(Some(TreeStep {
        removed,
        done: true,
        next: 0,
    }))
}

/// One bounded piece of removing the tree at `root` (a path the caller has
/// checked; a file is removed as a one-entry tree, a missing path is done at
/// once). With `keep_root` the folder `root` itself stays, emptied.
///
/// `place` is 0 to start, or the `next` of the call before. A call has at
/// most [`MAX_READS`] entries shown by listings and removes at most
/// [`MAX_REMOVALS`] entries; the folders above the one being emptied are read
/// again to find it, and counted. A call that uses its reads before removing
/// anything still advances: the listing it stopped in is where the next one
/// goes on. Power loss or a failing call: what was removed is gone, the rest is
/// whole, and calling again from the last place or from 0 goes on.
///
/// A folder that is **gone** when the walk lists it (`NotFound`: a place named
/// one removed since, or a storage whose folders are only their files, where
/// one loses its last file and is no folder any more) holds nothing: the walk
/// goes on in its parent from its start, and a root gone after removals is
/// done.
///
/// Names that no path can hold (a trailing dot, a reserved name, over 255
/// bytes) are removed by their place ([`Storage::remove_listed`]: a file, or a
/// folder with nothing in it). Folders with something in them are walked
/// into, as deep as [`MAX_DEPTH`] levels; one below that ends the call with
/// [`StorageError::InvalidData`], what was removed stays removed.
///
/// # Errors
/// The storage's own (a path too long for it is its refusal, `Io` for a host
/// file system); [`StorageError::InvalidData`] below [`MAX_DEPTH`] levels.
pub fn remove_tree_step<S: Storage + ?Sized>(
    storage: &S,
    root: &str,
    keep_root: bool,
    place: u64,
) -> Result<TreeStep, StorageError> {
    if let Some(step) = whole_answer(storage, root)? {
        return Ok(step);
    }
    let mut path = Path::new(root)?;
    let (mut target, resume) = decode(place);
    // The check of the kind showed one entry.
    let (mut removed, mut reads, mut descent) = (0u32, 1u32, 0u32);
    let mut level = 0usize;
    // Where each folder's listing goes on; `END` once its last entry is taken.
    const END: u64 = u64::MAX;
    let mut positions: Vec<u64> = Vec::new();
    positions.try_reserve(1).map_err(|_| StorageError::Io)?;
    positions.push(if target == 0 { resume } else { 0 });
    let mut entry = Entry::EMPTY;
    loop {
        // A look at two entries and, for a name no path holds, the three
        // reads of its removal by place must fit, so that no call shows more
        // than its share.
        if removed >= MAX_REMOVALS || reads + 2 + PLACE_READS > MAX_READS {
            let position = positions.get(level).copied().unwrap_or(0);
            return Ok(TreeStep {
                removed,
                done: false,
                next: encode(level, if position == END { 0 } else { position })?,
            });
        }
        let folder = path.at(level);
        if positions.get(level).copied() == Some(END) {
            // The listing was over; whether the folder is empty is shown by a
            // listing from its start (one that skipped an entry as entries
            // went is not proof).
            positions[level] = 0;
            continue;
        }
        let here = positions.get(level).copied().unwrap_or(0);
        // Finding the folder of a place again (down to its depth) is the
        // descent: counted apart, so that every call advances.
        let counter = if level < target {
            if descent + 2 > DESCENT {
                return out_of_descent(removed, place);
            }
            &mut descent
        } else {
            &mut reads
        };
        let next = match fetch(storage, folder, here, &mut entry, counter) {
            Ok(next) => next,
            // The folder a resumed place names is gone or no longer a
            // folder: stale. That folder's parent starts over at 0 (the
            // place is used once), never an error.
            // A folder that is gone counts as empty (see the doc).
            Err(StorageError::NotFound | StorageError::InvalidPath) if level > 0 => {
                if level <= target {
                    target = usize::MAX;
                }
                level -= 1;
                positions.truncate(level + 1);
                positions[level] = 0;
                continue;
            }
            Err(StorageError::NotFound) if level == 0 && here == 0 && removed > 0 => {
                return Ok(TreeStep::new(removed, true, 0));
            }
            Err(error) => return Err(error),
        };
        let after = match next {
            Next::Nothing if here != 0 => {
                positions[level] = 0;
                continue;
            }
            Next::Nothing => {
                // Shown empty from its start: the folder goes.
                if level == 0 {
                    if !keep_root {
                        storage.remove_dir_all(folder)?;
                        removed += 1;
                    }
                    return Ok(TreeStep {
                        removed,
                        done: true,
                        next: 0,
                    });
                }
                storage.remove_dir_all(folder)?;
                removed += 1;
                level -= 1;
                positions.truncate(level + 1);
                continue;
            }
            Next::Last => END,
            Next::At(position) => position,
        };
        positions[level] = after;
        if entry.usable && !entry.is_dir {
            let child = path.child(level, entry.name())?;
            storage.remove(child)?;
            removed += 1;
            continue;
        }
        if entry.usable {
            // A folder: down into it.
            if level + 1 > MAX_DEPTH {
                return Err(StorageError::InvalidData);
            }
            path.child(level, entry.name())?;
            level += 1;
            positions.truncate(level);
            positions.try_reserve(1).map_err(|_| StorageError::Io)?;
            positions.push(if level == target { resume } else { 0 });
            continue;
        }
        // A name no path holds: by its place (a file, or a folder with
        // nothing in it). The check that it is empty reads a few entries.
        reads += PLACE_READS;
        storage.remove_listed(path.at(level), here)?;
        removed += 1;
    }
}
