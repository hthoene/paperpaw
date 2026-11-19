//! Where an image-only owner's batches start in its image (#248, D4 and D5).
//!
//! An owner whose batches lie within 64 KiB of the first keeps one start for
//! all of them, a two-byte offset for each and one mask of the kinds the
//! owner declares: 12 + 2 bytes a batch (D5). A lookup of a kind then walks
//! every batch of an owner that declares it (`declaration_scan.rs`), where a
//! mask per batch skipped the batches without it. Any other owner (one batch,
//! or batches further apart) keeps a start and a mask for each batch: twelve
//! bytes a batch, as D4 left it. Both are one allocation of 16-bit words.
use alloc::{boxed::Box, vec::Vec};

/// Where a batch starts in the owner's image and the kinds it declares
/// (`1 << kind`), while the starts are made at commit.
#[derive(Clone, Copy)]
pub(super) struct BatchStart {
    start: u32,
    kinds: u64,
}

impl BatchStart {
    pub(super) fn new(start: u32) -> Self {
        Self { start, kinds: 0 }
    }

    pub(super) fn add_kind(&mut self, kind: u32) {
        self.kinds |= 1_u64.checked_shl(kind).unwrap_or(0);
    }
}

/// The first word: how the rest is laid out.
const WIDE: u16 = 0;
const PACKED: u16 = 1;
/// A packed owner's words before the offsets: the mode, the first start (two
/// words) and the owner's kinds (four).
const PACKED_HEAD: usize = 7;
/// A wide owner's words for each batch: its start (two) and its kinds (four),
/// after the mode word.
const WIDE_WORDS: usize = 6;

/// The starts of an image-only owner's batches, in batch order.
pub(super) struct Starts(Box<[u16]>);

impl Starts {
    /// The starts of `batches`, packed where they allow it; `None` without
    /// batches or without memory (the owner keeps its index then).
    pub(super) fn try_new(batches: &[BatchStart]) -> Option<Self> {
        let first = batches.iter().map(|batch| batch.start).min()?;
        let last = batches.iter().map(|batch| batch.start).max()?;
        let packed = batches.len() >= 2 && last - first <= u32::from(u16::MAX);
        let words = if packed {
            PACKED_HEAD.checked_add(batches.len())?
        } else {
            WIDE_WORDS.checked_mul(batches.len())?.checked_add(1)?
        };
        let mut out = Vec::new();
        out.try_reserve_exact(words).ok()?;
        if packed {
            let kinds = batches.iter().fold(0, |kinds, batch| kinds | batch.kinds);
            out.push(PACKED);
            push_u32(&mut out, first);
            push_u64(&mut out, kinds);
            for batch in batches {
                out.push(u16::try_from(batch.start - first).ok()?);
            }
        } else {
            out.push(WIDE);
            for batch in batches {
                push_u32(&mut out, batch.start);
                push_u64(&mut out, batch.kinds);
            }
        }
        Some(Self(out.into_boxed_slice()))
    }

    fn packed(&self) -> bool {
        self.0.first() == Some(&PACKED)
    }

    /// How many batches.
    pub(super) fn len(&self) -> usize {
        if self.packed() {
            self.0.len().saturating_sub(PACKED_HEAD)
        } else {
            self.0.len().saturating_sub(1) / WIDE_WORDS
        }
    }

    /// Where batch `batch` starts in the image; `None` past the last.
    pub(super) fn start(&self, batch: usize) -> Option<u32> {
        if self.packed() {
            let offset = *self.0.get(PACKED_HEAD.checked_add(batch)?)?;
            word_u32(&self.0, 1)?.checked_add(u32::from(offset))
        } else {
            word_u32(&self.0, batch.checked_mul(WIDE_WORDS)?.checked_add(1)?)
        }
    }

    /// The kinds batch `batch` may declare (`1 << kind`): its own where
    /// starts are wide, the owner's where they are packed; none past the last.
    pub(super) fn kinds(&self, batch: usize) -> u64 {
        if batch >= self.len() {
            return 0;
        }
        let at = if self.packed() {
            3
        } else {
            batch * WIDE_WORDS + 3
        };
        word_u64(&self.0, at).unwrap_or(0)
    }

    /// Whether the batches share one start and one mask.
    #[cfg(test)]
    pub(super) fn is_packed(&self) -> bool {
        self.packed()
    }

    /// The bytes kept, for the size tests.
    #[cfg(test)]
    pub(super) fn bytes(&self) -> usize {
        self.0.len() * 2
    }
}

fn push_u32(out: &mut Vec<u16>, value: u32) {
    // The halves: each cast keeps the low 16 bits.
    out.push(value as u16);
    out.push((value >> 16) as u16);
}

fn push_u64(out: &mut Vec<u16>, value: u64) {
    for shift in [0, 16, 32, 48] {
        out.push((value >> shift) as u16);
    }
}

fn word_u32(words: &[u16], at: usize) -> Option<u32> {
    let parts = words.get(at..at.checked_add(2)?)?;
    Some(u32::from(parts[0]) | u32::from(parts[1]) << 16)
}

fn word_u64(words: &[u16], at: usize) -> Option<u64> {
    let parts = words.get(at..at.checked_add(4)?)?;
    Some(
        parts
            .iter()
            .rev()
            .fold(0, |value, part| value << 16 | u64::from(*part)),
    )
}

#[cfg(test)]
#[path = "declaration_starts_tests.rs"]
mod tests;
