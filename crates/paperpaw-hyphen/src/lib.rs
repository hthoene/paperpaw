//! Hyphenation with Liang's algorithm (as in TeX), on pattern tables that
//! are read in place.
//!
//! Patterns are compiled ahead of time ([`compile`], e.g. in a build
//! script) into a sorted table with a sparse index, so a device can keep
//! them in flash and look them up by binary search without loading them:
//! hyphenating needs a few hundred bytes of RAM, whatever the size of the
//! pattern set.
//!
//! Table format (little endian):
//!
//! | Field | Size |
//! |-------|------|
//! | magic `PPHP`, version 1 | 5 |
//! | minimum characters before / after a hyphen | 2 |
//! | longest pattern in characters | 1 |
//! | number of patterns, number of index blocks | 4 + 4 |
//! | offset and number of exceptions | 4 + 4 |
//! | index: offset of every [`BLOCK`]th pattern | 4 per block |
//! | patterns: key length, key (UTF-8), points (4 bits each) | |
//! | exceptions: length, word with `-` at the allowed breaks | |

#![no_std]

extern crate alloc;

#[cfg(feature = "kernel")]
mod check;
mod compile;

use alloc::vec::Vec;
use core::cmp::Ordering;
use core::fmt;

#[cfg(feature = "kernel")]
pub use check::{
    Layout, MAX_BLOCK_BYTES, MAX_BLOCKS, MAX_EXCEPTION_BYTES, TableError, check_table,
};
pub use compile::compile;

/// Patterns per index entry.
pub const BLOCK: usize = 16;
const MAGIC: &[u8; 5] = b"PPHP\x01";
/// Patterns made before PaperPaw had its name (the same format): still read.
const OLD_MAGIC: &[u8; 5] = b"QIHP\x01"; // before PPHP
const HEADER: usize = 24;
/// Longest word that is hyphenated, in characters.
const MAX_WORD: usize = 48;
/// The most characters [`Patterns::with_minimums`] keeps before or after a
/// hyphen.
pub const MAX_MINIMUM: usize = 16;

/// Why a pattern table cannot be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidPatterns;

impl fmt::Display for InvalidPatterns {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid hyphenation patterns")
    }
}

/// A compiled pattern table.
#[derive(Clone, Copy)]
pub struct Patterns<'a> {
    data: &'a [u8],
    left_min: usize,
    right_min: usize,
    max_len: usize,
    blocks: usize,
    exceptions: (usize, usize),
}

impl fmt::Debug for Patterns<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Patterns")
            .field("bytes", &self.data.len())
            .field("left_min", &self.left_min)
            .field("right_min", &self.right_min)
            .finish_non_exhaustive()
    }
}

fn u32_at(data: &[u8], at: usize) -> Option<usize> {
    let bytes = data.get(at..at + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize)
}

