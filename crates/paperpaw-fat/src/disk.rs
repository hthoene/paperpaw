//! A disk of 512-byte blocks (an SD card) as the bytes a FAT file system
//! reads and writes: [`BlockDisk`] keeps the [`SLOTS`] blocks used last in
//! memory (the FAT, folders and the files being read stay at hand: each
//! block read from a card costs a command and its wait) and writes changed
//! ones back when their place is needed or on flush. Whole blocks of a
//! file read or written pass by them, several in one command: a picture
//! streamed through would push the FAT and the folders out.
//!
//! Changed blocks reach the disk in the order they were changed, and
//! before whole blocks written past them: cut off at any point, the disk
//! holds the changes up to one of them, none after (a file's data is not
//! in clusters whose freeing is not on the disk yet). A block changed
//! again after another one was is written back first, as it was.
//!
//! [`BlockDisk::volume`] finds the FAT volume on a card, [`BlockDisk::find`]
//! a FAT or an exFAT one: most cards hold a partition table (MBR,
//! sometimes GPT) and the file system in a partition after it; some hold
//! the file system from the first block.

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use fatfs::{IoBase, Read, Seek, SeekFrom, Write};

/// Size of a block.
pub const BLOCK: usize = 512;
/// Blocks kept in memory.
pub const SLOTS: usize = 24;
/// Blocks read at once where they are read one after the other (a
/// folder's entries, a file read in small parts): a card answers each
/// command after a wait, so one command for eight blocks takes about a
/// third of the time of eight.
pub const AHEAD: usize = 8;

/// The disk failed to read or write a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskError;

impl fatfs::IoError for DiskError {
    fn is_interrupted(&self) -> bool {
        false
    }
    fn new_unexpected_eof_error() -> Self {
        Self
    }
    fn new_write_zero_error() -> Self {
        Self
    }
}

/// A disk read and written in whole blocks, e.g. an SD card.
pub trait Blocks {
    /// Reads block `index` into `block`.
    fn read_block(&mut self, index: u32, block: &mut [u8; BLOCK]) -> Result<(), DiskError>;
    /// Writes `block` as block `index`.
    fn write_block(&mut self, index: u32, block: &[u8; BLOCK]) -> Result<(), DiskError>;
    /// Reads blocks from `index` on into `data` (whole blocks), in one
    /// command where the disk can.
    fn read_blocks(&mut self, index: u32, data: &mut [u8]) -> Result<(), DiskError> {
        (index..)
            .zip(data.as_chunks_mut::<BLOCK>().0)
            .try_for_each(|(i, block)| self.read_block(i, block))
    }
    /// Writes `data` (whole blocks) as blocks from `index` on, in one
    /// command where the disk can.
    fn write_blocks(&mut self, index: u32, data: &[u8]) -> Result<(), DiskError> {
        (index..)
            .zip(data.as_chunks::<BLOCK>().0)
            .try_for_each(|(i, block)| self.write_block(i, block))
    }
    /// Number of blocks.
    fn blocks(&mut self) -> Result<u32, DiskError>;
}

/// Which file system a volume holds ([`BlockDisk::find`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// FAT12, FAT16 or FAT32.
    Fat,
    /// exFAT.
    ExFat,
}

/// Why no FAT volume was found on a disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeError {
    /// The disk could not be read.
    Disk,
    /// It is formatted as exFAT, not FAT32 ([`BlockDisk::volume`] only).
    ExFat,
    /// It is formatted as NTFS.
    Ntfs,
    /// Neither a FAT volume nor a partition with one.
    NoFat,
}

impl From<DiskError> for VolumeError {
    fn from(_: DiskError) -> Self {
        Self::Disk
    }
}

/// What the first block of a volume says it is.
#[derive(PartialEq)]
enum Boot {
    Fat,
    ExFat,
    Ntfs,
    Other,
}

