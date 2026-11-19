//! The patterns of one or two letters, kept whole in memory.
//!
//! A word asks for every substring of it, and most of the asks that read a
//! block from the card are for these short keys: each start of the word is a
//! different letter, so a different block. They are few (545 of 36,709 in
//! `de`), so the large tables, which have no filter, keep them: the answer to a
//! key of one or two letters is found here, without a read, whether it is a
//! pattern or not.
//!
//! Heap: a fixed [`BYTES`] (4,096), for any table that keeps them.

use crate::Points;
use alloc::vec::Vec;

/// Short patterns kept; a table with more keeps none (the shipped ones that
/// need it have at most 554).
pub(crate) const MAX_ENTRIES: usize = 640;
/// Bytes of the set of prefixes that longer patterns start with.
const PREFIX_BYTES: usize = 256;
/// The heap taken.
pub(crate) const BYTES: usize = MAX_ENTRIES * core::mem::size_of::<Entry>() + PREFIX_BYTES;

/// A pattern of one or two letters: the letters (0 for none) and its points
/// (one byte for one letter, two for two).
#[derive(Clone, Copy)]
struct Entry {
    letters: [u16; 2],
    points: [u8; 2],
}

pub(crate) struct Short {
    /// By letters, ascending.
    entries: Vec<Entry>,
    /// A bit for each prefix of one or two letters of a longer pattern: a
    /// key may be answered "no longer pattern" only where its bit is clear.
    prefixes: Vec<u8>,
}

/// A letter that fits 16 bits and is not the padding.
fn letter(c: char) -> Option<u16> {
    u16::try_from(u32::from(c)).ok().filter(|&c| c != 0)
}

/// The bit of the prefix `letters` in the set.
fn bit(letters: [u16; 2]) -> (usize, u8) {
    let hash = u32::from(letters[0]).wrapping_mul(0x9e37_79b1)
        ^ u32::from(letters[1]).wrapping_mul(0x85eb_ca6b);
    let bit = (hash >> (32 - 11)) as usize;
    (bit / 8, 1 << (bit % 8))
}

impl Short {
    /// Room for the entries and the prefixes, `None` where the heap has none.
    pub(crate) fn new() -> Option<Self> {
        let mut entries = Vec::new();
        entries.try_reserve_exact(MAX_ENTRIES).ok()?;
        let mut prefixes = Vec::new();
        prefixes.try_reserve_exact(PREFIX_BYTES).ok()?;
        prefixes.resize(PREFIX_BYTES, 0);
        Some(Self { entries, prefixes })
    }

    pub(crate) fn heap_bytes(&self) -> usize {
        self.entries.capacity() * core::mem::size_of::<Entry>() + self.prefixes.capacity()
    }

    /// Takes the next pattern of the table (in its order); `false` where
    /// these cannot be kept: the keys are not sorted text, or there are too
    /// many.
    pub(crate) fn add(&mut self, key: &[u8], points: &[u8]) -> bool {
        let Ok(text) = core::str::from_utf8(key) else {
            return false;
        };
        let mut chars = text.chars();
        let (first, second, third) = (chars.next(), chars.next(), chars.next());
        let letters = [first.and_then(letter), second.and_then(letter)];
        if let (Some(a), Some(_)) = (letters[0], second) {
            let (at, mask) = bit([a, 0]);
            self.prefixes[at] |= mask;
            if let (Some(b), Some(_)) = (letters[1], third) {
                let (at, mask) = bit([a, b]);
                self.prefixes[at] |= mask;
            }
        }
        if third.is_some() {
            return true;
        }
        // One or two letters, all of them letters this keeps.
        let (Some(a), kept) = (letters[0], letters[1]) else {
            return true;
        };
        if second.is_some() && kept.is_none() {
            return true;
        }
        let letters = [a, kept.unwrap_or(0)];
        let mut stored = [0u8; 2];
        let room = &points[..points.len().min(2)];
        stored[..room.len()].copy_from_slice(room);
        let ascending = self.entries.last().is_none_or(|l| l.letters < letters);
        if !ascending || self.entries.len() == MAX_ENTRIES {
            return false;
        }
        self.entries.push(Entry {
            letters,
            points: stored,
        });
        true
    }

    /// As the table would answer for `key` (whether it is a pattern, with
    /// its points in `points`, and whether a longer pattern may start with
    /// it), if `key` is one or two letters this keeps; `None` for any other.
    pub(crate) fn lookup(&self, key: &[u8], points: &mut Points) -> Option<(bool, bool)> {
        if key.len() > 8 {
            return None;
        }
        let text = core::str::from_utf8(key).ok()?;
        let mut chars = text.chars();
        let first = letter(chars.next()?)?;
        let second = match chars.next() {
            Some(c) => letter(c)?,
            None => 0,
        };
        if chars.next().is_some() {
            return None;
        }
        let letters = [first, second];
        let (at, mask) = bit(letters);
        let longer = self.prefixes[at] & mask != 0;
        let found = self.entries.binary_search_by_key(&letters, |e| e.letters);
        Some(match found {
            Ok(i) => {
                points[..2].copy_from_slice(&self.entries[i].points);
                (true, longer)
            }
            Err(_) => (false, longer),
        })
    }
}
