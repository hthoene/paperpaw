//! The substitution tables of a font pack (slot 3, language forms, and
//! slot 4, substitutions; the format is in `bitmap.rs`'s documentation):
//! keys drawn as other keys, as several, or several as one, alone or
//! where the keys around them say (contextual).
//!
//! Read by the Indic shaper (`crate::indic`) from a pack in memory or on
//! a card (through [`Bytes`]), a run of keys at a time ([`Run`]).

#[cfg(any(test, fuzzing))]
use alloc::vec::Vec;

use crate::anchors::Bytes;

/// The table's versions this reader knows: keys and offsets of 4 bytes,
/// and of 2 (compact, the Indic packs'; see `bitmap.rs`).
const VERSION: u8 = 1;
const COMPACT: u8 = 2;
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
const CONTEXT: u8 = 4;
const CLASSES: u8 = 5;
/// A multiple whose keys are each drawn as the same keys around it.
const AROUND: u8 = 6;
/// The most keys a ligature joins, or one key is drawn as, and the most a
/// contextual rule looks at on each side and in its input.
pub(crate) const MAX_KEYS: usize = 16;
/// The most bytes a table may have to be kept in memory where its pack
/// is read from a card (a shaper reads it for every syllable: from a
/// card, a page would take thousands of reads); the pack builder keeps
/// its tables within it.
pub(crate) const MAX_HELD: usize = 32 * 1024;
/// A feature's flag: on unless the text turns it off.
const ON_BY_DEFAULT: u8 = 1;

/// The keys a lookup is applied to: a syllable being shaped, or a list.
pub(crate) trait Run {
    /// How many keys there are.
    fn len(&self) -> usize;
    /// The key at `i` (below [`Run::len`]).
    fn key(&self, i: usize) -> u32;
    /// Whether the lookup being applied may change the key at `i` (the
    /// feature's mask, in a shaper).
    fn may(&self, i: usize) -> bool;
    /// Draws the key at `i` as `key`.
    fn set(&mut self, i: usize, key: u32);
    /// Draws the `n` keys from `i` as one, `key`.
    fn join(&mut self, i: usize, n: usize, key: u32);
    /// Draws the key at `i` as `keys`; `false` (nothing changed) where
    /// there is no room for them.
    fn split(&mut self, i: usize, keys: &[u32]) -> bool;
    /// Moves the key at `from` to `to` (before it), those between one on.
    fn bring(&mut self, from: usize, to: usize);
}

/// A list of keys that grows to at most `limit`.
#[cfg(any(test, fuzzing))]
pub(crate) struct Keys<'a> {
    pub(crate) keys: &'a mut Vec<u32>,
    pub(crate) limit: usize,
}

#[cfg(any(test, fuzzing))]
impl Run for Keys<'_> {
    fn len(&self) -> usize {
        self.keys.len()
    }
    fn key(&self, i: usize) -> u32 {
        self.keys.get(i).copied().unwrap_or(0)
    }
    fn may(&self, _: usize) -> bool {
        true
    }
    fn set(&mut self, i: usize, key: u32) {
        if let Some(k) = self.keys.get_mut(i) {
            *k = key;
        }
    }
    fn join(&mut self, i: usize, n: usize, key: u32) {
        if i < self.keys.len() && n <= self.keys.len() - i {
            self.keys.splice(i..i + n, [key]);
        }
    }
    fn split(&mut self, i: usize, keys: &[u32]) -> bool {
        if i >= self.keys.len() || self.keys.len() - 1 + keys.len() > self.limit {
            return false;
        }
        self.keys.splice(i..=i, keys.iter().copied());
        true
    }
    fn bring(&mut self, from: usize, to: usize) {
        if to < from && from < self.keys.len() {
            let key = self.keys.remove(from);
            self.keys.insert(to, key);
        }
    }
}

/// A substitution table of version 1 or 2, in memory or read through
/// [`Bytes`] (a pack on a card).
pub(crate) struct Substitutions<'a, B: Bytes + ?Sized = [u8]> {
    data: &'a B,
    /// Its length, as its head says.
    len: usize,
    /// How wide its parts are, as its version says.
    w: Widths,
    /// How many key sets lookups skip it lists at its end.
    skips: usize,
}

