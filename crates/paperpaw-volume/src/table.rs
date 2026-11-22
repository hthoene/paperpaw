use core::borrow::BorrowMut;
use core::cell::Cell;
use core::marker::PhantomData;

use crate::error::{Error, IoError};
use crate::fs::{FatType, FsStatusFlags};
use crate::io::{self, Read, ReadLeExt, Seek, Write, WriteLeExt};

struct Fat<S> {
    phantom: PhantomData<S>,
}

#[cfg(feature = "fat16")]
type Fat12 = Fat<u8>;
#[cfg(feature = "fat16")]
type Fat16 = Fat<u16>;
type Fat32 = Fat<u32>;

pub const RESERVED_FAT_ENTRIES: u32 = 2;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub(crate) enum FatValue {
    Free,
    Data(u32),
    Bad,
    EndOfChain,
}

trait FatTrait {
    fn get_raw<S, E>(fat: &mut S, cluster: u32) -> Result<u32, Error<E>>
    where
        S: Read + Seek,
        E: IoError,
        Error<E>: From<S::Error>;

    fn get<S, E>(fat: &mut S, cluster: u32) -> Result<FatValue, Error<E>>
    where
        S: Read + Seek,
        E: IoError,
        Error<E>: From<S::Error>;

    fn set_raw<S, E>(fat: &mut S, cluster: u32, raw_value: u32) -> Result<(), Error<E>>
    where
        S: Read + Write + Seek,
        E: IoError,
        Error<E>: From<S::Error>;

    fn set<S, E>(fat: &mut S, cluster: u32, value: FatValue) -> Result<(), Error<E>>
    where
        S: Read + Write + Seek,
        E: IoError,
        Error<E>: From<S::Error>;

    fn find_free<S, E>(fat: &mut S, start_cluster: u32, end_cluster: u32) -> Result<u32, Error<E>>
    where
        S: Read + Seek,
        E: IoError,
        Error<E>: From<S::Error>;

    fn count_free<S, E>(fat: &mut S, end_cluster: u32) -> Result<u32, Error<E>>
    where
        S: Read + Seek,
        E: IoError,
        Error<E>: From<S::Error>;
}

pub(crate) fn read_fat<S, E>(
    fat: &mut S,
    fat_type: FatType,
    cluster: u32,
) -> Result<FatValue, Error<E>>
where
    S: Read + Seek,
    E: IoError,
    Error<E>: From<S::Error>,
{
    match fat_type {
        #[cfg(feature = "fat16")]
        FatType::Fat12 => Fat12::get(fat, cluster),
        #[cfg(feature = "fat16")]
        FatType::Fat16 => Fat16::get(fat, cluster),
        _ => Fat32::get(fat, cluster),
    }
}

pub(crate) fn write_fat<S, E>(
    fat: &mut S,
    fat_type: FatType,
    cluster: u32,
    value: FatValue,
) -> Result<(), Error<E>>
where
    S: Read + Write + Seek,
    E: IoError,
    Error<E>: From<S::Error>,
{
    trace!("write FAT - cluster {} value {:?}", cluster, value);
    match fat_type {
        #[cfg(feature = "fat16")]
        FatType::Fat12 => Fat12::set(fat, cluster, value),
        #[cfg(feature = "fat16")]
        FatType::Fat16 => Fat16::set(fat, cluster, value),
        _ => Fat32::set(fat, cluster, value),
    }
}

fn get_next_cluster<S, E>(
    fat: &mut S,
    fat_type: FatType,
    cluster: u32,
) -> Result<Option<u32>, Error<E>>
where
    S: Read + Seek,
    E: IoError,
    Error<E>: From<S::Error>,
{
    let val = read_fat(fat, fat_type, cluster)?;
    match val {
        FatValue::Data(n) => Ok(Some(n)),
        _ => Ok(None),
    }
}

fn find_free_cluster<S, E>(
    fat: &mut S,
    fat_type: FatType,
    start_cluster: u32,
    end_cluster: u32,
) -> Result<u32, Error<E>>
where
    S: Read + Seek,
    E: IoError,
    Error<E>: From<S::Error>,
{
    match fat_type {
        #[cfg(feature = "fat16")]
        FatType::Fat12 => Fat12::find_free(fat, start_cluster, end_cluster),
        #[cfg(feature = "fat16")]
        FatType::Fat16 => Fat16::find_free(fat, start_cluster, end_cluster),
        _ => Fat32::find_free(fat, start_cluster, end_cluster),
    }
}

// Modified for PaperPaw: `taken` added, counted as soon as the cluster is marked taken (see
// `FileSystem::cluster_changes`), so one left taken by a failed link is counted too.
pub(crate) fn alloc_cluster<S, E>(
    fat: &mut S,
    fat_type: FatType,
    prev_cluster: Option<u32>,
    hint: Option<u32>,
    total_clusters: u32,
    taken: &Cell<u64>,
) -> Result<u32, Error<E>>
where
    S: Read + Write + Seek,
    E: IoError,
    Error<E>: From<S::Error>,
{
    alloc_cluster_said(
        fat,
        fat_type,
        prev_cluster,
        hint,
        total_clusters,
        taken,
        &mut |_, _, _, _| Ok(()),
    )
}

/// Called before clusters are marked taken: with the FAT, the first of them, the bytes about to
/// be written at its entry (none where one cluster is taken as `alloc_cluster` does, and where
/// the next says so) and whether they go in one write with the link to them (a cut cannot leave
/// them taken and linked from nowhere then). See `FileSystem::claim_taking`.
pub(crate) trait Before<S, E>:
    FnMut(&mut S, u32, &[u8], bool) -> Result<(), Error<E>>
{
}
impl<S, E, F: FnMut(&mut S, u32, &[u8], bool) -> Result<(), Error<E>>> Before<S, E> for F {}

