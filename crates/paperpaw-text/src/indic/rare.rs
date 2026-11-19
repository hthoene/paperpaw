//! Khmer, Lao and Tibetan (#314): what their characters are in a
//! syllable, the characters a syllable is shaped from, and Khmer's
//! reordering, as OpenType's shapers for them (HarfBuzz's Khmer shaper,
//! its Thai one for Lao, the Universal Shaping Engine for Tibetan) do.
//!
//! - Khmer: a consonant (or independent vowel) with the consonants joined
//!   below it by coeng (U+17D2), its vowel signs and signs. Coeng and ro
//!   move before the base (the font draws them there, `pref`), as do the
//!   vowel signs drawn left of it (U+17C1 to U+17C3); a split vowel is
//!   e (U+17C1) and its own glyph after the consonants. The forms below
//!   and after the base apply to all but it.
//! - Lao: a consonant with its vowels and tone marks above and below; sara
//!   am (U+0EB3) is niggahita and sara aa, the niggahita before the tone
//!   marks over the letter. Placed by the font's anchors.
//! - Tibetan: a consonant with the subjoined ones it stacks (U+0F90 to
//!   U+0FBC) and its vowel signs; the characters Unicode decomposes are
//!   their parts. The font draws the stacks (`ccmp`, `blws`, `abvs`).

use super::{Cat, Glyphs, POST, Script};

/// What `ch` (of `script`, one of the three or Myanmar) is in a syllable.
pub(super) fn category(script: Script, ch: char) -> Cat {
    match script {
        Script::Khmer => khmer(ch),
        Script::Lao => lao(ch),
        Script::Myanmar => super::myanmar::category(ch),
        _ => tibetan(ch),
    }
}

fn khmer(ch: char) -> Cat {
    match ch {
        '\u{1780}'..='\u{17A2}' => Cat::Consonant,
        '\u{17A3}'..='\u{17B3}' => Cat::Vowel,
        '\u{17B6}' => Cat::PostMatra,
        '\u{17B7}'..='\u{17BA}' => Cat::AboveMatra,
        '\u{17BB}'..='\u{17BD}' => Cat::BelowMatra,
        '\u{17BE}'..='\u{17C0}' | '\u{17C4}'..='\u{17C5}' => Cat::SplitMatra,
        '\u{17C1}'..='\u{17C3}' => Cat::PreMatra,
        '\u{17B4}'..='\u{17B5}' | '\u{17C6}'..='\u{17D1}' | '\u{17D3}' | '\u{17DD}' => {
            Cat::Modifier
        }
        '\u{17D2}' => Cat::Virama,
        _ => Cat::Other,
    }
}

fn lao(ch: char) -> Cat {
    match ch {
        '\u{E81}'..='\u{EAE}' | '\u{EBD}' | '\u{EDC}'..='\u{EDF}' => Cat::Consonant,
        '\u{EB0}' | '\u{EB2}' | '\u{EB3}' => Cat::PostMatra,
        '\u{EB1}' | '\u{EB4}'..='\u{EB7}' | '\u{EBB}' => Cat::AboveMatra,
        '\u{EB8}'..='\u{EBA}' | '\u{EBC}' => Cat::BelowMatra,
        '\u{EC8}'..='\u{ECE}' => Cat::Modifier,
        _ => Cat::Other,
    }
}

fn tibetan(ch: char) -> Cat {
    match ch {
        '\u{F40}'..='\u{F6C}' | '\u{F88}'..='\u{F8C}' => Cat::Consonant,
        '\u{F3E}'..='\u{F3F}' | '\u{F7F}' => Cat::PostMatra,
        '\u{F39}' | '\u{F72}' | '\u{F73}' | '\u{F7A}'..='\u{F7E}' | '\u{F80}'..='\u{F83}' => {
            Cat::AboveMatra
        }
        '\u{F18}'..='\u{F19}'
        | '\u{F35}'
        | '\u{F37}'
        | '\u{F71}'
        | '\u{F74}'..='\u{F79}'
        | '\u{F84}'
        | '\u{F8D}'..='\u{F97}'
        | '\u{F99}'..='\u{FBC}'
        | '\u{FC6}' => Cat::BelowMatra,
        '\u{F86}'..='\u{F87}' => Cat::Modifier,
        _ => Cat::Other,
    }
}