fn boot(block: &[u8; BLOCK]) -> Boot {
    match &block[3..11] {
        b"EXFAT   " => return Boot::ExFat,
        b"NTFS    " => return Boot::Ntfs,
        _ => {}
    }
    let jump = matches!(block[0], 0xEB | 0xE9);
    // FAT12 and FAT16 name their type at 0x36, FAT32 at 0x52.
    let named = &block[0x36..0x39] == b"FAT" || &block[0x52..0x55] == b"FAT";
    let sector = u16::from_le_bytes([block[11], block[12]]);
    if jump && named && sector.is_power_of_two() && (512..=4096).contains(&sector) {
        Boot::Fat
    } else {
        Boot::Other
    }
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from(u32_at(bytes, at)) | u64::from(u32_at(bytes, at + 4)) << 32
}

/// The partitions in the partition table `mbr` (the disk's first block):
/// the first block of each and its number of blocks.
fn partitions<B: Blocks>(disk: &mut B, mbr: &[u8; BLOCK]) -> Result<Vec<(u64, u64)>, DiskError> {
    let mut found = Vec::new();
    if mbr[510..] != [0x55, 0xAA] {
        return Ok(found);
    }
    for entry in mbr[446..510].as_chunks::<16>().0 {
        match entry[4] {
            0 => {}
            // A protective entry: the table is a GPT.
            0xEE => return gpt(disk),
            _ => found.push((u64::from(u32_at(entry, 8)), u64::from(u32_at(entry, 12)))),
        }
    }
    Ok(found)
}

/// The partitions of a GPT, whose header is in the second block.
fn gpt<B: Blocks>(disk: &mut B) -> Result<Vec<(u64, u64)>, DiskError> {
    let mut found = Vec::new();
    let mut block = [0u8; BLOCK];
    disk.read_block(1, &mut block)?;
    if &block[..8] != b"EFI PART" {
        return Ok(found);
    }
    let first_entry = u64_at(&block, 72);
    let count = u64::from(u32_at(&block, 80).min(128));
    let size = u64::from(u32_at(&block, 84));
    if !(128..=BLOCK as u64).contains(&size) || !(BLOCK as u64).is_multiple_of(size) {
        return Ok(found);
    }
    let per_block = BLOCK as u64 / size;
    for i in 0..count {
        if i % per_block == 0 {
            let index = u32::try_from(first_entry + i / per_block).map_err(|_| DiskError)?;
            disk.read_block(index, &mut block)?;
        }
        let at = ((i % per_block) * size) as usize;
        let entry = &block[at..at + size as usize];
        if entry[..16].iter().any(|&b| b != 0) {
            let (first, last) = (u64_at(entry, 32), u64_at(entry, 40));
            if last >= first {
                found.push((first, last - first + 1));
            }
        }
    }
    Ok(found)
}

/// `len` zero bytes, if there is memory for them.
fn zeroed(len: usize) -> Option<Vec<u8>> {
    let mut data = Vec::new();
    data.try_reserve_exact(len).ok()?;
    data.resize(len, 0);
    Some(data)
}

/// A block kept in memory.
#[derive(Debug, Clone, Copy)]
struct Slot {
    /// The disk's block it holds, if any.
    index: Option<u32>,
    /// When it was changed last (a count of changes), if since it was
    /// read or written back.
    changed: Option<u64>,
    /// When it was used last (a count of uses).
    used: u32,
    data: [u8; BLOCK],
}

const EMPTY: Slot = Slot {
    index: None,
    changed: None,
    used: 0,
    data: [0; BLOCK],
};

/// The bytes of a [`Blocks`] disk, or of the part of it a volume takes.
#[derive(Debug)]
pub struct BlockDisk<B: Blocks> {
    disk: B,
    /// The disk's block the bytes start at.
    first: u32,
    position: u64,
    size: u64,
    /// On the heap: kept in the struct, they made it 12 KB, copied over
    /// the stack wherever it was moved (into a file system, into a box).
    slots: Box<[Slot]>,
    /// Uses so far, to find the block used longest ago.
    uses: u32,
    /// Changes so far, to write changed blocks back in order.
    changes: u64,
    /// The block used last, to see blocks used one after the other.
    last: Option<u32>,
}