// Modified for PaperPaw: `alloc_cluster` with `before` called before the cluster is marked.
pub(crate) fn alloc_cluster_said<S, E>(
    fat: &mut S,
    fat_type: FatType,
    prev_cluster: Option<u32>,
    hint: Option<u32>,
    total_clusters: u32,
    taken: &Cell<u64>,
    before: &mut impl Before<S, E>,
) -> Result<u32, Error<E>>
where
    S: Read + Write + Seek,
    E: IoError,
    Error<E>: From<S::Error>,
{
    let end_cluster = total_clusters + RESERVED_FAT_ENTRIES;
    let start_cluster = match hint {
        Some(n) if n < end_cluster => n,
        _ => RESERVED_FAT_ENTRIES,
    };
    let new_cluster = match find_free_cluster(fat, fat_type, start_cluster, end_cluster) {
        Ok(n) => n,
        Err(_) if start_cluster > RESERVED_FAT_ENTRIES => {
            find_free_cluster(fat, fat_type, RESERVED_FAT_ENTRIES, start_cluster)?
        }
        Err(e) => return Err(e),
    };
    before(fat, new_cluster, &[], false)?;
    // Count first: a mirror write can fail after the primary FAT has changed.
    taken.set(taken.get().wrapping_add(1));
    write_fat(fat, fat_type, new_cluster, FatValue::EndOfChain)?;
    if let Some(n) = prev_cluster {
        write_fat(fat, fat_type, n, FatValue::Data(new_cluster))?;
    }
    trace!("allocated cluster {}", new_cluster);
    Ok(new_cluster)
}

// Modified for PaperPaw: a run of clusters taken at once. `first` is the first free cluster at or
// after `hint`; up to `want` free clusters in a row from it are chained (the last one ends the
// chain), and `prev_cluster`'s link to `first` is written with them in one write when it is the
// cluster just before: the entries of one FAT sector reach each FAT copy once, not once for each
// entry (written one by one they were written back in order, entry by entry, each time to both
// copies). Returns the first cluster and how many were taken (at least one). FAT12 takes one.
pub(crate) fn alloc_run<S, E>(
    fat: &mut S,
    fat_type: FatType,
    prev_cluster: Option<u32>,
    hint: Option<u32>,
    total_clusters: u32,
    taken: &Cell<u64>,
    want: u32,
    before: &mut impl Before<S, E>,
) -> Result<(u32, u32), Error<E>>
where
    S: Read + Write + Seek,
    E: IoError,
    Error<E>: From<S::Error>,
{
    if fat_type == FatType::Fat12 {
        let first = alloc_cluster_said(
            fat,
            fat_type,
            prev_cluster,
            hint,
            total_clusters,
            taken,
            before,
        )?;
        return Ok((first, 1));
    }
    let end_cluster = total_clusters + RESERVED_FAT_ENTRIES;
    let start_cluster = match hint {
        Some(n) if n < end_cluster => n,
        _ => RESERVED_FAT_ENTRIES,
    };
    let first = match find_free_cluster(fat, fat_type, start_cluster, end_cluster) {
        Ok(n) => n,
        Err(_) if start_cluster > RESERVED_FAT_ENTRIES => {
            find_free_cluster(fat, fat_type, RESERVED_FAT_ENTRIES, start_cluster)?
        }
        Err(e) => return Err(e),
    };
    // A run is written in one block of the FAT, with the link into it where the cluster before
    // it lies in that block: one write, which a cut cannot leave half done. Where it would
    // straddle two blocks it is cut short at the end of the block; where the cluster before it
    // lies in another block it is one cluster, and `before` is told that the link is a write of
    // its own.
    let entries_per_block = 512 / if fat_type.wide() { 4 } else { 2 };
    let same_block =
        prev_cluster.is_some_and(|n| n / entries_per_block == first / entries_per_block);
    let want = if same_block || prev_cluster.is_none() {
        want.min(entries_per_block - first % entries_per_block)
    } else {
        1
    }
    .clamp(1, MAX_RUN);
    let mut count = 1;
    while count < want
        && first + count < end_cluster
        && read_fat(fat, fat_type, first + count)? == FatValue::Free
    {
        count += 1;
    }
    if let Some(prev) = prev_cluster.filter(|&n| same_block && n + 1 != first) {
        // In its block but not next to it (a file that grows around others): the block is
        // written once, with the link and the run in it.
        before(fat, first, &[], true)?;
        taken.set(taken.get().wrapping_add(u64::from(count)));
        link_in_block(fat, fat_type, prev, first, count)?;
        trace!("allocated {} clusters from {}", count, first);
        return Ok((first, count));
    }
    // The link from the cluster before goes in the same write when the run follows it.
    let joined = prev_cluster.filter(|&n| n + 1 == first);
    let begin = joined.unwrap_or(first);
    let mut raw = [0u8; (MAX_RUN as usize + 1) * 4];
    let mut at = 0;
    if joined.is_some() {
        at += put_entry(&mut raw[at..], fat, fat_type, begin, FatValue::Data(first))?;
    }
    let run = at;
    for i in 0..count {
        let value = if i + 1 < count {
            FatValue::Data(first + i + 1)
        } else {
            FatValue::EndOfChain
        };
        at += put_entry(&mut raw[at..], fat, fat_type, first + i, value)?;
    }
    // The run and the link to it in one block are one write; any other way, a cut may fall
    // between the two.
    before(fat, first, &raw[run..at], same_block)?;
    // Count first: a mirror write can fail after the primary FAT has changed.
    taken.set(taken.get().wrapping_add(u64::from(count)));
    write_entries(fat, fat_type, begin, &raw[..at], true)?;
    if let (Some(n), None) = (prev_cluster, joined) {
        write_fat(fat, fat_type, n, FatValue::Data(first))?;
    }
    trace!("allocated {} clusters from {}", count, first);
    Ok((first, count))
}

// Modified for PaperPaw: `block_of`, `hash_block`, `free_in_block`, `set_in_block` and `BLOCK` added:
// the entries of one 512-byte block of the FAT (FAT16, FAT32) changed together and written once.

/// Bytes of a block of the FAT: what reaches the disk as a whole.
pub(crate) const BLOCK: usize = 512;

fn entry_size(fat_type: FatType) -> u64 {
    if fat_type.wide() { 4 } else { 2 }
}

/// Where in the FAT the block holding the entry of `cluster` starts.
pub(crate) fn block_of(fat_type: FatType, cluster: u32) -> u64 {
    u64::from(cluster) * entry_size(fat_type) / BLOCK as u64 * BLOCK as u64
}

/// The block holding the entry of `cluster`, read into `block`.
pub(crate) fn read_block<S, E>(
    fat: &mut S,
    fat_type: FatType,
    cluster: u32,
    block: &mut [u8; BLOCK],
) -> Result<(), Error<E>>
where
    S: Read + Seek,
    E: IoError,
    Error<E>: From<S::Error>,
{
    fat.seek(io::SeekFrom::Start(block_of(fat_type, cluster)))?;
    fat.read_exact(block)?;
    Ok(())
}

