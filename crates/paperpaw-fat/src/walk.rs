//! A folder tree removed a piece at a time ([`Storage::remove_tree_step`],
//! `docs/abi/files.md` 2.5), by position: no name is looked up, so a name no
//! path can hold (made by a computer: reserved, a trailing dot, over 255
//! bytes) is no obstacle, and the walk goes as deep as FAT does, up to
//! [`DEPTH`] levels.
//!
//! The place is the depth of the folder being emptied below the tree's root
//! and the slot of its next entry. The folder is found again by going down
//! from the root through the **first subfolder** of each folder: the entries
//! before the one the walk went into are gone (it removes in the order of the
//! slots), so that subfolder is the one it was in. A place that does not lead
//! there (the chain is shorter, the slot begins no entry) starts the nearest
//! folder it can find over at its first entry: slower, never outside the
//! tree. The walk only goes down by what it has listed; it never follows a
//! folder's `..`.
//!
//! Reads, as the entries rust-fatfs gives (a deleted slot is skipped inside
//! it): at most [`MAX_READS`] in the folders being emptied, and at most
//! [`DESCENT`] finding a folder again (three a level: `.`, `..` and the
//! subfolder; files met on the way are removed as they are read), the working folder's `.` and `..` among the former. At most
//! [`MAX_REMOVALS`] entries are removed. A folder is removed once the walk
//! has taken every entry out of it, without reading it through again.
//! Finding a folder again reads, with its entries, the deleted slots of its
//! parents before the subfolder (rust-fatfs skips them inside its iterator and
//! they are not counted): blocks, not entries, and only while that subfolder is
//! being emptied.
//!
//! A folder whose entries tell that its clusters hold something else
//! ([`damaged`]) is cut off where its parent names it: nothing in it is
//! followed or freed. Files a write goes through (`is_temp_name`) are
//! unlinked, their clusters left to the file they may belong to (a cut-off
//! write is the only way to have any).

use super::*;
use paperpaw_platform::tree::{MAX_READS, MAX_REMOVALS, TreeStep};

/// Deepest folder below the root of a tree the walk goes into.
pub const DEPTH: usize = 256;
/// Most entries read finding the working folder again in one call.
pub(super) const DESCENT: u32 = 3 * DEPTH as u32;
const SLOT: u64 = 32;

/// What a call read from the folders.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Reads {
    /// Entries read in the folders being emptied.
    pub(super) work: u32,
    /// Entries read finding them again.
    pub(super) descent: u32,
}

fn encode(depth: usize, slot: u32) -> u64 {
    (depth as u64) << 32 | u64::from(slot)
}

/// A place that is none of ours (a depth past the walk's) is stale: 0.
fn decode(place: u64) -> (usize, u32) {
    let depth = place >> 32;
    if depth > DEPTH as u64 {
        return (0, 0);
    }
    (depth as usize, place as u32)
}

/// A folder the walk is in.
struct Frame<IO: ReadWriteSeek + 'static> {
    dir: FatDir<IO>,
    /// Its folder and its entry there; none for the root of the tree.
    up: Option<(FatDir<IO>, FatEntry<IO>)>,
    /// Its path, while every name on it is one a path holds.
    path: Option<String>,
    /// The first cluster it has (what its `.` names, and what its subfolders'
    /// `..` must name); none for the root of the card.
    own: Option<u32>,
    /// What its `.` and `..` must name, where that is known (a folder met
    /// from its parent): a folder entry cross-linked to another folder fails
    /// it.
    place: Option<damaged::Place>,
    /// Entries before the first that are not `.` and `..`: 2 where there
    /// are none (the root of the card).
    base: usize,
}

impl<IO: ReadWriteSeek + 'static> Frame<IO> {
    /// The folder of `entry` in this one.
    fn child(&self, entry: FatEntry<IO>, name: &str) -> Self {
        let own = entry.first_cluster();
        Self {
            dir: entry.to_dir(),
            path: child_path(&self.path, name),
            own,
            place: Some((own, self.own)),
            base: 0,
            up: Some((self.dir.clone(), entry)),
        }
    }
}