/// How many bytes the parts of a table take, by its version.
#[derive(Clone, Copy)]
struct Widths {
    key: usize,
    offset: usize,
    /// Reserved after a count of 2 bytes (of a key set, a class definition,
    /// rules by classes).
    pad: usize,
    /// Before a ligature's keys: its length and what is reserved.
    ligature: usize,
    /// After a multiple's key, before its keys: its count and what is
    /// reserved.
    multiple: usize,
    /// A rule's record: its place in the input and the offset of a
    /// lookup's record.
    record: usize,
}

impl Widths {
    const WIDE: Widths = Widths {
        key: 4,
        offset: 4,
        pad: 2,
        ligature: 4,
        multiple: 4,
        record: 4,
    };
    const COMPACT: Widths = Widths {
        key: 2,
        offset: 2,
        pad: 0,
        ligature: 1,
        multiple: 1,
        record: 3,
    };
}

/// The first compact key of a glyph without code point
/// ([`GLYPHS`](crate::indic::GLYPHS) and its number); from
/// [`COMPACT_MARKS`], of a mark's ([`MARKS`](crate::indic::MARKS)). Below
/// it, a character's: keys of characters from U+D800 on and of glyphs
/// numbered from 4,096 (marks 6,144) on have no compact key.
const COMPACT_GLYPHS: u16 = 0xD800;
const COMPACT_MARKS: u16 = 0xE800;

/// The key a compact table's `key` stands for.
fn wide_key(key: u16) -> u32 {
    match key {
        0..COMPACT_GLYPHS => u32::from(key),
        COMPACT_GLYPHS..COMPACT_MARKS => crate::indic::GLYPHS + u32::from(key - COMPACT_GLYPHS),
        _ => crate::indic::MARKS + u32::from(key - COMPACT_MARKS),
    }
}

/// The compact key of `key`, if it has one (see [`wide_key`]).
#[cfg(feature = "outline")]
pub(crate) fn compact_key(key: u32) -> Option<u16> {
    let (glyphs, marks) = (crate::indic::GLYPHS, crate::indic::MARKS);
    let in_range = |base: u32, first: u16, end: u16| {
        let n = key.checked_sub(base)?;
        u16::try_from(n)
            .ok()
            .filter(|n| *n < end - first)
            .map(|n| first + n)
    };
    match key {
        k if k < u32::from(COMPACT_GLYPHS) => u16::try_from(k).ok(),
        k if k >= marks => in_range(marks, COMPACT_MARKS, u16::MAX),
        k if k >= glyphs => in_range(glyphs, COMPACT_GLYPHS, COMPACT_MARKS),
        _ => None,
    }
}

impl<B: Bytes + ?Sized> Clone for Substitutions<'_, B> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<B: Bytes + ?Sized> Copy for Substitutions<'_, B> {}

#[cfg(any(test, fuzzing))]
impl<'a> Substitutions<'a, [u8]> {
    /// The table in `table` (as a pack's directory gives it), if it is of
    /// a version this reader knows and as long as it says.
    pub(crate) fn new(table: &'a [u8]) -> Option<Self> {
        Substitutions::read(table)
    }
}

impl<'a, B: Bytes + ?Sized> Substitutions<'a, B> {
    /// The table `table` holds from its start, if it is of a version this
    /// reader knows and has the bytes its head says.
    pub(crate) fn read(table: &'a B) -> Option<Self> {
        let mut head = [0u8; HEAD];
        if !table.bytes(0, &mut head) {
            return None;
        }
        let w = match head[0] {
            VERSION => Widths::WIDE,
            COMPACT => Widths::COMPACT,
            _ => return None,
        };
        let len = usize::try_from(u32::from_le_bytes([head[4], head[5], head[6], head[7]])).ok()?;
        let mut last = [0u8];
        let mut count = [0u8; 4];
        if len < HEAD + 4 || !table.bytes(len - 1, &mut last) || !table.bytes(HEAD, &mut count) {
            return None;
        }
        // Lookups' skip sets, listed at the end (none in a table made
        // before them, whose bytes here are 0).
        let skips = usize::from(u16::from_le_bytes([count[2], count[3]]));
        let skips = if skips * w.offset <= len - HEAD - 4 {
            skips
        } else {
            0
        };
        Some(Substitutions {
            data: table,
            len,
            w,
            skips,
        })
    }

