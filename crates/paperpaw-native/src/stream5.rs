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

use crate::header::{CODE_OFFSET, Problem};
use crate::header5::Header5;
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
        let distance = self.runs_at(to).wrapping_sub(self.runs_at(from));
        let bits = kind.bits(distance).to_le_bytes();
        let kept = kind.kept().to_le_bytes();
        for i in 0..4 {
            let Some(byte) = (site + i)
                .checked_sub(at)
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
