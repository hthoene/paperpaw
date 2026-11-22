//! The substitutions of a font's scripts shaped by syllables (the Indic
//! ones, Khmer, Lao, Tibetan and Myanmar: [`TAGS`]) as a pack keeps them
//! (slot 4, see `bitmap.rs`): what the shaper ([`super::Shaper`]) applies,
//! read from the font's GSUB for the scripts' second version (`bng2`,
//! `dev2`; else the first), feature by feature in the order the shaper
//! applies them; the presentation features as one (`pres`), their lookups
//! in the font's order, as OpenType applies them together.
//!
//! Glyphs are keys: the character the pack draws them for, else
//! [`GLYPHS`] or [`MARKS`] and their number. Contextual lookups (GSUB 5
//! and 6, all three formats) become rules of kind 4, listed by the keys
//! they start at; reverse and alternate lookups are left out, as are the
//! contextual lookups a contextual one applies.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;

use read_fonts::tables::gsub::{SingleSubst, SubstitutionSubtables};
use read_fonts::tables::layout::{
    ChainedSequenceContext, ClassDef, CoverageTable, SequenceContext, SequenceLookupRecord,
};
use read_fonts::types::{GlyphId16, Tag};
use read_fonts::{FontRef, TableProvider};

use super::{GLYPHS, MARKS, MAX_GLYPH, Script};
use crate::marks::lookups;
use crate::substitutions::{MAX_HELD, MAX_KEYS};

/// The features in the order the shaper applies them; `pres` stands for
/// all presentation features together.
const ORDER: [&[u8; 4]; 15] = [
    b"ccmp", b"locl", b"nukt", b"akhn", b"rphf", b"rkrf", b"pref", b"blwf", b"abvf", b"half",
    b"pstf", b"vatu", b"cjct", b"init", b"pres",
];

/// The presentation features (and the common ones OpenType applies with
/// them), applied as one.
const PRESENTATION: [&[u8; 4]; 10] = [
    b"pres", b"abvs", b"blws", b"psts", b"haln", b"calt", b"clig", b"liga", b"rclt", b"rlig",
];

/// The table and the keys of the glyphs without code point it draws.
pub(crate) struct Made {
    /// The whole table, its head included.
    pub(crate) table: Vec<u8>,
    pub(crate) keys: Vec<char>,
}

/// A contextual rule: key sets before (nearest first), of the input after
/// its first key, after, and the lookups it applies (place, lookup).
struct Rule {
    back: Vec<Vec<u32>>,
    input: Vec<Vec<u32>>,
    ahead: Vec<Vec<u32>>,
    records: Vec<(u8, u16)>,
}

struct Writer<'a> {
    font: FontRef<'a>,
    /// Glyph to key, for the glyphs of the pack's characters.
    cmap: BTreeMap<u16, u32>,
    marks: Option<ClassDef<'a>>,
    out: Vec<u8>,
    /// GSUB lookup to its record (8 bytes), once written.
    records: BTreeMap<u16, Option<[u8; 8]>>,
    /// GSUB lookup to where its record stands alone (for rules).
    alone: BTreeMap<u16, usize>,
    sets: BTreeMap<Vec<u32>, usize>,
    classes: BTreeMap<Vec<(u32, u16)>, usize>,
    /// Glyphs drawn as results.
    drawn: BTreeSet<u16>,
    /// The glyphs text can come to: those of the pack's characters and
    /// what the lookups draw; others are left out of the table.
    reachable: BTreeSet<u16>,
    /// Keys and offsets of 2 bytes (version 2), not 4.
    compact: bool,
    /// A key or an offset that has no 2 bytes was written: the table is
    /// to be written in version 1.
    too_wide: bool,
    /// Where the key sets lookups skip are, in the order the table lists
    /// them.
    skips: Vec<usize>,
}

fn put_u32(out: &mut Vec<u8>, v: usize) {
    out.extend_from_slice(&u32::try_from(v).unwrap_or(0).to_le_bytes());
}

fn set_u32(out: &mut [u8], at: usize, v: usize) {
    if let Some(b) = out.get_mut(at..at + 4) {
        b.copy_from_slice(&u32::try_from(v).unwrap_or(0).to_le_bytes());
    }
}

/// A rule's record: the place in its input in the top byte, the offset of
/// the lookup's record below.
fn record(place: u8, lookup: usize) -> u32 {
    (u32::from(place) << 24) | (u32::try_from(lookup).unwrap_or(0) & 0x00FF_FFFF)
}

fn coverage(c: &CoverageTable<'_>) -> Vec<u16> {
    c.iter().map(|g| g.to_u16()).collect()
}