    fn bytes<const N: usize>(&self, at: usize) -> Option<[u8; N]> {
        if at.checked_add(N)? > self.len {
            return None;
        }
        let mut buf = [0u8; N];
        self.data.bytes(at, &mut buf).then_some(buf)
    }

    fn u8_at(&self, at: usize) -> Option<u8> {
        self.bytes::<1>(at).map(|b| b[0])
    }

    fn u16_at(&self, at: usize) -> Option<u16> {
        self.bytes::<2>(at).map(u16::from_le_bytes)
    }

    fn u32_at(&self, at: usize) -> Option<u32> {
        self.bytes::<4>(at).map(u32::from_le_bytes)
    }

    fn usize_at(&self, at: usize) -> Option<usize> {
        self.u32_at(at).and_then(|v| usize::try_from(v).ok())
    }

    /// The key at `at`.
    fn key_at(&self, at: usize) -> Option<u32> {
        match self.w.key {
            2 => self.u16_at(at).map(wide_key),
            _ => self.u32_at(at),
        }
    }

    /// A rule's record at `at`: its place in the input and where the
    /// lookup's record is (version 1: a u32, the place in its top byte;
    /// 2: the place u8, the offset u16).
    fn record_at(&self, at: usize) -> Option<(usize, usize)> {
        match self.w.record {
            3 => Some((
                usize::from(self.u8_at(at)?),
                usize::from(self.u16_at(at + 1)?),
            )),
            _ => {
                let record = self.u32_at(at)?;
                Some(((record >> 24) as usize, (record & 0x00FF_FFFF) as usize))
            }
        }
    }

    /// The offset at `at` (of a part of the table).
    fn offset_at(&self, at: usize) -> Option<usize> {
        match self.w.offset {
            2 => self.u16_at(at).map(usize::from),
            _ => self.usize_at(at),
        }
    }

    /// The features, in the order they apply.
    pub(crate) fn features(&self) -> impl Iterator<Item = Feature<'a, B>> + 'a {
        let table = *self;
        let count = table.u16_at(HEAD).unwrap_or(0);
        (0..usize::from(count)).map_while(move |i| {
            let at = HEAD + 4 + i * FEATURE;
            let record = table.bytes::<FEATURE>(at)?;
            Some(Feature {
                table,
                tag: [record[0], record[1], record[2], record[3]],
                script: record[4],
                flags: record[5],
                language: [record[8], record[9], record[10], record[11]],
                lookups: usize::try_from(u32::from_le_bytes([
                    record[12], record[13], record[14], record[15],
                ]))
                .ok()?,
            })
        })
    }

    /// Applies the features on by default for text of `script` (its index
    /// in `paperpaw_lang::scripts()` and 1) and `language` (a language
    /// code: `hi`, `sa`) to `keys`, in their order. `keys` grows to at most
    /// `limit` keys: a substitution that would make it longer is not made.
    #[cfg(any(test, fuzzing))]
    pub(crate) fn apply(&self, script: u8, language: &str, keys: &mut Vec<u32>, limit: usize) {
        let mut run = Keys { keys, limit };
        for feature in self.features() {
            if feature.on_by_default() && feature.applies(script, language) {
                feature.apply(&mut run);
            }
        }
    }