impl<B: Blocks> BlockDisk<B> {
    /// The whole disk.
    pub fn new(mut disk: B) -> Result<Self, DiskError> {
        let blocks = disk.blocks()?;
        Ok(Self::part(disk, 0, blocks))
    }

    /// The FAT volume on the disk: the whole of it, or the first partition
    /// that holds one.
    pub fn volume(disk: B) -> Result<Self, VolumeError> {
        match Self::find(disk)? {
            (disk, Kind::Fat) => Ok(disk),
            (_, Kind::ExFat) => Err(VolumeError::ExFat),
        }
    }

    /// The FAT or exFAT volume on the disk, and which it is: the whole of
    /// the disk, or the first partition that holds one.
    pub fn find(mut disk: B) -> Result<(Self, Kind), VolumeError> {
        let blocks = disk.blocks()?;
        let mut block = [0u8; BLOCK];
        disk.read_block(0, &mut block)?;
        match boot(&block) {
            Boot::Fat => return Ok((Self::part(disk, 0, blocks), Kind::Fat)),
            Boot::ExFat => return Ok((Self::part(disk, 0, blocks), Kind::ExFat)),
            Boot::Ntfs => return Err(VolumeError::Ntfs),
            Boot::Other => {}
        }
        let mut ntfs = false;
        for (first, count) in partitions(&mut disk, &block)? {
            if first == 0 || count == 0 || first >= u64::from(blocks) {
                continue;
            }
            // A partition said to go past the card's end (as on some cards
            // as sold, by some 90 MB) ends there: other readers read them
            // too. Within the disk, so both fit in a u32.
            let count = count.min(u64::from(blocks) - first);
            let (first, count) = (first as u32, count as u32);
            disk.read_block(first, &mut block)?;
            match boot(&block) {
                Boot::Fat => return Ok((Self::part(disk, first, count), Kind::Fat)),
                Boot::ExFat => return Ok((Self::part(disk, first, count), Kind::ExFat)),
                Boot::Ntfs => ntfs = true,
                Boot::Other => {}
            }
        }
        Err(if ntfs {
            VolumeError::Ntfs
        } else {
            VolumeError::NoFat
        })
    }

    /// Number of blocks of the volume.
    pub fn blocks(&self) -> u64 {
        self.size / BLOCK as u64
    }

    /// `blocks` blocks of the disk from block `first` on.
    fn part(disk: B, first: u32, blocks: u32) -> Self {
        Self {
            disk,
            first,
            position: 0,
            size: u64::from(blocks) * BLOCK as u64,
            slots: vec![EMPTY; SLOTS].into_boxed_slice(),
            uses: 0,
            changes: 0,
            last: None,
        }
    }

    /// The slot holding block `index`, read into the one used longest ago
    /// (written back first if it changed); `read` false for a block about
    /// to be written whole.
    ///
    /// Read after the block before it, it is read with the ones after it
    /// that are not kept, up to [`AHEAD`], in one command.
    fn load(&mut self, index: u32, read: bool) -> Result<usize, DiskError> {
        self.uses = self.uses.wrapping_add(1);
        let next = self.last.and_then(|last| last.checked_add(1)) == Some(index);
        self.last = Some(index);
        if let Some(i) = self.slots.iter().position(|s| s.index == Some(index)) {
            self.slots[i].used = self.uses;
            return Ok(i);
        }
        if read && next {
            let end = u64::from(self.first) + self.size / BLOCK as u64;
            let left = end.saturating_sub(u64::from(index)) as usize;
            let ahead = self.passing(index, AHEAD.min(left));
            if ahead > 1
                && let Some(mut data) = zeroed(ahead * BLOCK)
            {
                self.disk.read_blocks(index, &mut data)?;
                let mut first = 0;
                for (k, block) in data.as_chunks::<BLOCK>().0.iter().enumerate() {
                    let i = self.oldest()?;
                    let slot = &mut self.slots[i];
                    slot.data = *block;
                    slot.index = Some(index + k as u32);
                    slot.used = self.uses;
                    if k == 0 {
                        first = i;
                    }
                }
                return Ok(first);
            }
        }
        let i = self.oldest()?;
        let slot = &mut self.slots[i];
        if read {
            self.disk.read_block(index, &mut slot.data)?;
        }
        slot.index = Some(index);
        slot.used = self.uses;
        Ok(i)
    }