impl Writer<'_> {
    /// Writes key `k`.
    fn put_key(&mut self, k: u32) {
        if !self.compact {
            return put_u32(&mut self.out, k as usize);
        }
        let compact = crate::substitutions::compact_key(k);
        self.too_wide |= compact.is_none();
        self.out
            .extend_from_slice(&compact.unwrap_or(0).to_le_bytes());
    }

    /// The bytes an offset takes.
    fn offset_width(&self) -> usize {
        if self.compact { 2 } else { 4 }
    }

    /// Writes offset `at` at `place` (`None`: at the end).
    fn put_offset(&mut self, place: Option<usize>, at: usize) {
        let bytes = match self.compact {
            true => {
                let at = u16::try_from(at);
                self.too_wide |= at.is_err();
                at.unwrap_or(0).to_le_bytes().to_vec()
            }
            false => u32::try_from(at).unwrap_or(0).to_le_bytes().to_vec(),
        };
        match place.and_then(|p| self.out.get_mut(p..p + bytes.len())) {
            Some(slot) => slot.copy_from_slice(&bytes),
            None => self.out.extend_from_slice(&bytes),
        }
    }

    /// Writes a count of 2 bytes and, in version 1, 2 reserved.
    fn put_count(&mut self, n: usize) {
        self.out
            .extend_from_slice(&u16::try_from(n).unwrap_or(0).to_le_bytes());
        if !self.compact {
            self.out.extend_from_slice(&[0, 0]);
        }
    }

    /// Writes a count of 1 byte (a ligature's length, a multiple's keys)
    /// and, in version 1, 3 reserved.
    fn put_small(&mut self, n: usize) {
        self.out.push(n as u8);
        if !self.compact {
            self.out.extend_from_slice(&[0, 0, 0]);
        }
    }

    /// Writes a rule's record: the place in its input, the offset of the
    /// lookup's record.
    fn put_record(&mut self, place: u8, lookup: usize) {
        if !self.compact {
            return put_u32(&mut self.out, record(place, lookup) as usize);
        }
        self.out.push(place);
        self.put_offset(None, lookup);
    }

    /// The key of glyph `g`.
    fn key(&self, g: u16) -> Option<u32> {
        if let Some(&k) = self.cmap.get(&g) {
            return Some(k);
        }
        if u32::from(g) >= MAX_GLYPH || !self.reachable.contains(&g) {
            return None;
        }
        let mark = self
            .marks
            .as_ref()
            .is_some_and(|c| c.get(GlyphId16::new(g)) == 3);
        Some(u32::from(g) + if mark { MARKS } else { GLYPHS })
    }

    /// The key of a glyph drawn as a result.
    fn drawn_key(&mut self, g: u16) -> Option<u32> {
        let key = self.key(g)?;
        if !self.cmap.contains_key(&g) {
            self.drawn.insert(g);
        }
        Some(key)
    }

    fn keys(&self, glyphs: impl IntoIterator<Item = u16>) -> Vec<u32> {
        let mut keys: Vec<u32> = glyphs.into_iter().filter_map(|g| self.key(g)).collect();
        keys.sort_unstable();
        keys.dedup();
        keys
    }

    /// Where the key set `keys` is written.
    fn set(&mut self, keys: Vec<u32>) -> usize {
        if let Some(&at) = self.sets.get(&keys) {
            return at;
        }
        let at = self.out.len();
        self.put_count(keys.len());
        for &k in &keys {
            self.put_key(k);
        }
        self.sets.insert(keys, at);
        at
    }

    /// The record of GSUB lookup `index` in a pack's form (written once),
    /// if it is of a kind a pack keeps; `nested`: for a rule (no
    /// contextual ones).
    fn lookup(&mut self, index: u16, nested: bool) -> Option<[u8; 8]> {
        if let Some(&record) = self.records.get(&index) {
            // A rule applies no contextual lookups.
            return record.filter(|r| !nested || !matches!(r[0], 4 | 5));
        }
        // Not twice, also not while it is being written.
        self.records.insert(index, None);
        let list = self.font.gsub().ok()?.lookup_list().ok()?;
        let lookup = list.lookups().get(usize::from(index)).ok()?;
        let record = match lookup.subtables().ok()? {
            SubstitutionSubtables::Single(subtables) => {
                self.singles(subtables.iter().flatten().collect())
            }
            SubstitutionSubtables::Multiple(subtables) => {
                self.multiples(subtables.iter().flatten().collect())
            }
            SubstitutionSubtables::Ligature(subtables) => {
                self.ligatures(subtables.iter().flatten().collect())
            }
            SubstitutionSubtables::Contextual(subtables) => {
                let all: Vec<_> = subtables.iter().flatten().collect();
                if let [SequenceContext::Format2(t)] = all.as_slice() {
                    let made = self.context_classes(t);
                    return self
                        .finish(index, made)
                        .filter(|r| !nested || !matches!(r[0], 4 | 5));
                }
                let mut rules = Vec::new();
                for t in subtables.iter().flatten() {
                    self.context(&t, &mut rules);
                }
                self.rules(rules)
            }
            SubstitutionSubtables::ChainContextual(subtables) => {
                let all: Vec<_> = subtables.iter().flatten().collect();
                if let [ChainedSequenceContext::Format2(t)] = all.as_slice() {
                    let made = self.chained_classes(t);
                    return self
                        .finish(index, made)
                        .filter(|r| !nested || !matches!(r[0], 4 | 5));
                }
                let mut rules = Vec::new();
                for t in subtables.iter().flatten() {
                    self.chained(&t, &mut rules);
                }
                self.rules(rules)
            }
            _ => None,
        };
        self.finish(index, record)
            .filter(|r| !nested || !matches!(r[0], 4 | 5))
    }

    fn singles(&mut self, subtables: Vec<SingleSubst<'_>>) -> Option<(u8, usize, usize)> {
        let mut map = BTreeMap::new();
        for t in subtables {
            let pairs: Vec<(u16, u16)> = match &t {
                SingleSubst::Format1(t) => t.coverage().map_or(Vec::new(), |c| {
                    let delta = t.delta_glyph_id();
                    coverage(&c)
                        .into_iter()
                        .map(|g| (g, (i32::from(g) + i32::from(delta)) as u16))
                        .collect()
                }),
                SingleSubst::Format2(t) => t.coverage().map_or(Vec::new(), |c| {
                    coverage(&c)
                        .into_iter()
                        .zip(t.substitute_glyph_ids().iter().map(|g| g.get().to_u16()))
                        .collect()
                }),
            };
            for (from, to) in pairs {
                if let (Some(f), Some(to)) = (self.key(from), self.drawn_key(to)) {
                    map.entry(f).or_insert(to);
                }
            }
        }
        let entries = self.out.len();
        for (f, t) in &map {
            self.put_key(*f);
            self.put_key(*t);
        }
        Some((1u8, map.len(), entries))
    }

    fn multiples(
        &mut self,
        subtables: Vec<read_fonts::tables::gsub::MultipleSubstFormat1<'_>>,
    ) -> Option<(u8, usize, usize)> {
        let mut map: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for t in subtables {
            let Ok(c) = t.coverage() else { continue };
            for (g, seq) in coverage(&c).into_iter().zip(t.sequences().iter()) {
                let Ok(seq) = seq else { continue };
                let to: Option<Vec<u32>> = seq
                    .substitute_glyph_ids()
                    .iter()
                    .map(|g| self.drawn_key(g.get().to_u16()))
                    .collect();
                if let (Some(f), Some(to)) = (self.key(g), to)
                    && (1..=MAX_KEYS).contains(&to.len())
                {
                    map.entry(f).or_insert(to);
                }
            }
        }
        if let Some(made) = self.around(&map) {
            return Some(made);
        }
        let (entries, o) = (self.out.len(), self.offset_width());
        self.out.resize(entries + map.len() * o, 0);
        for (i, (f, to)) in map.iter().enumerate() {
            let at = self.out.len();
            self.put_offset(Some(entries + i * o), at);
            self.put_key(*f);
            self.put_small(to.len());
            for k in to {
                self.put_key(*k);
            }
        }
        Some((3, map.len(), entries))
    }

    /// Writes `map` as a lookup of kind 6 if each key in it is drawn as
    /// the same keys around it (a spacing glyph before a letter, 300
    /// times in Noto Serif Myanmar): its key set and those keys once.
    fn around(&mut self, map: &BTreeMap<u32, Vec<u32>>) -> Option<(u8, usize, usize)> {
        let (&first, to) = map.iter().next().filter(|_| map.len() > 1)?;
        let at = to.iter().position(|&k| k == first)?;
        let (before, after) = (&to[..at], &to[at + 1..]);
        let same = |(&f, to): (&u32, &Vec<u32>)| {
            to.len() == before.len() + 1 + after.len()
                && to[..at] == *before
                && to[at] == f
                && to[at + 1..] == *after
        };
        if !map.iter().all(same) {
            return None;
        }
        let (before, after) = (before.to_vec(), after.to_vec());
        let set = self.set(map.keys().copied().collect());
        let entries = self.out.len();
        self.put_offset(None, set);
        self.out
            .extend_from_slice(&[before.len() as u8, after.len() as u8]);
        if !self.compact {
            self.out.extend_from_slice(&[0, 0]);
        }
        for &k in before.iter().chain(&after) {
            self.put_key(k);
        }
        Some((6, map.len(), entries))
    }

    fn ligatures(
        &mut self,
        subtables: Vec<read_fonts::tables::gsub::LigatureSubstFormat1<'_>>,
    ) -> Option<(u8, usize, usize)> {
        let mut all: Vec<(Vec<u32>, u32)> = Vec::new();
        for t in subtables {
            let Ok(c) = t.coverage() else { continue };
            for (g, set) in coverage(&c).into_iter().zip(t.ligature_sets().iter()) {
                let Ok(set) = set else { continue };
                for lig in set.ligatures().iter().flatten() {
                    let keys: Option<Vec<u32>> = core::iter::once(g)
                        .chain(lig.component_glyph_ids().iter().map(|g| g.get().to_u16()))
                        .map(|g| self.key(g))
                        .collect();
                    let to = self.drawn_key(lig.ligature_glyph().to_u16());
                    if let (Some(keys), Some(to)) = (keys, to)
                        && (2..=MAX_KEYS).contains(&keys.len())
                    {
                        all.push((keys, to));
                    }
                }
            }
        }
        // By first key, stably: the font's order among them.
        all.sort_by_key(|(keys, _)| keys[0]);
        let (entries, o) = (self.out.len(), self.offset_width());
        self.out.resize(entries + all.len() * o, 0);
        for (i, (keys, to)) in all.iter().enumerate() {
            let at = self.out.len();
            self.put_offset(Some(entries + i * o), at);
            self.put_small(keys.len());
            for k in keys.iter().chain([to]) {
                self.put_key(*k);
            }
        }
        Some((2, all.len(), entries))
    }

    /// The record of a lookup written as `made` (kind, count, where its
    /// entries are), kept for GSUB lookup `index`.
    fn finish(&mut self, index: u16, made: Option<(u8, usize, usize)>) -> Option<[u8; 8]> {
        let record = made.and_then(|(kind, count, entries)| {
            let count = u16::try_from(count).ok()?;
            let mut r = [kind, self.skip(index), 0, 0, 0, 0, 0, 0];
            r[2..4].copy_from_slice(&count.to_le_bytes());
            r[4..8].copy_from_slice(&u32::try_from(entries).ok()?.to_le_bytes());
            Some(r)
        });
        self.records.insert(index, record);
        record
    }

    /// The number (from 1) of the key set GSUB lookup `index` passes over
    /// in the table's list of them, as its lookup flags say (bases,
    /// ligatures, marks, the marks not in its filtering set or not of its
    /// attachment class); 0 for none.
    fn skip(&mut self, index: u16) -> u8 {
        let Some(lookup) = self
            .font
            .gsub()
            .and_then(|g| g.lookup_list())
            .ok()
            .and_then(|l| l.lookups().get(usize::from(index)).ok())
        else {
            return 0;
        };
        let flag = lookup.lookup_flag().to_bits();
        if flag & 0xFF1E == 0 {
            return 0;
        }
        let gdef = self.font.gdef().ok();
        let attach = gdef
            .as_ref()
            .and_then(|g| g.mark_attach_class_def())
            .and_then(Result::ok);
        let filter = lookup
            .mark_filtering_set()
            .filter(|_| flag & 0x10 != 0)
            .and_then(|n| {
                let sets = gdef.as_ref()?.mark_glyph_sets_def()?.ok()?;
                sets.coverages().get(usize::from(n)).ok()
            });
        let skipped = |g: u16| {
            let g16 = GlyphId16::new(g);
            match self.marks.as_ref().map_or(0, |c| c.get(g16)) {
                1 => flag & 2 != 0,
                2 => flag & 4 != 0,
                3 => {
                    flag & 8 != 0
                        || filter.as_ref().is_some_and(|f| f.get(g16).is_none())
                        || (flag >> 8 != 0
                            && attach.as_ref().map_or(0, |a| a.get(g16)) != flag >> 8)
                }
                _ => false,
            }
        };
        let glyphs: Vec<u16> = self
            .reachable
            .iter()
            .copied()
            .filter(|&g| skipped(g))
            .collect();
        let keys = self.keys(glyphs);
        if keys.is_empty() {
            return 0;
        }
        let at = self.set(keys);
        let n = match self.skips.iter().position(|&s| s == at) {
            Some(n) => n,
            None => {
                self.skips.push(at);
                self.skips.len() - 1
            }
        };
        u8::try_from(n + 1).unwrap_or(0)
    }

    /// Where the class definition `classes` is written (its keys' classes,
    /// class 0 left out); 0 for none.
    fn class_def(&mut self, classes: Option<&ClassDef<'_>>) -> usize {
        let Some(classes) = classes else {
            return 0;
        };
        let mut entries: Vec<(u32, u16)> = classes
            .iter()
            .filter(|&(_, c)| c != 0)
            .filter_map(|(g, c)| Some((self.key(g.to_u16())?, c)))
            .collect();
        entries.sort_unstable();
        entries.dedup_by_key(|e| e.0);
        if let Some(&at) = self.classes.get(&entries) {
            return at;
        }
        let at = self.out.len();
        self.put_count(entries.len());
        for &(k, c) in &entries {
            self.put_key(k);
            self.out.extend_from_slice(&c.to_le_bytes());
        }
        self.classes.insert(entries, at);
        at
    }

    /// Writes a lookup of kind 5 (rules by classes; see the reader's
    /// `Lookup::classes`): `defs` before, of the input, after; the glyphs
    /// it starts at; per class of the first glyph its rules (classes
    /// before, of the input after the first, after, and the records).
    #[allow(clippy::type_complexity, reason = "the parts of one subtable")]
    fn class_lookup(
        &mut self,
        defs: [Option<&ClassDef<'_>>; 3],
        starts: &CoverageTable<'_>,
        sets: Vec<Vec<(Vec<u16>, Vec<u16>, Vec<u16>, Vec<(u8, u16)>)>>,
    ) -> Option<(u8, usize, usize)> {
        let defs = defs.map(|d| self.class_def(d));
        let starts = self.keys(coverage(starts));
        let starts = self.set(starts);
        let mut offsets = Vec::new();
        for rules in sets {
            let mut written = Vec::new();
            for (back, input, ahead, records) in rules {
                let classes: Option<Vec<u8>> = back
                    .iter()
                    .chain(&input)
                    .chain(&ahead)
                    .map(|&c| u8::try_from(c).ok())
                    .collect();
                let records: Vec<(u8, usize)> = records
                    .iter()
                    .filter_map(|&(place, index)| Some((place, self.alone(index)?)))
                    .collect();
                let Some(classes) = classes else { continue };
                if back.len() > MAX_KEYS || input.len() >= MAX_KEYS || ahead.len() > MAX_KEYS {
                    continue;
                }
                let at = self.out.len();
                self.out.extend_from_slice(&[
                    back.len() as u8,
                    input.len() as u8 + 1,
                    ahead.len() as u8,
                    records.len().min(255) as u8,
                ]);
                self.out.extend_from_slice(&classes);
                for &(place, lookup) in records.iter().take(255) {
                    self.put_record(place, lookup);
                }
                written.push(at);
            }
            if written.is_empty() {
                offsets.push(0);
                continue;
            }
            let at = self.out.len();
            self.put_count(written.len());
            for w in written {
                self.put_offset(None, w);
            }
            offsets.push(at);
        }
        let head = self.out.len();
        for at in defs
            .into_iter()
            .chain([starts])
            .chain(offsets.iter().copied())
        {
            self.put_offset(None, at);
        }
        Some((5, offsets.len(), head))
    }

    fn context_classes(
        &mut self,
        t: &read_fonts::tables::layout::SequenceContextFormat2<'_>,
    ) -> Option<(u8, usize, usize)> {
        let (c, classes) = (t.coverage().ok()?, t.class_def().ok()?);
        let sets = t
            .class_seq_rule_sets()
            .iter()
            .map(|set| match set {
                Some(Ok(set)) => set
                    .class_seq_rules()
                    .iter()
                    .flatten()
                    .map(|r| {
                        let input = r.input_sequence().iter().map(|c| c.get()).collect();
                        (
                            Vec::new(),
                            input,
                            Vec::new(),
                            Self::records(r.seq_lookup_records()),
                        )
                    })
                    .collect(),
                _ => Vec::new(),
            })
            .collect();
        self.class_lookup([None, Some(&classes), None], &c, sets)
    }

    fn chained_classes(
        &mut self,
        t: &read_fonts::tables::layout::ChainedSequenceContextFormat2<'_>,
    ) -> Option<(u8, usize, usize)> {
        let c = t.coverage().ok()?;
        let (back, input, ahead) = (
            t.backtrack_class_def().ok()?,
            t.input_class_def().ok()?,
            t.lookahead_class_def().ok()?,
        );
        let sets = t
            .chained_class_seq_rule_sets()
            .iter()
            .map(|set| match set {
                Some(Ok(set)) => set
                    .chained_class_seq_rules()
                    .iter()
                    .flatten()
                    .map(|r| {
                        let seq = |s: &[read_fonts::types::BigEndian<u16>]| -> Vec<u16> {
                            s.iter().map(|c| c.get()).collect()
                        };
                        (
                            seq(r.backtrack_sequence()),
                            seq(r.input_sequence()),
                            seq(r.lookahead_sequence()),
                            Self::records(r.seq_lookup_records()),
                        )
                    })
                    .collect(),
                _ => Vec::new(),
            })
            .collect();
        self.class_lookup([Some(&back), Some(&input), Some(&ahead)], &c, sets)
    }

    /// Where the record of GSUB lookup `index` stands alone, for a rule.
    fn alone(&mut self, index: u16) -> Option<usize> {
        if let Some(&at) = self.alone.get(&index) {
            return Some(at);
        }
        let record = self.lookup(index, true)?;
        let at = self.out.len();
        self.out.extend_from_slice(&record);
        self.alone.insert(index, at);
        Some(at)
    }

    fn records(r: &[SequenceLookupRecord]) -> Vec<(u8, u16)> {
        r.iter()
            .filter_map(|r| {
                Some((
                    u8::try_from(r.sequence_index()).ok()?,
                    r.lookup_list_index(),
                ))
            })
            .collect()
    }

    /// The rules of a contextual subtable, each with its first glyphs.
    fn context(&self, t: &SequenceContext<'_>, rules: &mut Vec<(Vec<u16>, Rule)>) {
        match t {
            SequenceContext::Format1(t) => {
                let Ok(c) = t.coverage() else { return };
                for (g, set) in coverage(&c).into_iter().zip(t.seq_rule_sets().iter()) {
                    let Some(Ok(set)) = set else { continue };
                    for rule in set.seq_rules().iter().flatten() {
                        let input = rule.input_sequence().iter().map(|g| vec1(g.get().to_u16()));
                        rules.push((
                            alloc::vec![g],
                            Rule {
                                back: Vec::new(),
                                input: input.map(|s| self.keys(s)).collect(),
                                ahead: Vec::new(),
                                records: Self::records(rule.seq_lookup_records()),
                            },
                        ));
                    }
                }
            }
            SequenceContext::Format2(t) => {
                let (Ok(c), Ok(classes)) = (t.coverage(), t.class_def()) else {
                    return;
                };
                let of = class_members(&classes);
                let sets: Vec<_> = t.class_seq_rule_sets().iter().collect();
                for (class, firsts) in by_class(&c, &classes) {
                    let Some(Some(Ok(set))) = sets.get(usize::from(class)) else {
                        continue;
                    };
                    for rule in set.class_seq_rules().iter().flatten() {
                        let input: Option<Vec<Vec<u32>>> = rule
                            .input_sequence()
                            .iter()
                            .map(|c| members(&of, c.get()).map(|m| self.keys(m)))
                            .collect();
                        let Some(input) = input else { continue };
                        rules.push((
                            firsts.clone(),
                            Rule {
                                back: Vec::new(),
                                input,
                                ahead: Vec::new(),
                                records: Self::records(rule.seq_lookup_records()),
                            },
                        ));
                    }
                }
            }
            SequenceContext::Format3(t) => {
                let covers: Vec<Vec<u16>> = t
                    .coverages()
                    .iter()
                    .flatten()
                    .map(|c| coverage(&c))
                    .collect();
                let Some((first, input)) = covers.split_first() else {
                    return;
                };
                rules.push((
                    first.clone(),
                    Rule {
                        back: Vec::new(),
                        input: input.iter().map(|s| self.keys(s.iter().copied())).collect(),
                        ahead: Vec::new(),
                        records: Self::records(t.seq_lookup_records()),
                    },
                ));
            }
        }
    }

    /// The rules of a chained contextual subtable.
    fn chained(&self, t: &ChainedSequenceContext<'_>, rules: &mut Vec<(Vec<u16>, Rule)>) {
        let glyphs = |s: &[read_fonts::types::BigEndian<GlyphId16>]| -> Vec<Vec<u32>> {
            s.iter()
                .map(|g| self.keys(vec1(g.get().to_u16())))
                .collect()
        };
        match t {
            ChainedSequenceContext::Format1(t) => {
                let Ok(c) = t.coverage() else { return };
                for (g, set) in coverage(&c)
                    .into_iter()
                    .zip(t.chained_seq_rule_sets().iter())
                {
                    let Some(Ok(set)) = set else { continue };
                    for rule in set.chained_seq_rules().iter().flatten() {
                        rules.push((
                            alloc::vec![g],
                            Rule {
                                back: glyphs(rule.backtrack_sequence()),
                                input: glyphs(rule.input_sequence()),
                                ahead: glyphs(rule.lookahead_sequence()),
                                records: Self::records(rule.seq_lookup_records()),
                            },
                        ));
                    }
                }
            }
            ChainedSequenceContext::Format2(t) => {
                let (Ok(c), Ok(back), Ok(input), Ok(ahead)) = (
                    t.coverage(),
                    t.backtrack_class_def(),
                    t.input_class_def(),
                    t.lookahead_class_def(),
                ) else {
                    return;
                };
                let (of_back, of_input, of_ahead) = (
                    class_members(&back),
                    class_members(&input),
                    class_members(&ahead),
                );
                let sets: Vec<_> = t.chained_class_seq_rule_sets().iter().collect();
                let seq = |of: &BTreeMap<u16, Vec<u16>>,
                           s: &[read_fonts::types::BigEndian<u16>]| {
                    s.iter()
                        .map(|c| members(of, c.get()).map(|m| self.keys(m)))
                        .collect::<Option<Vec<_>>>()
                };
                for (class, firsts) in by_class(&c, &input) {
                    let Some(Some(Ok(set))) = sets.get(usize::from(class)) else {
                        continue;
                    };
                    for rule in set.chained_class_seq_rules().iter().flatten() {
                        let (Some(b), Some(i), Some(a)) = (
                            seq(&of_back, rule.backtrack_sequence()),
                            seq(&of_input, rule.input_sequence()),
                            seq(&of_ahead, rule.lookahead_sequence()),
                        ) else {
                            continue;
                        };
                        rules.push((
                            firsts.clone(),
                            Rule {
                                back: b,
                                input: i,
                                ahead: a,
                                records: Self::records(rule.seq_lookup_records()),
                            },
                        ));
                    }
                }
            }
            ChainedSequenceContext::Format3(t) => {
                let sets = |list: Vec<Vec<u16>>| -> Vec<Vec<u32>> {
                    list.into_iter().map(|s| self.keys(s)).collect()
                };
                let input = covers(t.input_coverages());
                let Some((first, rest)) = input.split_first() else {
                    return;
                };
                rules.push((
                    first.clone(),
                    Rule {
                        back: sets(covers(t.backtrack_coverages())),
                        input: sets(rest.to_vec()),
                        ahead: sets(covers(t.lookahead_coverages())),
                        records: Self::records(t.seq_lookup_records()),
                    },
                ));
            }
        }
    }

    /// Writes `rules` as a lookup of kind 4: kind, count, where its
    /// entries are.
    fn rules(&mut self, rules: Vec<(Vec<u16>, Rule)>) -> Option<(u8, usize, usize)> {
        let mut entries: Vec<(u32, usize)> = Vec::new();
        for (first, rule) in rules {
            // A rule that needs a glyph text never comes to never applies.
            let never = rule
                .back
                .iter()
                .chain(&rule.input)
                .chain(&rule.ahead)
                .any(Vec::is_empty);
            if never
                || rule.back.len() > MAX_KEYS
                || rule.input.len() >= MAX_KEYS
                || rule.ahead.len() > MAX_KEYS
                || rule.records.len() > 255
            {
                continue;
            }
            let lookups: Vec<(u8, usize)> = rule
                .records
                .iter()
                .filter_map(|&(place, index)| Some((place, self.alone(index)?)))
                .filter(|&(_, at)| at < 1 << 24)
                .collect();
            let (back, input, ahead) = (rule.back.len(), rule.input.len() + 1, rule.ahead.len());
            let sets: Vec<usize> = rule
                .back
                .into_iter()
                .chain(rule.input)
                .chain(rule.ahead)
                .map(|s| self.set(s))
                .collect();
            let at = self.out.len();
            // Counts: before, the input with its first key, after, records.
            self.out.extend_from_slice(&[
                back as u8,
                input as u8,
                ahead as u8,
                lookups.len() as u8,
            ]);
            for &s in &sets {
                self.put_offset(None, s);
            }
            for &(place, lookup) in &lookups {
                self.put_record(place, lookup);
            }
            for g in first {
                if let Some(k) = self.key(g) {
                    entries.push((k, at));
                }
            }
        }
        entries.sort_by_key(|e| e.0);
        let at = self.out.len();
        for &(k, rule) in &entries {
            self.put_key(k);
            self.put_offset(None, rule);
        }
        Some((4, entries.len(), at))
    }
}

