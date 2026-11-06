//! Patterns read from a card: the sparse index and the exceptions are kept,
//! blocks of patterns are read on demand through a small cache.
//!
//! The table is untrusted for the reader's whole lifetime. It is checked
//! once ([`check_source`]); after that every read is bounded by the size
//! seen at open, and anything unexpected (a failed or short read, an entry
//! that does not fit its block) makes the reader fail for good: it
//! hyphenates nothing from then on.
//!
//! # Memory and reads (measured with the 13 shipped tables)
//!
//! Kept: the index (4 bytes per block), the fences (4 bytes per block: the
//! first four bytes of each block's first key, so a search is decided in
//! memory and reads only the one block it needs), the exceptions whole, and
//! `cache` slots of the table's largest block each.
//!
//! | table | file | index+fences | exceptions | blocks | max / avg block | heap, cache 4 | heap, cache 8 |
//! |-------|-----:|-----:|-----:|-----:|-----:|-----:|-----:|
//! | de | 351,145 | 18,360 | 0 | 2,295 | 214 / 148 | 19,376 | 20,392 |
//! | nb | 251,949 | 13,728 | 17 | 1,716 | 184 / 142 | 14,641 | 15,537 |
//! | ru (most exceptions) | 74,905 | 3,512 | 3,192 | 439 | 220 / 159 | 7,744 | 8,784 |
//! | en | 41,460 | 1,352 | 1,260 | 169 | 309 / 129 | 3,457 | 4,273 |
//! | it (smallest) | 2,341 | 192 | 120 | 24 | 145 / 92 | 932 | 1,672 |
//!
//! The retained heap is the file's pattern data replaced by 8 bytes per
//! block: at most about 20 KB for the largest table (`de`), against 351 KB
//! on the card. The caps are those of the shared check: [`MAX_BLOCKS`] (4,096,
//! the most shipped is 2,295, `de`), [`MAX_BLOCK_BYTES`] (512, the largest
//! shipped is 346, `uk`) and [`MAX_EXCEPTION_BYTES`] (8 KiB, the largest shipped
//! is 3,192, `ru`); larger tables are refused at open with
//! [`TableError::TooLarge`], before anything grows with them. With them the
//! retained heap of any table is at most: index and fences 32 KiB, exceptions
//! 8 KiB, and the cache, one [`MAX_BLOCK_BYTES`] slot per cached block. That is
//! about 44 KiB with [`CACHE_BLOCKS`] (8) and about 72 KiB with
//! [`MAX_CACHE_BLOCKS`] (64). On the X4 (about 256 KiB of heap) the caller
//! passes [`CACHE_BLOCKS`] or fewer: the worst case is then 44 KiB, against
//! about 20 KB measured for `de`, the largest shipped table.
//!
//! Reads: opening reads the fences in sector-aligned 512-byte windows, so
//! each sector of the patterns about once, plus a few reads for the check,
//! the index and the exceptions, all in file order: 708 reads for `de` (686
//! sectors; one 5-byte read per block made it 2,305), 10 for `it`
//! (`tests/stored.rs`, `opening_reads_each_sector_of_the_patterns_about_once`).
//! The card reads whole sectors either way; what this saves is calls.
//! Hyphenating a word looks up every substring of it: pseudo-words of 4 to
//! 14 letters made from pattern keys (not running text: a worst case, as
//! real words repeat and share blocks) cost, with cache 4, about 26 reads a
//! word for `de` (7,800 per 300-word page, cold or warm alike, worst word
//! 59), 19 for `en` (5,800 per page, worst 40) and 7 for `pt` (2,000 per
//! page). Cache 8 instead of 4 saves 2 % for the large tables (`de`, `nb`)
//! and 10 to 25 % for the small ones (`da`, `fr`, `pl`, `it`, `pt`): the
//! cache pays off only when it holds a good part of the table. Without the fences a
//! word cost about 290 reads for `de` (the binary search read a block per
//! step); that is why they are kept.
//!
//! What this does not say: the time of a read on the X4 (a FAT read of 5 to
//! 350 bytes through the sector cache). That measurement waits for the owner
//! (a device); the numbers above are read counts from `tests/stored_budget.rs`
//! (`cargo test -p paperpaw-hyphen --features kernel --test stored_budget --
//! --nocapture`). A page-level word cache in the caller would cut reads more
//! than a bigger block cache.

