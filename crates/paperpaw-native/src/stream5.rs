//! Relocating a format-5 image without holding its words in memory.
//!
//! A format-4 file's words (those to relocate, then the patches) are read
//! whole into a `Vec` before the image is placed. Format 5 forbids it
//! (`abi/ppn5.md`, "Size caps and memory"): the words are read where they lie
//! in the file, through a buffer of fixed size, at the offsets the header
//! gives. Both lists are sorted, so a piece of the image is served by a binary
//! search for its first word and a short run of sequential reads; no part of
//! the file is read twice in proportion to its size.
//!
//! [`Tail::check`] is [`check_relocations`](crate::check_relocations) for
//! such a file; [`Relocator5`] is [`Relocator`](crate::Relocator) for it, minus
//! the table words that format 5 does not have (no host slot, no families).
//! Nothing here writes flash or runs code.

use core::cell::RefCell;

use crate::header::{CODE_OFFSET, Problem};
use crate::header5::{
    EntryFacts, EntryRefusal, HEADER_LEN, Header5, Host, META_MAX, RawEntry, RecordFacts,
    X4_ENTRY_BYTES,
};
use crate::reloc::PatchKind;

/// A read that did not deliver all its bytes: past the end of the file, or
/// the storage failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadError;

/// Where the bytes of a plugin file come from: read at an offset, all of
/// `buf` or an error.
pub trait Source {
    /// Fills `buf` from the file at `at`.
    fn read_at(&self, at: u64, buf: &mut [u8]) -> Result<(), ReadError>;
}

impl Source for [u8] {
    fn read_at(&self, at: u64, buf: &mut [u8]) -> Result<(), ReadError> {
        let at = usize::try_from(at).map_err(|_| ReadError)?;
        let end = at.checked_add(buf.len()).ok_or(ReadError)?;
        buf.copy_from_slice(self.get(at..end).ok_or(ReadError)?);
        Ok(())
    }
}

/// Why [`Relocator5::check_entry`] refused the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryCheck {
    /// Its bytes could not be read or relocated, or the record is not one
    /// this host reads.
    File(Problem),
    /// The entry disagrees with the header or the record.
    Entry(EntryRefusal),
}

/// Why [`Relocator5::write_image`] stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Written<E> {
    /// The file could not be read or relocated, or its bytes are not the
    /// ones the digest covers ([`Problem::Damaged`]).
    File(Problem),
    /// The sink (the flash) refused a piece.
    Sink(E),
}

/// A [`Source`] that serves the small reads of the lookups from one block
/// of the file held in `block`: a binary search probes a word or a pair at
/// a time, and a probe in the block read last costs no read of the inner
/// source. Reads of a block or more go to it directly. The file is not
/// written while it is read, so the block is never stale.
///
/// On the X4 the inner source is a file of the card (`FatStorage`), where
/// every read costs a sector; how much the block saves is measured on the
/// device (`abi/ppn5.md`, "Size caps and memory"), not assumed.
pub struct Cached<'a, S: Source + ?Sized> {
    inner: &'a S,
    len: u64,
    state: RefCell<CacheState<'a>>,
}

struct CacheState<'a> {
    block: &'a mut [u8],
    /// Where in the file the block's bytes begin, and how many are valid
    /// (0: none).
    start: u64,
    valid: usize,
}

impl<'a, S: Source + ?Sized> Cached<'a, S> {
    /// Reads of `inner`, a file of `len` bytes, through `block` (at least
    /// one byte).
    pub fn new(inner: &'a S, len: u64, block: &'a mut [u8]) -> Self {
        Self {
            inner,
            len,
            state: RefCell::new(CacheState {
                block,
                start: 0,
                valid: 0,
            }),
        }
    }
}

impl<S: Source + ?Sized> core::fmt::Debug for Cached<'_, S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Cached")
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}

