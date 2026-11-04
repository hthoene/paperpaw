//! Damaged folders: ones whose cluster holds something else (another
//! file's data, or what was there before), read as entries with names no
//! file system gives and sizes past the card. One such entry tells the
//! whole folder is: the entries that look sane are as much garbage, their
//! clusters may be another file's. So a damaged folder is cut off where
//! its parent names it, nothing in it written, followed or freed: freed,
//! another file's clusters would be given to a third, and a write there
//! would overwrite it. Its clusters and what its entries point at stay
//! taken (in the FAT and in the free space followed) until the card is
//! checked on a computer.

use super::*;

/// Folders of names PaperPaw does not take, below one removed, followed
/// this deep (see [`FatStorage::sweep`]); deeper ones are cut off.
const SWEPT_DEPTH: u8 = 16;

/// Whether `name`, as rust-fatfs reads an entry's name, is none a file
/// system gives: bytes it cannot read (U+FFFD), control characters, or
/// none a long name may hold.
fn is_damaged_name(name: &str) -> bool {
    name.contains('\u{FFFD}')
        || name.chars().any(char::is_control)
        || !fatfs::is_valid_long_name(name)
}

/// Whether `size` bytes are more than the card holds.
fn is_past_card<IO: ReadWriteSeek + 'static>(fs: &FileSystem<IO>, size: u64) -> bool {
    size > u64::from(fs.total_clusters()) * u64::from(fs.cluster_size())
}

/// Where a folder followed from its parent is: the first clusters its
/// `.` and `..` must name (its entry's, and its parent's).
pub(super) type Place = (Option<u32>, Option<u32>);

/// Whether `entry`, named `name`, the `at`th of a folder (from 0), tells
/// the folder is damaged: its name (see [`is_damaged_name`]), its first
/// cluster or its size past the card, or the first two not `.` and `..`
/// (which each folder but the root begins with), at the clusters of
/// `place` where it is known. Not one that leads back up, or into
/// another folder (an entry cross-linked).
pub(super) fn is_damaged<IO: ReadWriteSeek + 'static>(
    fs: &FileSystem<IO>,
    entry: &FatEntry<IO>,
    name: &str,
    at: usize,
    place: Option<Place>,
) -> bool {
    let clusters = fs.total_clusters();
    // Clusters are numbered from 2.
    let past = entry
        .first_cluster()
        .is_some_and(|first| first < 2 || first - 2 >= clusters);
    let dots = match at {
        0 => {
            name == "."
                && entry.is_dir()
                && place.is_none_or(|(own, _)| own == entry.first_cluster())
        }
        1 => {
            name == ".."
                && entry.is_dir()
                && place.is_none_or(|(_, up)| up == entry.first_cluster())
        }
        _ => name != "." && name != "..",
    };
    !dots || is_damaged_name(name) || past || is_past_card(fs, entry.len())
}

impl<IO: ReadWriteSeek + 'static> FatStorage<IO> {
    /// Cuts damaged folder `path` off where its parent names it (see the
    /// [module documentation](self)).
    pub(super) fn unlink_damaged(&self, path: &str) -> Result<(), StorageError> {
        self.forget(path);
        let parent = self.dir(parent(path))?;
        match lookup(&parent, name(path), true)? {
            Lookup {
                entry: Some(entry),
                others: false,
                ..
            } => parent.unlink_found(&entry).map_err(error),
            Lookup { entry: None, .. } => Ok(()),
            Lookup { .. } => Err(StorageError::InvalidPath),
        }
    }

    /// Takes out of folder `dir`, not damaged, `depth` below the one
    /// removed, what is left in it after its entries were removed by
    /// name: ones of names PaperPaw does not take (made elsewhere,
    /// `nul.txt`), each where it is found, with its data. A folder among
    /// them is swept the same way first, if it is sound (see
    /// [`FatStorage::is_sound`]); else it is cut off, its clusters left.
    pub(super) fn sweep(&self, dir: &FatDir<IO>, depth: u8) -> Result<(), StorageError> {
        let mut own = None;
        let mut left = Vec::new();
        for entry in entries(dir) {
            let entry = entry?;
            match entry.file_name().as_str() {
                "." => own = entry.first_cluster(),
                ".." => {}
                _ => left.push(entry),
            }
        }
        // Each removed in its place: the others stay where found.
        for entry in &left {
            if !entry.is_dir() {
                dir.remove_found(entry).map_err(error)?;
            } else if depth < SWEPT_DEPTH && self.is_sound(entry, own)? {
                self.sweep(&entry.to_dir(), depth + 1)?;
                dir.remove_found(entry).map_err(error)?;
            } else {
                dir.unlink_found(entry).map_err(error)?;
            }
        }
        Ok(())
    }

    /// Whether folder `entry`, of the folder at cluster `parent`, is one
    /// to follow: none of its entries damaged (see [`is_damaged`]), its
    /// `.` at its own cluster and its `..` at its parent's.
    fn is_sound(&self, entry: &FatEntry<IO>, parent: Option<u32>) -> Result<bool, StorageError> {
        match parent {
            Some(_) => self.is_whole(entry, Some(parent)),
            None => Ok(false),
        }
    }

    /// Whether folder `entry` is intact but for where its `..` points:
    /// none of its entries damaged, its `.` at its own cluster. One moved
    /// whose move was cut off before its `..` was pointed up.
    pub(super) fn is_intact(&self, entry: &FatEntry<IO>) -> Result<bool, StorageError> {
        self.is_whole(entry, None)
    }

    /// [`is_sound`](Self::is_sound) with its `..` at `up` where that is
    /// given, else anywhere ([`is_intact`](Self::is_intact)).
    fn is_whole(
        &self,
        entry: &FatEntry<IO>,
        up: Option<Option<u32>>,
    ) -> Result<bool, StorageError> {
        let own = entry.first_cluster();
        if own.is_none() {
            return Ok(false);
        }
        let mut seen = 0;
        for (at, found) in entries(&entry.to_dir()).enumerate() {
            let found = found?;
            seen = at + 1;
            let place = (own, up.unwrap_or(found.first_cluster()));
            if is_damaged(self.fs, &found, &found.file_name(), at, Some(place)) {
                return Ok(false);
            }
        }
        // Its `.` and `..` at least.
        Ok(seen >= 2)
    }
}