use alloc::vec::Vec;
use core::cmp::Ordering;
use core::fmt;

use crate::check::{CheckError, Layout, Source, TableError, check_source};
use crate::{
    Access, BLOCK, HEADER, MAX_MINIMUM, Points, Rules, entry_at, exception_in, hyphenate_with,
};
#[cfg(doc)]
use crate::{MAX_BLOCK_BYTES, MAX_BLOCKS, MAX_EXCEPTION_BYTES};

/// Blocks kept in memory when the caller has no better number, and the most
/// an X4 caller should pass: the reader's worst case is then about 44 KiB.
pub const CACHE_BLOCKS: usize = 8;

/// The most blocks that can be kept (a worst case of about 72 KiB).
pub const MAX_CACHE_BLOCKS: usize = 64;

/// Bytes of buffer for the check and for copying; freed after opening.
const SCRATCH: usize = 512;

/// Why a table could not be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpenError<E> {
    /// Reading failed, or the table is not sound.
    Check(CheckError<E>),
    /// There is not enough memory.
    NoMemory,
}

impl<E> From<TableError> for OpenError<E> {
    fn from(error: TableError) -> Self {
        Self::Check(CheckError::Table(error))
    }
}

/// A read that did not happen.
enum Unread<E> {
    /// Past the size seen at open.
    Bounds,
    /// No room for what was to be kept.
    NoMemory,
    Io(E),
}

struct Slot {
    /// Which block, `usize::MAX` for none.
    block: usize,
    /// When it was used last.
    stamp: u64,
    data: Vec<u8>,
}

/// A compiled pattern table on a card: the same hyphenation as
/// [`Patterns`](crate::Patterns), with the sparse index and the exceptions
/// in memory and a few blocks of patterns cached.
pub struct StoredPatterns<S: Source> {
    src: S,
    identity: u64,
    len: u64,
    rules: Rules,
    entries_at: u64,
    patterns_len: u32,
    index: Vec<u32>,
    /// The first four bytes of each block's first key (zero padded): most
    /// searches are decided here, without reading a block.
    fences: Vec<[u8; 4]>,
    exceptions: Vec<u8>,
    exception_count: usize,
    slots: Vec<Slot>,
    clock: u64,
    failed: bool,
    reads: u64,
}

impl<S: Source> fmt::Debug for StoredPatterns<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredPatterns")
            .field("bytes", &self.len)
            .field("blocks", &self.index.len())
            .field("failed", &self.failed)
            .finish_non_exhaustive()
    }
}

fn reserved<T>(n: usize) -> Result<Vec<T>, TryReserveFailed> {
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| TryReserveFailed)?;
    Ok(v)
}

struct TryReserveFailed;

impl<E> From<TryReserveFailed> for OpenError<E> {
    fn from(_: TryReserveFailed) -> Self {
        Self::NoMemory
    }
}

fn raw_read<S: Source>(
    src: &mut S,
    len: u64,
    at: u64,
    buf: &mut [u8],
) -> Result<(), Unread<S::Error>> {
    match at.checked_add(buf.len() as u64) {
        Some(end) if end <= len => src.read_at(at, buf).map_err(Unread::Io),
        _ => Err(Unread::Bounds),
    }
}