/// A hash (FNV-1a) of the block holding the entry of `cluster`, as it is with `entries` written
/// at that entry (they must end inside the block). Read a part at a time: no block on the stack.
pub(crate) fn hash_block<S, E>(
    fat: &mut S,
    fat_type: FatType,
    cluster: u32,
    entries: &[u8],
) -> Result<u32, Error<E>>
where
    S: Read + Seek,
    E: IoError,
    Error<E>: From<S::Error>,
{
    const PART: usize = 32;
    let begin = block_of(fat_type, cluster);
    let from = (u64::from(cluster) * entry_size(fat_type) - begin) as usize;
    let mut hash = 0x811C_9DC5_u32;
    fat.seek(io::SeekFrom::Start(begin))?;
    for part in 0..BLOCK / PART {
        let mut bytes = [0u8; PART];
        fat.read_exact(&mut bytes)?;
        for (i, byte) in bytes.iter_mut().enumerate() {
            if let Some(new) = (part * PART + i)
                .checked_sub(from)
                .and_then(|k| entries.get(k))
            {
                *byte = *new;
            }
            hash = (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193);
        }
    }
    Ok(hash)
}

/// Writes `value` at the entry of `cluster` in `block`, the block of the FAT that holds it (a
/// FAT32 entry keeps its four reserved bits).
pub(crate) fn set_in_block(
    block: &mut [u8; BLOCK],
    fat_type: FatType,
    cluster: u32,
    value: FatValue,
) {
    let at = (u64::from(cluster) * entry_size(fat_type) - block_of(fat_type, cluster)) as usize;
    let raw = match value {
        FatValue::Free => 0,
        FatValue::Bad => 0x0FFF_FFF7,
        FatValue::EndOfChain => 0x0FFF_FFFF,
        FatValue::Data(n) => n,
    };
    if fat_type.wide() {
        let old = u32::from_le_bytes([block[at], block[at + 1], block[at + 2], block[at + 3]]);
        block[at..at + 4]
            .copy_from_slice(&((raw & 0x0FFF_FFFF) | (old & 0xF000_0000)).to_le_bytes());
    } else {
        block[at..at + 2].copy_from_slice(&(raw as u16).to_le_bytes());
    }
}

/// Where the chain from `start` leaves the block of the FAT its entry is in: the first of its
/// clusters whose entry lies in another block, if it goes on.
pub(crate) fn leaves_block<S, E>(
    fat: &mut S,
    fat_type: FatType,
    start: u32,
) -> Result<Option<u32>, Error<E>>
where
    S: Read + Seek,
    E: IoError,
    Error<E>: From<S::Error>,
{
    let begin = block_of(fat_type, start);
    let mut cluster = start;
    for _ in 0..BLOCK as u64 / entry_size(fat_type) {
        match read_fat(fat, fat_type, cluster)? {
            FatValue::Data(next) if block_of(fat_type, next) == begin => cluster = next,
            FatValue::Data(next) => return Ok(Some(next)),
            _ => return Ok(None),
        }
    }
    Ok(None)
}

/// `prev` linked to `first` and `count` clusters from `first` chained (the last one ends the
/// chain), all in one block of the FAT, which is written once. The block is on the stack here
/// only.
#[inline(never)]
fn link_in_block<S, E>(
    fat: &mut S,
    fat_type: FatType,
    prev: u32,
    first: u32,
    count: u32,
) -> Result<(), Error<E>>
where
    S: Read + Write + Seek,
    E: IoError,
    Error<E>: From<S::Error>,
{
    let mut block = [0u8; BLOCK];
    read_block(fat, fat_type, first, &mut block)?;
    set_in_block(&mut block, fat_type, prev, FatValue::Data(first));
    for i in 0..count {
        let value = if i + 1 < count {
            FatValue::Data(first + i + 1)
        } else {
            FatValue::EndOfChain
        };
        set_in_block(&mut block, fat_type, first + i, value);
    }
    fat.seek(io::SeekFrom::Start(block_of(fat_type, first)))?;
    fat.write_all(&block)?;
    Ok(())
}

/// Marks free, in `block` (the block of the FAT that holds the entry of `start`, as `fat` has
/// it), the clusters of the chain from `start` as far as their entries lie in it. How many, and
/// where the chain goes on in another block. A cluster that is free already ends it.
pub(crate) fn free_in_block<S, E>(
    fat: &mut S,
    fat_type: FatType,
    block: &mut [u8; BLOCK],
    start: u32,
) -> Result<(u32, Option<u32>), Error<E>>
where
    S: Read + Seek,
    E: IoError,
    Error<E>: From<S::Error>,
{
    let begin = block_of(fat_type, start);
    let mut cluster = start;
    let mut freed = 0;
    loop {
        // As the disk has it; freed here already (a chain that comes back to it), it reads free.
        let at = (u64::from(cluster) * entry_size(fat_type) - begin) as usize;
        let cleared = if fat_type.wide() {
            block[at..at + 4] == [0, 0, 0, block[at + 3] & 0xF0]
        } else {
            block[at..at + 2] == [0, 0]
        };
        let value = read_fat(fat, fat_type, cluster)?;
        if cleared || value == FatValue::Free {
            return Ok((freed, None));
        }
        set_in_block(block, fat_type, cluster, FatValue::Free);
        freed += 1;
        match value {
            FatValue::Data(next) if block_of(fat_type, next) == begin => cluster = next,
            FatValue::Data(next) => return Ok((freed, Some(next))),
            _ => return Ok((freed, None)),
        }
    }
}

/// The most clusters a run takes at once.
pub(crate) const MAX_RUN: u32 = 32;

/// The bytes of one FAT16/FAT32 entry, as `Fat16::set` and `Fat32::set` write it (a FAT32 entry
/// keeps its four reserved bits).
fn put_entry<S, E>(
    out: &mut [u8],
    fat: &mut S,
    fat_type: FatType,
    cluster: u32,
    value: FatValue,
) -> Result<usize, Error<E>>
where
    S: Read + Seek,
    E: IoError,
    Error<E>: From<S::Error>,
{
    match fat_type.wide() {
        true => {
            let reserved = Fat32::get_raw(fat, cluster)? & 0xF000_0000;
            let raw = match value {
                FatValue::Free => 0,
                FatValue::Bad => 0x0FFF_FFF7,
                FatValue::EndOfChain => 0x0FFF_FFFF,
                FatValue::Data(n) => n,
            } | reserved;
            out[..4].copy_from_slice(&raw.to_le_bytes());
            Ok(4)
        }
        false => {
            let raw: u16 = match value {
                FatValue::Free => 0,
                FatValue::Bad => 0xFFF7,
                FatValue::EndOfChain => 0xFFFF,
                FatValue::Data(n) => n as u16,
            };
            out[..2].copy_from_slice(&raw.to_le_bytes());
            Ok(2)
        }
    }
}

