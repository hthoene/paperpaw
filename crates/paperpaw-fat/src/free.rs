//! A card's free space, counted without reading all of its FAT at once.
//!
//! rust-fatfs counts the free clusters by reading the whole FAT when the
//! volume was not unmounted cleanly (which a reader that is switched off
//! never does): megabytes over a card's bus in one call. Here the FAT is
//! counted [`STEP`] bytes at each ask, and the clusters the file system
//! marks taken and free from then on (`FileSystem::cluster_changes` of
//! the patched rust-fatfs, counted as each mark is written: also by a
//! write that fails part of the way, and by a folder that grows) are taken
//! off or added. Until
//! the count is done the free space is not known: the count the card keeps
//! in its FS information sector (FAT32) is not used, as after a power cut
//! it may say more than is free (#238), and a reserve kept by it would not
//! be one.
//!
//! Counted once, the count is kept on the card (FAT32), so that the next
//! start does not count again (megabytes of FAT, seconds of card time on a
//! large card): `FatStorage::keep_free` writes it to the FS information
//! sector when everything is written back, with a record of its own in the
//! sector's reserved bytes ([`note`]), and the next mount takes it
//! (`FatStorage::trust_kept`) where all of this holds:
//!
//! - The record is there and whole, and says the count and the hint the
//!   sector holds, for a card of this many clusters: this storage counted
//!   them, and no other writer of the sector has written another count or
//!   hint since. Computers that change the card write theirs when they
//!   let go of it (Windows and macOS count the FAT themselves; Linux moves
//!   the hint with each cluster it takes, and the count where it has
//!   counted): a count that still agrees with the record is the count.
//! - The card is not marked dirty (taken out of a computer while it was
//!   written: its sector was not brought up to date).
//! - The FAT was not written since. Before its first write of the FAT
//!   after a count was kept, the patched rust-fatfs takes the count back on
//!   the card (unknown, the record gone: `FileSystem::before_fat_write`),
//!   and the disk takes changes in the order they were made. Cut off at
//!   any point, the card so holds a kept count with the FAT it was counted
//!   from, or no count: never the count of #238.
//!
//! What this cannot see: a device that takes clusters and writes neither
//! count nor hint. A Linux computer that only removes files, and never
//! asks for the free space, leaves the count as it was: too low until the
//! FAT is next counted, never too high.

/// Bytes of the FAT read at most each time the free space is asked for.
pub(crate) const STEP: usize = 128 * 1024;
/// Bytes of the FAT read at once, into a buffer on the stack: nothing is
/// taken from the heap, which on the X4 is small and in pieces while the
/// radio is up.
const SECTOR: usize = 512;

/// Reads bytes of the volume at an offset; `false` if it cannot.
pub(crate) type Read = dyn Fn(u64, &mut [u8]) -> bool;

/// Where the FAT is and what it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Geometry {
    /// The FAT's first byte in the volume.
    fat: u64,
    /// The bits of an entry: 12, 16 or 32.
    bits: u8,
    /// Clusters for data; their entries are 2 to `clusters + 1`.
    clusters: u32,
    /// Bytes of a cluster.
    cluster: u64,
}

impl Geometry {
    /// The geometry the boot sector `boot` says.
    fn of(boot: &[u8; 512]) -> Option<Geometry> {
        let u16_at = |at: usize| u32::from(u16::from_le_bytes([boot[at], boot[at + 1]]));
        let u32_at =
            |at: usize| u32::from_le_bytes([boot[at], boot[at + 1], boot[at + 2], boot[at + 3]]);
        let sector = u16_at(11);
        let per_cluster = u32::from(boot[13]);
        let reserved = u16_at(14);
        let fats = u32::from(boot[16]);
        let roots = u16_at(17);
        let total = match u16_at(19) {
            0 => u32_at(32),
            total => total,
        };
        let fat_size = match u16_at(22) {
            0 => u32_at(36),
            size => size,
        };
        if sector == 0 || per_cluster == 0 || fats == 0 || fat_size == 0 {
            return None;
        }
        let root_sectors = (roots * 32).div_ceil(sector);
        let data = total.checked_sub(reserved + fats * fat_size + root_sectors)?;
        let clusters = data / per_cluster;
        let bits = match clusters {
            0..4085 => 12,
            4085..65525 => 16,
            _ => 32,
        };
        Some(Geometry {
            fat: u64::from(reserved) * u64::from(sector),
            bits,
            clusters,
            cluster: u64::from(sector) * u64::from(per_cluster),
        })
    }

    /// The bytes of the FAT entries `first..first + count`.
    fn span(&self, first: u32, count: u32) -> (u64, usize) {
        match self.bits {
            12 => {
                let start = u64::from(first) * 3 / 2;
                // The last entry's two bytes.
                let end = u64::from(first + count - 1) * 3 / 2 + 2;
                (self.fat + start, (end - start) as usize)
            }
            bits => {
                let size = u64::from(bits / 8);
                (
                    self.fat + u64::from(first) * size,
                    (u64::from(count) * size) as usize,
                )
            }
        }
    }

