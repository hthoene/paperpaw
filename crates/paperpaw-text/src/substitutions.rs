//! The substitution tables of a font pack (slot 3, language forms, and
//! slot 4, substitutions; the format is in `bitmap.rs`'s documentation):
//! keys drawn as other keys, as several, or several as one.
//!
//! Read only by the tests and the fuzz target until a shaper uses them
//! (Indic, Khmer, Burmese; #92, D5): the firmware does not carry the code
//! before then.

use alloc::vec::Vec;

/// The table's version this reader knows.
const VERSION: u8 = 1;
/// The table's head: version, 3 reserved, length.
const HEAD: usize = 8;
/// A feature's record.
const FEATURE: usize = 16;
/// A lookup's record in a feature's lookup list.
const LOOKUP: usize = 8;
/// The kinds of lookup this reader knows; others are skipped.
const SINGLE: u8 = 1;
const LIGATURE: u8 = 2;
const MULTIPLE: u8 = 3;
/// The most keys a ligature joins, or one key is drawn as.
pub(crate) const MAX_KEYS: usize = 16;
/// A feature's flag: on unless the text turns it off.
const ON_BY_DEFAULT: u8 = 1;

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    let bytes = data.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    let bytes = data.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn usize_at(data: &[u8], at: usize) -> Option<usize> {
    u32_at(data, at).and_then(|v| usize::try_from(v).ok())
}

/// A substitution table of version 1.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Substitutions<'a> {
    /// The table, as long as its head says.
    data: &'a [u8],
}

impl<'a> Substitutions<'a> {
    /// The table in `table` (as a pack's directory gives it), if it is of
    /// a version this reader knows and as long as it says.
    pub(crate) fn new(table: &'a [u8]) -> Option<Self> {
        if *table.first()? != VERSION {
            return None;
        }
        let len = usize_at(table, 4)?;
        let data = table.get(..len)?;
        (len >= HEAD + 4).then_some(Substitutions { data })
    }

    /// The features, in the order they apply.
    pub(crate) fn features(&self) -> impl Iterator<Item = Feature<'a>> + 'a {
        let data = self.data;
        let count = u16_at(data, HEAD).unwrap_or(0);
        (0..usize::from(count)).map_while(move |i| {
            let at = HEAD + 4 + i * FEATURE;
            let record = data.get(at..at + FEATURE)?;
            Some(Feature {
                data,
                tag: [record[0], record[1], record[2], record[3]],
                script: record[4],
                flags: record[5],
                language: [record[8], record[9], record[10], record[11]],
                lookups: usize_at(record, 12)?,
            })
        })
    }

    /// Applies the features on by default for text of `script` (its index
    /// in `paperpaw_lang::scripts()` and 1) and `language` (a language
    /// code: `hi`, `sa`) to `keys`, in their order. `keys` grows to at most
    /// `limit` keys: a substitution that would make it longer is not made.
    pub(crate) fn apply(&self, script: u8, language: &str, keys: &mut Vec<u32>, limit: usize) {
        for feature in self.features() {
            if feature.on_by_default() && feature.applies(script, language) {
                for lookup in feature.lookups() {
                    lookup.apply(keys, limit);
                }
            }
        }
    }
}

/// A feature: an OpenType feature tag and the lookups it applies.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Feature<'a> {
    data: &'a [u8],
    /// The OpenType feature tag (`half`, `blwf`, `pres`, `liga`, `locl`).
    pub(crate) tag: [u8; 4],
    /// The script it is for (index in `paperpaw_lang::scripts()` and 1),
    /// 0: any.
    pub(crate) script: u8,
    flags: u8,
    /// The language it is for, its code in ASCII, zero-padded; all zero:
    /// any.
    pub(crate) language: [u8; 4],
    /// Where its lookup list is.
    lookups: usize,
}

impl<'a> Feature<'a> {
    /// Whether it applies unless the text turns it off.
    pub(crate) fn on_by_default(&self) -> bool {
        self.flags & ON_BY_DEFAULT != 0
    }

    /// Whether it is for text of `script` and `language`.
    pub(crate) fn applies(&self, script: u8, language: &str) -> bool {
        let code = self.language.iter().take_while(|b| **b != 0).count();
        (self.script == 0 || self.script == script)
            && (code == 0 || self.language.get(..code) == Some(language.as_bytes()))
    }

    /// Its lookups of the kinds this reader knows, in their order.
    pub(crate) fn lookups(&self) -> impl Iterator<Item = Lookup<'a>> + 'a {
        let (data, list) = (self.data, self.lookups);
        let count = u16_at(data, list).unwrap_or(0);
        (0..usize::from(count))
            .map_while(move |i| {
                let at = list.checked_add(4 + i * LOOKUP)?;
                let record = data.get(at..at.checked_add(LOOKUP)?)?;
                Some((record[0], u16_at(record, 2)?, usize_at(record, 4)?))
            })
            .filter_map(move |(kind, count, entries)| {
                let lookup = Lookup {
                    data,
                    entries,
                    count: usize::from(count),
                    kind,
                };
                matches!(kind, SINGLE | LIGATURE | MULTIPLE).then_some(lookup)
            })
    }
}