impl<S: Source + ?Sized> Source for Cached<'_, S> {
    fn read_at(&self, mut at: u64, mut buf: &mut [u8]) -> Result<(), ReadError> {
        let mut state = self.state.borrow_mut();
        let size = state.block.len();
        while !buf.is_empty() {
            if buf.len() >= size {
                return self.inner.read_at(at, buf);
            }
            let cached = at >= state.start && at - state.start < state.valid as u64;
            if !cached {
                let start = at - at % size as u64;
                let valid = self.len.saturating_sub(start).min(size as u64) as usize;
                if at >= self.len {
                    return Err(ReadError);
                }
                state.valid = 0;
                self.inner.read_at(start, &mut state.block[..valid])?;
                state.start = start;
                state.valid = valid;
            }
            let from = (at - state.start) as usize;
            let take = buf.len().min(state.valid - from);
            let (now, rest) = buf.split_at_mut(take);
            now.copy_from_slice(&state.block[from..from + take]);
            at += take as u64;
            buf = rest;
        }
        Ok(())
    }
}

/// The most patches a format-5 file may list. Patches are the instructions
/// that reach across the hot code's edge (at most 16 KiB of it); their
/// lookups read the file, so the count is bounded rather than the time.
pub const MAX_PATCHES: u32 = 8192;

/// How many words (relocations) or patches one buffer holds.
const RELOCS_PER_READ: u32 = 32;
const PATCHES_PER_READ: u32 = 16;

fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// The words after a format-5 image, in the file.
#[derive(Debug, Clone, Copy)]
pub struct Tail<'a, S: Source + ?Sized> {
    src: &'a S,
    relocs_at: u64,
    relocs: u32,
    patches_at: u64,
    patches: u32,
}

impl<'a, S: Source + ?Sized> Tail<'a, S> {
    /// The tail of the file of `header`, read from `src`.
    pub fn new(header: &Header5, src: &'a S) -> Self {
        Self {
            src,
            relocs_at: header.relocs_at(),
            relocs: header.relocs,
            patches_at: header.patches_at(),
            patches: header.patches,
        }
    }

    fn read(&self, at: u64, buf: &mut [u8]) -> Result<(), Problem> {
        self.src
            .read_at(at, buf)
            .map_err(|ReadError| Problem::Damaged)
    }

    fn reloc(&self, i: u32) -> Result<u32, Problem> {
        let mut word = [0; 4];
        self.read(self.relocs_at + 4 * u64::from(i), &mut word)?;
        Ok(u32::from_le_bytes(word))
    }

    fn patch(&self, i: u32) -> Result<[u32; 2], Problem> {
        let mut bytes = [0; 8];
        self.read(self.patches_at + 8 * u64::from(i), &mut bytes)?;
        Ok([le32(&bytes, 0), le32(&bytes, 4)])
    }

