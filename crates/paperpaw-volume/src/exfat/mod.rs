//! exFAT: the crate's second volume type, behind [`crate::vol`].
//!
//! Written for PaperPaw from the exFAT code of hadris 3.0.0-rc.3 (MIT, The
//! Hadris Project Contributors, see `LICENSE-hadris.txt`): its layouts,
//! boot sector checks, checksums and the order of its writes, without its
//! node table, its mount-time walk of every allocation and its copies of
//! the bitmap and the up-case table. What is kept in RAM is this struct and
//! the handles a caller holds; the bitmap and the FAT are read a piece at a
//! time through the disk (which keeps blocks, see `paperpaw-fat`).
//!
//! # What a power cut leaves
//!
//! The disk writes blocks whole, in the order they were changed. Every
//! change to a directory is made so that one block's write is the change:
//!
//! - A new entry set is written from its end; the block holding its file
//!   entry goes last, in one write. Before that write the set is entries
//!   in use that no file entry counts: read by nobody, taken again by the
//!   next set that needs room.
//! - An entry set is removed by one write to the block of its file entry;
//!   what it leaves in the next block is the same kind of leftover.
//! - A file entry and its stream entry (size, first cluster) lie in one
//!   block for every set written here ([`dir`] passes a slot over where
//!   they would not), so a file's new size is one write. For a set a
//!   computer placed across two blocks, the update is three writes that
//!   say what they are doing (see `dir::update_set`): cut off, the set is
//!   the old one or is finished when next read. No set is ever made valid
//!   by guessing.
//! - Clusters are marked taken before anything points at them and marked
//!   free after nothing does. A cut may leave clusters taken by no file
//!   (space, given back by a computer's check); never a file whose
//!   clusters are free.
//!
//! An entry set whose checksum is wrong for any other reason is not shown
//! and not touched: its slots and clusters stay as they are.

pub(crate) mod dir;
pub(crate) mod file;
#[cfg(any(feature = "format", test))]
pub(crate) mod format;
pub(crate) mod layout;

use core::cell::{Cell, RefCell};

use crate::error::Error;
use crate::fs::ReadWriteSeek;
use crate::io::SeekFrom;
use layout::{FAT_END, Geometry, VOLUME_DIRTY, u32_at, u64_at};

pub(crate) use dir::{XDir, XEntry, XIter};
pub(crate) use file::XFile;

/// Directories noted at a time (see `Dir::note`).
const NOTES: usize = 8;

/// A run of clusters: one after the other on the disk, or linked in the FAT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Chain {
    pub(crate) first: u32,
    pub(crate) contiguous: bool,
}

/// A mounted exFAT volume.
pub struct ExFat<IO: ReadWriteSeek> {
    disk: RefCell<IO>,
    geo: Geometry,
    /// Clusters the disk has room for: none past them is taken (a volume
    /// said to be longer than its card).
    usable: u32,
    bitmap: Chain,
    /// Where the next free cluster is looked for.
    hint: Cell<u32>,
    /// Clusters marked taken and free since the mount.
    changes: Cell<(u64, u64)>,
    /// The card said it was in use when it was mounted: left so.
    was_dirty: bool,
    /// Marked in use by this mount, not yet taken back.
    dirtied: Cell<bool>,
    notes: RefCell<[Option<(u32, u64)>; NOTES]>,
    written: Cell<u64>,
    /// How often a directory grew: a handle of one reads its size anew.
    grown: Cell<u64>,
}

type Res<T, IO> = Result<T, Error<<IO as crate::io::IoBase>::Error>>;