    /// The lookup whose 8-byte record is at `at`.
    fn lookup_at(&self, at: usize) -> Option<Lookup<'a, B>> {
        let record = self.bytes::<LOOKUP>(at)?;
        let kind = record[0];
        let count = usize::from(u16::from_le_bytes([record[2], record[3]]));
        let entries = usize::try_from(u32::from_le_bytes([
            record[4], record[5], record[6], record[7],
        ]))
        .ok()?;
        // The keys it skips (OpenType's lookup flags): a set the table
        // lists at its end, from 1.
        let skip = match usize::from(record[1]) {
            0 => 0,
            n if n <= self.skips => {
                self.offset_at(self.len - self.w.offset * (self.skips + 1 - n))?
            }
            _ => 0,
        };
        matches!(
            kind,
            SINGLE | LIGATURE | MULTIPLE | CONTEXT | CLASSES | AROUND
        )
        .then_some(Lookup {
            table: *self,
            entries,
            count,
            kind,
            skip,
        })
    }

    /// Whether the key set at `at` (count u16, 2 reserved, sorted keys)
    /// holds `key`.
    fn set_has(&self, at: usize, key: u32) -> bool {
        let Some(count) = self.u16_at(at) else {
            return false;
        };
        let keys = at + 2 + self.w.pad;
        let (mut low, mut high) = (0, usize::from(count));
        while low < high {
            let mid = low + (high - low) / 2;
            let Some(found) = self.key_at(keys + mid * self.w.key) else {
                return false;
            };
            match found.cmp(&key) {
                core::cmp::Ordering::Less => low = mid + 1,
                core::cmp::Ordering::Greater => high = mid,
                core::cmp::Ordering::Equal => return true,
            }
        }
        false
    }

    /// The class of `key` in the class definition at `at` (count u16, 2
    /// reserved, then key u32 and class u16 per key, sorted by key): 0
    /// for a key it does not list, or no definition (`at` 0).
    fn class_of(&self, at: usize, key: u32) -> u16 {
        let Some(count) = self.u16_at(at).filter(|_| at != 0) else {
            return 0;
        };
        let (mut low, mut high) = (0, usize::from(count));
        while low < high {
            let mid = low + (high - low) / 2;
            let entry = at + 2 + self.w.pad + mid * (self.w.key + 2);
            let Some(found) = self.key_at(entry) else {
                return 0;
            };
            match found.cmp(&key) {
                core::cmp::Ordering::Less => low = mid + 1,
                core::cmp::Ordering::Greater => high = mid,
                core::cmp::Ordering::Equal => {
                    return self.u16_at(entry + self.w.key).unwrap_or(0);
                }
            }
        }
        0
    }
}

/// A feature: an OpenType feature tag and the lookups it applies.
pub(crate) struct Feature<'a, B: Bytes + ?Sized = [u8]> {
    table: Substitutions<'a, B>,
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

impl<B: Bytes + ?Sized> Clone for Feature<'_, B> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<B: Bytes + ?Sized> Copy for Feature<'_, B> {}

impl<'a, B: Bytes + ?Sized> Feature<'a, B> {
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
    pub(crate) fn lookups(&self) -> impl Iterator<Item = Lookup<'a, B>> + 'a {
        let (table, list) = (self.table, self.lookups);
        let count = table.u16_at(list).unwrap_or(0);
        (0..usize::from(count))
            .map_while(move |i| list.checked_add(4 + i * LOOKUP))
            .filter_map(move |at| table.lookup_at(at))
    }

    /// Applies its lookups to `run`, each to all of it before the next.
    pub(crate) fn apply(&self, run: &mut impl Run) {
        for lookup in self.lookups() {
            lookup.apply(run);
        }
    }
}

/// A lookup: keys drawn as one other key (single), several keys as one
/// (ligature), one key as several (multiple), or other lookups applied
/// where keys around say so (contextual).
pub(crate) struct Lookup<'a, B: Bytes + ?Sized = [u8]> {
    table: Substitutions<'a, B>,
    entries: usize,
    count: usize,
    kind: u8,
    /// Where the key set of the keys it passes over is (0: none).
    skip: usize,
}

impl<B: Bytes + ?Sized> Clone for Lookup<'_, B> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<B: Bytes + ?Sized> Copy for Lookup<'_, B> {}