/// `bytes` (entries as `put_entry` made them) written at the entry of `cluster`: one write per
/// 512-byte block of the FAT they touch (a block reaches the card as a whole, in the order its
/// changes were made), the last block first where `later_first` (a chain made: every link points
/// to what is already written, the link into the run last), the first block first where not (a
/// chain cut short: its end first, then what is freed).
fn write_entries<S, E>(
    fat: &mut S,
    fat_type: FatType,
    cluster: u32,
    bytes: &[u8],
    later_first: bool,
) -> Result<(), Error<E>>
where
    S: Read + Write + Seek,
    E: IoError,
    Error<E>: From<S::Error>,
{
    const BLOCK: u64 = 512;
    let size: u64 = if fat_type.wide() { 4 } else { 2 };
    let begin = u64::from(cluster) * size;
    let end = begin + bytes.len() as u64;
    // The cut points: every block boundary inside. The entries of a run are fewer bytes than a
    // block (`MAX_RUN` + 1 of them), so there is one boundary at most: three points.
    debug_assert!(bytes.len() as u64 <= BLOCK);
    let mut parts = [0usize; 3];
    let mut n = 0;
    let mut at = 0usize;
    parts[n] = 0;
    n += 1;
    let mut next_boundary = (begin / BLOCK + 1) * BLOCK;
    while next_boundary < end {
        at = (next_boundary - begin) as usize;
        parts[n] = at;
        n += 1;
        next_boundary += BLOCK;
    }
    let _ = at;
    parts[n] = bytes.len();
    for k in 0..n {
        let k = if later_first { n - 1 - k } else { k };
        let (from, to) = (parts[k], parts[k + 1]);
        fat.seek(io::SeekFrom::Start(begin + from as u64))?;
        fat.write_all(&bytes[from..to])?;
    }
    Ok(())
}

pub(crate) fn read_fat_flags<S, E>(
    fat: &mut S,
    fat_type: FatType,
) -> Result<FsStatusFlags, Error<E>>
where
    S: Read + Seek,
    E: IoError,
    Error<E>: From<S::Error>,
{
    // check MSB (except in FAT12)
    let val = match fat_type {
        #[cfg(feature = "fat16")]
        FatType::Fat12 => 0xFFF,
        #[cfg(feature = "fat16")]
        FatType::Fat16 => Fat16::get_raw(fat, 1)?,
        _ => Fat32::get_raw(fat, 1)?,
    };
    let dirty = match fat_type {
        #[cfg(feature = "fat16")]
        FatType::Fat12 => false,
        #[cfg(feature = "fat16")]
        FatType::Fat16 => val & (1 << 15) == 0,
        _ => val & (1 << 27) == 0,
    };
    let io_error = match fat_type {
        #[cfg(feature = "fat16")]
        FatType::Fat12 => false,
        #[cfg(feature = "fat16")]
        FatType::Fat16 => val & (1 << 14) == 0,
        _ => val & (1 << 26) == 0,
    };
    Ok(FsStatusFlags { dirty, io_error })
}

pub(crate) fn count_free_clusters<S, E>(
    fat: &mut S,
    fat_type: FatType,
    total_clusters: u32,
) -> Result<u32, Error<E>>
where
    S: Read + Seek,
    E: IoError,
    Error<E>: From<S::Error>,
{
    let end_cluster = total_clusters + RESERVED_FAT_ENTRIES;
    match fat_type {
        #[cfg(feature = "fat16")]
        FatType::Fat12 => Fat12::count_free(fat, end_cluster),
        #[cfg(feature = "fat16")]
        FatType::Fat16 => Fat16::count_free(fat, end_cluster),
        _ => Fat32::count_free(fat, end_cluster),
    }
}

pub(crate) fn format_fat<S, E>(
    fat: &mut S,
    fat_type: FatType,
    media: u8,
    bytes_per_fat: u64,
    total_clusters: u32,
) -> Result<(), Error<E>>
where
    S: Read + Write + Seek,
    E: IoError,
    Error<E>: From<S::Error>,
{
    const BITS_PER_BYTE: u64 = 8;
    // init first two reserved entries to FAT ID
    match fat_type {
        FatType::Fat12 => {
            fat.write_u8(media)?;
            fat.write_u16_le(0xFFFF)?;
        }
        FatType::Fat16 => {
            fat.write_u16_le(u16::from(media) | 0xFF00)?;
            fat.write_u16_le(0xFFFF)?;
        }
        FatType::Fat32 => {
            fat.write_u32_le(u32::from(media) | 0xFFF_FF00)?;
            fat.write_u32_le(0xFFFF_FFFF)?;
        }
    }
    // mark entries at the end of FAT as used (after FAT but before sector end)
    let start_cluster = total_clusters + RESERVED_FAT_ENTRIES;
    let end_cluster =
        (bytes_per_fat * BITS_PER_BYTE / u64::from(fat_type.bits_per_fat_entry())) as u32;
    for cluster in start_cluster..end_cluster {
        write_fat(fat, fat_type, cluster, FatValue::EndOfChain)?;
    }
    // mark special entries 0x0FFFFFF0 - 0x0FFFFFFF as BAD if they exists on FAT32 volume
    if end_cluster > 0x0FFF_FFF0 {
        let end_bad_cluster = (0x0FFF_FFFF + 1).min(end_cluster);
        for cluster in 0x0FFF_FFF0..end_bad_cluster {
            write_fat(fat, fat_type, cluster, FatValue::Bad)?;
        }
    }
    Ok(())
}

#[cfg(feature = "fat16")]
impl FatTrait for Fat12 {
    fn get_raw<S, E>(fat: &mut S, cluster: u32) -> Result<u32, Error<E>>
    where
        S: Read + Seek,
        E: IoError,
        Error<E>: From<S::Error>,
    {
        let fat_offset = cluster + (cluster / 2);
        fat.seek(io::SeekFrom::Start(u64::from(fat_offset)))?;
        let packed_val = fat.read_u16_le()?;
        Ok(u32::from(match cluster & 1 {
            0 => packed_val & 0x0FFF,
            _ => packed_val >> 4,
        }))
    }

    fn get<S, E>(fat: &mut S, cluster: u32) -> Result<FatValue, Error<E>>
    where
        S: Read + Seek,
        E: IoError,
        Error<E>: From<S::Error>,
    {
        let val = Self::get_raw(fat, cluster)?;
        Ok(match val {
            0 => FatValue::Free,
            0xFF7 => FatValue::Bad,
            0xFF8..=0xFFF => FatValue::EndOfChain,
            n => FatValue::Data(n),
        })
    }