impl<IO: ReadWriteSeek> ExFat<IO> {
    /// Mounts the exFAT volume on `disk`: its boot sector, the entry of its
    /// allocation bitmap in the root directory and the bitmap's clusters in
    /// the FAT are read, three blocks on a card as formatted.
    pub(crate) fn mount(mut disk: IO) -> Res<Self, IO> {
        let mut boot = [0u8; 512];
        disk.seek(SeekFrom::Start(0))?;
        disk.read_exact(&mut boot)?;
        let geo = layout::parse_boot(&boot).ok_or(Error::CorruptedFileSystem)?;
        let size = disk.seek(SeekFrom::End(0))?;
        let room = size.saturating_sub(geo.heap) >> geo.cluster_shift;
        let mut fs = Self {
            disk: RefCell::new(disk),
            geo,
            usable: geo.clusters.min(u32::try_from(room).unwrap_or(u32::MAX)),
            bitmap: Chain {
                first: 0,
                contiguous: false,
            },
            hint: Cell::new(2),
            changes: Cell::new((0, 0)),
            was_dirty: geo.flags & VOLUME_DIRTY != 0,
            dirtied: Cell::new(false),
            notes: RefCell::new([None; NOTES]),
            written: Cell::new(0),
            grown: Cell::new(0),
        };
        fs.bitmap = fs.find_bitmap()?;
        Ok(fs)
    }

    /// The first allocation bitmap, from its entry in the root directory.
    fn find_bitmap(&self) -> Res<Chain, IO> {
        let mut cluster = self.geo.root;
        // The root's clusters, bounded: a FAT that runs in a loop ends.
        for _ in 0..self.geo.clusters.min(4096) {
            let base = self.cluster_offset(cluster)?;
            let mut at = 0;
            while at < self.cluster_size() {
                let mut entry = [0u8; layout::ENTRY];
                self.read_at(base + at, &mut entry)?;
                match entry[0] {
                    0 => return Err(Error::CorruptedFileSystem),
                    layout::ENTRY_BITMAP if entry[1] & 1 == 0 => {
                        return self.bitmap_chain(u32_at(&entry, 20), u64_at(&entry, 24));
                    }
                    _ => {}
                }
                at += layout::ENTRY as u64;
            }
            cluster = self.fat_next(cluster)?.ok_or(Error::CorruptedFileSystem)?;
        }
        Err(Error::CorruptedFileSystem)
    }

    /// The bitmap at `first`, `len` bytes: checked to cover every cluster,
    /// and whether its clusters lie one after the other.
    fn bitmap_chain(&self, first: u32, len: u64) -> Res<Chain, IO> {
        if !self.is_cluster(first) || len < u64::from(self.geo.clusters).div_ceil(8) {
            return Err(Error::CorruptedFileSystem);
        }
        let count = len.div_ceil(self.cluster_size());
        let mut contiguous = true;
        let mut cluster = first;
        for _ in 1..count {
            let next = self.fat_next(cluster)?.ok_or(Error::CorruptedFileSystem)?;
            contiguous &= next == cluster.wrapping_add(1);
            cluster = next;
        }
        Ok(Chain { first, contiguous })
    }

    pub(crate) fn cluster_size(&self) -> u64 {
        1 << self.geo.cluster_shift
    }

    pub(crate) fn cluster_shift(&self) -> u8 {
        self.geo.cluster_shift
    }

    pub(crate) fn clusters(&self) -> u32 {
        self.geo.clusters
    }

    pub(crate) fn root(&self) -> u32 {
        self.geo.root
    }

    pub(crate) fn is_cluster(&self, cluster: u32) -> bool {
        cluster >= 2 && cluster - 2 < self.geo.clusters
    }

    /// Where `cluster` begins on the volume.
    pub(crate) fn cluster_offset(&self, cluster: u32) -> Res<u64, IO> {
        if !self.is_cluster(cluster) {
            return Err(Error::CorruptedFileSystem);
        }
        Ok(self.geo.heap + (u64::from(cluster - 2) << self.geo.cluster_shift))
    }

    pub(crate) fn read_at(&self, at: u64, buf: &mut [u8]) -> Res<(), IO> {
        let mut disk = self.disk.borrow_mut();
        disk.seek(SeekFrom::Start(at))?;
        disk.read_exact(buf)?;
        Ok(())
    }

    /// Writes `buf` at `at`, the volume marked in use first.
    pub(crate) fn write_at(&self, at: u64, buf: &[u8]) -> Res<(), IO> {
        self.mark_dirty()?;
        let mut disk = self.disk.borrow_mut();
        disk.seek(SeekFrom::Start(at))?;
        disk.write_all(buf)?;
        Ok(())
    }

