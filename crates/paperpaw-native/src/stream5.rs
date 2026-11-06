//! Relocating a format-5 image without holding its words in memory.
//!
//! A format-4 file's words (those to relocate, then the patches) are read
//! whole into a `Vec` before the image is placed. Format 5 forbids it
//! (`abi/ppn5.md`, "Size caps and memory"): the words are read where they lie
//! in the file, through buffers of fixed size, at the offsets the header
//! gives. Both lists are sorted by the image offset they act on, and every
//! patch record holds all it needs, so the write path reads each list **in
//! order, once**, and hashes what it read; [`Relocator5::write_image`]
//! compares those digests with the file's own. A binary search is used only to
//! start in the middle of the lists ([`Relocator5::apply`]).
//!
//! [`Tail::check`] is [`check_relocations`](crate::check_relocations) for
//! such a file; [`Relocator5`] is [`Relocator`](crate::Relocator) for it, minus
//! the table words that format 5 does not have (no host slot, no families).
//! Nothing here writes flash or runs code.

use core::cell::RefCell;

use crate::header::{CODE_OFFSET, Problem};
use paperpaw_digest::sha256::Sha256;

use crate::header5::{
    EntryFacts, EntryRefusal, HEADER_LEN, Header5, Host, META_MAX, PATCH_BYTES, RawEntry,
    RecordFacts, X4_ENTRY_BYTES,
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

/// The most patches a format-5 file may list. Every patch is read by the
/// relocation in order, so the count bounds its work.
pub const MAX_PATCHES: u32 = 8192;

/// How many words (relocations) or patches one buffer holds.
const RELOCS_PER_READ: u32 = 16;
const PATCHES_PER_READ: u32 = 8;

fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// A patch of a format-5 file: three words, `kind | site`, `from` and `to`.
/// It sets the bits of the instruction at `site` for the distance from the
/// place `from` to the place `to` once the image is placed: `from` is the
/// site itself for an upper part (an `auipc`), and the `auipc`'s site for a
/// lower part. The record holds all it needs; nothing is looked up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Patch {
    kind: PatchKind,
    site: u32,
    from: u32,
    to: u32,
}

impl Patch {
    /// The patch in `bytes` ([`PATCH_BYTES`] long), if it is one of this
    /// image: a known kind, at an instruction in the code, both ends inside
    /// the image, and an upper part's `from` its own site.
    fn read(bytes: &[u8], header: &Header5) -> Result<Self, Problem> {
        let (kind, site) = PatchKind::of(le32(bytes, 0)).ok_or(Problem::Relocation)?;
        let (from, to) = (le32(bytes, 4), le32(bytes, 8));
        let in_code = |at: u32| {
            at >= header.code_at
                && at.is_multiple_of(2)
                && at.checked_add(4).is_some_and(|end| end <= header.image_len)
        };
        let fits = in_code(site)
            && to < header.image_len
            && match kind {
                PatchKind::Hi20 => from == site,
                PatchKind::Lo12I | PatchKind::Lo12S => in_code(from),
            };
        if fits {
            Ok(Self {
                kind,
                site,
                from,
                to,
            })
        } else {
            Err(Problem::Relocation)
        }
    }
}

/// The words after a format-5 image, in the file.
#[derive(Debug, Clone, Copy)]
pub struct Tail<'a, S: Source + ?Sized> {
    src: &'a S,
    header: Header5,
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
            header: *header,
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

    /// The first word of patch `i`: its kind and site.
    fn patch_record(&self, i: u32) -> Result<u32, Problem> {
        let mut word = [0; 4];
        self.read(self.patches_at + PATCH_BYTES * u64::from(i), &mut word)?;
        Ok(u32::from_le_bytes(word))
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

    /// The first patch with an instruction that reaches `at` or beyond (its
    /// four bytes end after `at`); the patches are sorted by site.
    fn first_patch_from(&self, at: u32) -> Result<u32, Problem> {
        let (mut lo, mut hi) = (0, self.patches);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if (self.patch_record(mid)? & 0x0FFF_FFFF) + 4 <= at {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        Ok(lo)
    }

    /// Checks the words as [`check_relocations`](crate::check_relocations)
    /// does, reading them once: the words to relocate increasing, each a
    /// whole word of the image outside the hot code; the patches with
    /// increasing sites, each a patch of this image (`Patch::read`); no
    /// more than [`MAX_PATCHES`].
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
                // The heap word is written by the loader, never relocated.
                if !fits || header.heap_at == Some(at) {
                    return Err(Problem::Relocation);
                }
                next = at + 4;
            }
            i += n as u32;
        }
        let mut buf = [0u8; PATCH_BYTES as usize * PATCHES_PER_READ as usize];
        let mut last = None;
        let mut i = 0;
        while i < self.patches {
            let n = (self.patches - i).min(PATCHES_PER_READ) as usize;
            let bytes = &mut buf[..PATCH_BYTES as usize * n];
            self.read(self.patches_at + PATCH_BYTES * u64::from(i), bytes)?;
            for item in bytes.as_chunks::<{ PATCH_BYTES as usize }>().0 {
                let patch = Patch::read(item, header)?;
                if last.is_some_and(|last| patch.site <= last) {
                    return Err(Problem::Relocation);
                }
                last = Some(patch.site);
            }
            i += n as u32;
        }
        Ok(())
    }
}