/// A lookup: keys drawn as one other key (single), several keys as one
/// (ligature), or one key as several (multiple).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Lookup<'a> {
    data: &'a [u8],
    entries: usize,
    count: usize,
    kind: u8,
}

impl Lookup<'_> {
    /// The place of an entry whose key is `key`, by a binary search of the
    /// entries' keys (`key_at` says where entry `i`'s is). Entries out of
    /// order are not found, nothing worse.
    fn search(&self, key: u32, key_at: impl Fn(usize) -> Option<usize>) -> Option<usize> {
        let (mut low, mut high) = (0, self.count);
        while low < high {
            let mid = low + (high - low) / 2;
            let found = u32_at(self.data, key_at(mid)?)?;
            match found.cmp(&key) {
                core::cmp::Ordering::Less => low = mid + 1,
                core::cmp::Ordering::Greater => high = mid,
                core::cmp::Ordering::Equal => return Some(mid),
            }
        }
        None
    }

    /// Where entry `i` of a ligature or multiple lookup is.
    fn entry(&self, i: usize) -> Option<usize> {
        usize_at(self.data, self.entries.checked_add(i.checked_mul(4)?)?)
    }

    /// Where the first key of a ligature lookup's entry `i` is.
    fn first_key_at(&self, i: usize) -> Option<usize> {
        self.entry(i)?.checked_add(4)
    }

    /// The key `key` is drawn as, by a single lookup.
    pub(crate) fn single(&self, key: u32) -> Option<u32> {
        if self.kind != SINGLE {
            return None;
        }
        let record = |i: usize| self.entries.checked_add(i.checked_mul(8)?);
        let i = self.search(key, record)?;
        u32_at(self.data, record(i)?.checked_add(4)?)
    }

    /// The keys `key` is drawn as, by a multiple lookup.
    pub(crate) fn multiple(&self, key: u32) -> Option<impl Iterator<Item = u32> + '_> {
        if self.kind != MULTIPLE {
            return None;
        }
        let at = self.entry(self.search(key, |i| self.entry(i))?)?;
        let count = usize::from(*self.data.get(at.checked_add(4)?)?);
        let keys = at.checked_add(8)?;
        let end = keys.checked_add(count * 4)?;
        if !(1..=MAX_KEYS).contains(&count) || self.data.len() < end {
            return None;
        }
        Some((0..count).filter_map(move |k| u32_at(self.data, keys + k * 4)))
    }

    /// The ligature `keys` start with, by a ligature lookup: how many keys
    /// it joins and the key it is drawn as. Of the entries that start with
    /// the same key, the first that matches (the table puts the longest
    /// first).
    pub(crate) fn ligature(&self, keys: &[u32]) -> Option<(usize, u32)> {
        if self.kind != LIGATURE {
            return None;
        }
        let first = *keys.first()?;
        let starts = |i: usize| {
            self.first_key_at(i)
                .and_then(|at| u32_at(self.data, at))
                == Some(first)
        };
        let mut i = self.search(first, |i| self.first_key_at(i))?;
        while i > 0 && starts(i - 1) {
            i -= 1;
        }
        while i < self.count && starts(i) {
            let at = self.entry(i)?;
            let len = usize::from(*self.data.get(at)?);
            if (2..=MAX_KEYS).contains(&len) && len <= keys.len() {
                let key = |k: usize| u32_at(self.data, at.checked_add(4 + k * 4)?);
                if (1..len).all(|k| key(k) == Some(keys[k])) {
                    return Some((len, key(len)?));
                }
            }
            i += 1;
        }
        None
    }

    /// Applies the lookup to `keys`, keeping them at most `limit` long.
    fn apply(&self, keys: &mut Vec<u32>, limit: usize) {
        let mut i = 0;
        while i < keys.len() {
            match self.kind {
                SINGLE => {
                    if let Some(to) = self.single(keys[i]) {
                        keys[i] = to;
                    }
                }
                LIGATURE => {
                    if let Some((len, to)) = self.ligature(&keys[i..]) {
                        keys.splice(i..i + len, [to]);
                    }
                }
                _ => {
                    if let Some(to) = self.multiple(keys[i]) {
                        let to: Vec<u32> = to.collect();
                        if keys.len() - 1 + to.len() <= limit {
                            let n = to.len();
                            keys.splice(i..=i, to);
                            i += n;
                            continue;
                        }
                    }
                }
            }
            i += 1;
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