    /// The first word to relocate that is not below `at`.
    fn first_reloc_from(&self, at: u32) -> Result<u32, Problem> {
        let (mut lo, mut hi) = (0, self.relocs);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.reloc(mid)? < at {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        Ok(lo)
    }

    /// The first patch in `lo..hi` for which `keep` is false, the patches
    /// being sorted so that it holds for a prefix.
    fn patch_partition(
        &self,
        lo: u32,
        hi: u32,
        keep: impl Fn([u32; 2]) -> bool,
    ) -> Result<u32, Problem> {
        let (mut lo, mut hi) = (lo, hi);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if keep(self.patch(mid)?) {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        Ok(lo)
    }

    /// How many patches are upper parts: they come first.
    fn high_parts(&self) -> Result<u32, Problem> {
        self.patch_partition(0, self.patches, |p| p[0] < PatchKind::Lo12I.record(0))
    }

    /// What the `auipc` patched at `site` reaches, by the first `hi20`
    /// patches (the upper parts).
    fn target(&self, hi20: u32, site: u32) -> Result<Option<u32>, Problem> {
        let record = PatchKind::Hi20.record(site);
        let at = self.patch_partition(0, hi20, |p| p[0] < record)?;
        if at >= hi20 {
            return Ok(None);
        }
        let found = self.patch(at)?;
        Ok((found[0] == record).then_some(found[1]))
    }

    /// Checks the words as [`check_relocations`](crate::check_relocations)
    /// does, reading them once: the words to relocate increasing, each a
    /// whole word of the image outside the hot code; the patches in order,
    /// each at an instruction in the code, a lower part's `auipc` patched
    /// too; no more than [`MAX_PATCHES`].
    pub fn check(&self, header: &Header5) -> Result<(), Problem> {
        if self.patches > MAX_PATCHES {
            return Err(Problem::Relocation);
        }
        let mut buf = [0u8; 4 * RELOCS_PER_READ as usize];
        let mut next = 0u32;
        let mut i = 0;
        while i < self.relocs {
            let n = (self.relocs - i).min(RELOCS_PER_READ) as usize;
            self.read(self.relocs_at + 4 * u64::from(i), &mut buf[..4 * n])?;
            for k in 0..n {
                let at = le32(&buf, 4 * k);
                let fits = at >= next
                    && at.is_multiple_of(4)
                    && at.checked_add(4).is_some_and(|end| end <= header.image_len)
                    && !header.in_hot(at);
                if !fits {
                    return Err(Problem::Relocation);
                }
                next = at + 4;
            }
            i += n as u32;
        }
        let hi20 = self.high_parts()?;
        let mut buf = [0u8; 8 * PATCHES_PER_READ as usize];
        let mut last = None;
        let mut i = 0;
        while i < self.patches {
            let n = (self.patches - i).min(PATCHES_PER_READ) as usize;
            self.read(self.patches_at + 8 * u64::from(i), &mut buf[..8 * n])?;
            for k in 0..n {
                let (record, reference) = (le32(&buf, 8 * k), le32(&buf, 8 * k + 4));
                let (kind, site) = PatchKind::of(record).ok_or(Problem::Relocation)?;
                let fits = last.is_none_or(|last| record > last)
                    && site >= header.code_at
                    && site.is_multiple_of(2)
                    && site
                        .checked_add(4)
                        .is_some_and(|end| end <= header.image_len)
                    && match kind {
                        PatchKind::Hi20 => reference < header.image_len,
                        PatchKind::Lo12I | PatchKind::Lo12S => {
                            self.target(hi20, reference)?.is_some()
                        }
                    };
                if !fits {
                    return Err(Problem::Relocation);
                }
                last = Some(record);
            }
            i += n as u32;
        }
        Ok(())
    }
}

/// Relocates a format-5 image, a piece at a time, for the data address it is
/// placed at, and for where its hot code runs; the words are read from the
/// file as each piece needs them.
#[derive(Debug)]
pub struct Relocator5<'a, S: Source + ?Sized> {
    header: &'a Header5,
    tail: Tail<'a, S>,
    /// How many patches are upper parts, and how many are lower parts of
    /// I-type instructions (the rest are S-type).
    hi20: u32,
    lo12i: u32,
    base: u32,
    delta: u32,
    hot: u32,
}

impl<'a, S: Source + ?Sized> Relocator5<'a, S> {
    /// For the image of `header` placed at data address `base`, its words
    /// read from `src` (checked here, once). Its hot code runs in place
    /// unless [`Relocator5::hot_at`] says otherwise.
    pub fn new(header: &'a Header5, src: &'a S, base: u32) -> Result<Self, Problem> {
        let tail = Tail::new(header, src);
        tail.check(header)?;
        if !base.is_multiple_of(header.align)
            || base
                .checked_add(header.image_len)
                .and_then(|end| end.checked_add(CODE_OFFSET))
                .is_none()
        {
            return Err(Problem::Malformed);
        }
        let hi20 = tail.high_parts()?;
        let lo12i = tail
            .patch_partition(hi20, header.patches, |p| p[0] < PatchKind::Lo12S.record(0))?
            - hi20;
        Ok(Self {
            header,
            tail,
            hi20,
            lo12i,
            base,
            delta: base.wrapping_sub(header.base),
            hot: base + CODE_OFFSET + header.hot_at,
        })
    }