    /// How many of the entries `first..first + count`, read into `bytes`
    /// (see [`Geometry::span`]), are free.
    fn free_in(&self, first: u32, count: u32, bytes: &[u8]) -> u32 {
        match self.bits {
            12 => {
                let base = first as usize * 3 / 2;
                (first..first + count)
                    .filter(|&n| {
                        let at = n as usize * 3 / 2 - base;
                        let (low, high) = (bytes[at], bytes[at + 1]);
                        let value = if n % 2 == 0 {
                            u16::from(low) | (u16::from(high & 0x0F) << 8)
                        } else {
                            u16::from(low >> 4) | (u16::from(high) << 4)
                        };
                        value == 0
                    })
                    .count() as u32
            }
            16 => bytes
                .as_chunks::<2>()
                .0
                .iter()
                .filter(|e| **e == [0, 0])
                .count() as u32,
            _ => bytes
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|e| u32::from_le_bytes(**e) & 0x0FFF_FFFF == 0)
                .count() as u32,
        }
    }
}

/// Bytes of the record kept beside the count (see [`note`]).
pub(crate) const NOTE: usize = 20;

/// The record this storage writes into the FS information sector's
/// reserved bytes when it keeps a count there: `PPFC`, the count of free
/// clusters, the hint and the card's clusters as 4 bytes each, and a hash
/// of those 16 bytes (`fnv`'s lower half).
pub(crate) fn note(free: u32, next: u32, clusters: u32) -> [u8; NOTE] {
    let mut note = [0u8; NOTE];
    note[..4].copy_from_slice(b"PPFC");
    note[4..8].copy_from_slice(&free.to_le_bytes());
    note[8..12].copy_from_slice(&next.to_le_bytes());
    note[12..16].copy_from_slice(&clusters.to_le_bytes());
    let hash = crate::fnv(note[..16].iter().copied()) as u32;
    note[16..].copy_from_slice(&hash.to_le_bytes());
    note
}

/// The clusters of the card `record` was written for, where it is this
/// storage's record ([`note`]) of the count `free` and the hint `next`
/// that the sector holds. Not of one free cluster: a Linux computer that
/// takes the only free one, and it is the one after the hint, leaves the
/// hint as it was.
pub(crate) fn noted(record: &[u8; NOTE], free: u32, next: u32) -> Option<u32> {
    let word = |at: usize| {
        u32::from_le_bytes([record[at], record[at + 1], record[at + 2], record[at + 3]])
    };
    let clusters = word(12);
    (*record == note(free, next, clusters) && free != 1).then_some(clusters)
}

/// The free space, as far as it is known; see the [module
/// documentation](self).
#[derive(Debug, Default)]
pub(crate) struct Free {
    geometry: Option<Geometry>,
    /// Free clusters, once counted (and followed since); `None` before.
    known: Option<u64>,
    /// The count going on: the next entry, and the free ones before it.
    counting: Option<(u32, u64)>,
    /// Clusters taken while the count goes on, taken off when it is done.
    /// Whether one was in the part counted already is not known: so taken
    /// ones are taken off even where the count sees them taken too, and
    /// freed ones are not added. The count can then only be low, never
    /// more than is free.
    taken_meanwhile: u64,
    /// The file system's `cluster_changes` followed last: taken, freed.
    seen: (u64, u64),
    /// The FAT changed while the count goes on: what it ends with may be
    /// low (see `taken_meanwhile`).
    changed_meanwhile: bool,
    /// `known` is every free cluster, none less: counted with no change
    /// meanwhile, or kept so on the card, and followed since.
    sure: bool,
}

impl Free {
    /// Nothing known yet, the file system's `cluster_changes` being
    /// `seen` now.
    pub(crate) fn new(seen: (u64, u64)) -> Free {
        Free {
            seen,
            ..Free::default()
        }
    }

    /// Follows the file system's `cluster_changes`, now `changes`: what
    /// it marked taken and free since they were followed last.
    pub(crate) fn follow(&mut self, changes: (u64, u64)) {
        let taken = changes.0.wrapping_sub(self.seen.0);
        let freed = changes.1.wrapping_sub(self.seen.1);
        self.seen = changes;
        self.changed(taken, freed);
    }

    /// Counts on (one step), and the free bytes once counted.
    pub(crate) fn ask(&mut self, read: &Read) -> Option<u64> {
        let geometry = self.geometry(read)?;
        let Some((mut next, mut free)) = self.counting else {
            return self.bytes();
        };
        let last = geometry.clusters + 2;
        // A sector's worth of entries at a time, into a buffer of that
        // size (an even number of FAT12's, which share bytes in pairs).
        let per_read = match geometry.bits {
            12 => (SECTOR as u32 * 2 / 3) & !1,
            bits => (SECTOR / usize::from(bits / 8)) as u32,
        };
        let mut buffer = [0u8; SECTOR];
        let mut read_so_far = 0;
        while next < last && read_so_far < STEP {
            let count = per_read.min(last - next);
            let (at, len) = geometry.span(next, count);
            let bytes = &mut buffer[..len];
            if !read(at, bytes) {
                self.counting = Some((next, free));
                return self.bytes();
            }
            free += u64::from(geometry.free_in(next, count, bytes));
            next += count;
            read_so_far += len;
        }
        if next >= last {
            self.known = Some(free.saturating_sub(self.taken_meanwhile));
            self.counting = None;
            self.taken_meanwhile = 0;
            self.sure = !core::mem::take(&mut self.changed_meanwhile);
        } else {
            self.counting = Some((next, free));
        }
        self.bytes()
    }