    /// Zeros from `at` on, `len` bytes.
    pub(crate) fn zero(&self, at: u64, len: u64) -> Res<(), IO> {
        self.mark_dirty()?;
        let mut disk = self.disk.borrow_mut();
        disk.seek(SeekFrom::Start(at))?;
        crate::fs::write_zeros(&mut *disk, len)?;
        Ok(())
    }

    /// Writes back what the disk keeps.
    pub(crate) fn flush_disk(&self) -> Res<(), IO> {
        self.disk.borrow_mut().flush()?;
        Ok(())
    }

    /// Says on the card that it is in use, before the first write since the
    /// mount or the last [`Self::flush`]: a cut then leaves the mark, and a
    /// computer checks the card (giving back clusters taken by no file).
    fn mark_dirty(&self) -> Res<(), IO> {
        if self.was_dirty || self.dirtied.get() {
            return Ok(());
        }
        self.dirtied.set(true);
        self.write_flags(self.geo.flags | VOLUME_DIRTY)
    }

    fn write_flags(&self, flags: u16) -> Res<(), IO> {
        let mut disk = self.disk.borrow_mut();
        disk.seek(SeekFrom::Start(106))?;
        disk.write_all(&flags.to_le_bytes())?;
        Ok(())
    }

    /// Whether the card said it was in use when it was mounted.
    pub(crate) fn marked_dirty(&self) -> bool {
        self.was_dirty
    }

    /// Everything written back, then the card marked no longer in use
    /// where this mount marked it.
    pub(crate) fn flush(&self) -> Res<(), IO> {
        self.flush_disk()?;
        if self.dirtied.get() {
            self.write_flags(self.geo.flags)?;
            self.flush_disk()?;
            self.dirtied.set(false);
        }
        Ok(())
    }

    /// The cluster the FAT links after `cluster`; `None` at a chain's end.
    pub(crate) fn fat_next(&self, cluster: u32) -> Res<Option<u32>, IO> {
        if !self.is_cluster(cluster) {
            return Err(Error::CorruptedFileSystem);
        }
        let mut entry = [0u8; 4];
        self.read_at(self.geo.fat + u64::from(cluster) * 4, &mut entry)?;
        match u32::from_le_bytes(entry) {
            FAT_END => Ok(None),
            next if self.is_cluster(next) => Ok(Some(next)),
            _ => Err(Error::CorruptedFileSystem),
        }
    }

    pub(crate) fn fat_set(&self, cluster: u32, next: u32) -> Res<(), IO> {
        if !self.is_cluster(cluster) {
            return Err(Error::CorruptedFileSystem);
        }
        self.write_at(self.geo.fat + u64::from(cluster) * 4, &next.to_le_bytes())
    }

    /// The cluster after `cluster` in `chain`; `None` where the FAT ends it.
    /// A contiguous run has no end of its own: its length tells it.
    pub(crate) fn next_in(&self, chain: Chain, cluster: u32) -> Res<Option<u32>, IO> {
        if chain.contiguous {
            let next = cluster.wrapping_add(1);
            Ok(self.is_cluster(next).then_some(next))
        } else {
            self.fat_next(cluster)
        }
    }

    /// The cluster `index` clusters into `chain`, walked from `at` (an
    /// index and its cluster, kept by the caller) where that is not past
    /// it; `None` where the chain ends before.
    pub(crate) fn cluster_at(
        &self,
        chain: Chain,
        at: &mut (u32, u32),
        index: u32,
    ) -> Res<Option<u32>, IO> {
        if chain.contiguous {
            let cluster = chain.first.checked_add(index);
            return Ok(cluster.filter(|&c| self.is_cluster(c)));
        }
        if at.0 > index || !self.is_cluster(at.1) {
            *at = (0, chain.first);
        }
        while at.0 < index {
            match self.fat_next(at.1)? {
                Some(next) => *at = (at.0 + 1, next),
                None => return Ok(None),
            }
        }
        Ok(Some(at.1))
    }