    /// Runs the hot code at instruction address `hot` (a multiple of 4), not
    /// in place: the device copies it there once relocated. Without hot code
    /// it changes nothing.
    pub fn hot_at(mut self, hot: u32) -> Result<Self, Problem> {
        if self.header.hot_len == 0 {
            return Ok(self);
        }
        if !hot.is_multiple_of(4) || hot.checked_add(self.header.hot_len).is_none() {
            return Err(Problem::Malformed);
        }
        self.hot = hot;
        Ok(self)
    }

    /// Everything that can refuse the file before a byte is written (step 8
    /// and the relocation's own checks): reads the file once, `buf` at a
    /// time, feeds the digest with the header and all after it, and runs
    /// [`Relocator5::apply`] on each piece as the device will, but on the
    /// piece alone, so a word that cannot be relocated refuses here. Then
    /// the digest must be the header's ([`Header5::holds`]).
    ///
    /// `buf` is the only memory used, at least 4 bytes (a multiple of 4 is
    /// used). The device passes one sector. A file whose bytes were changed
    /// is `Damaged` or `Relocation`, whichever its changed bytes show first.
    pub fn verify(&self, buf: &mut [u8]) -> Result<(), Problem> {
        let step = buf.len() & !3;
        if step == 0 {
            return Err(Problem::Malformed);
        }
        let mut sha = self.header.digest();
        let mut done = 0u32;
        while done < self.header.image_len {
            let len = (self.header.image_len - done).min(step as u32) as usize;
            let piece = &mut buf[..len];
            self.tail.read(HEADER_LEN as u64 + u64::from(done), piece)?;
            sha.update(piece);
            self.apply(done, piece)?;
            done += len as u32;
        }
        let end = self.header.file_len();
        let mut at = self.header.relocs_at();
        while at < end {
            let len = (end - at).min(step as u64) as usize;
            self.tail.read(at, &mut buf[..len])?;
            sha.update(&buf[..len]);
            at += len as u64;
        }
        if self.header.holds(sha) {
            Ok(())
        } else {
            Err(Problem::Damaged)
        }
    }

    /// The write pass: reads the image once more, `buf` at a time, hashes
    /// **each piece as read** (before it is relocated), relocates it as
    /// [`Relocator5::apply`] does and hands it to `sink` with its image
    /// offset, padded with `0xFF` to whole words. After the image the lists
    /// are hashed once more, in order, and the digest must be the header's
    /// ([`Header5::holds`]): the bytes that went to the sink are therefore
    /// the bytes the header covers, even where the source answers
    /// otherwise than it did for [`Relocator5::verify`]. A mismatch is
    /// `Damaged`, and the caller takes back what the sink was given.
    ///
    /// The lookups [`Relocator5::apply`] makes into the lists are separate
    /// reads and not covered by that digest: a source that lies on them
    /// alone can only move a relocation to another valid word.
    ///
    /// `buf` is a multiple of 4 bytes, at least 4.
    pub fn write_image<E>(
        &self,
        buf: &mut [u8],
        mut sink: impl FnMut(u32, &[u8]) -> Result<(), E>,
    ) -> Result<(), Written<E>> {
        let step = buf.len() & !3;
        if step == 0 {
            return Err(Written::File(Problem::Malformed));
        }
        let mut sha = self.header.digest();
        let mut done = 0u32;
        while done < self.header.image_len {
            let len = (self.header.image_len - done).min(step as u32) as usize;
            let words = len.next_multiple_of(4);
            self.tail
                .read(HEADER_LEN as u64 + u64::from(done), &mut buf[..len])
                .map_err(Written::File)?;
            sha.update(&buf[..len]);
            self.apply(done, &mut buf[..len]).map_err(Written::File)?;
            buf[len..words].fill(0xFF);
            sink(done, &buf[..words]).map_err(Written::Sink)?;
            done += len as u32;
        }
        let end = self.header.file_len();
        let mut at = self.header.relocs_at();
        while at < end {
            let len = (end - at).min(step as u64) as usize;
            self.tail.read(at, &mut buf[..len]).map_err(Written::File)?;
            sha.update(&buf[..len]);
            at += len as u64;
        }
        if self.header.holds(sha) {
            Ok(())
        } else {
            Err(Written::File(Problem::Damaged))
        }
    }

