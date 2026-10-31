//! What a [`StoredPack`](super::StoredPack) keeps in memory of its
//! characters and their advances, compactly: a script of thousands of
//! characters (Hangul has 11,172 syllables) takes some bytes for a block
//! it holds whole, a bit a code point where it has gaps, and its advances
//! as runs where they are the same (as Hangul's and CJK's are). Both
//! are built within a budget of bytes, so a pack that would need more is
//! refused (its index) or read on demand (its advances), not an
//! allocation the heap cannot give.

use alloc::vec::Vec;
use core::mem::size_of;

/// A gap of more code points than this between two characters starts a
/// new segment: a segment costs about as much as 128 bits do.
const GAP: u32 = 128;

/// Code points of a segment: a block whole, or those whose bits are set.
#[derive(Debug, Clone, Copy)]
struct Segment {
    first: u32,
    /// Code points from `first` it covers.
    span: u32,
    /// The index of its first character in the pack.
    base: u32,
    /// Where its bits start in [`CharIndex::words`]; [`DENSE`] where it
    /// holds every code point of its span.
    words: u32,
}

const DENSE: u32 = u32::MAX;

/// Why a [`CharIndex`] or [`Advances`] could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Refused {
    /// A character not after the one before.
    Order,
    /// Memory ran out.
    Memory,
    /// It would take more bytes than its budget.
    Budget,
}

/// The characters of a pack, in order: each one's index in the pack.
#[derive(Debug, Clone, Default)]
pub(super) struct CharIndex {
    segments: Vec<Segment>,
    /// The bits of the segments that have gaps, 64 code points a word.
    pub(super) words: Vec<u64>,
    /// For each word, the characters of its segment before it.
    ranks: Vec<u16>,
    len: u32,
}

impl CharIndex {
    /// How many characters it holds.
    pub(super) fn len(&self) -> usize {
        self.len as usize
    }

    /// The bytes it takes on the heap.
    pub(super) fn bytes(&self) -> usize {
        self.segments.capacity() * size_of::<Segment>()
            + self.words.capacity() * size_of::<u64>()
            + self.ranks.capacity() * size_of::<u16>()
    }

    /// The bytes it takes with `segments` segments and `words` words.
    fn bytes_for(segments: usize, words: usize) -> usize {
        segments * size_of::<Segment>() + words * (size_of::<u64>() + size_of::<u16>())
    }

    /// The index of `ch` in the pack, if it holds it.
    pub(super) fn find(&self, ch: char) -> Option<usize> {
        let c = u32::from(ch);
        let at = self
            .segments
            .partition_point(|s| s.first <= c)
            .checked_sub(1)?;
        let segment = self.segments[at];
        let offset = c - segment.first;
        if offset >= segment.span {
            return None;
        }
        if segment.words == DENSE {
            return Some((segment.base + offset) as usize);
        }
        let word = (segment.words + offset / 64) as usize;
        let bits = *self.words.get(word)?;
        let bit = offset % 64;
        if bits >> bit & 1 == 0 {
            return None;
        }
        let before = (bits & ((1u64 << bit) - 1)).count_ones();
        Some((segment.base + u32::from(self.ranks[word]) + before) as usize)
    }
}

/// Makes a [`CharIndex`] of characters given in order.
pub(super) struct IndexBuilder {
    index: CharIndex,
    /// The segment being filled: its first character, the last one and
    /// how many it has. It has words only once it has a gap.
    open: Option<(u32, u32, u32)>,
    /// The most bytes the index may take.
    budget: usize,
}

impl IndexBuilder {
    /// A builder of an index of at most `budget` bytes.
    pub(super) fn new(budget: usize) -> Self {
        Self {
            index: CharIndex::default(),
            open: None,
            budget,
        }
    }