impl<S: Source> StoredPatterns<S> {
    /// Opens the table in `src`: checks it ([`check_source`]), then keeps
    /// its index, its exceptions and room for `cache_blocks` blocks (1 to
    /// [`MAX_CACHE_BLOCKS`]; [`CACHE_BLOCKS`] is a start). Memory is
    /// reserved fallibly: [`OpenError::NoMemory`], never an abort.
    ///
    /// `identity` is any number that tells the file from another or from a
    /// newer one (the kernel derives it from the size and the first
    /// cluster); see [`still_valid`](Self::still_valid).
    pub fn open(
        mut src: S,
        identity: u64,
        cache_blocks: usize,
    ) -> Result<Self, OpenError<S::Error>> {
        let mut scratch = reserved::<u8>(SCRATCH)?;
        scratch.resize(SCRATCH, 0);
        let layout = check_source(&mut src, &mut scratch).map_err(OpenError::Check)?;
        // The shared limits hold: at most MAX_BLOCKS blocks of MAX_BLOCK_BYTES,
        // and MAX_EXCEPTION_BYTES of exceptions (`TableError::TooLarge`).
        let len = layout.table_len();
        let io = |e: Unread<S::Error>| match e {
            Unread::Bounds => OpenError::from(TableError::Header),
            Unread::NoMemory => OpenError::NoMemory,
            Unread::Io(e) => OpenError::Check(CheckError::Source(e)),
        };

        // The header again: a table that changed since the check is refused.
        raw_read(&mut src, len, 0, &mut scratch[..HEADER]).map_err(io)?;
        let same = |at: usize, now: u64| {
            let mut word = [0u8; 4];
            word.copy_from_slice(&scratch[at..at + 4]);
            u64::from(u32::from_le_bytes(word)) == now
        };
        if !same(12, (layout.blocks as usize) as u64)
            || !same(16, u64::from(layout.exceptions_at))
            || !same(20, (layout.exceptions as usize) as u64)
        {
            return Err(TableError::Header.into());
        }
        let rules = Rules {
            left_min: usize::from(scratch[5]).max(1),
            right_min: usize::from(scratch[6]).max(1),
            max_len: usize::from(scratch[7]),
        };

        let patterns_len = (u64::from(layout.exceptions_at) - layout.entries_at()) as u32;
        let index = Self::read_index(&mut src, &layout, &mut scratch).map_err(io)?;
        let fences = Self::read_fences(&mut src, &layout, &index, &mut scratch).map_err(io)?;
        let mut exceptions = reserved::<u8>(u64::from(layout.exceptions_len) as usize)?;
        exceptions.resize(u64::from(layout.exceptions_len) as usize, 0);
        raw_read(
            &mut src,
            len,
            u64::from(layout.exceptions_at),
            &mut exceptions,
        )
        .map_err(io)?;

        let cache_blocks = cache_blocks.clamp(1, MAX_CACHE_BLOCKS);
        let mut slots = reserved::<Slot>(cache_blocks)?;
        if (layout.blocks as usize) > 0 {
            for _ in 0..cache_blocks {
                slots.push(Slot {
                    block: usize::MAX,
                    stamp: 0,
                    data: reserved::<u8>(layout.largest_block as usize)?,
                });
            }
        }
        Ok(Self {
            src,
            identity,
            len,
            rules,
            entries_at: layout.entries_at(),
            patterns_len,
            index,
            fences,
            exceptions,
            exception_count: (layout.exceptions as usize),
            slots,
            clock: 0,
            failed: false,
            reads: 0,
        })
    }

    /// The index, read again and checked again as it is copied.
    fn read_index(
        src: &mut S,
        layout: &Layout,
        scratch: &mut [u8],
    ) -> Result<Vec<u32>, Unread<S::Error>> {
        let blocks = layout.blocks as usize;
        let mut index = reserved::<u32>(blocks).map_err(|_| Unread::NoMemory)?;
        let window = scratch.len() & !3;
        let patterns = u64::from(layout.exceptions_at) - layout.entries_at();
        let (mut pos, mut left) = (HEADER as u64, blocks * 4);
        while left > 0 {
            let n = left.min(window);
            raw_read(src, layout.table_len(), pos, &mut scratch[..n])?;
            for chunk in scratch[..n].as_chunks::<4>().0 {
                let offset = u32::from_le_bytes(*chunk);
                let ascending = index.last().is_none_or(|&before| offset > before);
                let size = offset - index.last().copied().unwrap_or(offset);
                if !ascending
                    || u64::from(offset) >= patterns
                    || size as usize > (layout.largest_block as usize)
                {
                    return Err(Unread::Bounds);
                }
                index.push(offset);
            }
            pos += n as u64;
            left -= n;
        }
        Ok(index)
    }