    /// Step 9 on the image **as it will be after relocation**, built in RAM
    /// from the file, before anything is written: the entry and the metadata
    /// view are read from the file and relocated for this placement
    /// ([`Relocator5::apply`] on each), then given to
    /// [`Header5::check_entry`] with the base the image is placed at and
    /// `make` mapped to its image offset (flash or hot arena). A file
    /// whose entry is refused is therefore refused before a sector is
    /// erased, on every start alike, and wears nothing.
    ///
    /// `scratch` holds the record twice (`2 * meta_len` bytes, at most
    /// [`META_MAX`] each); the device passes the sector it verifies with.
    pub fn check_entry(&self, host: &Host, scratch: &mut [u8]) -> Result<(), EntryCheck> {
        let header = self.header;
        let len = header.meta_len as usize;
        if len > META_MAX as usize || scratch.len() < 2 * len {
            return Err(EntryCheck::File(Problem::Malformed));
        }
        let (record, view) = scratch[..2 * len].split_at_mut(len);
        let meta_at = HEADER_LEN as u64 + u64::from(header.meta_at);
        self.tail.read(meta_at, record).map_err(EntryCheck::File)?;
        view.copy_from_slice(record);
        self.apply(header.meta_at, view).map_err(EntryCheck::File)?;
        let mut entry = [0u8; X4_ENTRY_BYTES as usize];
        let entry_at = HEADER_LEN as u64 + u64::from(header.entry);
        self.tail
            .read(entry_at, &mut entry)
            .map_err(EntryCheck::File)?;
        self.apply(header.entry, &mut entry)
            .map_err(EntryCheck::File)?;
        let raw = RawEntry::read(&entry).ok_or(EntryCheck::File(Problem::Damaged))?;
        let facts = RecordFacts::read(record).ok_or(EntryCheck::File(Problem::Damaged))?;
        let found = EntryFacts {
            size: raw.size,
            abi_major: raw.abi_major,
            needs_minor: raw.needs_minor,
            metadata_ptr: raw.metadata_ptr,
            metadata_len: raw.metadata_len,
            metadata: view,
            make_at: header.make_offset(raw.make, self.base, self.hot),
        };
        header
            .check_entry(self.base, host, &facts, record, &found)
            .map_err(EntryCheck::Entry)
    }

    fn hot_moved(&self) -> bool {
        self.hot != self.base + CODE_OFFSET + self.header.hot_at
    }

    /// Relocates `piece`, the image's bytes from offset `at` on (a multiple
    /// of 4; every piece but the image's last a whole number of words), as
    /// [`Relocator::apply`](crate::Relocator::apply) does.
    pub fn apply(&self, at: u32, piece: &mut [u8]) -> Result<(), Problem> {
        if !at.is_multiple_of(4) {
            return Err(Problem::Relocation);
        }
        let end = at.saturating_add(piece.len() as u32);
        self.words(at, end, piece)?;
        self.patch(at, end, piece)
    }

    fn words(&self, at: u32, end: u32, piece: &mut [u8]) -> Result<(), Problem> {
        let mut buf = [0u8; 4 * RELOCS_PER_READ as usize];
        let mut next = at;
        let mut i = self.tail.first_reloc_from(at)?;
        while i < self.header.relocs {
            let n = (self.header.relocs - i).min(RELOCS_PER_READ) as usize;
            self.tail
                .read(self.tail.relocs_at + 4 * u64::from(i), &mut buf[..4 * n])?;
            for k in 0..n {
                let word = le32(&buf, 4 * k);
                if word >= end {
                    return Ok(());
                }
                // The list was checked by `Tail::check`, but is read again
                // here: a source that serves other bytes the second time
                // (a card swapped, a flaky read) is refused, never trusted.
                if word < next || !word.is_multiple_of(4) {
                    return Err(Problem::Relocation);
                }
                next = word.checked_add(4).ok_or(Problem::Relocation)?;
                let at_word = (word - at) as usize;
                let Some(bytes) = piece.get_mut(at_word..at_word + 4) else {
                    return Err(Problem::Relocation);
                };
                let value = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                let moved = self.moved(value).ok_or(Problem::Relocation)?;
                bytes.copy_from_slice(&moved.to_le_bytes());
            }
            i += n as u32;
        }
        Ok(())
    }