    /// Adds `ch`, which must come after the last one added.
    pub(super) fn push(&mut self, ch: char) -> Result<(), Refused> {
        let c = u32::from(ch);
        if let Some((first, last, count)) = self.open {
            if c <= last {
                return Err(Refused::Order);
            }
            // u16 ranks: at most 65,535 characters a segment.
            if c - last <= GAP && count < u32::from(u16::MAX) {
                let whole = count == last - first + 1;
                if !whole || c != last + 1 {
                    if whole {
                        self.spell_out(first, last)?;
                    }
                    self.set(first, c, count)?;
                }
                self.open = Some((first, c, count + 1));
                self.index.len += 1;
                return Ok(());
            }
            self.close();
        }
        let segments = self.index.segments.len() + 1;
        if CharIndex::bytes_for(segments, self.index.words.len()) > self.budget {
            return Err(Refused::Budget);
        }
        if self.index.segments.try_reserve(1).is_err() {
            return Err(Refused::Memory);
        }
        self.index.segments.push(Segment {
            first: c,
            span: 0,
            base: self.index.len,
            words: u32::try_from(self.index.words.len()).unwrap_or(DENSE),
        });
        self.open = Some((c, c, 1));
        self.index.len += 1;
        Ok(())
    }

    /// Adds the words of the open segment, held whole from `first` to
    /// `last` until now: its first gap.
    fn spell_out(&mut self, first: u32, last: u32) -> Result<(), Refused> {
        let span = last - first + 1;
        self.grow(span.div_ceil(64) as usize)?;
        let start = self.words_start();
        for (w, at) in (0..span).step_by(64).enumerate() {
            let bits = (span - at).min(64);
            self.index.words[start + w] = if bits == 64 {
                u64::MAX
            } else {
                (1 << bits) - 1
            };
            self.index.ranks[start + w] = at as u16;
        }
        Ok(())
    }

    /// Where the open segment's words start.
    fn words_start(&self) -> usize {
        self.index.segments.last().map_or(0, |s| s.words as usize)
    }

    /// Gives the open segment `n` words at least, the new ones empty and
    /// ranked `count`.
    fn grow_to(&mut self, n: usize, count: u32) -> Result<(), Refused> {
        let words = self.words_start() + n;
        if CharIndex::bytes_for(self.index.segments.len(), words) > self.budget {
            return Err(Refused::Budget);
        }
        let more = words.saturating_sub(self.index.words.len());
        if self.index.words.try_reserve(more).is_err()
            || self.index.ranks.try_reserve(more).is_err()
        {
            return Err(Refused::Memory);
        }
        while self.index.words.len() < words {
            self.index.words.push(0);
            self.index.ranks.push(count as u16);
        }
        Ok(())
    }

    fn grow(&mut self, n: usize) -> Result<(), Refused> {
        self.grow_to(n, 0)
    }

    /// Sets the bit of `c` in the open segment starting at `first`, which
    /// has `count` characters before it, adding words as it needs: every
    /// character added so far lies before a new word.
    fn set(&mut self, first: u32, c: u32, count: u32) -> Result<(), Refused> {
        let offset = c - first;
        self.grow_to((offset / 64) as usize + 1, count)?;
        let word = self.words_start() + (offset / 64) as usize;
        self.index.words[word] |= 1 << (offset % 64);
        Ok(())
    }

    /// Ends the open segment: one held whole keeps no words.
    fn close(&mut self) {
        let Some((first, last, count)) = self.open.take() else {
            return;
        };
        let Some(segment) = self.index.segments.last_mut() else {
            return;
        };
        segment.span = last - first + 1;
        if count == segment.span {
            segment.words = DENSE;
        }
    }

    pub(super) fn finish(mut self) -> CharIndex {
        self.close();
        let mut index = self.index;
        index.segments.shrink_to_fit();
        index.words.shrink_to_fit();
        index.ranks.shrink_to_fit();
        index
    }
}

/// A strike's advances in 1/16 px, by character index.
#[derive(Debug, Clone)]
pub(super) enum Advances {
    /// One a character.
    Each(Vec<u16>),
    /// Runs of characters of the same advance: where each starts and the
    /// advance, for packs where that takes less.
    Runs(Vec<(u16, u16)>),
}

impl Advances {
    /// The bytes it takes on the heap.
    pub(super) fn bytes(&self) -> usize {
        match self {
            Advances::Each(each) => each.capacity() * size_of::<u16>(),
            Advances::Runs(runs) => runs.capacity() * size_of::<(u16, u16)>(),
        }
    }

    pub(super) fn get(&self, index: usize) -> Option<u16> {
        match self {
            Advances::Each(each) => each.get(index).copied(),
            Advances::Runs(runs) => {
                let at = runs
                    .partition_point(|&(start, _)| usize::from(start) <= index)
                    .checked_sub(1)?;
                Some(runs[at].1)
            }
        }
    }
}