impl<'a> Patterns<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self, InvalidPatterns> {
        if data.len() < HEADER || (&data[..5] != MAGIC && &data[..5] != OLD_MAGIC) {
            return Err(InvalidPatterns);
        }
        let blocks = u32_at(data, 12).ok_or(InvalidPatterns)?;
        let exceptions = (
            u32_at(data, 16).ok_or(InvalidPatterns)?,
            u32_at(data, 20).ok_or(InvalidPatterns)?,
        );
        let index_end = blocks
            .checked_mul(4)
            .and_then(|index| index.checked_add(HEADER));
        if index_end.is_none_or(|end| end > data.len()) || exceptions.0 > data.len() {
            return Err(InvalidPatterns);
        }
        Ok(Self {
            data,
            left_min: usize::from(data[5]).max(1),
            right_min: usize::from(data[6]).max(1),
            max_len: usize::from(data[7]),
            blocks,
            exceptions,
        })
    }

    /// Minimum characters before and after a hyphen: those of the table,
    /// or those given to [`with_minimums`](Self::with_minimums).
    pub fn minimums(&self) -> (usize, usize) {
        (self.left_min, self.right_min)
    }

    /// These patterns with other minimums, e.g. a reader's: the fewest
    /// characters to keep before (`left`) and after (`right`) a hyphen.
    /// 0 keeps the table's own. A value is at most [`MAX_MINIMUM`]; a word
    /// that cannot keep both is left whole.
    #[must_use]
    pub fn with_minimums(mut self, left: usize, right: usize) -> Self {
        if left > 0 {
            self.left_min = left.min(MAX_MINIMUM);
        }
        if right > 0 {
            self.right_min = right.min(MAX_MINIMUM);
        }
        self
    }

    fn entries(&self) -> usize {
        HEADER + self.blocks * 4
    }

    /// The pattern at `at` (relative to the entries): key, points and the
    /// offset of the next pattern.
    fn entry(&self, at: usize) -> Option<(&'a [u8], &'a [u8], usize)> {
        // Offsets come from the table: checked, as `usize` may be 32 bits.
        let key_at = self.entries().checked_add(at)?.checked_add(1)?;
        let len = usize::from(*self.data.get(key_at - 1)?);
        let key = self.data.get(key_at..key_at + len)?;
        let chars = key.iter().filter(|&&b| b & 0xc0 != 0x80).count();
        let points_len = (chars + 2) / 2;
        let points = self.data.get(key_at + len..key_at + len + points_len)?;
        Some((key, points, at + 1 + len + points_len))
    }

    /// The points of the pattern `key`, if there is one, and whether a
    /// longer pattern may start with `key`: `false` only where the table
    /// shows that none does. Patterns are sorted by their bytes, so those
    /// that start with `key` come right after it; where the first one
    /// greater than `key` does not, none does.
    fn lookup(&self, key: &[u8]) -> (Option<&'a [u8]>, bool) {
        self.find(key).unwrap_or((None, true))
    }

    /// [`Patterns::lookup`], `None` where the table is damaged.
    fn find(&self, key: &[u8]) -> Option<(Option<&'a [u8]>, bool)> {
        let block_key = |b: usize| -> Option<&'a [u8]> {
            let at = u32_at(self.data, HEADER + b * 4)?;
            self.entry(at).map(|(k, ..)| k)
        };
        // The last block whose first key is not greater than `key`.
        let (mut low, mut high) = (0, self.blocks);
        while low < high {
            let mid = (low + high) / 2;
            if block_key(mid)? <= key {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        let block = low.checked_sub(1)?;
        let mut at = u32_at(self.data, HEADER + block * 4)?;
        let ended = |at: usize| self.entries().saturating_add(at) >= self.exceptions.0;
        for _ in 0..BLOCK {
            let (k, points, next) = self.entry(at)?;
            match k.cmp(key) {
                Ordering::Equal => {
                    let longer = !ended(next)
                        && self
                            .entry(next)
                            .is_none_or(|(after, ..)| after.starts_with(key));
                    return Some((Some(points), longer));
                }
                Ordering::Greater => return Some((None, k.starts_with(key))),
                Ordering::Less => at = next,
            }
            if ended(at) {
                return Some((None, false));
            }
        }
        None
    }

    /// Calls `out` with the byte offset of every position in `word` where
    /// it may be hyphenated, in ascending order. `word` should consist of
    /// letters only; it is matched without regard to case.
    pub fn hyphenate(&self, word: &str, out: &mut dyn FnMut(usize)) {
        // A character takes at most 4 bytes.
        if word.len() > 4 * MAX_WORD {
            return;
        }
        // Too short for the minimums or too long: known before lowercasing.
        let n = word.chars().count();
        if n < self.left_min + self.right_min || n > MAX_WORD {
            return;
        }
        let mut chars = ['.'; MAX_WORD + 2];
        // Where each letter starts in `word`, at most `4 * MAX_WORD` bytes in.
        let mut offsets = [0u8; MAX_WORD];
        for (i, (at, c)) in word.char_indices().enumerate() {
            let mut lower = c.to_lowercase();
            match (lower.next(), lower.next(), u8::try_from(at)) {
                (Some(l), None, Ok(at)) => (chars[i + 1], offsets[i]) = (l, at),
                // Characters that change length when lowercased are rare;
                // such words are left alone.
                _ => return,
            }
        }
        let chars = &chars[..n + 2];
        let allowed = self.left_min..=n - self.right_min;
        if let Some(found) = self.exception(&chars[1..=n]) {
            // Exceptions keep the minimums too, as in TeX.
            for p in found.into_iter().filter(|p| allowed.contains(p)) {
                out(usize::from(offsets[p]));
            }
            return;
        }

        // values[k]: the highest point between chars[k - 1] and chars[k].
        let mut values = [0u8; MAX_WORD + 3];
        let mut key = [0u8; 4 * 16];
        for start in 0..chars.len() {
            let mut len = 0;
            for (count, &c) in chars[start..].iter().enumerate().take(self.max_len) {
                if len + c.len_utf8() > key.len() {
                    break;
                }
                len += c.encode_utf8(&mut key[len..]).len();
                let (found, longer) = self.lookup(&key[..len]);
                if let Some(points) = found {
                    for k in 0..=count + 1 {
                        let byte = points.get(k / 2).copied().unwrap_or(0);
                        let point = if k % 2 == 0 { byte >> 4 } else { byte & 0x0f };
                        let slot = &mut values[start + k];
                        *slot = (*slot).max(point);
                    }
                }
                // No pattern starts with this key: none with a longer one.
                if !longer {
                    break;
                }
            }
        }
        // A hyphen before letter p (0-based) sits at values[p + 1].
        for p in allowed {
            if values[p + 1] % 2 == 1 {
                out(usize::from(offsets[p]));
            }
        }
    }

    /// Positions (letter indices) from the exception list, if `word` is in
    /// it: ascending, and only between two letters.
    fn exception(&self, word: &[char]) -> Option<Vec<usize>> {
        let (mut at, count) = self.exceptions;
        for _ in 0..count {
            let len = usize::from(*self.data.get(at)?);
            let entry = core::str::from_utf8(self.data.get(at + 1..at + 1 + len)?).ok()?;
            at += 1 + len;
            // Most differ in the first letter: no need to count them.
            if !entry.chars().filter(|&c| c != '-').eq(word.iter().copied()) {
                continue;
            }
            let mut positions = Vec::new();
            let mut index = 0;
            for c in entry.chars() {
                if c == '-' {
                    // Not before the first letter or after the last, and
                    // once for `--`.
                    if index > 0 && index < word.len() && positions.last() != Some(&index) {
                        positions.push(index);
                    }
                } else {
                    index += 1;
                }
            }
            return Some(positions);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;

    fn hyphenated(patterns: &Patterns<'_>, word: &str) -> String {
        let mut breaks = Vec::new();
        patterns.hyphenate(word, &mut |at| breaks.push(at));
        let mut out = String::new();
        for (i, c) in word.char_indices() {
            if breaks.contains(&i) {
                out.push('-');
            }
            out.push(c);
        }
        out
    }

    #[test]
    fn applies_patterns_like_tex() {
        // Liang's example patterns for "hyphenation".
        let table = compile("hy3ph he2n hena4 hen5at 1na n2at 1tio 2io o2n", "", 2, 3);
        let patterns = Patterns::new(&table).unwrap();
        assert_eq!(hyphenated(&patterns, "hyphenation"), "hy-phen-ation");
        assert_eq!(hyphenated(&patterns, "Hyphenation"), "Hy-phen-ation");
        // Too short for the minimums.
        assert_eq!(hyphenated(&patterns, "hyph"), "hyph");
    }

    #[test]
    fn exceptions_win() {
        let table = compile("1na", "ta-ble", 2, 2);
        let patterns = Patterns::new(&table).unwrap();
        assert_eq!(hyphenated(&patterns, "table"), "ta-ble");
        assert_eq!(hyphenated(&patterns, "banana"), "ba-na-na");
    }

    #[test]
    fn exceptions_break_only_between_letters() {
        // As written by hand in a community pattern file.
        let table = compile("", "table- -word dou--ble", 1, 1);
        let patterns = Patterns::new(&table).unwrap();
        assert_eq!(hyphenated(&patterns, "table"), "table");
        assert_eq!(hyphenated(&patterns, "word"), "word");
        assert_eq!(hyphenated(&patterns, "double"), "dou-ble");

        // A table made elsewhere may still hold such exceptions.
        let mut table = compile("", "", 1, 1);
        let count = u32::from_le_bytes(table[20..24].try_into().unwrap()) + 3;
        table[20..24].copy_from_slice(&count.to_le_bytes());
        for word in ["table-", "-word", "dou--ble"] {
            table.push(word.len() as u8);
            table.extend_from_slice(word.as_bytes());
        }
        let patterns = Patterns::new(&table).unwrap();
        assert_eq!(hyphenated(&patterns, "table"), "table");
        assert_eq!(hyphenated(&patterns, "word"), "word");
        assert_eq!(hyphenated(&patterns, "double"), "dou-ble");
    }

    #[test]
    fn patterns_with_letters_that_lowercase_to_several() {
        // "İ" lowercases to two characters; the patterns after it must
        // still be found.
        let table = compile("İ1 1x", "", 1, 1);
        let patterns = Patterns::new(&table).unwrap();
        assert_eq!(hyphenated(&patterns, "axa"), "a-xa");
    }

    #[test]
    fn handles_non_ascii_letters() {
        let table = compile("1ü ä1", "", 1, 1);
        let patterns = Patterns::new(&table).unwrap();
        assert_eq!(hyphenated(&patterns, "Mädchenbüro"), "Mä-dchenb-üro");
    }

    #[test]
    fn minimums_can_be_given_and_0_keeps_the_tables() {
        let table = compile("1b", "", 2, 2);
        let table = Patterns::new(&table).unwrap();
        assert_eq!(table.minimums(), (2, 2));
        // The table's own: no break fits.
        assert_eq!(hyphenated(&table, "abab"), "abab");
        assert_eq!(hyphenated(&table.with_minimums(0, 0), "abab"), "abab");
        // Both given.
        let both = table.with_minimums(1, 1);
        assert_eq!(both.minimums(), (1, 1));
        assert_eq!(hyphenated(&both, "abab"), "a-ba-b");
        // One given, the other the table's.
        assert_eq!(hyphenated(&table.with_minimums(1, 0), "abab"), "a-bab");
        assert_eq!(hyphenated(&table.with_minimums(0, 1), "abab"), "aba-b");
        // More than a word can keep: it stays whole, whatever the number.
        for huge in [3, 16, 1000, usize::MAX] {
            let wide = table.with_minimums(huge, huge);
            assert!(wide.minimums().0 <= MAX_MINIMUM);
            assert_eq!(hyphenated(&wide, "abab"), "abab");
        }
        // Exceptions keep the given minimums too.
        let table = compile("", "ab-ab", 1, 1);
        let table = Patterns::new(&table).unwrap();
        assert_eq!(hyphenated(&table, "abab"), "ab-ab");
        assert_eq!(hyphenated(&table.with_minimums(3, 0), "abab"), "abab");
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(Patterns::new(b"nope").unwrap_err(), InvalidPatterns);
    }
}