/// The words to relocate, read in order a short run at a time, each run
/// also hashed when `hash` says so: the digest of what was read, which
/// [`Relocator5::write_image`] compares with the file's own words.
struct Words {
    buf: [u8; 4 * RELOCS_PER_READ as usize],
    used: usize,
    have: usize,
    loaded: u32,
    sha: Sha256,
    hash: bool,
}

impl Words {
    fn new(from: u32, hash: bool) -> Self {
        Self {
            buf: [0; 4 * RELOCS_PER_READ as usize],
            used: 0,
            have: 0,
            loaded: from,
            sha: Sha256::new(),
            hash,
        }
    }

    fn load<S: Source + ?Sized>(&mut self, tail: &Tail<'_, S>) -> Result<bool, Problem> {
        if self.loaded >= tail.relocs {
            return Ok(false);
        }
        let n = (tail.relocs - self.loaded).min(RELOCS_PER_READ) as usize;
        let bytes = &mut self.buf[..4 * n];
        tail.read(tail.relocs_at + 4 * u64::from(self.loaded), bytes)?;
        if self.hash {
            self.sha.update(bytes);
        }
        (self.used, self.have) = (0, n);
        self.loaded += n as u32;
        Ok(true)
    }

    fn peek<S: Source + ?Sized>(&mut self, tail: &Tail<'_, S>) -> Result<Option<u32>, Problem> {
        if self.used == self.have && !self.load(tail)? {
            return Ok(None);
        }
        Ok(Some(le32(&self.buf, 4 * self.used)))
    }
}

/// The patches, read in order like [`Words`].
struct Patches {
    buf: [u8; PATCH_BYTES as usize * PATCHES_PER_READ as usize],
    used: usize,
    have: usize,
    loaded: u32,
    sha: Sha256,
    hash: bool,
}

impl Patches {
    fn new(from: u32, hash: bool) -> Self {
        Self {
            buf: [0; PATCH_BYTES as usize * PATCHES_PER_READ as usize],
            used: 0,
            have: 0,
            loaded: from,
            sha: Sha256::new(),
            hash,
        }
    }

    fn load<S: Source + ?Sized>(&mut self, tail: &Tail<'_, S>) -> Result<bool, Problem> {
        if self.loaded >= tail.patches {
            return Ok(false);
        }
        let n = (tail.patches - self.loaded).min(PATCHES_PER_READ) as usize;
        let bytes = &mut self.buf[..PATCH_BYTES as usize * n];
        tail.read(
            tail.patches_at + PATCH_BYTES * u64::from(self.loaded),
            bytes,
        )?;
        if self.hash {
            self.sha.update(bytes);
        }
        (self.used, self.have) = (0, n);
        self.loaded += n as u32;
        Ok(true)
    }

    fn peek<S: Source + ?Sized>(&mut self, tail: &Tail<'_, S>) -> Result<Option<Patch>, Problem> {
        if self.used == self.have && !self.load(tail)? {
            return Ok(None);
        }
        let at = PATCH_BYTES as usize * self.used;
        let item = &self.buf[at..at + PATCH_BYTES as usize];
        Patch::read(item, &tail.header).map(Some)
    }
}

/// Where a relocation stands in the two lists: both are consumed in the
/// order of the image, each item read once, so that what was read can be
/// hashed ([`Walk::finish`]).
struct Walk {
    words: Words,
    patches: Patches,
    /// The word after the last relocated, and the last patched site.
    next_word: u32,
    last_site: Option<u32>,
    /// A patch whose instruction reaches into the next piece.
    carry: Option<Patch>,
}

impl Walk {
    /// From the start of the image, hashing what it reads.
    fn start() -> Self {
        Self {
            words: Words::new(0, true),
            patches: Patches::new(0, true),
            next_word: 0,
            last_site: None,
            carry: None,
        }
    }