/// Whether `ch` (of `script`, category `cat`) is drawn on the letter before
/// it, taking no room of its own.
pub(super) fn is_mark(script: Script, ch: char, cat: Cat) -> bool {
    if script == Script::Myanmar {
        return super::myanmar::is_mark(ch, cat);
    }
    match cat {
        Cat::Virama | Cat::AboveMatra | Cat::BelowMatra => true,
        // Reahmuk and yuukaleapintu take room.
        Cat::Modifier => !(script == Script::Khmer && matches!(ch, '\u{17C7}' | '\u{17C8}')),
        _ => false,
    }
}

/// Where the syllable that starts with `first` goes on: whether the next
/// character `next` (category `cat`, after `before`, category
/// `before_cat`) belongs to it.
pub(super) fn takes(script: Script, first: Cat, before_cat: Cat, cat: Cat) -> bool {
    if script == Script::Myanmar {
        return super::myanmar::takes(first, before_cat, cat);
    }
    let sign = |c: Cat| c.is_matra() || matches!(c, Cat::Modifier | Cat::Virama | Cat::Nukta);
    match cat {
        // A consonant joined by coeng.
        Cat::Consonant | Cat::Vowel => script == Script::Khmer && before_cat == Cat::Virama,
        Cat::Zwj | Cat::Zwnj => script == Script::Khmer,
        _ if sign(cat) => first.is_base() || sign(first),
        _ => false,
    }
}

/// The characters `ch` is shaped from in `script`, into `out`: itself, or
/// the parts Unicode or the script's shaper decomposes it into.
pub(super) fn push(script: Script, ch: char, out: &mut Glyphs) {
    let parts: &[u32] = match (script, ch) {
        (Script::Khmer, '\u{17BE}'..='\u{17C0}' | '\u{17C4}'..='\u{17C5}') => {
            out.push(0x17C1, Cat::PreMatra);
            out.push(u32::from(ch), Cat::PostMatra);
            return;
        }
        (Script::Lao, '\u{EB3}') => {
            // Niggahita before the tone marks and vowels above.
            let mut at = out.len;
            while at > 0
                && matches!(out.keys[at - 1], 0xEB1 | 0xEB4..=0xEB7 | 0xEBB | 0xEC7..=0xECE)
            {
                at -= 1;
            }
            out.push(0xECD, Cat::Modifier);
            out.move_glyph(out.len - 1, at);
            out.push(0xEB2, Cat::PostMatra);
            return;
        }
        (Script::Tibetan, _) => match ch {
            '\u{F43}' => &[0xF42, 0xFB7],
            '\u{F4D}' => &[0xF4C, 0xFB7],
            '\u{F52}' => &[0xF51, 0xFB7],
            '\u{F57}' => &[0xF56, 0xFB7],
            '\u{F5C}' => &[0xF5B, 0xFB7],
            '\u{F69}' => &[0xF40, 0xFB5],
            '\u{F73}' => &[0xF71, 0xF72],
            '\u{F75}' => &[0xF71, 0xF74],
            '\u{F76}' => &[0xFB2, 0xF80],
            '\u{F78}' => &[0xFB3, 0xF80],
            '\u{F81}' => &[0xF71, 0xF80],
            '\u{F93}' => &[0xF92, 0xFB7],
            '\u{F9D}' => &[0xF9C, 0xFB7],
            '\u{FA2}' => &[0xFA1, 0xFB7],
            '\u{FA7}' => &[0xFA6, 0xFB7],
            '\u{FAC}' => &[0xFAB, 0xFB7],
            '\u{FB9}' => &[0xF90, 0xFB5],
            _ => &[],
        },
        _ => &[],
    };
    if parts.is_empty() {
        out.push(u32::from(ch), category(script, ch));
    }
    for &part in parts {
        let cat = char::from_u32(part).map_or(Cat::Other, |c| category(script, c));
        out.push(part, cat);
    }
}

/// Khmer's syllable before its forms: all but the first glyph take the
/// forms below, above and after a base; coeng and ro (the first such pair,
/// within two coengs) move to the start, then the vowel signs drawn left.
pub(super) fn khmer_order(out: &mut Glyphs) {
    for i in 1..out.len {
        out.info[i].mask |= POST;
    }
    let mut coengs = 0;
    let mut i = 1;
    while i < out.len {
        if out.cat(i) == Cat::Virama && coengs <= 2 && i + 1 < out.len {
            coengs += 1;
            if out.keys[i + 1] == 0x179A {
                out.move_glyph(i, 0);
                out.move_glyph(i + 1, 1);
                coengs = 2;
            }
        } else if out.cat(i) == Cat::PreMatra {
            out.move_glyph(i, 0);
        }
        i += 1;
    }
}
