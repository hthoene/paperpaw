//! Looking at one path, and removing a folder tree a piece at a time, over
//! any [`Storage`].
//!
//! [`Storage::remove_dir_all`] walks a whole tree in one call and, on the
//! storages that hold names on the heap while they do, needs room for every
//! name of a folder at once. [`remove_tree_step`] is the bounded form the
//! interface-7 file service (`docs/abi/files.md` 2.5) needs: one call reads
//! at most [`MAX_READS`] folder entries and removes at most [`MAX_REMOVALS`],
//! keeps nothing between calls but a 64-bit place it hands back, and never
//! puts a name on the heap.
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

use crate::storage::{DirEntry, Storage, StorageError, is_valid_path};

/// Most removals one [`remove_tree_step`] makes (a file, or a folder
/// that is empty).
pub const MAX_REMOVALS: u32 = 64;
/// Most folder entries one [`remove_tree_step`] reads below its descent.
pub const MAX_READS: u32 = 256;
/// Deepest folder the walk goes into itself; a folder deeper is removed whole
/// by the storage ([`Storage::remove_dir_all`]).
pub const MAX_DEPTH: usize = 16;
/// Longest path the walk builds, in bytes.
pub const MAX_PATH: usize = 640;
/// Longest name the walk uses, in bytes (a segment of a path).
pub const MAX_NAME: usize = 255;
/// Bits of a place that hold the storage's place; the rest hold the depth.
const POSITION_BITS: u32 = 56;
/// What a path names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
pub fn kind(storage: &dyn Storage, path: &str) -> Result<Kind, StorageError> {
    match storage.size(path) {
        Ok(_) => return Ok(Kind::File),
        Err(StorageError::NotFound) => {}
        Err(StorageError::InvalidPath) => {}
        Err(other) => return Err(other),
    }
    // A folder is no file for `size`; listing tells whether it is one. The
    // first entry only is asked for.
    match storage.list_from(path, 0, &mut |_| false) {
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
pub struct TreeStep {
    /// Entries removed by this call.
    pub removed: u32,
    /// Whether the tree is gone (or, with `keep_root`, empty).
    pub done: bool,
    /// Where to go on (0 once `done`).
    pub next: u64,
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
    if depth >= MAX_DEPTH {
        return (0, 0);
    }
    (depth, place & ((1 << POSITION_BITS) - 1))
}

/// The path being walked: the root, then `/name` per level.
struct Path {
    bytes: [u8; MAX_PATH],
    /// The length of the path at each depth.
    ends: [usize; MAX_DEPTH + 1],
}

impl Path {
    fn new(root: &str) -> Option<Self> {
        let mut bytes = [0; MAX_PATH];
        bytes
            .get_mut(..root.len())?
            .copy_from_slice(root.as_bytes());
        let mut ends = [0; MAX_DEPTH + 1];
        ends[0] = root.len();
        Some(Self { bytes, ends })
    }

    fn at(&self, depth: usize) -> &str {
        // Only whole names, copied from `&str`, are put in.
        core::str::from_utf8(&self.bytes[..self.ends[depth]]).unwrap_or_default()
    }

    /// The path of `name` in the folder at `depth`, if it fits.
    fn child(&mut self, depth: usize, name: &str) -> Option<&str> {
        let start = self.ends[depth];
        let end = start.checked_add(1)?.checked_add(name.len())?;
        let slot = self.bytes.get_mut(start..end)?;
        slot[0] = b'/';
        slot[1..].copy_from_slice(name.as_bytes());
        self.ends[depth + 1] = end;
        core::str::from_utf8(&self.bytes[..end]).ok()
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

    fn take(&mut self, entry: &DirEntry<'_>, room: usize) {
        self.usable = is_usable_name(entry.name) && entry.name.len() <= room;
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
/// which is still there when the first is gone.
fn fetch(
    storage: &dyn Storage,
    dir: &str,
    position: u64,
    entry: &mut Entry,
) -> Result<Next, StorageError> {
    let room = room(dir);
    let mut seen = 0u8;
    let stopped = storage.list_from(dir, position, &mut |found| {
        if seen == 0 {
            entry.take(&found, room);
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

/// The longest name that still fits a path below `dir`.
fn room(dir: &str) -> usize {
    MAX_PATH.saturating_sub(dir.len() + 1)
}

/// Whether the folder holds an entry the walk can use.
fn holds_usable(storage: &dyn Storage, dir: &str) -> Result<bool, StorageError> {
    let mut found = false;
    let room = room(dir);
    storage.list_from(dir, 0, &mut |entry| {
        found = is_usable_name(entry.name) && entry.name.len() <= room;
        !found
    })?;
    Ok(found)
}

/// One bounded piece of removing the tree at `root` (a path the caller has
/// checked; a file is removed as a one-entry tree, a missing path is done at
/// once). With `keep_root` the folder `root` itself stays, emptied.
///
/// `place` is 0 to start, or the `next` of the call before. At most
/// [`MAX_REMOVALS`] entries are removed and [`MAX_READS`] folder entries read
/// (the folders above the one being emptied are read again to find it, and
/// not counted). A call that reads its share before removing anything
/// still advances. Power loss or a failing call: what was removed is gone, the
/// rest is whole, and calling again from the last place or from 0 goes on.
///
/// Names the walk cannot use are not followed and not counted: they are
/// removed with the folder that holds them by the storage's own
/// [`Storage::remove_dir_all`] once nothing else is in it (so a folder
/// kept with `keep_root` that holds only such names is left as it is).
///
/// # Errors
/// The storage's own; a `root` path longer than [`MAX_PATH`] is
/// [`StorageError::InvalidPath`].
pub fn remove_tree_step(
    storage: &dyn Storage,
    root: &str,
    keep_root: bool,
    place: u64,
) -> Result<TreeStep, StorageError> {
    match kind(storage, root)? {
        Kind::Missing => {
            return Ok(TreeStep {
                removed: 0,
                done: true,
                next: 0,
            });
        }
        Kind::File => {
            storage.remove(root)?;
            return Ok(TreeStep {
                removed: 1,
                done: true,
                next: 0,
            });
        }
        Kind::Folder => {}
    }
    let mut path = Path::new(root).ok_or(StorageError::InvalidPath)?;
    let (target, resume) = decode(place);
    let (mut removed, mut reads) = (0u32, 0u32);
    let mut level = 0usize;
    // Where each folder's listing goes on; `END` once its last entry is taken.
    const END: u64 = u64::MAX;
    let mut positions = [0u64; MAX_DEPTH + 1];
    positions[0] = if target == 0 { resume } else { 0 };
    let mut entry = Entry::EMPTY;
    loop {
        if removed >= MAX_REMOVALS || reads >= MAX_READS {
            let position = positions[level];
            return Ok(TreeStep {
                removed,
                done: false,
                next: encode(level, if position == END { 0 } else { position })?,
            });
        }
        let folder = path.at(level);
        let counted = level >= target;
        let next = if positions[level] == END {
            Next::Nothing
        } else {
            fetch(storage, folder, positions[level], &mut entry)?
        };
        let after = match next {
            Next::Nothing => {
                // The listing is over. The folder is done when it holds
                // nothing the walk can use (a listing that skipped an entry
                // as entries were removed is checked from its start).
                if holds_usable(storage, folder)? {
                    positions[level] = 0;
                    continue;
                }
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
                continue;
            }
            Next::Last => END,
            Next::At(position) => position,
        };
        if counted {
            reads += 1;
        }
        positions[level] = after;
        if !entry.usable {
            continue;
        }
        if !entry.is_dir {
            let Some(child) = path.child(level, entry.name()) else {
                continue;
            };
            storage.remove(child)?;
            removed += 1;
            continue;
        }
        let Some(child) = path.child(level, entry.name()) else {
            // Too long a path for the walk: its parent's sweep takes it.
            continue;
        };
        if level + 1 >= MAX_DEPTH {
            storage.remove_dir_all(child)?;
            removed += 1;
            continue;
        }
        level += 1;
        positions[level] = if level == target { resume } else { 0 };
    }
}