impl<B: Bytes + ?Sized> Lookup<'_, B> {
    /// Whether it passes over `key` (a mark it ignores): neither applies to
    /// it nor looks at it.
    fn skips(&self, key: u32) -> bool {
        self.skip != 0 && self.table.set_has(self.skip, key)
    }

    /// The place of the first key after `i` it does not pass over.
    fn next(&self, run: &impl Run, i: usize) -> Option<usize> {
        (i + 1..run.len()).find(|&k| !self.skips(run.key(k)))
    }

    /// The place of the last key before `i` it does not pass over.
    fn before(&self, run: &impl Run, i: usize) -> Option<usize> {
        (0..i).rev().find(|&k| !self.skips(run.key(k)))
    }

    /// The place of an entry whose key is `key`, by a binary search of the
    /// entries' keys (`key_at` says where entry `i`'s is): the first of
    /// those with that key. Entries out of order are not found, nothing
    /// worse.
    fn search(&self, key: u32, key_at: impl Fn(usize) -> Option<usize>) -> Option<usize> {
        let (mut low, mut high) = (0, self.count);
        let found = |i: usize| key_at(i).and_then(|at| self.table.key_at(at));
        while low < high {
            let mid = low + (high - low) / 2;
            match found(mid)?.cmp(&key) {
                core::cmp::Ordering::Less => low = mid + 1,
                core::cmp::Ordering::Greater => high = mid,
                core::cmp::Ordering::Equal => {
                    let mut i = mid;
                    while i > 0 && found(i - 1) == Some(key) {
                        i -= 1;
                    }
                    return Some(i);
                }
            }
        }
        None
    }

    /// Where entry `i` of a ligature or multiple lookup is.
    fn entry(&self, i: usize) -> Option<usize> {
        let w = self.table.w;
        self.table
            .offset_at(self.entries.checked_add(i.checked_mul(w.offset)?)?)
    }

    /// Where the first key of a ligature lookup's entry `i` is.
    fn first_key_at(&self, i: usize) -> Option<usize> {
        self.entry(i)?.checked_add(self.table.w.ligature)
    }

    /// Where the record of a contextual lookup's entry `i` is (key, the
    /// offset of its rule).
    fn context_record(&self, i: usize) -> Option<usize> {
        let w = self.table.w;
        self.entries.checked_add(i.checked_mul(w.key + w.offset)?)
    }

    /// The key `key` is drawn as, by a single lookup.
    pub(crate) fn single(&self, key: u32) -> Option<u32> {
        if self.kind != SINGLE {
            return None;
        }
        let w = self.table.w.key;
        let record = |i: usize| self.entries.checked_add(i.checked_mul(2 * w)?);
        let i = self.search(key, record)?;
        self.table.key_at(record(i)?.checked_add(w)?)
    }

    /// The keys `key` is drawn as, by a multiple lookup, into `out`: how
    /// many.
    pub(crate) fn multiple(&self, key: u32, out: &mut [u32; MAX_KEYS]) -> Option<usize> {
        if self.kind == AROUND {
            return self.around(key, out);
        }
        if self.kind != MULTIPLE {
            return None;
        }
        let w = self.table.w;
        let at = self.entry(self.search(key, |i| self.entry(i))?)?;
        let count = usize::from(self.table.u8_at(at.checked_add(w.key)?)?);
        if !(1..=MAX_KEYS).contains(&count) {
            return None;
        }
        let keys = at.checked_add(w.key + w.multiple)?;
        for (k, slot) in out.iter_mut().enumerate().take(count) {
            *slot = self.table.key_at(keys + k * w.key)?;
        }
        Some(count)
    }

    /// The keys `key` is drawn as by a lookup of kind 6, into `out`: how
    /// many. Its entries: the offset of the key set of the keys it draws,
    /// how many keys go before and after (u8 each; in version 1 2
    /// reserved), those keys.
    fn around(&self, key: u32, out: &mut [u32; MAX_KEYS]) -> Option<usize> {
        let t = self.table;
        if !t.set_has(t.offset_at(self.entries)?, key) {
            return None;
        }
        let at = self.entries.checked_add(t.w.offset)?;
        let [before, after] = t.bytes::<2>(at)?.map(usize::from);
        let count = before + 1 + after;
        if count > MAX_KEYS {
            return None;
        }
        let keys = at + 2 + t.w.pad;
        out[before] = key;
        for (k, slot) in (0..before).chain(before + 1..count).enumerate() {
            out[slot] = t.key_at(keys + k * t.w.key)?;
        }
        Some(count)
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
        let starts =
            |i: usize| self.first_key_at(i).and_then(|at| self.table.key_at(at)) == Some(first);
        let mut i = self.search(first, |i| self.first_key_at(i))?;
        while i < self.count && starts(i) {
            let at = self.entry(i)?;
            let len = usize::from(self.table.u8_at(at)?);
            if (2..=MAX_KEYS).contains(&len) && len <= keys.len() {
                let w = self.table.w;
                let key = |k: usize| self.table.key_at(at.checked_add(w.ligature + k * w.key)?);
                if (1..len).all(|k| key(k) == Some(keys[k])) {
                    return Some((len, key(len)?));
                }
            }
            i += 1;
        }
        None
    }

    /// Applies the lookup to `run`, at each place from its start.
    pub(crate) fn apply(&self, run: &mut impl Run) {
        let mut i = 0;
        while i < run.len() {
            i = self.apply_at(run, i, true).unwrap_or(i + 1);
        }
    }

    /// Applies the lookup at place `i` of `run`, if it makes a change
    /// there: where to go on from. A contextual lookup applies others
    /// (`nested` false: those do not apply contextual ones in turn).
    fn apply_at(&self, run: &mut impl Run, i: usize, nested: bool) -> Option<usize> {
        if !run.may(i) || self.skips(run.key(i)) {
            return None;
        }
        match self.kind {
            SINGLE => {
                let to = self.single(run.key(i))?;
                run.set(i, to);
                Some(i + 1)
            }
            LIGATURE => {
                // The keys from `i` on it does not pass over, and where.
                let (mut keys, mut at) = ([0u32; MAX_KEYS], [0usize; MAX_KEYS]);
                let (mut n, mut place) = (0, Some(i));
                while let Some(p) = place.filter(|_| n < MAX_KEYS) {
                    (keys[n], at[n]) = (run.key(p), p);
                    n += 1;
                    place = self.next(run, p);
                }
                let (len, to) = self.ligature(&keys[..n])?;
                if !at[..len].iter().all(|&k| run.may(k)) {
                    return None;
                }
                // Those passed over go after the ligature.
                for (k, &from) in at.iter().enumerate().take(len).skip(1) {
                    run.bring(from, i + k);
                }
                run.join(i, len, to);
                Some(i + 1)
            }
            MULTIPLE | AROUND => {
                let mut keys = [0u32; MAX_KEYS];
                let n = self.multiple(run.key(i), &mut keys)?;
                run.split(i, &keys[..n]).then_some(i + n)
            }
            CONTEXT if nested => self.context(run, i),
            CLASSES if nested => self.classes(run, i),
            _ => None,
        }
    }

    /// A contextual lookup at place `i`: the first of the rules for its
    /// key whose keys around match, its lookups applied; where to go on
    /// from (after its input).
    fn context(&self, run: &mut impl Run, i: usize) -> Option<usize> {
        let key = run.key(i);
        let mut e = self.search(key, |e| self.context_record(e))?;
        while e < self.count {
            let record = self.context_record(e)?;
            if self.table.key_at(record)? != key {
                return None;
            }
            let rule = self
                .table
                .offset_at(record.checked_add(self.table.w.key)?)?;
            if let Some(next) = self.rule(run, i, rule) {
                return Some(next);
            }
            e += 1;
        }
        None
    }

    /// The rule at `rule`, tried at place `i` (whose key is its first);
    /// where to go on from, if it matched. A rule: backtrack, input and
    /// lookahead counts and its records' count (u8 each), the offsets of
    /// the key sets of the backtrack (nearest first), of the input after
    /// its first key and of the lookahead, then its records (see
    /// [`Substitutions::record_at`]).
    fn rule(&self, run: &mut impl Run, i: usize, rule: usize) -> Option<usize> {
        let t = self.table;
        let [back, input, ahead, records] = t.bytes::<4>(rule)?.map(usize::from);
        if input == 0 || back > MAX_KEYS || input > MAX_KEYS || ahead > MAX_KEYS {
            return None;
        }
        let set = |n: usize| t.offset_at(rule + 4 + n * t.w.offset);
        let mut at = i;
        for k in 0..back {
            at = self.before(run, at)?;
            if !t.set_has(set(k)?, run.key(at)) {
                return None;
            }
        }
        let mut last = i;
        for k in 1..input {
            last = self.next(run, last)?;
            if !run.may(last) || !t.set_has(set(back + k - 1)?, run.key(last)) {
                return None;
            }
        }
        let mut at = last;
        for k in 0..ahead {
            at = self.next(run, at)?;
            if !t.set_has(set(back + input - 1 + k)?, run.key(at)) {
                return None;
            }
        }
        let first = rule + 4 + (back + input - 1 + ahead) * t.w.offset;
        Some(self.records(run, i, last + 1, first, records))
    }

    /// Applies the `count` records at `at` (each its place in the input
    /// and the offset of a lookup's record, see [`Substitutions::record_at`])
    /// to the input from place `i` to `end`: where to go on from.
    fn records(
        &self,
        run: &mut impl Run,
        i: usize,
        mut end: usize,
        at: usize,
        count: usize,
    ) -> usize {
        let t = self.table;
        // In their order; a ligature or a multiple changes how long the
        // input is.
        for r in 0..count {
            let Some((place, record)) = t.record_at(at + r * t.w.record) else {
                break;
            };
            let Some(lookup) = t.lookup_at(record) else {
                continue;
            };
            // The input's key `place`, those passed over not counted.
            let Some(p) = (0..place).try_fold(i, |p, _| self.next(run, p)) else {
                continue;
            };
            if p >= end.min(run.len()) {
                continue;
            }
            let before = run.len();
            if lookup.apply_at(run, p, false).is_some() {
                end = (end + run.len()).saturating_sub(before).max(i + 1);
            }
        }
        end.max(i + 1)
    }

    /// A lookup of rules by classes at place `i`. Its entries: the
    /// offsets of its class definitions before, of the input and after
    /// (0: none), of the key set of the keys it starts at, then
    /// per class of its first key the offset of its rules (0: none; as
    /// many as the lookup's count). Rules: count u16 (in version 1 and 2
    /// reserved), the offset of each; a rule: backtrack, input and lookahead counts
    /// and its records' count (u8 each), the classes (u8 each) before
    /// (nearest first), of the input after its first key and after, then
    /// its records (see [`Lookup::records`]).
    fn classes(&self, run: &mut impl Run, i: usize) -> Option<usize> {
        let t = self.table;
        let h = self.entries;
        let o = t.w.offset;
        let [back_def, input_def, ahead_def, starts] =
            [0, o, 2 * o, 3 * o].map(|at| t.offset_at(h + at).unwrap_or(0));
        let key = run.key(i);
        if !t.set_has(starts, key) {
            return None;
        }
        let class = usize::from(t.class_of(input_def, key));
        if class >= self.count {
            return None;
        }
        let rules = t.offset_at(h + 4 * o + class * o).filter(|&r| r != 0)?;
        let count = usize::from(t.u16_at(rules)?);
        for r in 0..count {
            let rule = t.offset_at(rules + 2 + t.w.pad + r * o)?;
            let [back, input, ahead, records] = t.bytes::<4>(rule)?.map(usize::from);
            if input == 0 {
                continue;
            }
            let class_at = |n: usize| t.u8_at(rule + 4 + n).map(u16::from);
            if let Some(end) = self.class_fits(
                run,
                i,
                [back, input, ahead],
                [back_def, input_def, ahead_def],
                class_at,
            ) {
                let first = rule + 4 + back + input - 1 + ahead;
                return Some(self.records(run, i, end, first, records));
            }
        }
        None
    }
}

impl<B: Bytes + ?Sized> Lookup<'_, B> {
    /// Whether the keys around place `i` (those it passes over not looked
    /// at) are of the classes a rule of `counts` (before, input, after)
    /// says, by the class definitions `defs`: where its input ends.
    fn class_fits(
        &self,
        run: &impl Run,
        i: usize,
        counts: [usize; 3],
        defs: [usize; 3],
        class_at: impl Fn(usize) -> Option<u16>,
    ) -> Option<usize> {
        let t = self.table;
        let [back, input, ahead] = counts;
        let mut at = i;
        for k in 0..back {
            at = self.before(run, at)?;
            if class_at(k) != Some(t.class_of(defs[0], run.key(at))) {
                return None;
            }
        }
        let mut last = i;
        for k in 1..input {
            last = self.next(run, last)?;
            if !run.may(last) || class_at(back + k - 1) != Some(t.class_of(defs[1], run.key(last)))
            {
                return None;
            }
        }
        let mut at = last;
        for k in 0..ahead {
            at = self.next(run, at)?;
            if class_at(back + input - 1 + k) != Some(t.class_of(defs[2], run.key(at))) {
                return None;
            }
        }
        Some(last + 1)
    }
}

#[cfg(test)]
pub(crate) mod tests;