    /// From the piece at `at` (by a binary search; nothing hashed).
    fn seek<S: Source + ?Sized>(tail: &Tail<'_, S>, at: u32) -> Result<Self, Problem> {
        Ok(Self {
            words: Words::new(tail.first_reloc_from(at)?, false),
            patches: Patches::new(tail.first_patch_from(at)?, false),
            next_word: at,
            last_site: None,
            carry: None,
        })
    }

    /// Reads the rest of both lists and gives the digests of all of each.
    fn finish<S: Source + ?Sized>(
        mut self,
        tail: &Tail<'_, S>,
    ) -> Result<([u8; 32], [u8; 32]), Problem> {
        while self.words.load(tail)? {}
        while self.patches.load(tail)? {}
        Ok((self.words.sha.finish(), self.patches.sha.finish()))
    }
}

/// Relocates a format-5 image, a piece at a time, for the data address it is
/// placed at, and for where its hot code runs; the words are read from the
/// file as each piece needs them.
#[derive(Debug)]
pub struct Relocator5<'a, S: Source + ?Sized> {
    header: &'a Header5,
    tail: Tail<'a, S>,
    base: u32,
    delta: u32,
    hot: u32,
    heap: Option<u32>,
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
        Ok(Self {
            header,
            tail,
            base,
            delta: base.wrapping_sub(header.base),
            hot: base + CODE_OFFSET + header.hot_at,
            heap: None,
        })
    }

    /// Writes `table`, the address of the host's `PpHeap` table, into the
    /// plugin's heap word (`abi/heap-word.md`) in the piece that holds it,
    /// after relocating that piece: the same flash pass as the rest of the
    /// sector, since NOR flash only clears bits. A plugin without the word
    /// is unchanged. Without this call the word stays 0 ("no heap").
    pub fn heap_word(mut self, table: u32) -> Self {
        self.heap = Some(table);
        self
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
    /// and the relocation's own checks): the whole of
    /// [`Relocator5::write_image`] with nothing written. The file is read
    /// once, `buf` at a time, the image relocated piece by piece as the
    /// device will, and the digests must hold (the header's, and those of
    /// the words and of the patches as the relocation read them). A file
    /// whose bytes were changed is `Damaged` or `Relocation`, whichever its
    /// changed bytes show first.
    ///
    /// `buf` is the only memory of size, at least 4 bytes (a multiple of 4
    /// is used). The device passes one sector.
    pub fn verify(&self, buf: &mut [u8]) -> Result<(), Problem> {
        match self.write_image(buf, |_, _| Ok::<(), core::convert::Infallible>(())) {
            Ok(()) => Ok(()),
            Err(Written::File(problem)) => Err(problem),
            Err(Written::Sink(never)) => match never {},
        }
    }

    /// The write pass: reads the image once more, `buf` at a time, hashes
    /// **each piece as read** (before it is relocated), relocates it and
    /// hands it to `sink` with its image offset, padded with `0xFF` to whole
    /// words. The words to relocate and the patches are consumed in order,
    /// each item read once and hashed as it is read, by one digest for the
    /// words and one for the patches. After the image the file's own words
    /// and patches are read in order and hashed too: the header's digest
    /// must hold, and the digests of what the relocation consumed must
    /// equal those of the file's lists. Every byte that decides what the
    /// sink was given is therefore a byte the header's digest covers,
    /// whatever the source answers now. A mismatch is `Damaged`, and the
    /// caller takes back what the sink was given.
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
        let mut walk = Walk::start();
        let mut done = 0u32;
        while done < self.header.image_len {
            let len = (self.header.image_len - done).min(step as u32) as usize;
            let words = len.next_multiple_of(4);
            self.tail
                .read(HEADER_LEN as u64 + u64::from(done), &mut buf[..len])
                .map_err(Written::File)?;
            sha.update(&buf[..len]);
            self.relocate(&mut walk, done, &mut buf[..len])
                .map_err(Written::File)?;
            buf[len..words].fill(0xFF);
            sink(done, &buf[..words]).map_err(Written::Sink)?;
            done += len as u32;
        }
        let (mut of_words, mut of_patches) = (Sha256::new(), Sha256::new());
        let regions = [
            (
                self.header.relocs_at(),
                self.header.patches_at(),
                &mut of_words,
            ),
            (
                self.header.patches_at(),
                self.header.file_len(),
                &mut of_patches,
            ),
        ];
        for (from, end, list) in regions {
            let mut at = from;
            while at < end {
                let len = (end - at).min(step as u64) as usize;
                self.tail.read(at, &mut buf[..len]).map_err(Written::File)?;
                sha.update(&buf[..len]);
                list.update(&buf[..len]);
                at += len as u64;
            }
        }
        let (consumed_words, consumed_patches) = walk.finish(&self.tail).map_err(Written::File)?;
        if self.header.holds(sha)
            && consumed_words == of_words.finish()
            && consumed_patches == of_patches.finish()
        {
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
    /// [`Relocator::apply`](crate::Relocator::apply) does, for any piece in
    /// any order: the lists are searched for it, and nothing is hashed. The
    /// write pass ([`Relocator5::write_image`]) does not use this.
    pub fn apply(&self, at: u32, piece: &mut [u8]) -> Result<(), Problem> {
        if !at.is_multiple_of(4) {
            return Err(Problem::Relocation);
        }
        let mut walk = Walk::seek(&self.tail, at)?;
        self.relocate(&mut walk, at, piece)
    }

    /// Relocates `piece` from `at`, taking the words and patches it needs
    /// from `walk`, which lies at `at`: the next words are at or after `at`,
    /// in order, and so are the patches' sites.
    fn relocate(&self, walk: &mut Walk, at: u32, piece: &mut [u8]) -> Result<(), Problem> {
        if !at.is_multiple_of(4) {
            return Err(Problem::Relocation);
        }
        let end = at.saturating_add(piece.len() as u32);
        while let Some(word) = walk.words.peek(&self.tail)? {
            if word >= end {
                break;
            }
            // The list was checked by `Tail::check`, but is read again
            // here: a source that serves other bytes the second time
            // (a card swapped, a flaky read) is refused, never trusted.
            if word < walk.next_word || word < at || !word.is_multiple_of(4) {
                return Err(Problem::Relocation);
            }
            walk.next_word = word.checked_add(4).ok_or(Problem::Relocation)?;
            let at_word = (word - at) as usize;
            let Some(bytes) = piece.get_mut(at_word..at_word + 4) else {
                return Err(Problem::Relocation);
            };
            let value = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            let moved = self.moved(value).ok_or(Problem::Relocation)?;
            bytes.copy_from_slice(&moved.to_le_bytes());
            walk.words.used += 1;
        }
        self.write_heap_word(at, piece)?;
        if let Some(patch) = walk.carry.take() {
            self.write_patch(&patch, at, piece);
        }
        while let Some(patch) = walk.patches.peek(&self.tail)? {
            if patch.site >= end {
                break;
            }
            if walk.last_site.is_some_and(|last| patch.site <= last) {
                return Err(Problem::Relocation);
            }
            walk.last_site = Some(patch.site);
            self.write_patch(&patch, at, piece);
            if patch.site + 4 > end {
                walk.carry = Some(patch);
            }
            walk.patches.used += 1;
        }
        Ok(())
    }

    /// The heap word, if it lies in `piece` (the bytes from `at` on): it
    /// must be 0 in the file (a plugin that ships a value is refused), and
    /// gets the table address if one was given.
    fn write_heap_word(&self, at: u32, piece: &mut [u8]) -> Result<(), Problem> {
        let Some(word) = self.header.heap_at else {
            return Ok(());
        };
        let Some(bytes) = word
            .checked_sub(at)
            .and_then(|from| piece.get_mut(from as usize..from as usize + 4))
        else {
            return Ok(());
        };
        // Whatever the relocation did to it, it was no relocation word
        // (`Tail::check`): a nonzero word is the file's own value.
        if bytes != [0; 4] {
            return Err(Problem::Malformed);
        }
        if let Some(table) = self.heap {
            bytes.copy_from_slice(&table.to_le_bytes());
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

    /// Writes what `patch` sets of the instruction at its site into `piece`
    /// (the bytes from `at` on), a byte at a time: those of the four that lie
    /// in it.
    fn write_patch(&self, patch: &Patch, at: u32, piece: &mut [u8]) {
        let distance = self
            .runs_at(patch.to)
            .wrapping_sub(self.runs_at(patch.from));
        let bits = patch.kind.bits(distance).to_le_bytes();
        let kept = patch.kind.kept().to_le_bytes();
        for i in 0..4 {
            let Some(byte) = patch
                .site
                .checked_add(i)
                .and_then(|byte_at| byte_at.checked_sub(at))
                .and_then(|j| piece.get_mut(j as usize))
            else {
                continue;
            };
            *byte = (*byte & kept[i as usize]) | bits[i as usize];
        }
    }
}

#[cfg(test)]
mod tests;
