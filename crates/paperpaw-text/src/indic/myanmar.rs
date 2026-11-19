//! Burmese (#397): what Myanmar characters are in a syllable and the
//! order a syllable is drawn in, as HarfBuzz's Myanmar shaper has them.
//!
//! A syllable is a consonant (or independent vowel), maybe after a kinzi
//! (nga, asat and virama: U+1004 U+103A U+1039, drawn above the next
//! consonant), with the consonants stacked below it by virama (U+1039),
//! its medials (ya, ra, wa, ha), vowel signs, asat and tones. Before the
//! forms apply, kinzi moves after the base, the vowel sign e (U+1031) and
//! medial ra (U+103C, drawn around the consonant) before it; the font's
//! `rphf`, `pref`, `blwf` and `pstf` then apply to the whole syllable.

use super::{Cat, Glyphs, POS_AFTER_MAIN, POS_AFTER_SUB, POS_BASE, POS_BEFORE_SUB, POS_BELOW_C};
use super::{POS_PRE_C, POS_PRE_M};

/// What each character of the Myanmar block (U+1000 to U+109F) is in a
/// syllable: a table, since a `match` of ranges made jump tables.
static CATS: [Cat; 0xA0] = cats(&[
    (0x1000, 0x1020, Cat::Consonant),
    (0x103F, 0x103F, Cat::Consonant),
    (0x104E, 0x104E, Cat::Consonant),
    (0x1050, 0x1051, Cat::Consonant),
    (0x105A, 0x105D, Cat::Consonant),
    (0x1061, 0x1061, Cat::Consonant),
    (0x1065, 0x1066, Cat::Consonant),
    (0x106E, 0x1070, Cat::Consonant),
    (0x1075, 0x1081, Cat::Consonant),
    (0x108E, 0x108E, Cat::Consonant),
    (0x1021, 0x102A, Cat::Vowel),
    (0x104C, 0x104D, Cat::Vowel),
    (0x104F, 0x104F, Cat::Vowel),
    (0x1052, 0x1055, Cat::Vowel),
    (0x1031, 0x1031, Cat::PreMatra),
    (0x1084, 0x1084, Cat::PreMatra),
    (0x103C, 0x103C, Cat::SplitMatra),
    (0x102D, 0x102E, Cat::AboveMatra),
    (0x1033, 0x1035, Cat::AboveMatra),
    (0x1071, 0x1074, Cat::AboveMatra),
    (0x1085, 0x1086, Cat::AboveMatra),
    (0x109D, 0x109D, Cat::AboveMatra),
    (0x102F, 0x1030, Cat::BelowMatra),
    (0x1058, 0x1059, Cat::BelowMatra),
    (0x102B, 0x102C, Cat::PostMatra),
    (0x1056, 0x1057, Cat::PostMatra),
    (0x1062, 0x1062, Cat::PostMatra),
    (0x1067, 0x1068, Cat::PostMatra),
    (0x1083, 0x1083, Cat::PostMatra),
    (0x103B, 0x103B, Cat::Medial),
    (0x103D, 0x103E, Cat::Medial),
    (0x105E, 0x1060, Cat::Medial),
    (0x1082, 0x1082, Cat::Medial),
    (0x1032, 0x1032, Cat::Modifier),
    (0x1036, 0x1036, Cat::Modifier),
    (0x1037, 0x1038, Cat::Tone),
    (0x1063, 0x1064, Cat::Tone),
    (0x1069, 0x106D, Cat::Tone),
    (0x1087, 0x108D, Cat::Tone),
    (0x108F, 0x108F, Cat::Tone),
    (0x109A, 0x109C, Cat::Tone),
    (0x1039, 0x1039, Cat::Virama),
    (0x103A, 0x103A, Cat::Nukta),
]);

/// The table from `(first, last, category)` ranges (the rest is `Other`).
const fn cats(ranges: &[(usize, usize, Cat)]) -> [Cat; 0xA0] {
    let mut t = [Cat::Other; 0xA0];
    let mut r = 0;
    while r < ranges.len() {
        let mut c = ranges[r].0;
        while c <= ranges[r].1 {
            t[c - 0x1000] = ranges[r].2;
            c += 1;
        }
        r += 1;
    }
    t
}

/// What `ch` (of the Myanmar block) is in a syllable.
pub(super) fn category(ch: char) -> Cat {
    let at = (u32::from(ch)).wrapping_sub(0x1000) as usize;
    CATS.get(at).copied().unwrap_or(Cat::Other)
}

/// Whether `ch` (category `cat`) is drawn on the letter before it, taking
/// no room of its own: the signs above and below, asat, virama, the
/// medials but ya and ra, dot below (not the vowels after the consonant,
/// visarga and the other tones).
pub(super) fn is_mark(ch: char, cat: Cat) -> bool {
    match cat {
        Cat::Nukta | Cat::Virama | Cat::AboveMatra | Cat::BelowMatra | Cat::Modifier => true,
        Cat::Medial => ch != '\u{103B}',
        Cat::Tone => matches!(ch, '\u{1037}' | '\u{108D}'),
        _ => false,
    }
}

/// Whether the next character (category `cat`, after one of category
/// `before`) belongs to the syllable that starts with one of `first`: a
/// consonant stacked by virama, signs after a letter or sign.
pub(super) fn takes(first: Cat, before: Cat, cat: Cat) -> bool {
    match cat {
        Cat::Consonant | Cat::Vowel => before == Cat::Virama,
        Cat::Zwj | Cat::Zwnj => first.is_base(),
        Cat::Other => false,
        _ => first != Cat::Other,
    }
}

/// Whether `key` makes a kinzi with asat and virama.
const fn is_ra(key: u32) -> bool {
    matches!(key, 0x1004 | 0x101B | 0x105A)
}

/// Puts the syllable in `out` in the order it is drawn: kinzi after the
/// base, the vowel sign e and medial ra before it.
pub(super) fn order(out: &mut Glyphs) {
    let n = out.len;
    let kinzi = n > 3
        && is_ra(out.keys[0])
        && out.cat(1) == Cat::Nukta
        && out.cat(2) == Cat::Virama
        && out.cat(3).is_base();
    let first = if kinzi { 3 } else { 0 };
    let Some(base) = (first..n).find(|&i| out.cat(i).is_base()) else {
        return;
    };
    for i in 0..base {
        out.info[i].pos = if i < first { POS_AFTER_MAIN } else { POS_PRE_C };
    }
    out.info[base].pos = POS_BASE;
    let mut pos = POS_AFTER_MAIN;
    for i in base + 1..n {
        let cat = out.cat(i);
        out.info[i].pos = match cat {
            Cat::SplitMatra => POS_PRE_C,
            Cat::PreMatra => POS_PRE_M,
            Cat::BelowMatra if pos == POS_AFTER_MAIN => {
                pos = POS_BELOW_C;
                pos
            }
            Cat::Modifier if pos == POS_BELOW_C => POS_BEFORE_SUB,
            Cat::BelowMatra => pos,
            _ if pos == POS_BELOW_C => {
                pos = POS_AFTER_SUB;
                pos
            }
            _ => pos,
        };
    }
    // Sorted by position, stably.
    for i in 1..n {
        let mut j = i;
        while j > 0 && out.info[j - 1].pos > out.info[j].pos {
            out.keys.swap(j - 1, j);
            out.info.swap(j - 1, j);
            j -= 1;
        }
    }
}
