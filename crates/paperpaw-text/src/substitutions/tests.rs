#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests")]

use alloc::vec;
use alloc::vec::Vec;

use super::*;

/// A lookup to write.
pub(crate) enum Write {
    Single(Vec<(u32, u32)>),
    Ligature(Vec<(Vec<u32>, u32)>),
    Multiple(Vec<(u32, Vec<u32>)>),
    /// A kind this reader does not know, with no entries.
    Unknown(u8),
}

/// A feature to write: tag, script, language, on by default, lookups.
pub(crate) type FeatureWrite = (&'static [u8; 4], u8, &'static str, bool, Vec<Write>);

fn push_u32(out: &mut Vec<u8>, v: usize) {
    out.extend_from_slice(&u32::try_from(v).unwrap().to_le_bytes());
}

fn set_u32(out: &mut [u8], at: usize, v: usize) {
    out[at..at + 4].copy_from_slice(&u32::try_from(v).unwrap().to_le_bytes());
}

/// A table of version 1, as the pack builder writes it: entries sorted,
/// ligatures of the same first key longest first.
pub(crate) fn table(features: Vec<FeatureWrite>) -> Vec<u8> {
    let mut out = vec![VERSION, 0, 0, 0, 0, 0, 0, 0];
    out.extend_from_slice(&u16::try_from(features.len()).unwrap().to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    let records = out.len();
    out.resize(records + features.len() * FEATURE, 0);
    for (f, (tag, script, language, on, lookups)) in features.into_iter().enumerate() {
        let record = records + f * FEATURE;
        out[record..record + 4].copy_from_slice(tag);
        out[record + 4] = script;
        out[record + 5] = u8::from(on);
        out[record + 8..record + 8 + language.len()].copy_from_slice(language.as_bytes());
        let list = out.len();
        set_u32(&mut out, record + 12, list);
        out.extend_from_slice(&u16::try_from(lookups.len()).unwrap().to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out.resize(list + 4 + lookups.len() * LOOKUP, 0);
        for (l, lookup) in lookups.into_iter().enumerate() {
            let at = list + 4 + l * LOOKUP;
            let entries = out.len();
            set_u32(&mut out, at + 4, entries);
            let (kind, count) = match lookup {
                Write::Single(mut pairs) => {
                    pairs.sort_unstable();
                    for (from, to) in &pairs {
                        push_u32(&mut out, *from as usize);
                        push_u32(&mut out, *to as usize);
                    }
                    (SINGLE, pairs.len())
                }
                Write::Ligature(mut ligatures) => {
                    ligatures.sort_by(|a, b| a.0[0].cmp(&b.0[0]).then(b.0.len().cmp(&a.0.len())));
                    out.resize(entries + ligatures.len() * 4, 0);
                    for (i, (from, to)) in ligatures.iter().enumerate() {
                        let entry = out.len();
                        set_u32(&mut out, entries + i * 4, entry);
                        out.extend_from_slice(&[u8::try_from(from.len()).unwrap(), 0, 0, 0]);
                        for key in from.iter().chain([to]) {
                            push_u32(&mut out, *key as usize);
                        }
                    }
                    (LIGATURE, ligatures.len())
                }
                Write::Multiple(mut multiples) => {
                    multiples.sort_unstable();
                    out.resize(entries + multiples.len() * 4, 0);
                    for (i, (from, to)) in multiples.iter().enumerate() {
                        let entry = out.len();
                        set_u32(&mut out, entries + i * 4, entry);
                        push_u32(&mut out, *from as usize);
                        out.extend_from_slice(&[u8::try_from(to.len()).unwrap(), 0, 0, 0]);
                        for key in to {
                            push_u32(&mut out, *key as usize);
                        }
                    }
                    (MULTIPLE, multiples.len())
                }
                Write::Unknown(kind) => (kind, 0),
            };
            out[at] = kind;
            out[at + 2..at + 4].copy_from_slice(&u16::try_from(count).unwrap().to_le_bytes());
        }
    }
    let len = out.len();
    set_u32(&mut out, 4, len);
    out
}

/// Plane 15 keys: glyphs without a code point (the pack builder numbers
/// them).
const KA_SSA: u32 = 0xF_0000;
const HALF_KA: u32 = 0xF_0001;
const KA: u32 = 0x915;
const VIRAMA: u32 = 0x94D;
const SSA: u32 = 0x937;

fn sample() -> Vec<u8> {
    table(vec![
        (
            b"half",
            0,
            "",
            true,
            vec![Write::Ligature(vec![
                (vec![KA, VIRAMA], HALF_KA),
                (vec![KA, VIRAMA, SSA], KA_SSA),
            ])],
        ),
        (
            b"locl",
            0,
            "mr",
            true,
            vec![Write::Single(vec![(SSA, 0xF_0002)])],
        ),
        (
            b"salt",
            0,
            "",
            false,
            vec![Write::Single(vec![(KA, 0xF_0003)])],
        ),
        (
            b"pres",
            12,
            "",
            true,
            vec![
                Write::Unknown(9),
                Write::Multiple(vec![(0x0E33, vec![0x0E4D, 0x0E32])]),
            ],
        ),
    ])
}

#[test]
fn the_longest_ligature_wins_and_language_forms_apply_to_their_language() {
    let data = sample();
    let table = Substitutions::new(&data).unwrap();
    let tags: Vec<[u8; 4]> = table.features().map(|f| f.tag).collect();
    assert_eq!(tags, [*b"half", *b"locl", *b"salt", *b"pres"]);

    let mut keys = vec![KA, VIRAMA, SSA, KA, VIRAMA, KA];
    table.apply(5, "hi", &mut keys, 32);
    assert_eq!(keys, [KA_SSA, HALF_KA, KA]);

    // Marathi's own form of ssa; the alternate is off by default.
    let mut keys = vec![SSA, KA];
    table.apply(5, "mr", &mut keys, 32);
    assert_eq!(keys, [0xF_0002, KA]);
}

#[test]
fn a_lookup_of_an_unknown_kind_is_skipped_and_one_key_becomes_several() {
    let data = sample();
    let table = Substitutions::new(&data).unwrap();
    let pres = table.features().nth(3).unwrap();
    assert_eq!(pres.lookups().count(), 1);
    // Thai sara am (script 12 in the test) as nikhahit and sara aa.
    let mut keys = vec![0x0E01, 0x0E33, 0x0E33];
    table.apply(12, "th", &mut keys, 32);
    assert_eq!(keys, [0x0E01, 0x0E4D, 0x0E32, 0x0E4D, 0x0E32]);
    // Not past the limit; not for another script.
    let mut keys = vec![0x0E33, 0x0E33];
    table.apply(12, "th", &mut keys, 3);
    assert_eq!(keys, [0x0E4D, 0x0E32, 0x0E33]);
    let mut keys = vec![0x0E33];
    table.apply(5, "hi", &mut keys, 32);
    assert_eq!(keys, [0x0E33]);
}

#[test]
fn another_version_or_a_short_table_is_not_read() {
    let mut data = sample();
    assert!(Substitutions::new(&data[..data.len() - 1]).is_none());
    data[0] = 2;
    assert!(Substitutions::new(&data).is_none());
    assert!(Substitutions::new(&[]).is_none());
}

#[test]
fn any_damage_reads_as_less_never_as_a_panic() {
    let data = sample();
    // Every byte of the table set to each of a few values: whatever it
    // reads, it does not panic, and keys never grow past the limit.
    for at in 0..data.len() {
        for value in [0, 1, 0x7F, 0xFF] {
            let mut damaged = data.clone();
            damaged[at] = value;
            if let Some(table) = Substitutions::new(&damaged) {
                let mut keys = vec![KA, VIRAMA, SSA, 0x0E33, SSA, KA];
                table.apply(5, "mr", &mut keys, 8);
                table.apply(12, "th", &mut keys, 8);
                assert!(keys.len() <= 8);
            }
        }
    }
}