fn covers<'a>(
    c: read_fonts::ArrayOfOffsets<'a, CoverageTable<'a>, read_fonts::types::Offset16>,
) -> Vec<Vec<u16>> {
    c.iter().flatten().map(|c| coverage(&c)).collect()
}

/// The glyphs of coverage `c` by their class in `classes`.
fn by_class(c: &CoverageTable<'_>, classes: &ClassDef<'_>) -> BTreeMap<u16, Vec<u16>> {
    let mut of: BTreeMap<u16, Vec<u16>> = BTreeMap::new();
    for g in coverage(c) {
        of.entry(classes.get(GlyphId16::new(g)))
            .or_default()
            .push(g);
    }
    of
}

fn vec1(g: u16) -> Vec<u16> {
    alloc::vec![g]
}

/// The glyphs of each class of a class definition (class 0 left out).
fn class_members(classes: &ClassDef<'_>) -> BTreeMap<u16, Vec<u16>> {
    let mut of: BTreeMap<u16, Vec<u16>> = BTreeMap::new();
    for (g, c) in classes.iter() {
        of.entry(c).or_default().push(g.to_u16());
    }
    of
}

/// The glyphs of class `class`; `None` for class 0 (every glyph not in
/// another), which a pack does not list.
fn members(of: &BTreeMap<u16, Vec<u16>>, class: u16) -> Option<Vec<u16>> {
    (class != 0).then(|| of.get(&class).cloned().unwrap_or_default())
}