    /// The start of each block's first key. The keys are read in windows of
    /// `scratch` aligned to its length (a card sector for 512): one read holds
    /// the starts of all blocks that begin in it, so opening reads each sector
    /// of the patterns about once instead of making one small read per block.
    fn read_fences(
        src: &mut S,
        layout: &Layout,
        index: &[u32],
        scratch: &mut [u8],
    ) -> Result<Vec<[u8; 4]>, Unread<S::Error>> {
        let mut fences = reserved::<[u8; 4]>(index.len()).map_err(|_| Unread::NoMemory)?;
        let entries_at = layout.entries_at();
        let patterns_end = u64::from(layout.exceptions_at);
        let patterns = patterns_end - entries_at;
        let size = scratch.len() as u64;
        let (mut window_at, mut window_len) = (0u64, 0u64);
        for (b, &start) in index.iter().enumerate() {
            let end = index.get(b + 1).map_or(patterns, |&next| u64::from(next));
            let n = (end - u64::from(start)).min(5);
            let at = entries_at + u64::from(start);
            if at < window_at || at + n > window_at + window_len {
                let aligned = at - at % size;
                window_at = if aligned + size >= at + n {
                    aligned
                } else {
                    at
                };
                // The patterns end at or after `at + n`, so the window holds it.
                window_len = (patterns_end - window_at).min(size);
                let window = scratch
                    .get_mut(..usize::try_from(window_len).map_err(|_| Unread::Bounds)?)
                    .ok_or(Unread::Bounds)?;
                raw_read(src, layout.table_len(), window_at, window)?;
            }
            // Both below `size`, which is a `usize`.
            let from = (at - window_at) as usize;
            let buf = scratch.get(from..from + n as usize).ok_or(Unread::Bounds)?;
            let key = buf.get(1..1 + usize::from(*buf.first().ok_or(Unread::Bounds)?).min(4));
            let key = key.ok_or(Unread::Bounds)?;
            // Zero is the padding: a key with it would not compare right.
            if key.contains(&0) {
                return Err(Unread::Bounds);
            }
            let mut fence = [0u8; 4];
            fence[..key.len()].copy_from_slice(key);
            fences.push(fence);
        }
        Ok(fences)
    }

    /// Minimum characters before and after a hyphen: those of the table,
    /// or those given to [`with_minimums`](Self::with_minimums).
    pub fn minimums(&self) -> (usize, usize) {
        (self.rules.left_min, self.rules.right_min)
    }

    /// These patterns with other minimums, as
    /// [`Patterns::with_minimums`](crate::Patterns::with_minimums).
    #[must_use]
    pub fn with_minimums(mut self, left: usize, right: usize) -> Self {
        if left > 0 {
            self.rules.left_min = left.min(MAX_MINIMUM);
        }
        if right > 0 {
            self.rules.right_min = right.min(MAX_MINIMUM);
        }
        self
    }

    /// Whether reading failed once: the reader hyphenates nothing any more
    /// and should be dropped (and perhaps opened again).
    pub fn is_failed(&self) -> bool {
        self.failed
    }

    /// Whether the file is still the one that was opened: `current_identity`
    /// and `current_len` are what the kernel sees now, in the way it made
    /// the identity given to [`open`](Self::open). `false` means the file was
    /// replaced: drop the reader and open the new file. Cheap: no read.
    pub fn still_valid(&self, current_identity: u64, current_len: u64) -> bool {
        current_identity == self.identity && current_len == self.len
    }

    /// Reads from the source since it was opened (for measuring).
    pub fn reads(&self) -> u64 {
        self.reads
    }

    /// Bytes of heap the reader holds, not counting the source.
    pub fn heap_bytes(&self) -> usize {
        self.index.capacity() * 4
            + self.fences.capacity() * 4
            + self.exceptions.capacity()
            + self.slots.capacity() * core::mem::size_of::<Slot>()
            + self.slots.iter().map(|s| s.data.capacity()).sum::<usize>()
    }

    /// As [`Patterns::hyphenate`](crate::Patterns::hyphenate). A word is
    /// left whole when the table cannot be read ([`is_failed`](Self::is_failed)).
    pub fn hyphenate(&mut self, word: &str, out: &mut dyn FnMut(usize)) {
        if !self.failed {
            let rules = self.rules;
            hyphenate_with(self, rules, word, out);
        }
    }

    /// Reads, or fails for good.
    fn fetch(&mut self, at: u64, buf: &mut [u8]) -> bool {
        self.reads += 1;
        if raw_read(&mut self.src, self.len, at, buf).is_err() {
            self.failed = true;
        }
        !self.failed
    }

    fn damaged<T>(&mut self) -> Option<T> {
        self.failed = true;
        None
    }