    /// The slot used longest ago, emptied (written back first if it
    /// changed, after the blocks changed before it).
    fn oldest(&mut self) -> Result<usize, DiskError> {
        let i = (0..SLOTS)
            .min_by_key(|&i| self.slots[i].index.map_or(0, |_| self.slots[i].used))
            .unwrap_or(0);
        if let Some(changed) = self.slots[i].changed {
            self.write_back(changed)?;
        }
        self.slots[i].index = None;
        Ok(i)
    }

    /// Readies slot `i` for a change. Changed before another block was,
    /// it is written back first as it is, after the blocks changed before
    /// it: written back later with this change, it would reach the disk
    /// before a change made earlier.
    fn change(&mut self, i: usize) -> Result<(), DiskError> {
        match self.slots[i].changed {
            // Changed last: this change goes with the ones before.
            Some(changed) if changed == self.changes => return Ok(()),
            Some(changed) => self.write_back(changed)?,
            None => {}
        }
        self.changes += 1;
        self.slots[i].changed = Some(self.changes);
        Ok(())
    }

    /// The disk, for tests.
    #[cfg(test)]
    pub(crate) fn disk(&self) -> &B {
        &self.disk
    }

    /// Whether block `index` is kept in a slot.
    fn kept(&self, index: u32) -> bool {
        self.slots.iter().any(|s| s.index == Some(index))
    }

    /// Whole blocks from `index` on, at most `whole`, that are not kept.
    fn passing(&self, index: u32, whole: usize) -> usize {
        (0..whole as u32)
            .take_while(|&k| !self.kept(index + k))
            .count()
    }

    /// Writes back the blocks changed up to change `last`, in the order
    /// they were changed: cut off at any point, the disk holds the
    /// changes up to one of them, none after.
    fn write_back(&mut self, last: u64) -> Result<(), DiskError> {
        while let Some(i) = (0..SLOTS)
            .filter(|&i| self.slots[i].changed.is_some_and(|c| c <= last))
            .min_by_key(|&i| self.slots[i].changed)
        {
            let slot = &mut self.slots[i];
            if let Some(index) = slot.index {
                self.disk.write_block(index, &slot.data)?;
            }
            slot.changed = None;
        }
        Ok(())
    }

    /// The disk's block at the position and the offset in it.
    fn place(&self) -> (u32, usize) {
        (
            self.first + (self.position / BLOCK as u64) as u32,
            (self.position % BLOCK as u64) as usize,
        )
    }
}

impl<B: Blocks> IoBase for BlockDisk<B> {
    type Error = DiskError;
}

impl<B: Blocks> Read for BlockDisk<B> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, DiskError> {
        let left = self.size.saturating_sub(self.position);
        let len = buf.len().min(left as usize);
        let mut done = 0;
        while done < len {
            let (index, offset) = self.place();
            let whole = if offset == 0 { (len - done) / BLOCK } else { 0 };
            let passing = self.passing(index, whole);
            if passing > 0 {
                let bytes = &mut buf[done..done + passing * BLOCK];
                self.disk.read_blocks(index, bytes)?;
                done += passing * BLOCK;
                self.position += (passing * BLOCK) as u64;
                continue;
            }
            let slot = self.load(index, true)?;
            let n = (BLOCK - offset).min(len - done);
            buf[done..done + n].copy_from_slice(&self.slots[slot].data[offset..offset + n]);
            done += n;
            self.position += n as u64;
        }
        Ok(done)
    }
}