/// Adds to `drawn` the glyphs lookup `index` (and those it applies) draws
/// where its keys are among `reached`, the glyphs text of the pack's
/// characters may come to; `seen` the lookups already read.
fn outputs(
    font: &FontRef<'_>,
    index: u16,
    reached: &BTreeSet<u16>,
    drawn: &mut BTreeSet<u16>,
    seen: &mut BTreeSet<u16>,
) {
    if !seen.insert(index) {
        return;
    }
    let Some(lookup) = font
        .gsub()
        .ok()
        .and_then(|g| g.lookup_list().ok())
        .and_then(|l| l.lookups().get(usize::from(index)).ok())
    else {
        return;
    };
    let mut nested = Vec::new();
    match lookup.subtables() {
        Ok(SubstitutionSubtables::Single(subtables)) => {
            for t in subtables.iter().flatten() {
                match &t {
                    SingleSubst::Format1(t) => {
                        if let Ok(c) = t.coverage() {
                            let delta = t.delta_glyph_id();
                            drawn.extend(
                                coverage(&c)
                                    .into_iter()
                                    .filter(|g| reached.contains(g))
                                    .map(|g| (i32::from(g) + i32::from(delta)) as u16),
                            );
                        }
                    }
                    SingleSubst::Format2(t) => {
                        if let Ok(c) = t.coverage() {
                            drawn.extend(
                                coverage(&c)
                                    .into_iter()
                                    .zip(t.substitute_glyph_ids())
                                    .filter(|(g, _)| reached.contains(g))
                                    .map(|(_, to)| to.get().to_u16()),
                            );
                        }
                    }
                }
            }
        }
        Ok(SubstitutionSubtables::Multiple(subtables)) => {
            for t in subtables.iter().flatten() {
                let Ok(c) = t.coverage() else { continue };
                for (g, seq) in coverage(&c).into_iter().zip(t.sequences().iter()) {
                    if let Ok(seq) = seq
                        && reached.contains(&g)
                    {
                        drawn.extend(seq.substitute_glyph_ids().iter().map(|g| g.get().to_u16()));
                    }
                }
            }
        }
        Ok(SubstitutionSubtables::Ligature(subtables)) => {
            for t in subtables.iter().flatten() {
                let Ok(c) = t.coverage() else { continue };
                for (g, set) in coverage(&c).into_iter().zip(t.ligature_sets().iter()) {
                    let Ok(set) = set else { continue };
                    for lig in set.ligatures().iter().flatten() {
                        let parts = lig.component_glyph_ids().iter().map(|g| g.get().to_u16());
                        if reached.contains(&g) && { parts }.all(|p| reached.contains(&p)) {
                            drawn.insert(lig.ligature_glyph().to_u16());
                        }
                    }
                }
            }
        }
        Ok(SubstitutionSubtables::Contextual(subtables)) => {
            for t in subtables.iter().flatten() {
                nested.extend(applied(&t));
            }
        }
        Ok(SubstitutionSubtables::ChainContextual(subtables)) => {
            for t in subtables.iter().flatten() {
                nested.extend(chain_applied(&t));
            }
        }
        _ => {}
    }
    for index in nested {
        outputs(font, index, reached, drawn, seen);
    }
}