/// What working a folder came to. Small on purpose: the frames of the walk
/// are not copied around (a frame is some 250 bytes of stack on the devices,
/// and the walk is reached through `&dyn Storage` with 2,048 bytes to its
/// name): a subfolder to go into replaces the folder's frame in place.
enum Outcome {
    /// A limit was reached: go on from this slot.
    Yield(u32),
    /// The frame now is the subfolder to go into.
    Down,
    /// Every entry is out of the folder.
    Emptied,
    /// The folder is damaged.
    Cut,
}

fn child_path(parent: &Option<String>, name: &str) -> Option<String> {
    let path = child(parent.as_deref()?, name);
    (path.len() <= 640 && check(&path).is_ok()).then_some(path)
}

impl<IO: ReadWriteSeek + 'static> FatStorage<IO> {
    /// [`Storage::remove_tree_step`] with what it read counted. Not inlined,
    /// nor are the walk's other steps: each keeps its own stack frame, so
    /// that none adds to the others' (the largest is below 2,048 bytes on
    /// the ESP32-S3).
    #[inline(never)]
    pub(super) fn walk_tree(
        &self,
        root: &str,
        keep_root: bool,
        place: u64,
        reads: &mut Reads,
    ) -> Result<TreeStep, StorageError> {
        let root = root.trim_matches('/');
        self.settle_moves()?;
        // The sizes in the folders, as appended.
        self.sync()?;
        let top = match self.open_tree(root)? {
            Ok(top) => top,
            Err(step) => return Ok(step),
        };
        // The cluster the root of the tree has: what its `.` names.
        let own = match entries(&top).next() {
            Some(entry) => {
                let entry = entry?;
                (entry.file_name() == ".")
                    .then(|| entry.first_cluster())
                    .flatten()
            }
            None => None,
        };
        reads.work += 1;
        let start = || Frame {
            dir: top.clone(),
            up: None,
            path: Some(root.to_string()),
            own,
            place: None,
            base: if root.is_empty() { 2 } else { 0 },
        };
        let (mut depth, mut slot) = decode(place);
        let mut removed = 0u32;
        // A place whose folder cannot be found within the call's share of
        // reads (a folder of files where it names subfolders) is stale: the
        // walk starts over.
        let mut frame = start();
        let reached = match self.descend(&mut frame, depth, reads, &mut removed)? {
            Some(reached) => reached,
            None => {
                frame = start();
                0
            }
        };
        if reached < depth {
            (depth, slot) = (reached, 0);
        }
        loop {
            let outcome = self.work(
                &mut frame,
                depth,
                slot,
                root.is_empty(),
                &mut removed,
                reads,
            )?;
            match outcome {
                Outcome::Yield(next) => {
                    return Ok(TreeStep::new(removed, false, encode(depth, next)));
                }
                Outcome::Down => {
                    depth += 1;
                    slot = 0;
                }
                Outcome::Emptied | Outcome::Cut => {
                    let cut = matches!(outcome, Outcome::Cut);
                    let Some((parent, entry)) = frame.up.take() else {
                        return self.finish_tree(root, keep_root, cut, removed);
                    };
                    if cut {
                        parent.unlink_found(&entry)
                    } else {
                        parent.remove_found_emptied(&entry)
                    }
                    .map_err(error)?;
                    removed += 1;
                    if let Some(path) = &frame.path {
                        self.forget(path);
                    }
                    // On in the folder above, after the one removed.
                    depth -= 1;
                    slot = (entry.offset_range().1 / SLOT) as u32;
                    frame = start();
                    match self.descend(&mut frame, depth, reads, &mut removed)? {
                        Some(reached) => {
                            if reached < depth {
                                (depth, slot) = (reached, 0);
                            }
                        }
                        // This call's share of finding folders is used.
                        None => {
                            return Ok(TreeStep::new(removed, false, encode(depth, slot)));
                        }
                    }
                }
            }
        }
    }

    /// The folder at `root`, or the step that is the whole answer: nothing
    /// there (done), or a file there (removed).
    #[inline(never)]
    fn open_tree(&self, root: &str) -> Result<Result<FatDir<IO>, TreeStep>, StorageError> {
        let done = |removed| TreeStep::new(removed, true, 0);
        if !root.is_empty() {
            check(root)?;
            self.hold(root)?;
            self.close(root)?;
            self.forget(root);
        }
        match self.dir(root) {
            Ok(top) => Ok(Ok(top)),
            Err(StorageError::NotFound) => Ok(Err(done(0))),
            // A file there, or above it.
            Err(StorageError::InvalidPath) => match self.size(root) {
                Ok(_) => {
                    self.remove_entry(root, true)?;
                    Ok(Err(done(1)))
                }
                Err(StorageError::NotFound | StorageError::InvalidPath) => Ok(Err(done(0))),
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        }
    }

    /// The root of the tree is empty: removed (cut off where it is damaged),
    /// unless it is kept.
    #[inline(never)]
    fn finish_tree(
        &self,
        root: &str,
        keep_root: bool,
        cut: bool,
        mut removed: u32,
    ) -> Result<TreeStep, StorageError> {
        if cut {
            if root.is_empty() {
                return Err(StorageError::Io);
            }
            self.unlink_damaged(root)?;
            removed += 1;
            // A folder that is kept (the data folder) is there again, empty.
            if keep_root {
                self.create_dirs(root)?;
            }
        } else if !keep_root && !root.is_empty() {
            let up = self.dir(parent(root))?;
            if let Some(entry) = lookup(&up, name(root), true)?.entry {
                up.remove_found_emptied(&entry).map_err(error)?;
                removed += 1;
            }
        }
        self.forget(root);
        Ok(TreeStep::new(removed, true, 0))
    }

    /// File `entry` (`name`) of `frame`'s folder taken away: open ones
    /// written back first, the path forgotten.
    fn remove_file(
        &self,
        frame: &Frame<IO>,
        entry: &FatEntry<IO>,
        name: &str,
    ) -> Result<(), StorageError> {
        if let Some(path) = child_path(&frame.path, name) {
            self.hold(&path)?;
            self.close(&path)?;
            self.forget(&path);
        }
        frame.dir.remove_found(entry).map_err(error)
    }

    /// `frame` made the folder `depth` levels below it, found by the first
    /// subfolder of each folder, and how deep it got (less where a folder has
    /// none: `frame` is then the deepest folder there is); `None` where the
    /// call's share of reads is used (`frame` is then of no use).
    #[inline(never)]
    fn descend(
        &self,
        frame: &mut Frame<IO>,
        depth: usize,
        reads: &mut Reads,
        removed: &mut u32,
    ) -> Result<Option<usize>, StorageError> {
        for level in 0..depth {
            let mut found = None;
            let mut items = entries(&frame.dir);
            let mut at = frame.base;
            loop {
                // Both shares are checked before the entry is read.
                if reads.descent >= DESCENT || reads.work >= MAX_READS {
                    return Ok(None);
                }
                let Some(entry) = items.next() else {
                    break;
                };
                let entry = entry?;
                let name = entry.file_name();
                let here = at.min(2);
                at += 1;
                // A damaged folder is worked in, and cut off, not passed;
                // so is one whose `.` and `..` are not where its parent
                // says (cross-linked).
                let bad = damaged::is_damaged(self.fs, &entry, &name, here, frame.place);
                if name == "." || name == ".." || is_temp_name(&name) {
                    reads.descent += 1;
                    if bad && here < 2 {
                        break;
                    }
                    continue;
                }
                if !entry.is_dir() {
                    // A file before the subfolder (written since) is work:
                    // out, so that the subfolder is the third read of its
                    // level.
                    reads.work += 1;
                    if bad {
                        break;
                    }
                    if *removed >= MAX_REMOVALS {
                        return Ok(None);
                    }
                    self.remove_file(frame, &entry, &name)?;
                    *removed += 1;
                    continue;
                }
                reads.descent += 1;
                if !bad {
                    found = Some((entry, name));
                }
                break;
            }
            drop(items);
            let Some((entry, name)) = found else {
                return Ok(Some(level));
            };
            *frame = frame.child(entry, &name);
        }
        Ok(Some(depth))
    }

    /// Takes the entries out of `frame`'s folder from `slot` on, in order,
    /// until a subfolder (`frame` becomes it), the end, or a limit.
    #[inline(never)]
    fn work(
        &self,
        frame: &mut Frame<IO>,
        depth: usize,
        slot: u32,
        card_root: bool,
        removed: &mut u32,
        reads: &mut Reads,
    ) -> Result<Outcome, StorageError> {
        // Every folder but the root of the card begins with `.` and `..`.
        let top_of_card = card_root && depth == 0;
        if reads.work + 2 >= MAX_READS {
            return Ok(Outcome::Yield(slot));
        }
        if !top_of_card {
            let mut first = entries(&frame.dir);
            for at in 0..2 {
                let Some(entry) = first.next() else {
                    return Ok(Outcome::Cut);
                };
                let entry = entry?;
                reads.work += 1;
                if damaged::is_damaged(self.fs, &entry, &entry.file_name(), at, frame.place) {
                    return Ok(Outcome::Cut);
                }
            }
        }
        // A slot past the folder's end, or one in the middle of an entry,
        // begins none (an error of the seek is the same): from the start.
        let from = match slot >= 2
            && frame
                .dir
                .begins_entry(u64::from(slot) * SLOT)
                .unwrap_or(false)
        {
            true => slot,
            false => 0,
        };
        let open = |from: u32| -> Result<_, StorageError> {
            Ok(bounded(
                frame
                    .dir
                    .iter_from(u64::from(from) * SLOT)
                    .map_err(error)?
                    .map(|entry| entry.map_err(error)),
            ))
        };
        let (mut pass, mut next, mut again) = (from, from, false);
        let mut items = open(pass)?;
        let (entry, name) = loop {
            if *removed >= MAX_REMOVALS || reads.work >= MAX_READS {
                // In the second look at a folder (below) it starts over.
                return Ok(Outcome::Yield(if again { 0 } else { next }));
            }
            let Some(entry) = items.next() else {
                if pass > 0 {
                    // Taken out from the place on only: slots before it may
                    // have been written to since (an entry in a deleted one),
                    // and a folder freed with an entry in it loses it. Looked
                    // at once more from its first slot, within the limits.
                    (pass, next, again) = (0, 0, true);
                    items = open(0)?;
                    continue;
                }
                return Ok(Outcome::Emptied);
            };
            let entry = entry?;
            reads.work += 1;
            next = (entry.offset_range().1 / SLOT) as u32;
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            if damaged::is_damaged(self.fs, &entry, &name, 2, frame.place) {
                return Ok(Outcome::Cut);
            }
            if top_of_card && same_path(&name, JOURNAL) {
                continue;
            }
            if is_temp_name(&name) {
                // Not by name (see `check`); its clusters are the file's it
                // may be part of.
                frame.dir.unlink_found(&entry).map_err(error)?;
                *removed += 1;
                continue;
            }
            if entry.is_dir() {
                if depth + 1 > DEPTH {
                    return Err(StorageError::InvalidData);
                }
                break (entry, name);
            }
            self.remove_file(frame, &entry, &name)?;
            *removed += 1;
        };
        // The folder's iterator is done with before its frame is replaced.
        drop(items);
        *frame = frame.child(entry, &name);
        Ok(Outcome::Down)
    }
}