impl<B: Blocks> Write for BlockDisk<B> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, DiskError> {
        let left = self.size.saturating_sub(self.position);
        let len = buf.len().min(left as usize);
        let mut done = 0;
        while done < len {
            let (index, offset) = self.place();
            let whole = if offset == 0 { (len - done) / BLOCK } else { 0 };
            let passing = self.passing(index, whole);
            if passing > 0 {
                // After the changes before them: a file's data is not
                // written into clusters before their freeing is on the
                // disk, where a cut would leave them another file's.
                self.flush()?;
                let bytes = &buf[done..done + passing * BLOCK];
                self.disk.write_blocks(index, bytes)?;
                done += passing * BLOCK;
                self.position += (passing * BLOCK) as u64;
                continue;
            }
            let n = (BLOCK - offset).min(len - done);
            // A whole block need not be read first.
            let i = self.load(index, n < BLOCK)?;
            self.change(i)?;
            self.slots[i].data[offset..offset + n].copy_from_slice(&buf[done..done + n]);
            done += n;
            self.position += n as u64;
        }
        Ok(done)
    }

    fn flush(&mut self) -> Result<(), DiskError> {
        self.write_back(self.changes)
    }
}

impl<B: Blocks> Seek for BlockDisk<B> {
    fn seek(&mut self, to: SeekFrom) -> Result<u64, DiskError> {
        let position = match to {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::End(n) => i128::from(self.size) + i128::from(n),
            SeekFrom::Current(n) => i128::from(self.position) + i128::from(n),
        };
        if position < 0 || position > i128::from(self.size) {
            return Err(DiskError);
        }
        self.position = position as u64;
        Ok(self.position)
    }
}

/// A [`BlockDisk`] the file system and the storage share: the file system
/// reads and writes it, the storage writes back its changed blocks (those
/// of folders and of the FAT too) when a call that changes the card
/// returns and on [`Storage::sync`](paperpaw_platform::Storage::sync)
/// (see [`FatStorage::with_disk`](crate::FatStorage::with_disk)).
pub struct SharedDisk<B: Blocks>(alloc::rc::Rc<core::cell::RefCell<BlockDisk<B>>>);

impl<B: Blocks> SharedDisk<B> {
    pub fn new(disk: BlockDisk<B>) -> Self {
        Self(alloc::rc::Rc::new(core::cell::RefCell::new(disk)))
    }

    /// Writes back every changed block.
    pub fn flush_all(&self) -> Result<(), DiskError> {
        self.0.borrow_mut().flush()
    }

    /// Number of blocks of the volume.
    pub fn blocks(&self) -> u64 {
        self.0.borrow().blocks()
    }

    /// Reads `buf.len()` bytes at `at` of the volume.
    pub fn read_at(&self, at: u64, buf: &mut [u8]) -> Result<(), DiskError> {
        let mut disk = self.0.borrow_mut();
        disk.seek(SeekFrom::Start(at))?;
        disk.read_exact(buf).map_err(|_| DiskError)
    }

    /// Writes `data` at `at` of the volume.
    pub fn write_at(&self, at: u64, data: &[u8]) -> Result<(), DiskError> {
        let mut disk = self.0.borrow_mut();
        disk.seek(SeekFrom::Start(at))?;
        disk.write_all(data)
    }
}

impl<B: Blocks> Clone for SharedDisk<B> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<B: Blocks> core::fmt::Debug for SharedDisk<B> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SharedDisk").finish_non_exhaustive()
    }
}

impl<B: Blocks> IoBase for SharedDisk<B> {
    type Error = DiskError;
}

impl<B: Blocks> Read for SharedDisk<B> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, DiskError> {
        self.0.borrow_mut().read(buf)
    }
}

impl<B: Blocks> Write for SharedDisk<B> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, DiskError> {
        self.0.borrow_mut().write(buf)
    }

    fn flush(&mut self) -> Result<(), DiskError> {
        self.0.borrow_mut().flush()
    }
}

impl<B: Blocks> Seek for SharedDisk<B> {
    fn seek(&mut self, to: SeekFrom) -> Result<u64, DiskError> {
        self.0.borrow_mut().seek(to)
    }
}