/// The glyphs text of `cmap`'s characters may come to through the lookups
/// of `wanted`: theirs, and what lookups draw where their keys are among
/// them, until no more come.
fn reachable_from(
    font: &FontRef<'_>,
    cmap: &BTreeMap<u16, u32>,
    wanted: &[(u8, [u8; 4], Vec<u16>)],
) -> BTreeSet<u16> {
    let mut reached: BTreeSet<u16> = cmap.keys().copied().collect();
    // Each round adds a glyph or ends; a font has at most 65,536.
    for _ in 0..=u16::MAX {
        let (mut drawn, mut seen) = (reached.clone(), BTreeSet::new());
        for (_, _, list) in wanted {
            for &index in list {
                outputs(font, index, &reached, &mut drawn, &mut seen);
            }
        }
        if drawn.len() == reached.len() {
            break;
        }
        reached = drawn;
    }
    reached
}

/// `list` with the contextual lookups its contextual lookups apply, each
/// after the one that applies it: a pack's rules apply none, so such a
/// lookup applies where its own context is (Noto Serif Telugu chooses the
/// form of ra below by the consonant before it so).
fn with_nested(font: &FontRef<'_>, list: Vec<u16>) -> Vec<u16> {
    let Ok(lookups) = font.gsub().and_then(|g| g.lookup_list()) else {
        return list;
    };
    let applies = |index: u16| -> Option<Vec<u16>> {
        Some(
            match lookups
                .lookups()
                .get(usize::from(index))
                .ok()?
                .subtables()
                .ok()?
            {
                SubstitutionSubtables::Contextual(t) => {
                    t.iter().flatten().flat_map(|t| applied(&t)).collect()
                }
                SubstitutionSubtables::ChainContextual(t) => {
                    t.iter().flatten().flat_map(|t| chain_applied(&t)).collect()
                }
                _ => return None,
            },
        )
    };
    let mut out = Vec::with_capacity(list.len());
    for &index in &list {
        out.push(index);
        for inner in applies(index).unwrap_or_default() {
            if applies(inner).is_some() && !list.contains(&inner) && !out.contains(&inner) {
                out.push(inner);
            }
        }
    }
    out
}

