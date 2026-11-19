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

/// What `ch` (of the Myanmar block) is in a syllable.
pub(super) fn category(ch: char) -> Cat {
    match ch {
        '\u{1000}'..='\u{1020}'
        | '\u{103F}'
        | '\u{104E}'
        | '\u{1050}'..='\u{1051}'
        | '\u{105A}'..='\u{105D}'
        | '\u{1061}'
        | '\u{1065}'..='\u{1066}'
        | '\u{106E}'..='\u{1070}'
        | '\u{1075}'..='\u{1081}'
        | '\u{108E}' => Cat::Consonant,
        '\u{1021}'..='\u{102A}'
        | '\u{104C}'..='\u{104D}'
        | '\u{104F}'
        | '\u{1052}'..='\u{1055}' => Cat::Vowel,
        '\u{1031}' | '\u{1084}' => Cat::PreMatra,
        '\u{103C}' => Cat::SplitMatra,
        '\u{102D}'..='\u{102E}'
        | '\u{1033}'..='\u{1035}'
        | '\u{1071}'..='\u{1074}'
        | '\u{1085}'..='\u{1086}'
        | '\u{109D}' => Cat::AboveMatra,
        '\u{102F}'..='\u{1030}' | '\u{1058}'..='\u{1059}' => Cat::BelowMatra,
        '\u{102B}'..='\u{102C}'
        | '\u{1056}'..='\u{1057}'
        | '\u{1062}'
        | '\u{1067}'..='\u{1068}'
        | '\u{1083}' => Cat::PostMatra,
        '\u{103B}' | '\u{103D}'..='\u{103E}' | '\u{105E}'..='\u{1060}' | '\u{1082}' => Cat::Medial,
        '\u{1032}' | '\u{1036}' => Cat::Modifier,
        '\u{1037}'
        | '\u{1038}'
        | '\u{1063}'..='\u{1064}'
        | '\u{1069}'..='\u{106D}'
        | '\u{1087}'..='\u{108D}'
        | '\u{108F}'
        | '\u{109A}'..='\u{109C}' => Cat::Tone,
        '\u{1039}' => Cat::Virama,
        '\u{103A}' => Cat::Nukta,
        _ => Cat::Other,
    }
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