    fn set<S, E>(fat: &mut S, cluster: u32, value: FatValue) -> Result<(), Error<E>>
    where
        S: Read + Write + Seek,
        E: IoError,
        Error<E>: From<S::Error>,
    {
        let raw_val = match value {
            FatValue::Free => 0,
            FatValue::Bad => 0xFF7,
            FatValue::EndOfChain => 0xFFF,
            FatValue::Data(n) => n,
        };
        Self::set_raw(fat, cluster, raw_val)
    }

    fn set_raw<S, E>(fat: &mut S, cluster: u32, raw_val: u32) -> Result<(), Error<E>>
    where
        S: Read + Write + Seek,
        E: IoError,
        Error<E>: From<S::Error>,
    {
        let fat_offset = cluster + (cluster / 2);
        fat.seek(io::SeekFrom::Start(u64::from(fat_offset)))?;
        let old_packed = fat.read_u16_le()?;
        fat.seek(io::SeekFrom::Start(u64::from(fat_offset)))?;
        let new_packed = match cluster & 1 {
            0 => (old_packed & 0xF000) | raw_val as u16,
            _ => (old_packed & 0x000F) | ((raw_val as u16) << 4),
        };
        fat.write_u16_le(new_packed)?;
        Ok(())
    }

    fn find_free<S, E>(fat: &mut S, start_cluster: u32, end_cluster: u32) -> Result<u32, Error<E>>
    where
        S: Read + Seek,
        E: IoError,
        Error<E>: From<S::Error>,
    {
        let mut cluster = start_cluster;
        let fat_offset = cluster + (cluster / 2);
        fat.seek(io::SeekFrom::Start(u64::from(fat_offset)))?;
        let mut packed_val = fat.read_u16_le()?;
        loop {
            let val = match cluster & 1 {
                0 => packed_val & 0x0FFF,
                _ => packed_val >> 4,
            };
            if val == 0 {
                return Ok(cluster);
            }
            cluster += 1;
            if cluster == end_cluster {
                return Err(Error::NotEnoughSpace);
            }
            packed_val = if cluster & 1 == 0 {
                fat.read_u16_le()?
            } else {
                let next_byte = fat.read_u8()?;
                (packed_val >> 8) | (u16::from(next_byte) << 8)
            };
        }
    }

    fn count_free<S, E>(fat: &mut S, end_cluster: u32) -> Result<u32, Error<E>>
    where
        S: Read + Seek,
        E: IoError,
        Error<E>: From<S::Error>,
    {
        let mut count = 0;
        let mut cluster = RESERVED_FAT_ENTRIES;
        fat.seek(io::SeekFrom::Start(u64::from(cluster * 3 / 2)))?;
        let mut prev_packed_val = 0_u16;
        while cluster < end_cluster {
            let res = match cluster & 1 {
                0 => fat.read_u16_le(),
                _ => fat.read_u8().map(u16::from),
            };
            let packed_val = res?;
            let val = match cluster & 1 {
                0 => packed_val & 0x0FFF,
                _ => (packed_val << 8) | (prev_packed_val >> 12),
            };
            prev_packed_val = packed_val;
            if val == 0 {
                count += 1;
            }
            cluster += 1;
        }
        Ok(count)
    }
}

#[cfg(feature = "fat16")]
impl FatTrait for Fat16 {
    fn get_raw<S, E>(fat: &mut S, cluster: u32) -> Result<u32, Error<E>>
    where
        S: Read + Seek,
        E: IoError,
        Error<E>: From<S::Error>,
    {
        fat.seek(io::SeekFrom::Start(u64::from(cluster * 2)))?;
        Ok(u32::from(fat.read_u16_le()?))
    }

    fn get<S, E>(fat: &mut S, cluster: u32) -> Result<FatValue, Error<E>>
    where
        S: Read + Seek,
        E: IoError,
        Error<E>: From<S::Error>,
    {
        let val = Self::get_raw(fat, cluster)?;
        Ok(match val {
            0 => FatValue::Free,
            0xFFF7 => FatValue::Bad,
            0xFFF8..=0xFFFF => FatValue::EndOfChain,
            n => FatValue::Data(n),
        })
    }

    fn set_raw<S, E>(fat: &mut S, cluster: u32, raw_value: u32) -> Result<(), Error<E>>
    where
        S: Read + Write + Seek,
        E: IoError,
        Error<E>: From<S::Error>,
    {
        fat.seek(io::SeekFrom::Start(u64::from(cluster * 2)))?;
        fat.write_u16_le(raw_value as u16)?;
        Ok(())
    }

    fn set<S, E>(fat: &mut S, cluster: u32, value: FatValue) -> Result<(), Error<E>>
    where
        S: Read + Write + Seek,
        E: IoError,
        Error<E>: From<S::Error>,
    {
        let raw_value = match value {
            FatValue::Free => 0,
            FatValue::Bad => 0xFFF7,
            FatValue::EndOfChain => 0xFFFF,
            FatValue::Data(n) => n,
        };
        Self::set_raw(fat, cluster, raw_value)
    }

    fn find_free<S, E>(fat: &mut S, start_cluster: u32, end_cluster: u32) -> Result<u32, Error<E>>
    where
        S: Read + Seek,
        E: IoError,
        Error<E>: From<S::Error>,
    {
        let mut cluster = start_cluster;
        fat.seek(io::SeekFrom::Start(u64::from(cluster * 2)))?;
        while cluster < end_cluster {
            let val = fat.read_u16_le()?;
            if val == 0 {
                return Ok(cluster);
            }
            cluster += 1;
        }
        Err(Error::NotEnoughSpace)
    }

    fn count_free<S, E>(fat: &mut S, end_cluster: u32) -> Result<u32, Error<E>>
    where
        S: Read + Seek,
        E: IoError,
        Error<E>: From<S::Error>,
    {
        let mut count = 0;
        let mut cluster = RESERVED_FAT_ENTRIES;
        fat.seek(io::SeekFrom::Start(u64::from(cluster * 2)))?;
        while cluster < end_cluster {
            let val = fat.read_u16_le()?;
            if val == 0 {
                count += 1;
            }
            cluster += 1;
        }
        Ok(count)
    }
}

impl FatTrait for Fat32 {
    fn get_raw<S, E>(fat: &mut S, cluster: u32) -> Result<u32, Error<E>>
    where
        S: Read + Seek,
        E: IoError,
        Error<E>: From<S::Error>,
    {
        fat.seek(io::SeekFrom::Start(u64::from(cluster * 4)))?;
        Ok(fat.read_u32_le()?)
    }