/// The lookups a contextual subtable applies.
fn applied(t: &SequenceContext<'_>) -> Vec<u16> {
    let mut out = Vec::new();
    match t {
        SequenceContext::Format1(t) => {
            for set in t.seq_rule_sets().iter().flatten().flatten() {
                for r in set.seq_rules().iter().flatten() {
                    out.extend(r.seq_lookup_records().iter().map(|r| r.lookup_list_index()));
                }
            }
        }
        SequenceContext::Format2(t) => {
            for set in t.class_seq_rule_sets().iter().flatten().flatten() {
                for r in set.class_seq_rules().iter().flatten() {
                    out.extend(r.seq_lookup_records().iter().map(|r| r.lookup_list_index()));
                }
            }
        }
        SequenceContext::Format3(t) => {
            out.extend(t.seq_lookup_records().iter().map(|r| r.lookup_list_index()));
        }
    }
    out
}

/// The lookups a contextual subtable applies.
fn chain_applied(t: &ChainedSequenceContext<'_>) -> Vec<u16> {
    let mut out = Vec::new();
    match t {
        ChainedSequenceContext::Format1(t) => {
            for set in t.chained_seq_rule_sets().iter().flatten().flatten() {
                for r in set.chained_seq_rules().iter().flatten() {
                    out.extend(r.seq_lookup_records().iter().map(|r| r.lookup_list_index()));
                }
            }
        }
        ChainedSequenceContext::Format2(t) => {
            for set in t.chained_class_seq_rule_sets().iter().flatten().flatten() {
                for r in set.chained_class_seq_rules().iter().flatten() {
                    out.extend(r.seq_lookup_records().iter().map(|r| r.lookup_list_index()));
                }
            }
        }
        ChainedSequenceContext::Format3(t) => {
            out.extend(t.seq_lookup_records().iter().map(|r| r.lookup_list_index()));
        }
    }
    out
}