/// Makes the [`Advances`] of `len` characters given in order.
pub(super) struct AdvancesBuilder {
    advances: Advances,
    len: usize,
    added: usize,
    /// The most bytes they may take.
    budget: usize,
}

impl AdvancesBuilder {
    /// A builder of the advances of `len` characters, in at most `budget`
    /// bytes.
    pub(super) fn new(len: usize, budget: usize) -> Self {
        Self {
            advances: Advances::Runs(Vec::new()),
            len,
            added: 0,
            budget,
        }
    }

    /// Adds the next character's advance.
    pub(super) fn push(&mut self, advance: u16) -> Result<(), Refused> {
        let index = self.added;
        self.added += 1;
        match &mut self.advances {
            Advances::Runs(runs) => {
                if runs.last().is_some_and(|&(_, a)| a == advance) {
                    return Ok(());
                }
                // Runs take more than one a character would: switch.
                if (runs.len() + 1) * 2 > self.len {
                    if self.len * size_of::<u16>() > self.budget {
                        return Err(Refused::Budget);
                    }
                    let mut each = Vec::new();
                    if each.try_reserve_exact(self.len).is_err() {
                        return Err(Refused::Memory);
                    }
                    for (i, &(start, a)) in runs.iter().enumerate() {
                        let end = runs
                            .get(i + 1)
                            .map_or(index, |&(next, _)| usize::from(next));
                        each.extend(core::iter::repeat_n(a, end - usize::from(start)));
                    }
                    each.push(advance);
                    self.advances = Advances::Each(each);
                    return Ok(());
                }
                if (runs.len() + 1) * size_of::<(u16, u16)>() > self.budget {
                    return Err(Refused::Budget);
                }
                // Past 65,535 a pack holds no characters.
                let Ok(start) = u16::try_from(index) else {
                    return Err(Refused::Budget);
                };
                if runs.try_reserve(1).is_err() {
                    return Err(Refused::Memory);
                }
                runs.push((start, advance));
                Ok(())
            }
            Advances::Each(each) => {
                // Room for all of them was set aside.
                if each.len() < self.len {
                    each.push(advance);
                }
                Ok(())
            }
        }
    }