    fn get<S, E>(fat: &mut S, cluster: u32) -> Result<FatValue, Error<E>>
    where
        S: Read + Seek,
        E: IoError,
        Error<E>: From<S::Error>,
    {
        let val = Self::get_raw(fat, cluster)? & 0x0FFF_FFFF;
        Ok(match val {
            0 if (0x0FFF_FFF7..=0x0FFF_FFFF).contains(&cluster) => {
                let tmp = if cluster == 0x0FFF_FFF7 {
                    "BAD_CLUSTER"
                } else {
                    "end-of-chain"
                };
                warn!(
                    "cluster number {} is a special value in FAT to indicate {}; it should never be seen as free",
                    cluster, tmp
                );
                FatValue::Bad // avoid accidental use or allocation into a FAT chain
            }
            0 => FatValue::Free,
            0x0FFF_FFF7 => FatValue::Bad,
            0x0FFF_FFF8..=0x0FFF_FFFF => FatValue::EndOfChain,
            n if (0x0FFF_FFF7..=0x0FFF_FFFF).contains(&cluster) => {
                let tmp = if cluster == 0x0FFF_FFF7 {
                    "BAD_CLUSTER"
                } else {
                    "end-of-chain"
                };
                warn!(
                    "cluster number {} is a special value in FAT to indicate {}; hiding potential FAT chain value {} and instead reporting as a bad sector",
                    cluster, tmp, n
                );
                FatValue::Bad // avoid accidental use or allocation into a FAT chain
            }
            n => FatValue::Data(n),
        })
    }

    fn set_raw<S, E>(fat: &mut S, cluster: u32, raw_value: u32) -> Result<(), Error<E>>
    where
        S: Read + Write + Seek,
        E: IoError,
        Error<E>: From<S::Error>,
    {
        fat.seek(io::SeekFrom::Start(u64::from(cluster * 4)))?;
        fat.write_u32_le(raw_value)?;
        Ok(())
    }

    fn set<S, E>(fat: &mut S, cluster: u32, value: FatValue) -> Result<(), Error<E>>
    where
        S: Read + Write + Seek,
        E: IoError,
        Error<E>: From<S::Error>,
    {
        let old_reserved_bits = Self::get_raw(fat, cluster)? & 0xF000_0000;

        if value == FatValue::Free && (0x0FFF_FFF7..=0x0FFF_FFFF).contains(&cluster) {
            // NOTE: it is technically allowed for them to store FAT chain loops,
            //       or even have them all store value '4' as their next cluster.
            //       Some believe only FatValue::Bad should be allowed for this edge case.
            let tmp = if cluster == 0x0FFF_FFF7 {
                "BAD_CLUSTER"
            } else {
                "end-of-chain"
            };
            panic!(
                "cluster number {} is a special value in FAT to indicate {}; it should never be set as free",
                cluster, tmp
            );
        }
        let raw_val = match value {
            FatValue::Free => 0,
            FatValue::Bad => 0x0FFF_FFF7,
            FatValue::EndOfChain => 0x0FFF_FFFF,
            FatValue::Data(n) => n,
        };
        let raw_val = raw_val | old_reserved_bits; // must preserve original reserved values
        Self::set_raw(fat, cluster, raw_val)
    }

    fn find_free<S, E>(fat: &mut S, start_cluster: u32, end_cluster: u32) -> Result<u32, Error<E>>
    where
        S: Read + Seek,
        E: IoError,
        Error<E>: From<S::Error>,
    {
        let mut cluster = start_cluster;
        fat.seek(io::SeekFrom::Start(u64::from(cluster * 4)))?;
        while cluster < end_cluster {
            let val = fat.read_u32_le()? & 0x0FFF_FFFF;
            if val == 0 {
                return Ok(cluster);
            }
            cluster += 1;
        }
        Err(Error::NotEnoughSpace)
    }

    fn count_free<S, E>(fat: &mut S, end_cluster: u32) -> Result<u32, Error<E>>
    where
        S: Read + Seek,
        E: IoError,
        Error<E>: From<S::Error>,
    {
        let mut count = 0;
        let mut cluster = RESERVED_FAT_ENTRIES;
        fat.seek(io::SeekFrom::Start(u64::from(cluster * 4)))?;
        while cluster < end_cluster {
            let val = fat.read_u32_le()? & 0x0FFF_FFFF;
            if val == 0 {
                count += 1;
            }
            cluster += 1;
        }
        Ok(count)
    }
}

pub(crate) struct ClusterIterator<B, E, S = B> {
    fat: B,
    fat_type: FatType,
    cluster: Option<u32>,
    // Modified for PaperPaw: one past the volume's last cluster (see `holds`).
    end: u32,
    err: bool,
    // Modified for PaperPaw: clusters `free` marked free, a chain freed in part too.
    freed: u32,
    // Modified for PaperPaw: a free cluster marked taken by `truncate`.
    taken: u32,
    // phantom is needed to add type bounds on the storage type
    phantom_s: PhantomData<S>,
    phantom_e: PhantomData<E>,
}

impl<B, E, S> ClusterIterator<B, E, S>
where
    B: BorrowMut<S>,
    E: IoError,
    S: Read + Write + Seek,
    Error<E>: From<S::Error>,
{
    // Modified for PaperPaw: `total_clusters` added, the volume's (see `holds`).
    pub(crate) fn new(fat: B, fat_type: FatType, cluster: u32, total_clusters: u32) -> Self {
        Self {
            fat,
            fat_type,
            cluster: Some(cluster),
            end: total_clusters.saturating_add(RESERVED_FAT_ENTRIES),
            err: false,
            freed: 0,
            taken: 0,
            phantom_s: PhantomData,
            phantom_e: PhantomData,
        }
    }

    // Modified for PaperPaw: added.
    /// Checked to be one of the volume's clusters: a cluster number read from the card (a
    /// directory entry's first cluster, a link in the FAT) may be none, and its entry in the FAT,
    /// or its place in the data area, would be the FAT's reserved entries, the FAT itself or past
    /// the volume.
    fn holds(&self, cluster: u32) -> Result<u32, Error<E>> {
        if (RESERVED_FAT_ENTRIES..self.end).contains(&cluster) {
            Ok(cluster)
        } else {
            Err(Error::CorruptedFileSystem)
        }
    }

    pub(crate) fn truncate(&mut self) -> Result<u32, Error<E>> {
        if let Some(n) = self.cluster {
            // Modified for PaperPaw: see `holds`.
            self.holds(n)?;
            // Read once, keeping both the next link and whether this entry was free.
            let value = read_fat(self.fat.borrow_mut(), self.fat_type, n)?;
            self.cluster = if let FatValue::Data(next) = value {
                Some(next)
            } else {
                None
            };
            // A partial mirrored write must leave the count on the low side.
            if value == FatValue::Free {
                self.taken += 1;
            }
            write_fat(
                self.fat.borrow_mut(),
                self.fat_type,
                n,
                FatValue::EndOfChain,
            )?;
            // Free rest of chain
            self.free()
        } else {
            Ok(0)
        }
    }

    pub(crate) fn free(&mut self) -> Result<u32, Error<E>> {
        let mut num_free = 0;
        while let Some(n) = self.cluster {
            // Modified for PaperPaw: what comes before a link out of the volume is freed; freeing
            // fails there (see `holds`).
            self.holds(n)?;
            let value = read_fat(self.fat.borrow_mut(), self.fat_type, n)?;
            self.cluster = if let FatValue::Data(next) = value {
                Some(next)
            } else {
                None
            };
            if value != FatValue::Free {
                write_fat(self.fat.borrow_mut(), self.fat_type, n, FatValue::Free)?;
                num_free += 1;
                self.freed += 1;
            }
        }
        Ok(num_free)
    }

    // Modified for PaperPaw: added.
    /// The clusters `free` and `truncate` marked free so far, also where they failed on.
    pub(crate) fn freed(&self) -> u32 {
        self.freed
    }

    // Modified for PaperPaw: added.
    pub(crate) fn taken(&self) -> u32 {
        self.taken
    }
}