    /// Where the address `value` in the image as linked lies once placed
    /// (`None` if it is no address in the image, by data or by code).
    fn moved(&self, value: u32) -> Option<u32> {
        let len = self.header.image_len;
        let code = value.wrapping_sub(self.header.base + CODE_OFFSET);
        let data = value.wrapping_sub(self.header.base);
        if code <= len {
            Some(if self.header.in_hot(code) {
                self.runs_at(code)
            } else {
                value.wrapping_add(self.delta)
            })
        } else if data <= len && !(self.header.in_hot(data) && self.hot_moved()) {
            Some(value.wrapping_add(self.delta))
        } else {
            None
        }
    }

    /// The address image offset `at` is reached at once placed.
    fn runs_at(&self, at: u32) -> u32 {
        if at < self.header.code_at {
            self.base + at
        } else if self.header.in_hot(at) {
            self.hot + (at - self.header.hot_at)
        } else {
            self.base + CODE_OFFSET + at
        }
    }

    /// Writes the patches whose instructions lie in `piece`, from `at` to
    /// `end`, a byte at a time.
    fn patch(&self, at: u32, end: u32, piece: &mut [u8]) -> Result<(), Problem> {
        let blocks = [
            (0, self.hi20),
            (self.hi20, self.hi20 + self.lo12i),
            (self.hi20 + self.lo12i, self.header.patches),
        ];
        let mut buf = [0u8; 8 * PATCHES_PER_READ as usize];
        for (from, to) in blocks {
            let mut i = self
                .tail
                .patch_partition(from, to, |p| (p[0] & 0x0FFF_FFFF) + 4 <= at)?;
            while i < to {
                let n = (to - i).min(PATCHES_PER_READ) as usize;
                self.tail
                    .read(self.tail.patches_at + 8 * u64::from(i), &mut buf[..8 * n])?;
                for k in 0..n {
                    let (record, reference) = (le32(&buf, 8 * k), le32(&buf, 8 * k + 4));
                    let (kind, site) = PatchKind::of(record).ok_or(Problem::Relocation)?;
                    if site >= end {
                        break;
                    }
                    self.write_patch(kind, site, reference, at, piece)?;
                }
                // The block's remaining patches lie beyond this piece when
                // the loop above broke out of it.
                let last = le32(&buf, 8 * (n - 1));
                if PatchKind::of(last).is_none_or(|(_, site)| site >= end) {
                    break;
                }
                i += n as u32;
            }
        }
        Ok(())
    }

    fn write_patch(
        &self,
        kind: PatchKind,
        site: u32,
        reference: u32,
        at: u32,
        piece: &mut [u8],
    ) -> Result<(), Problem> {
        let (from, to) = match kind {
            PatchKind::Hi20 => (site, reference),
            PatchKind::Lo12I | PatchKind::Lo12S => (
                reference,
                self.tail
                    .target(self.hi20, reference)?
                    .ok_or(Problem::Relocation)?,
            ),
        };
        // Read again since `Tail::check`: both ends must still be in the image.
        let image_len = self.header.image_len;
        if from >= image_len || to >= image_len {
            return Err(Problem::Relocation);
        }
        let distance = self.runs_at(to).wrapping_sub(self.runs_at(from));
        let bits = kind.bits(distance).to_le_bytes();
        let kept = kind.kept().to_le_bytes();
        for i in 0..4 {
            let Some(byte) = site
                .checked_add(i)
                .and_then(|byte_at| byte_at.checked_sub(at))
                .and_then(|j| piece.get_mut(j as usize))
            else {
                continue;
            };
            *byte = (*byte & kept[i as usize]) | bits[i as usize];
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