    pub(super) fn finish(self) -> Advances {
        match self.advances {
            Advances::Runs(mut runs) => {
                runs.shrink_to_fit();
                Advances::Runs(runs)
            }
            each => each,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]
    use super::*;

    /// The budget of a pack's index (`stored::INDEX_BUDGET`).
    const BUDGET: usize = 8 * 1024;

    fn index(chars: impl IntoIterator<Item = char>) -> CharIndex {
        let mut builder = IndexBuilder::new(BUDGET);
        for ch in chars {
            assert_eq!(builder.push(ch), Ok(()), "{ch:?}");
        }
        builder.finish()
    }

    /// Finds what a sorted list would, for every code point of `range`.
    fn same_as_a_list(chars: &[char], range: core::ops::RangeInclusive<u32>) {
        let index = index(chars.iter().copied());
        assert_eq!(index.len(), chars.len());
        for c in range.filter_map(char::from_u32) {
            assert_eq!(index.find(c), chars.binary_search(&c).ok(), "{c:?}");
        }
    }

    #[test]
    fn finds_what_a_sorted_list_does() {
        let latin: Vec<char> = (' '..='~')
            .chain('\u{a0}'..='\u{17f}')
            .chain(['\u{2014}', '\u{fffd}'])
            .collect();
        same_as_a_list(&latin, 0..=0x3000);
        same_as_a_list(&latin, 0xff00..=0x10000);
        // Every 3rd syllable, and gaps right at the limit.
        let sparse: Vec<char> = (0xac00..=0xd7a3u32)
            .step_by(3)
            .chain([0xd7a3 + GAP, 0xd7a3 + 2 * GAP + 1])
            .filter_map(char::from_u32)
            .collect();
        same_as_a_list(&sparse, 0xab00..=0xd900);
        same_as_a_list(&['a'], 0..=0x100);
        same_as_a_list(&[], 0..=0x100);
        same_as_a_list(&['\u{0}', '\u{10ffff}'], 0..=0x200);
        assert_eq!(index(['\u{0}', '\u{10ffff}']).find('\u{10ffff}'), Some(1));
    }

    #[test]
    fn a_whole_block_keeps_no_bits() {
        let hangul = index('\u{ac00}'..='\u{d7a3}');
        assert_eq!((hangul.segments.len(), hangul.words.len()), (1, 0));
        assert_eq!(hangul.find('\u{d7a3}'), Some(11_171));
        // With a gap: bits, 175 words for the block.
        let gappy = index(('\u{ac00}'..='\u{d7a3}').filter(|&c| c != '\u{ac01}'));
        assert_eq!((gappy.segments.len(), gappy.words.len()), (1, 175));
        assert_eq!(gappy.find('\u{ac02}'), Some(1));
        assert_eq!(gappy.find('\u{ac01}'), None);
    }

    #[test]
    fn refuses_characters_out_of_order() {
        let mut builder = IndexBuilder::new(BUDGET);
        assert_eq!(builder.push('b'), Ok(()));
        assert_eq!(builder.push('b'), Err(Refused::Order));
        assert_eq!(builder.push('a'), Err(Refused::Order));
    }

    #[test]
    fn keeps_within_its_budget() {
        // A block held whole needs no bits, not even while it is built:
        // as many characters as a segment may have, then more.
        let whole = index((0x2_0000..0x2_0000 + 70_000).filter_map(char::from_u32));
        assert_eq!((whole.segments.len(), whole.words.len()), (2, 0));
        assert!(whole.bytes() <= 2 * size_of::<Segment>());
        // KS X 1001's Hangul (every 5th syllable, as scattered): bits.
        let ks = index(('\u{ac00}'..='\u{d7a3}').step_by(5));
        assert!(ks.bytes() < 2 * 1024, "{}", ks.bytes());
        // A character every 200 code points: a segment each, refused once
        // past the budget, the bytes it took not above it.
        let mut builder = IndexBuilder::new(BUDGET);
        let refused = (0..0x10_0000u32)
            .step_by(200)
            .filter_map(char::from_u32)
            .map(|ch| builder.push(ch))
            .find(|pushed| pushed.is_err());
        assert_eq!(refused, Some(Err(Refused::Budget)));
        assert!(builder.index.segments.len() * size_of::<Segment>() <= BUDGET);
        // One of every other code point: bits, refused past the budget.
        let mut builder = IndexBuilder::new(BUDGET);
        let refused = (0x4e00..0x4e00 + 65_000u32)
            .step_by(2)
            .filter_map(char::from_u32)
            .map(|ch| builder.push(ch))
            .find(|pushed| pushed.is_err());
        assert_eq!(refused, Some(Err(Refused::Budget)));
        assert!(builder.index.words.len() * 10 <= BUDGET);
    }

    fn advances(values: &[u16]) -> Advances {
        let mut builder = AdvancesBuilder::new(values.len(), 4096);
        for &v in values {
            assert_eq!(builder.push(v), Ok(()));
        }
        let advances = builder.finish();
        for (i, &v) in values.iter().enumerate() {
            assert_eq!(advances.get(i), Some(v), "{i}");
        }
        advances
    }

    #[test]
    fn same_advances_are_kept_as_runs() {
        let mut values = alloc::vec![160u16; 11_172];
        values.extend([40, 50, 50, 60]);
        assert!(matches!(advances(&values), Advances::Runs(r) if r.len() == 4));
        // All different: one a character.
        let values: Vec<u16> = (0..300).collect();
        assert!(matches!(advances(&values), Advances::Each(e) if e.len() == 300));
        // Runs at first, then too many.
        let mut values = alloc::vec![7u16; 100];
        values.extend(0..100);
        assert!(matches!(advances(&values), Advances::Each(_)));
        advances(&[]);
        advances(&[3]);
    }

    #[test]
    fn advances_keep_within_their_budget() {
        let refused = |values: &[u16]| {
            let mut builder = AdvancesBuilder::new(values.len(), 4096);
            values.iter().map(|&v| builder.push(v)).find(Result::is_err)
        };
        // 2,048 different ones fit, one more does not.
        let values: Vec<u16> = (0..2048).collect();
        assert_eq!(refused(&values), None);
        let values: Vec<u16> = (0..2049).collect();
        assert_eq!(refused(&values), Some(Err(Refused::Budget)));
        // Many characters in few runs fit; as many runs as fit in 4 KiB.
        let mut values = alloc::vec![10u16; 60_000];
        values.extend((0..1023).map(|i| i % 2));
        assert_eq!(refused(&values), None);
        values.extend([5, 6]);
        assert_eq!(refused(&values), Some(Err(Refused::Budget)));
    }
}