/// The scripts' OpenType tags: their second version's, else the first's.
const TAGS: [(Script, [&[u8; 4]; 2]); 14] = [
    (Script::Bengali, [b"bng2", b"beng"]),
    (Script::Devanagari, [b"dev2", b"deva"]),
    (Script::Khmer, [b"khmr", b"khmr"]),
    (Script::Lao, [b"lao ", b"lao "]),
    (Script::Tibetan, [b"tibt", b"tibt"]),
    (Script::Gujarati, [b"gjr2", b"gujr"]),
    (Script::Oriya, [b"ory2", b"orya"]),
    (Script::Gurmukhi, [b"gur2", b"guru"]),
    (Script::Tamil, [b"tml2", b"taml"]),
    (Script::Malayalam, [b"mlm2", b"mlym"]),
    (Script::Sinhala, [b"sinh", b"sinh"]),
    (Script::Telugu, [b"tel2", b"telu"]),
    (Script::Kannada, [b"knd2", b"knda"]),
    (Script::Myanmar, [b"mym2", b"mymr"]),
];

/// The substitutions of the Indic scripts among `chars` in `font`, if it
/// has any for them.
pub(crate) fn make(font: FontRef<'_>, chars: &[char]) -> Option<Made> {
    let gsub = font.gsub().ok()?;
    let (scripts, features) = (gsub.script_list().ok()?, gsub.feature_list().ok()?);
    let charmap = skrifa::MetadataProvider::charmap(&font);
    let mut cmap = BTreeMap::new();
    for &ch in chars {
        if let Some(g) = charmap.map(ch).and_then(|g| u16::try_from(g.to_u32()).ok()) {
            cmap.entry(g).or_insert(u32::from(ch));
        }
    }
    let marks = font
        .gdef()
        .ok()
        .and_then(|g| g.glyph_class_def())
        .and_then(Result::ok);
    // Per script it has letters of: its features, with their lookups.
    let mut wanted: Vec<(u8, [u8; 4], Vec<u16>)> = Vec::new();
    for (script, tags) in TAGS {
        if !chars.iter().any(|&c| Script::of(c) == Some(script)) {
            continue;
        }
        let Some(tag) = tags
            .iter()
            .find(|t| scripts.index_for_tag(Tag::new(t)).is_some())
        else {
            continue;
        };
        let number = super::number(script);
        for feature in ORDER {
            let mut list: Vec<u16> = match feature {
                b"pres" => PRESENTATION
                    .iter()
                    .flat_map(|f| lookups(&scripts, &features, Tag::new(tag), f))
                    .collect(),
                _ => lookups(&scripts, &features, Tag::new(tag), feature),
            };
            list.sort_unstable();
            list.dedup();
            let list = with_nested(&font, list);
            if !list.is_empty() {
                wanted.push((number, *feature, list));
            }
        }
    }
    if wanted.is_empty() {
        return None;
    }
    let reachable = reachable_from(&font, &cmap, &wanted);
    // Within what a pack read from a card keeps in memory: the largest
    // presentation lookups left out until it fits (forms chosen by
    // context, such as the length of Devanagari's i), never the forms of
    // the basic features.
    // The lookups chosen as tables of 4-byte keys and offsets keep them,
    // written with 2-byte ones (version 2, half the memory) where they fit.
    let writer = |compact| Writer {
        font: font.clone(),
        cmap: cmap.clone(),
        marks: marks.clone(),
        out: alloc::vec![0; 12],
        records: BTreeMap::new(),
        alone: BTreeMap::new(),
        sets: BTreeMap::new(),
        classes: BTreeMap::new(),
        drawn: BTreeSet::new(),
        reachable: reachable.clone(),
        compact,
        too_wide: false,
        skips: Vec::new(),
    };
    let burmese = wanted.iter().all(|w| w.0 == super::number(Script::Myanmar));
    let mut skip = BTreeSet::new();
    for _ in 0..16 {
        let (made, sizes) = write(writer(false), &wanted, &skip);
        let made = made?;
        let compact = write(writer(true), &wanted, &skip).0;
        // Burmese reads wrong without its presentation forms (a spacing
        // glyph before hundreds of letters): its table counts as kept, in
        // its compact form. Others count as written wide, so that their
        // tables do not grow.
        let kept_len = match &compact {
            Some(c) if burmese => c.table.len(),
            _ => made.table.len(),
        };
        let over = kept_len.saturating_sub(MAX_HELD);
        // Too large for a script whose clusters are kept shaped: those.
        if over > 0
            && skip.is_empty()
            && let Some(kept) = kept_shaped(&wanted, &made.table)
        {
            return Some(kept);
        }
        if over == 0 {
            return Some(compact.unwrap_or(made));
        }
        // Rules by classes first (hundreds of them choose among a vowel
        // sign's lengths), then the largest.
        let mut largest: Vec<(bool, usize, u16)> = sizes
            .into_iter()
            .filter(|&(tag, _, _, _)| tag == *b"pres")
            .map(|(_, index, size, kind)| (kind == 5, size, index))
            .collect();
        largest.sort_unstable_by(|a, b| b.cmp(a));
        let mut freed = 0;
        for (_, size, index) in largest {
            if freed > over {
                break;
            }
            skip.insert(index);
            freed += size;
        }
        if freed == 0 {
            return None;
        }
    }
    None
}