    /// Where byte `byte` of the bitmap is on the volume.
    fn bitmap_offset(&self, byte: u64) -> Res<u64, IO> {
        let index = u32::try_from(byte >> self.geo.cluster_shift)
            .map_err(|_| Error::CorruptedFileSystem)?;
        let mut at = (0, self.bitmap.first);
        let cluster = self
            .cluster_at(self.bitmap, &mut at, index)?
            .ok_or(Error::CorruptedFileSystem)?;
        Ok(self.cluster_offset(cluster)? + (byte & (self.cluster_size() - 1)))
    }

    /// Whether `cluster` is marked taken.
    pub(crate) fn is_taken(&self, cluster: u32) -> Res<bool, IO> {
        if !self.is_cluster(cluster) {
            return Err(Error::CorruptedFileSystem);
        }
        let bit = cluster - 2;
        let mut byte = [0u8; 1];
        self.read_at(self.bitmap_offset(u64::from(bit / 8))?, &mut byte)?;
        Ok(byte[0] & (1 << (bit % 8)) != 0)
    }

    /// Marks `count` clusters from `first` on taken or free, a piece of
    /// the bitmap at a time; the ones that changed are counted.
    pub(crate) fn mark(&self, first: u32, count: u32, taken: bool) -> Res<(), IO> {
        if count == 0 {
            return Ok(());
        }
        if !self.is_cluster(first) || count > self.geo.clusters - (first - 2) {
            return Err(Error::CorruptedFileSystem);
        }
        let mut bit = first - 2;
        let end = bit + count;
        let mut changed = 0u64;
        while bit < end {
            let byte = u64::from(bit / 8);
            // Up to the end of a piece that lies in one block.
            let piece_end = (((byte / 64) + 1) * 64).min(u64::from(end.div_ceil(8)));
            let len = (piece_end - byte) as usize;
            let mut piece = [0u8; 64];
            let at = self.bitmap_offset(byte)?;
            self.read_at(at, &mut piece[..len])?;
            let before = piece;
            while bit < end && u64::from(bit / 8) < piece_end {
                let mask = 1u8 << (bit % 8);
                let slot = &mut piece[(u64::from(bit / 8) - byte) as usize];
                if (*slot & mask != 0) != taken {
                    *slot ^= mask;
                    changed += 1;
                }
                bit += 1;
            }
            if piece != before {
                self.write_at(at, &piece[..len])?;
            }
        }
        let (took, freed) = self.changes.get();
        self.changes.set(match taken {
            true => (took + changed, freed),
            false => (took, freed + changed),
        });
        if !taken {
            // Looked at again: the lowest free cluster keeps files together.
            self.hint.set(self.hint.get().min(first));
        }
        Ok(())
    }

    /// Takes a free cluster: `near` where it is free (the one after a
    /// file's last, so the file stays in one run), else the first free one
    /// from where the last was found.
    pub(crate) fn take(&self, near: Option<u32>) -> Res<u32, IO> {
        if let Some(near) = near.filter(|&c| self.is_cluster(c) && c - 2 < self.usable)
            && !self.is_taken(near)?
        {
            self.mark(near, 1, true)?;
            return Ok(near);
        }
        let start = self.hint.get().clamp(2, self.usable.max(1) + 1) - 2;
        let found = match self.first_free(start, self.usable)? {
            Some(bit) => Some(bit),
            None => self.first_free(0, start)?,
        };
        let cluster = found.ok_or(Error::NotEnoughSpace)? + 2;
        self.mark(cluster, 1, true)?;
        self.hint.set(cluster + 1);
        Ok(cluster)
    }

    /// The first free bit in `from..to`.
    fn first_free(&self, from: u32, to: u32) -> Res<Option<u32>, IO> {
        let mut byte = u64::from(from / 8);
        let end = u64::from(to.div_ceil(8));
        while byte < end {
            let piece_end = ((byte / 64) + 1) * 64;
            let len = (piece_end.min(end) - byte) as usize;
            let mut piece = [0u8; 64];
            self.read_at(self.bitmap_offset(byte)?, &mut piece[..len])?;
            for (i, &bits) in piece[..len].iter().enumerate() {
                if bits == 0xFF {
                    continue;
                }
                for b in 0..8 {
                    let bit = (byte as u32 + i as u32) * 8 + b;
                    if bits & (1 << b) == 0 && bit >= from && bit < to {
                        return Ok(Some(bit));
                    }
                }
            }
            byte += len as u64;
        }
        Ok(None)
    }