    /// The bytes for files in all, used and free.
    pub(crate) fn total(&mut self, read: &Read) -> Option<u64> {
        let geometry = self.geometry(read)?;
        Some(u64::from(geometry.clusters) * geometry.cluster)
    }

    /// The volume's geometry, read from its boot sector at the first ask.
    fn geometry(&mut self, read: &Read) -> Option<Geometry> {
        if self.geometry.is_none() {
            let mut boot = [0u8; 512];
            let geometry = read(0, &mut boot).then(|| Geometry::of(&boot)).flatten()?;
            self.counting = Some((2, 0));
            self.geometry = Some(geometry);
        }
        self.geometry
    }

    fn bytes(&self) -> Option<u64> {
        Some(self.known? * self.geometry?.cluster)
    }

    /// `taken` clusters were taken and `freed` freed by a change of the
    /// storage: the count known follows, and a count going on takes the
    /// taken ones off when it is done (see `taken_meanwhile`).
    fn changed(&mut self, taken: u64, freed: u64) {
        let Some(geometry) = self.geometry else {
            return;
        };
        // Never more than the card holds: the sizes of a damaged entry
        // may tell more freed.
        if let Some(known) = self.known.as_mut() {
            let followed = known.saturating_add(freed).checked_sub(taken);
            // Not what the changes add up to: no longer every free one.
            self.sure &= followed.is_some_and(|free| free <= u64::from(geometry.clusters));
            *known = followed.unwrap_or(0).min(u64::from(geometry.clusters));
        }
        if self.counting.is_some() {
            self.taken_meanwhile = self.taken_meanwhile.saturating_add(taken);
            self.changed_meanwhile |= taken > 0 || freed > 0;
        }
    }

    /// Takes `free` free clusters as counted, on a FAT32 card of
    /// `clusters` clusters (as the card kept them, see the [module
    /// documentation](self)): nothing is counted then. `false` where the
    /// card is not that one, or cannot be read.
    pub(crate) fn trust(&mut self, read: &Read, free: u32, clusters: u32) -> bool {
        let Some(geometry) = self.geometry(read) else {
            return false;
        };
        if geometry.bits != 32 || geometry.clusters != clusters || free > clusters {
            return false;
        }
        self.known = Some(u64::from(free));
        self.counting = None;
        self.sure = true;
        true
    }

    /// The free clusters and the card's clusters, where every free one is
    /// known (see `sure`) on a FAT32 card.
    pub(crate) fn sure(&self) -> Option<(u32, u32)> {
        let geometry = self.geometry.filter(|g| g.bits == 32)?;
        let known = self
            .known
            .filter(|_| self.sure && self.counting.is_none())?;
        Some((u32::try_from(known).ok()?, geometry.clusters))
    }

    /// Whether the free clusters were counted (and followed since).
    #[cfg(test)]
    pub(crate) fn exact(&self) -> bool {
        self.known.is_some()
    }
}

impl<IO: fatfs::ReadWriteSeek + 'static> crate::FatStorage<IO> {
    /// At the mount: takes the free space the card keeps, where this
    /// storage kept it and nothing says the FAT changed since (see the
    /// [module documentation](self)). Two blocks read (the boot sector and
    /// the FS information sector, both read by the mount already).
    pub(crate) fn trust_kept(&self) {
        let Some(read) = &self.read else {
            return;
        };
        let mut record = [0u8; NOTE];
        let Ok(Some((free, next))) = self.fs.kept_free_count(&mut record) else {
            return;
        };
        let Some(clusters) = noted(&record, free, next).filter(|_| !self.fs.marked_dirty()) else {
            return;
        };
        let trusted = self.free.borrow_mut().trust(read.as_ref(), free, clusters);
        self.kept.set(trusted);
    }

    /// After everything was written back ([`Storage::sync`]): keeps the
    /// free space on the card where every free cluster is known and the
    /// card does not hold it yet. One block written (two where the card
    /// was marked dirty), none while the FAT is not written between.
    ///
    /// [`Storage::sync`]: paperpaw_platform::Storage::sync
    pub(crate) fn keep_free(&self) -> Result<(), paperpaw_platform::StorageError> {
        if self.read.is_none() || (self.kept.get() && self.fs.free_count_kept()) {
            return Ok(());
        }
        self.kept.set(false);
        let sure = {
            let mut free = self.free.borrow_mut();
            free.follow(self.fs.cluster_changes());
            free.sure()
        };
        let Some((free, clusters)) = sure else {
            return Ok(());
        };
        let record = note(free, self.fs.next_free_hint(), clusters);
        let kept = self
            .fs
            .keep_free_count(free, &record)
            .map_err(crate::error)?;
        self.write_back()?;
        self.kept.set(kept);
        Ok(())
    }
}
