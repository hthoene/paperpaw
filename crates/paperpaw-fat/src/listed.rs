//! A folder listed a piece at a time ([`Storage::list_from`]): a piece
//! goes on where the last one stopped, not from the folder's start, so a
//! folder of thousands of books is read about twice in all, not once a
//! piece. A place says where its entry begins in the folder (its first
//! slot: rust-fatfs lists from there, `Dir::iter_from`) and how many
//! entries were shown before it; where the entry that began there is gone
//! and another one was written over its slots, the place is found by
//! that count from the start instead. See the [crate documentation](crate).

use super::*;

/// The `DirEntry` of `entry` in folder `dir`, to `visit`, where it is
/// shown: not `.` and `..`, nor the storage's own files (see `check`).
pub(super) fn shown<IO: ReadWriteSeek + 'static, T>(
    dir: &str,
    entry: &FatEntry<IO>,
    visit: &mut dyn FnMut(DirEntry<'_>) -> T,
) -> Option<T> {
    let name = entry.file_name();
    let own = is_temp_name(&name) || (dir.is_empty() && same_path(&name, JOURNAL));
    if name == "." || name == ".." || own {
        return None;
    }
    let is_dir = entry.is_dir();
    let modified = Some(seconds(entry.modified()));
    Some(visit(
        DirEntry::new(&name, is_dir, if is_dir { 0 } else { entry.len() }).with_modified(modified),
    ))
}

/// Bytes of a slot of a folder.
const SLOT: u64 = 32;

/// The place of `entry`, `before` entries shown before it: the count in
/// the high half, its first slot in the low one (a folder holds at most
/// [`ENTRIES`] of either).
fn place<IO: ReadWriteSeek + 'static>(before: u64, entry: &FatEntry<IO>) -> u64 {
    let half = u64::from(u32::MAX);
    before.min(half) << 32 | (entry.offset_range().0 / SLOT).min(half)
}

/// Visits the entries of `items` that are shown, `before` shown before
/// them, until `visit` refuses one; its place, or `None` at the end.
/// Every entry is met by `read`, where given, also after one was refused.
fn visit_on<IO: ReadWriteSeek + 'static>(
    dir: &str,
    items: impl Iterator<Item = Result<FatEntry<IO>, StorageError>>,
    mut before: u64,
    mut read: Option<&mut Scan>,
    visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
) -> Result<Option<u64>, StorageError> {
    let mut stopped = None;
    for entry in items {
        let entry = entry?;
        if let Some(read) = read.as_deref_mut() {
            read.met(&entry);
        }
        if stopped.is_some() {
            continue;
        }
        match shown(dir, &entry, visit) {
            Some(true) => before += 1,
            Some(false) => {
                stopped = Some(place(before, &entry));
                // Nothing reads on past it where nobody wants the rest.
                if read.is_none() {
                    break;
                }
            }
            None => {}
        }
    }
    Ok(stopped)
}

impl<IO: ReadWriteSeek + 'static> FatStorage<IO> {
    /// [`Storage::list_from`]. From the start, the folder is read whole
    /// (the entries after the piece only looked at), as `list` does: a
    /// big one is noted for the lookups of its names that follow (see
    /// [`found::BIG`]).
    pub(super) fn list_piece(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        if !dir.is_empty() {
            check(dir)?;
        }
        // The sizes in the folders, as appended.
        self.sync()?;
        let folder = self.dir(dir)?;
        if from == 0 {
            let mut read = Scan::of(&folder);
            let stopped = visit_on(dir, entries(&folder), 0, Some(&mut read), visit)?;
            read.note(&folder);
            return Ok(stopped);
        }
        let (before, slot) = (from >> 32, from & u64::from(u32::MAX));
        if folder.begins_entry(slot * SLOT).map_err(error)? {
            let after = folder.iter_from(slot * SLOT).map_err(error)?;
            return visit_on(
                dir,
                bounded(after.map(|e| e.map_err(error))),
                before,
                None,
                visit,
            );
        }
        // Its entry's slots taken by another: as many shown skipped.
        let mut items = entries(&folder);
        let mut skip = before;
        while skip > 0 {
            match items.next() {
                Some(Ok(entry)) => {
                    if shown(dir, &entry, &mut |_| ()).is_some() {
                        skip -= 1;
                    }
                }
                Some(Err(e)) => return Err(e),
                None => return Ok(None),
            }
        }
        visit_on(dir, items, before, None, visit)
    }

    /// [`Storage::list_bounded`]: the entries from `from` on, none read past
    /// the one refused, from the start also (no whole folder read), and from
    /// the start where `from` no longer begins an entry (no counting to it).
    pub(super) fn list_head(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        if !dir.is_empty() {
            check(dir)?;
        }
        // The sizes in the folders, as appended.
        self.sync()?;
        let folder = self.dir(dir)?;
        let (before, slot) = (from >> 32, from & u64::from(u32::MAX));
        if from != 0 && folder.begins_entry(slot * SLOT).map_err(error)? {
            let after = folder.iter_from(slot * SLOT).map_err(error)?;
            return visit_on(
                dir,
                bounded(after.map(|e| e.map_err(error))),
                before,
                None,
                visit,
            );
        }
        visit_on(dir, entries(&folder), 0, None, visit)
    }

    /// [`Storage::remove_listed`]: the entry `list_head` shows first from
    /// `place`, taken away by itself: a file, or a folder with nothing in it
    /// (not even a name PaperPaw does not take); `InvalidPath` for a folder
    /// with something in it. Its name is not looked for: a name no path
    /// holds is removed all the same.
    pub(super) fn remove_listed_entry(&self, dir: &str, place: u64) -> Result<(), StorageError> {
        if !dir.is_empty() {
            check(dir)?;
        }
        self.settle_moves()?;
        // The sizes in the folders, as appended.
        self.sync()?;
        let folder = self.dir(dir)?;
        // The entry `list_head` shows first: at the place, or at the start
        // where the place no longer begins an entry.
        let slot = (place & u64::from(u32::MAX)) * SLOT;
        let entry = if place != 0 && folder.begins_entry(slot).map_err(error)? {
            bounded(
                folder
                    .iter_from(slot)
                    .map_err(error)?
                    .map(|e| e.map_err(error)),
            )
            .find(|e| {
                e.as_ref()
                    .map_or(true, |e| shown(dir, e, &mut |_| ()).is_some())
            })
        } else {
            entries(&folder).find(|e| {
                e.as_ref()
                    .map_or(true, |e| shown(dir, e, &mut |_| ()).is_some())
            })
        };
        let entry = entry.ok_or(StorageError::NotFound)??;
        let path = child(dir, &entry.file_name());
        // Empty: its `.` and `..` only. Reads up to the first other entry.
        let mut empty = true;
        if entry.is_dir() {
            for inner in entries(&entry.to_dir()) {
                let name = inner?.file_name();
                if name != "." && name != ".." {
                    empty = false;
                    break;
                }
            }
        }
        self.hold(&path)?;
        self.close(&path)?;
        self.forget(&path);
        if !empty {
            return Err(StorageError::InvalidPath);
        }
        self.written(dir_remove_found(&folder, &entry))
    }
}

/// `entry` of `dir` taken away where it is, not looked up by its name.
fn dir_remove_found<IO: ReadWriteSeek + 'static>(
    dir: &FatDir<IO>,
    entry: &FatEntry<IO>,
) -> Result<(), StorageError> {
    dir.remove_found(entry).map_err(error)
}
