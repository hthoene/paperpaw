//! Where an image-only owner's batches start in its image (#248, D4 and D5).
//!
//! An owner whose batches lie within 64 KiB of the first keeps one start for
//! all of them, a two-byte offset for each and the kinds each declares as a
//! byte into the owner's distinct masks (D5, #393): 10 bytes, 8 a mask and
//! 3 a batch. A lookup of a kind walks only the batches that declare it
//! (`declaration_scan.rs`). Any other owner (one batch, batches further
//! apart, more than 256 masks) keeps a start and a mask for each batch:
//! twelve bytes a batch, as D4 left it. Both are one allocation of 16-bit
//! words.
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
/// A packed owner's words before its masks: the mode, the first start (two
/// words), the number of batches and the number of distinct masks.
const PACKED_HEAD: usize = 5;
/// Words of one mask.
const MASK_WORDS: usize = 4;
/// A wide owner's words for each batch: its start (two) and its kinds (four),
/// after the mode word.
const WIDE_WORDS: usize = 6;

/// The starts of an image-only owner's batches, in batch order.
///
/// Packed, each batch keeps the kinds it declares as a one-byte index into
/// the owner's few distinct masks (#393): a lookup of a kind walks only the
/// batches that declare it, where one mask for the owner walked every batch
/// of an owner that declares the kind anywhere (the registry asks hundreds of
/// times a frame, and every batch walked is read from flash).
pub(super) struct Starts(Box<[u16]>);

impl Starts {
    /// The starts of `batches`, packed where they allow it; `None` without
    /// batches or without memory (the owner keeps its index then).
    pub(super) fn try_new(batches: &[BatchStart]) -> Option<Self> {
        let first = batches.iter().map(|batch| batch.start).min()?;
        let last = batches.iter().map(|batch| batch.start).max()?;
        let mut masks: Vec<u64> = Vec::new();
        let mut packed = batches.len() >= 2
            && last - first <= u32::from(u16::MAX)
            && u16::try_from(batches.len()).is_ok();
        if packed {
            for batch in batches {
                if masks.contains(&batch.kinds) {
                    continue;
                }
                if masks.len() > usize::from(u8::MAX) || masks.try_reserve(1).is_err() {
                    packed = false;
                    break;
                }
                masks.push(batch.kinds);
            }
        }
        let words = if packed {
            PACKED_HEAD
                .checked_add(masks.len().checked_mul(MASK_WORDS)?)?
                .checked_add(batches.len())?
                .checked_add(batches.len().div_ceil(2))?
        } else {
            WIDE_WORDS.checked_mul(batches.len())?.checked_add(1)?
        };
        let mut out = Vec::new();
        out.try_reserve_exact(words).ok()?;
        if packed {
            out.push(PACKED);
            push_u32(&mut out, first);
            out.push(u16::try_from(batches.len()).ok()?);
            out.push(u16::try_from(masks.len()).ok()?);
            for mask in &masks {
                push_u64(&mut out, *mask);
            }
            for batch in batches {
                out.push(u16::try_from(batch.start - first).ok()?);
            }
            for pair in batches.chunks(2) {
                let id = |batch: &BatchStart| {
                    masks
                        .iter()
                        .position(|mask| *mask == batch.kinds)
                        .and_then(|at| u8::try_from(at).ok())
                        .unwrap_or(0)
                };
                let low = u16::from(id(&pair[0]));
                let high = pair.get(1).map_or(0, |batch| u16::from(id(batch)));
                out.push(low | high << 8);
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

    /// The distinct masks of a packed owner.
    fn mask_count(&self) -> usize {
        usize::from(self.0.get(4).copied().unwrap_or(0))
    }

    /// How many batches.
    pub(super) fn len(&self) -> usize {
        if self.packed() {
            usize::from(self.0.get(3).copied().unwrap_or(0))
        } else {
            self.0.len().saturating_sub(1) / WIDE_WORDS
        }
    }

    /// Where batch `batch` starts in the image; `None` past the last.
    pub(super) fn start(&self, batch: usize) -> Option<u32> {
        if self.packed() {
            if batch >= self.len() {
                return None;
            }
            let at = PACKED_HEAD
                .checked_add(self.mask_count().checked_mul(MASK_WORDS)?)?
                .checked_add(batch)?;
            word_u32(&self.0, 1)?.checked_add(u32::from(*self.0.get(at)?))
        } else {
            word_u32(&self.0, batch.checked_mul(WIDE_WORDS)?.checked_add(1)?)
        }
    }

    /// The kinds batch `batch` declares (`1 << kind`); none past the last.
    pub(super) fn kinds(&self, batch: usize) -> u64 {
        if batch >= self.len() {
            return 0;
        }
        if !self.packed() {
            return word_u64(&self.0, batch * WIDE_WORDS + 3).unwrap_or(0);
        }
        let ids = PACKED_HEAD + self.mask_count() * MASK_WORDS + self.len();
        let Some(pair) = self.0.get(ids + batch / 2) else {
            return 0;
        };
        let id = usize::from(if batch % 2 == 0 { pair & 0xff } else { pair >> 8 });
        word_u64(&self.0, PACKED_HEAD + id * MASK_WORDS).unwrap_or(0)
    }

    /// Whether the batches share one start.
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