    /// Frees the first `count` clusters of `chain`, after whatever named
    /// them is gone from the card (the disk keeps the order).
    pub(crate) fn free(&self, chain: Chain, count: u32) -> Res<(), IO> {
        if count == 0 || !self.is_cluster(chain.first) {
            return Ok(());
        }
        if chain.contiguous {
            let count = count.min(self.geo.clusters - (chain.first - 2));
            return self.mark(chain.first, count, false);
        }
        let mut cluster = chain.first;
        for left in (0..count).rev() {
            // Read before it is freed: the link stays as it is, but so
            // nothing depends on that.
            let next = if left > 0 { self.fat_next(cluster) } else { Ok(None) };
            self.mark(cluster, 1, false)?;
            match next {
                Ok(Some(next)) => cluster = next,
                // A chain shorter than its file's size: as far as it goes.
                _ => break,
            }
        }
        Ok(())
    }

    pub(crate) fn cluster_changes(&self) -> (u64, u64) {
        self.changes.get()
    }

    /// Where the bitmap begins on the volume, where its clusters lie one
    /// after the other (as every formatter lays it).
    pub(crate) fn bitmap_start(&self) -> Option<u64> {
        self.bitmap
            .contiguous
            .then(|| self.cluster_offset(self.bitmap.first).ok())
            .flatten()
    }

    /// The free clusters, by reading the whole bitmap.
    pub(crate) fn count_free(&self) -> Res<u32, IO> {
        let mut free = 0;
        let mut byte = 0u64;
        let end = u64::from(self.geo.clusters.div_ceil(8));
        while byte < end {
            let len = (end - byte).min(64) as usize;
            let mut piece = [0u8; 64];
            self.read_at(self.bitmap_offset(byte)?, &mut piece[..len])?;
            for (i, &bits) in piece[..len].iter().enumerate() {
                let first = (byte as u32 + i as u32) * 8;
                let valid = self.geo.clusters.saturating_sub(first).min(8);
                let mask = if valid >= 8 { 0xFF } else { (1u8 << valid) - 1 };
                free += (!bits & mask).count_ones();
            }
            byte += len as u64;
        }
        Ok(free)
    }

    pub(crate) fn note(&self, dir: u32) -> Option<u64> {
        let notes = self.notes.borrow();
        notes.iter().flatten().find(|n| n.0 == dir).map(|n| n.1)
    }

    pub(crate) fn set_note(&self, dir: u32, note: u64) {
        let mut notes = self.notes.borrow_mut();
        let at = notes
            .iter()
            .position(|n| n.is_some_and(|n| n.0 == dir))
            .or_else(|| notes.iter().position(Option::is_none));
        match at {
            Some(at) => notes[at] = Some((dir, note)),
            None => {
                // The one noted longest ago goes.
                notes.rotate_left(1);
                notes[NOTES - 1] = Some((dir, note));
            }
        }
    }

    pub(crate) fn drop_note(&self, dir: u32) {
        for note in self.notes.borrow_mut().iter_mut() {
            if note.is_some_and(|n| n.0 == dir) {
                *note = None;
            }
        }
    }

    pub(crate) fn entries_written(&self) -> u64 {
        self.written.get()
    }

    pub(crate) fn entry_written(&self, dir: u32) {
        self.written.set(self.written.get() + 1);
        self.drop_note(dir);
    }

    pub(crate) fn root_dir(&self) -> XDir<'_, IO> {
        XDir::root(self)
    }

    pub(crate) fn grown(&self) -> u64 {
        self.grown.get()
    }

    pub(crate) fn grew(&self) {
        self.grown.set(self.grown.get() + 1);
    }
}