impl<B, E, S> Iterator for ClusterIterator<B, E, S>
where
    B: BorrowMut<S>,
    E: IoError,
    S: Read + Write + Seek,
    Error<E>: From<S::Error>,
{
    type Item = Result<u32, Error<E>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.err {
            return None;
        }
        if let Some(current_cluster) = self.cluster {
            // Modified for PaperPaw: the cluster read and the one it links to checked (see
            // `holds`): the next one is read or written in the data area before its own entry in
            // the FAT is read.
            let next = self.holds(current_cluster).and_then(|current| {
                get_next_cluster(self.fat.borrow_mut(), self.fat_type, current)
            });
            self.cluster = match next.and_then(|next| next.map(|n| self.holds(n)).transpose()) {
                Ok(next_cluster) => next_cluster,
                Err(err) => {
                    self.err = true;
                    return Some(Err(err));
                }
            }
        }
        self.cluster.map(Ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use io::StdIoWrapper;
    use std::io::Cursor;

    fn test_fat<S: Read + Write + Seek>(fat_type: FatType, mut cur: S) {
        // based on cluster maps from Wikipedia:
        // https://en.wikipedia.org/wiki/Design_of_the_FAT_file_system#Cluster_map
        assert_eq!(
            read_fat(&mut cur, fat_type, 1).ok(),
            Some(FatValue::EndOfChain)
        );
        assert_eq!(
            read_fat(&mut cur, fat_type, 4).ok(),
            Some(FatValue::Data(5))
        );
        assert_eq!(
            read_fat(&mut cur, fat_type, 5).ok(),
            Some(FatValue::Data(6))
        );
        assert_eq!(
            read_fat(&mut cur, fat_type, 8).ok(),
            Some(FatValue::EndOfChain)
        );
        assert_eq!(
            read_fat(&mut cur, fat_type, 9).ok(),
            Some(FatValue::Data(0xA))
        );
        assert_eq!(
            read_fat(&mut cur, fat_type, 0xA).ok(),
            Some(FatValue::Data(0x14))
        );
        assert_eq!(
            read_fat(&mut cur, fat_type, 0x12).ok(),
            Some(FatValue::Free)
        );
        assert_eq!(read_fat(&mut cur, fat_type, 0x17).ok(), Some(FatValue::Bad));
        assert_eq!(read_fat(&mut cur, fat_type, 0x18).ok(), Some(FatValue::Bad));
        assert_eq!(
            read_fat(&mut cur, fat_type, 0x1B).ok(),
            Some(FatValue::Free)
        );

        assert_eq!(
            find_free_cluster(&mut cur, fat_type, 2, 0x20).ok(),
            Some(0x12)
        );
        assert_eq!(
            find_free_cluster(&mut cur, fat_type, 0x12, 0x20).ok(),
            Some(0x12)
        );
        assert_eq!(
            find_free_cluster(&mut cur, fat_type, 0x13, 0x20).ok(),
            Some(0x1B)
        );
        assert!(find_free_cluster(&mut cur, fat_type, 0x13, 0x14).is_err());

        assert_eq!(count_free_clusters(&mut cur, fat_type, 0x1E).ok(), Some(5));

        // test allocation
        assert_eq!(
            alloc_cluster(&mut cur, fat_type, None, Some(0x13), 0x1E, &Cell::new(0)).ok(),
            Some(0x1B)
        );
        assert_eq!(
            read_fat(&mut cur, fat_type, 0x1B).ok(),
            Some(FatValue::EndOfChain)
        );
        assert_eq!(
            alloc_cluster(&mut cur, fat_type, Some(0x1B), None, 0x1E, &Cell::new(0)).ok(),
            Some(0x12)
        );
        assert_eq!(
            read_fat(&mut cur, fat_type, 0x1B).ok(),
            Some(FatValue::Data(0x12))
        );
        assert_eq!(
            read_fat(&mut cur, fat_type, 0x12).ok(),
            Some(FatValue::EndOfChain)
        );
        assert_eq!(count_free_clusters(&mut cur, fat_type, 0x1E).ok(), Some(3));
        // test reading from iterator
        {
            let iter = ClusterIterator::<&mut S, S::Error, S>::new(&mut cur, fat_type, 0x9, 0x1E);
            let actual_cluster_numbers = iter.map(Result::ok).collect::<Vec<_>>();
            let expected_cluster_numbers =
                [0xA_u32, 0x14_u32, 0x15_u32, 0x16_u32, 0x19_u32, 0x1A_u32]
                    .iter()
                    .copied()
                    .map(Some)
                    .collect::<Vec<_>>();
            assert_eq!(actual_cluster_numbers, expected_cluster_numbers);
        }
        // test truncating a chain
        {
            let mut iter =
                ClusterIterator::<&mut S, S::Error, S>::new(&mut cur, fat_type, 0x9, 0x1E);
            assert_eq!(iter.nth(3).map(Result::ok), Some(Some(0x16)));
            assert!(iter.truncate().is_ok());
        }
        assert_eq!(
            read_fat(&mut cur, fat_type, 0x16).ok(),
            Some(FatValue::EndOfChain)
        );
        assert_eq!(
            read_fat(&mut cur, fat_type, 0x19).ok(),
            Some(FatValue::Free)
        );
        assert_eq!(
            read_fat(&mut cur, fat_type, 0x1A).ok(),
            Some(FatValue::Free)
        );
        // test freeing a chain
        {
            let mut iter =
                ClusterIterator::<&mut S, S::Error, S>::new(&mut cur, fat_type, 0x9, 0x1E);
            assert!(iter.free().is_ok());
        }
        assert_eq!(read_fat(&mut cur, fat_type, 0x9).ok(), Some(FatValue::Free));
        assert_eq!(read_fat(&mut cur, fat_type, 0xA).ok(), Some(FatValue::Free));
        assert_eq!(
            read_fat(&mut cur, fat_type, 0x14).ok(),
            Some(FatValue::Free)
        );
        assert_eq!(
            read_fat(&mut cur, fat_type, 0x15).ok(),
            Some(FatValue::Free)
        );
        assert_eq!(
            read_fat(&mut cur, fat_type, 0x16).ok(),
            Some(FatValue::Free)
        );
    }

    #[test]
    fn corrupt_chains_count_only_actual_transitions() {
        for fat_type in [FatType::Fat12, FatType::Fat16, FatType::Fat32] {
            let mut fat = StdIoWrapper::new(Cursor::new(vec![0u8; 64]));
            // A corrupt link points into an already-free entry.
            write_fat(&mut fat, fat_type, 2, FatValue::Data(3)).unwrap();
            let mut iter = ClusterIterator::<_, std::io::Error, StdIoWrapper<Cursor<Vec<u8>>>>::new(
                &mut fat, fat_type, 2, 14,
            );
            assert_eq!(iter.free().unwrap(), 1);
            assert_eq!((iter.taken(), iter.freed()), (0, 1));
            // Freeing that chain again changes nothing.
            let mut iter = ClusterIterator::<_, std::io::Error, StdIoWrapper<Cursor<Vec<u8>>>>::new(
                &mut fat, fat_type, 2, 14,
            );
            assert_eq!(iter.free().unwrap(), 0);
            assert_eq!(iter.freed(), 0);
            // A cross-link into an already-freed chain cannot count it twice.
            write_fat(&mut fat, fat_type, 4, FatValue::Data(2)).unwrap();
            let mut iter = ClusterIterator::<_, std::io::Error, StdIoWrapper<Cursor<Vec<u8>>>>::new(
                &mut fat, fat_type, 4, 14,
            );
            assert_eq!(iter.free().unwrap(), 1);
            assert_eq!(iter.freed(), 1);
            // Truncating a free cluster takes it, rather than freeing anything.
            let mut iter = ClusterIterator::<_, std::io::Error, StdIoWrapper<Cursor<Vec<u8>>>>::new(
                &mut fat, fat_type, 2, 14,
            );
            assert_eq!(iter.truncate().unwrap(), 0);
            assert_eq!((iter.taken(), iter.freed()), (1, 0));
            let mut iter = ClusterIterator::<_, std::io::Error, StdIoWrapper<Cursor<Vec<u8>>>>::new(
                &mut fat, fat_type, 2, 14,
            );
            assert_eq!(iter.truncate().unwrap(), 0);
            assert_eq!((iter.taken(), iter.freed()), (0, 0));
        }
    }

    #[test]
    fn test_fat12() {
        let fat: Vec<u8> = vec![
            0xF0, 0xFF, 0xFF, 0x03, 0x40, 0x00, 0x05, 0x60, 0x00, 0x07, 0x80, 0x00, 0xFF, 0xAF,
            0x00, 0x14, 0xC0, 0x00, 0x0D, 0xE0, 0x00, 0x0F, 0x00, 0x01, 0x11, 0xF0, 0xFF, 0x00,
            0xF0, 0xFF, 0x15, 0x60, 0x01, 0x19, 0x70, 0xFF, 0xF7, 0xAF, 0x01, 0xFF, 0x0F, 0x00,
            0x00, 0x70, 0xFF, 0x00, 0x00, 0x00,
        ];
        test_fat(
            FatType::Fat12,
            StdIoWrapper::new(Cursor::<Vec<u8>>::new(fat)),
        );
    }

    #[test]
    fn test_fat16() {
        let fat: Vec<u8> = vec![
            0xF0, 0xFF, 0xFF, 0xFF, 0x03, 0x00, 0x04, 0x00, 0x05, 0x00, 0x06, 0x00, 0x07, 0x00,
            0x08, 0x00, 0xFF, 0xFF, 0x0A, 0x00, 0x14, 0x00, 0x0C, 0x00, 0x0D, 0x00, 0x0E, 0x00,
            0x0F, 0x00, 0x10, 0x00, 0x11, 0x00, 0xFF, 0xFF, 0x00, 0x00, 0xFF, 0xFF, 0x15, 0x00,
            0x16, 0x00, 0x19, 0x00, 0xF7, 0xFF, 0xF7, 0xFF, 0x1A, 0x00, 0xFF, 0xFF, 0x00, 0x00,
            0x00, 0x00, 0xF7, 0xFF, 0x00, 0x00, 0x00, 0x00,
        ];
        test_fat(
            FatType::Fat16,
            StdIoWrapper::new(Cursor::<Vec<u8>>::new(fat)),
        );
    }

    #[test]
    fn test_fat32() {
        let fat: Vec<u8> = vec![
            0xF0, 0xFF, 0xFF, 0x0F, 0xFF, 0xFF, 0xFF, 0x0F, 0xFF, 0xFF, 0xFF, 0x0F, 0x04, 0x00,
            0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00,
            0x08, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0x0F, 0x0A, 0x00, 0x00, 0x00, 0x14, 0x00,
            0x00, 0x00, 0x0C, 0x00, 0x00, 0x00, 0x0D, 0x00, 0x00, 0x00, 0x0E, 0x00, 0x00, 0x00,
            0x0F, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x11, 0x00, 0x00, 0x00, 0xFF, 0xFF,
            0xFF, 0x0F, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0x0F, 0x15, 0x00, 0x00, 0x00,
            0x16, 0x00, 0x00, 0x00, 0x19, 0x00, 0x00, 0x00, 0xF7, 0xFF, 0xFF, 0x0F, 0xF7, 0xFF,
            0xFF, 0x0F, 0x1A, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0x0F, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0xF7, 0xFF, 0xFF, 0x0F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ];
        test_fat(
            FatType::Fat32,
            StdIoWrapper::new(Cursor::<Vec<u8>>::new(fat)),
        );
    }
}