    /// The slot that holds block `b`, least recently used first out.
    fn load(&mut self, b: usize) -> Option<usize> {
        if self.failed {
            return None;
        }
        self.clock += 1;
        if let Some(i) = self.slots.iter().position(|s| s.block == b) {
            self.slots[i].stamp = self.clock;
            return Some(i);
        }
        let victim = (0..self.slots.len()).min_by_key(|&i| self.slots[i].stamp)?;
        let start = u64::from(*self.index.get(b)?);
        let end = match self.index.get(b + 1) {
            Some(&next) => u64::from(next),
            None => u64::from(self.patterns_len),
        };
        let size = usize::try_from(end.checked_sub(start)?).ok()?;
        let mut data = core::mem::take(&mut self.slots[victim].data);
        self.slots[victim].block = usize::MAX;
        if size > data.capacity() {
            self.slots[victim].data = data;
            return self.damaged();
        }
        data.clear();
        data.resize(size, 0);
        let ok = self.fetch(self.entries_at + start, &mut data);
        self.slots[victim].data = data;
        if !ok {
            return None;
        }
        self.slots[victim].block = b;
        self.slots[victim].stamp = self.clock;
        Some(victim)
    }

    /// Whether the first key of block `b` is at most `key`.
    fn first_key_le(&mut self, b: usize, key: &[u8]) -> Option<bool> {
        if let Some(fence) = self.fences.get(b) {
            let head = &key[..key.len().min(4)];
            if !head.contains(&0) {
                let mut padded = [0u8; 4];
                padded[..head.len()].copy_from_slice(head);
                match fence.cmp(&padded) {
                    Ordering::Less => return Some(true),
                    Ordering::Greater => return Some(false),
                    Ordering::Equal if key.len() < 4 => return Some(true),
                    Ordering::Equal => {}
                }
            }
        }
        let slot = self.load(b)?;
        match entry_at(&self.slots[slot].data, 0) {
            Some((first, ..)) => Some(first <= key),
            None => self.damaged(),
        }
    }

    /// Whether the first key of block `b` starts with `key`.
    fn first_key_starts_with(&mut self, b: usize, key: &[u8]) -> Option<bool> {
        if let (Some(fence), true) = (self.fences.get(b), key.len() <= 4 && !key.contains(&0)) {
            return Some(fence.starts_with(key));
        }
        let slot = self.load(b)?;
        match entry_at(&self.slots[slot].data, 0) {
            Some((first, ..)) => Some(first.starts_with(key)),
            None => self.damaged(),
        }
    }

    /// [`Access::lookup`] on the cached blocks, `None` once failed.
    fn find(&mut self, key: &[u8], points: &mut Points) -> Option<(bool, bool)> {
        let blocks = self.index.len();
        // The last block whose first key is not greater than `key`.
        let (mut low, mut high) = (0, blocks);
        while low < high {
            let mid = (low + high) / 2;
            if self.first_key_le(mid, key)? {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        let Some(block) = low.checked_sub(1) else {
            return Some((false, true));
        };
        let last = block + 1 == blocks;
        let slot = self.load(block)?;
        let mut at = 0;
        for entry in 0..BLOCK {
            let data = &self.slots[slot].data;
            let Some((k, found, next)) = entry_at(data, at) else {
                return self.damaged();
            };
            match k.cmp(key) {
                Ordering::Equal => {
                    let n = found.len().min(points.len());
                    points[..n].copy_from_slice(&found[..n]);
                    let longer = if next < data.len() {
                        match entry_at(data, next) {
                            Some((after, ..)) => after.starts_with(key),
                            None => return self.damaged(),
                        }
                    } else if last {
                        false
                    } else {
                        self.first_key_starts_with(block + 1, key)?
                    };
                    return Some((true, longer));
                }
                Ordering::Greater => return Some((false, k.starts_with(key))),
                Ordering::Less => at = next,
            }
            if at >= data.len() {
                // The table's last pattern; or the end of a block, which
                // has 16 patterns, else the index does not fit the patterns.
                return match (last, entry + 1 == BLOCK) {
                    (true, _) => Some((false, false)),
                    (false, true) => Some((false, true)),
                    (false, false) => self.damaged(),
                };
            }
        }
        Some((false, true))
    }
}

impl<S: Source> Access for StoredPatterns<S> {
    fn lookup(&mut self, key: &[u8], points: &mut Points) -> (bool, bool) {
        if self.index.is_empty() {
            return (false, true);
        }
        self.find(key, points).unwrap_or((false, false))
    }

    fn exception(&mut self, word: &[char]) -> Option<Vec<usize>> {
        match exception_in(&self.exceptions, self.exception_count, word) {
            Ok(found) => found,
            Err(_) => self.damaged(),
        }
    }

    fn failed(&self) -> bool {
        self.failed
    }
}