/// The table of the clusters kept shaped (`super::clusters`) by `full`,
/// the whole table of `wanted`, if they are of one script that keeps them.
fn kept_shaped(wanted: &[(u8, [u8; 4], Vec<u16>)], full: &[u8]) -> Option<Made> {
    let &(number, ..) = wanted.first()?;
    if wanted.iter().any(|w| w.0 != number) {
        return None;
    }
    let script = super::SCRIPTS
        .into_iter()
        .find(|&s| super::number(s) == number)?;
    super::clusters::compact(full, script, number, super::clusters::list(script)?)
}

/// A lookup as written: its feature's tag, its GSUB index, its bytes and its
/// kind.
type Written = ([u8; 4], u16, usize, u8);

/// Writes the table of `wanted` (script, feature, lookups), the lookups
/// in `skip` left out (`None` where a compact one has a key or an offset
/// without 2 bytes); with the bytes each feature's lookup took and its
/// kind.
fn write(
    mut w: Writer<'_>,
    wanted: &[(u8, [u8; 4], Vec<u16>)],
    skip: &BTreeSet<u16>,
) -> (Option<Made>, Vec<Written>) {
    let mut sizes = Vec::new();
    let head = w.out.len();
    w.out.resize(head + wanted.len() * 16, 0);
    for (f, (script, tag, list)) in wanted.iter().enumerate() {
        let mut records: Vec<[u8; 8]> = Vec::new();
        for &index in list.iter().filter(|i| !skip.contains(i)) {
            let before = w.out.len();
            let record = w.lookup(index, false);
            if let Some(record) = record {
                records.push(record);
            }
            let kind = record.map_or(0, |r| r[0]);
            sizes.push((*tag, index, w.out.len() - before, kind));
        }
        let at = w.out.len();
        w.out
            .extend_from_slice(&u16::try_from(records.len()).unwrap_or(0).to_le_bytes());
        w.out.extend_from_slice(&[0, 0]);
        for r in &records {
            w.out.extend_from_slice(r);
        }
        let record = head + f * 16;
        w.out[record..record + 4].copy_from_slice(tag);
        w.out[record + 4] = *script;
        w.out[record + 5] = 1;
        set_u32(&mut w.out, record + 12, at);
    }
    // The key sets lookups skip, listed at the end (at most 255).
    w.skips.truncate(255);
    for i in 0..w.skips.len() {
        let at = w.skips[i];
        w.put_offset(None, at);
    }
    let skips = u16::try_from(w.skips.len()).unwrap_or(0);
    w.out[10..12].copy_from_slice(&skips.to_le_bytes());
    w.out[0] = if w.compact { 2 } else { 1 };
    let len = w.out.len();
    set_u32(&mut w.out, 4, len);
    w.out[8..10].copy_from_slice(&u16::try_from(wanted.len()).unwrap_or(0).to_le_bytes());
    let keys = w
        .drawn
        .iter()
        .filter_map(|&g| w.key(g).and_then(char::from_u32))
        .collect();
    let fits = !w.too_wide && (!w.compact || len <= usize::from(u16::MAX));
    (fits.then_some(Made { table: w.out, keys }), sizes)
}
