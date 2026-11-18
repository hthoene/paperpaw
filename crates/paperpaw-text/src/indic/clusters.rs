//! Tibetan in a pack (#314): Noto Serif Tibetan's substitutions (some
//! 1,100 stacks, chosen by context) take some 230 KB as a pack keeps them,
//! more than a pack read from a card may hold in memory
//! ([`crate::substitutions::MAX_HELD`]). A pack keeps instead what they
//! make of the clusters text uses most ([`LIST`]: a consonant with the
//! letters subjoined under it and its vowel signs, the most frequent in
//! the articles of the Tibetan Wikipedia, `tools/tibetan-clusters.py`),
//! shaped by the whole table when the pack is made: one ligature lookup
//! from each cluster's characters to its glyph or to a key that a multiple
//! lookup then draws as its glyphs, singles for clusters of one character.
//! A cluster not listed is drawn as the longest listed one it starts with
//! and its other characters.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use super::pack::Made;
use super::{GLYPHS, Glyphs, MARKS, MAX_GLYPH, Script, Shaper, rare};
use crate::substitutions::{MAX_KEYS, Substitutions};

/// The clusters, one a line.
const LIST: &str = include_str!("tibetan_clusters.txt");
/// The first key a cluster drawn as several glyphs is joined into before
/// they are drawn instead (a private code point no glyph has).
const JOINED: u32 = 0xF_F000;

/// The clusters a pack of `script` keeps shaped, if it keeps them so.
pub(super) fn list(script: Script) -> Option<&'static str> {
    (script == Script::Tibetan).then_some(LIST)
}

/// What the clusters become: joined as one key, one key drawn as another,
/// one key drawn as several.
#[derive(Default)]
struct Shaped {
    joins: Vec<(Vec<u32>, u32)>,
    singles: BTreeMap<u32, u32>,
    splits: BTreeMap<u32, Vec<u32>>,
}

/// The table of `list`'s clusters of `script` (its number in tables
/// `number`) as `full` (a whole table, head included) shapes them.
pub(super) fn compact(full: &[u8], script: Script, number: u8, list: &str) -> Option<Made> {
    let shaper = Shaper::new(Some(Substitutions::read(full)?));
    let mut shaped = Shaped::default();
    let mut out = Glyphs::default();
    let clusters = list
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'));
    for cluster in clusters {
        let mut input = Glyphs::default();
        for ch in cluster.chars() {
            rare::push(script, ch, &mut input);
        }
        shaper.shape(cluster, true, &mut out);
        let (from, to) = (&input.keys[..input.len], &out.keys[..out.len]);
        if from == to || from.len() > MAX_KEYS || !(1..=MAX_KEYS).contains(&to.len()) {
            continue;
        }
        match (from, to) {
            ([one], [drawn]) => {
                shaped.singles.entry(*one).or_insert(*drawn);
            }
            ([one], _) => {
                shaped.splits.entry(*one).or_insert_with(|| to.to_vec());
            }
            (_, [drawn]) => shaped.joins.push((from.to_vec(), *drawn)),
            _ => {
                let key = JOINED + u32::try_from(shaped.splits.len()).ok()?;
                shaped.splits.insert(key, to.to_vec());
                shaped.joins.push((from.to_vec(), key));
            }
        }
    }
    Some(write(&shaped, number))
}

fn put(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_at(out: &mut [u8], at: usize, v: usize) {
    if let (Some(b), Ok(v)) = (out.get_mut(at..at + 4), u32::try_from(v)) {
        b.copy_from_slice(&v.to_le_bytes());
    }
}

/// The table of `shaped`: one feature (`pres`, of script `number`) of
/// three lookups, the ligatures (longest first), the singles and the
/// multiples.
fn write(shaped: &Shaped, number: u8) -> Made {
    let mut joins: Vec<&(Vec<u32>, u32)> = shaped.joins.iter().collect();
    joins.sort_by(|a, b| a.0[0].cmp(&b.0[0]).then(b.0.len().cmp(&a.0.len())));
    let mut out = alloc::vec![1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0];
    // The feature, its lookup list after it.
    out.extend_from_slice(b"pres");
    out.extend_from_slice(&[number, 1, 0, 0, 0, 0, 0, 0]);
    put(&mut out, 28);
    out.extend_from_slice(&[3, 0, 0, 0]);
    let records = out.len();
    out.resize(records + 3 * 8, 0);
    let counts = [joins.len(), shaped.singles.len(), shaped.splits.len()];
    for (n, (kind, count)) in [2u8, 1, 3].into_iter().zip(counts).enumerate() {
        out[records + n * 8] = kind;
        let count = u16::try_from(count).unwrap_or(0).to_le_bytes();
        out[records + n * 8 + 2..records + n * 8 + 4].copy_from_slice(&count);
    }
    // Ligatures: an offset each, then the entries.
    let at = out.len();
    put_at(&mut out, records + 4, at);
    let offsets = out.len();
    out.resize(offsets + joins.len() * 4, 0);
    for (i, (keys, to)) in joins.iter().enumerate() {
        let at = out.len();
        put_at(&mut out, offsets + i * 4, at);
        out.extend_from_slice(&[keys.len() as u8, 0, 0, 0]);
        for &k in keys.iter().chain([to]) {
            put(&mut out, k);
        }
    }
    let at = out.len();
    put_at(&mut out, records + 12, at);
    for (&from, &to) in &shaped.singles {
        put(&mut out, from);
        put(&mut out, to);
    }
    let at = out.len();
    put_at(&mut out, records + 20, at);
    let offsets = out.len();
    out.resize(offsets + shaped.splits.len() * 4, 0);
    for (i, (&from, to)) in shaped.splits.iter().enumerate() {
        let at = out.len();
        put_at(&mut out, offsets + i * 4, at);
        put(&mut out, from);
        out.extend_from_slice(&[to.len() as u8, 0, 0, 0]);
        for &k in to {
            put(&mut out, k);
        }
    }
    let len = out.len();
    put_at(&mut out, 4, len);
    // The glyphs without code point drawn.
    let drawn = |k: &u32| {
        (GLYPHS..GLYPHS + MAX_GLYPH).contains(k) || (MARKS..MARKS + MAX_GLYPH).contains(k)
    };
    let mut keys: Vec<char> = joins
        .iter()
        .map(|(_, to)| to)
        .chain(shaped.singles.values())
        .chain(shaped.splits.values().flatten())
        .filter(|k| drawn(k))
        .filter_map(|&k| char::from_u32(k))
        .collect();
    keys.sort_unstable();
    keys.dedup();
    Made { table: out, keys }
}
