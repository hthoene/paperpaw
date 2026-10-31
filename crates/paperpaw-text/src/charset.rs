//! Character sets for bitmap font packs, by name ([`named`]), for the
//! tools that make packs: what a pack holds is in the pack, so the sets
//! are not part of the plugin interface and grow without changing it.

use core::ops::RangeInclusive;

/// Latin script for Western, Central and Northern European languages,
/// plus common typographic punctuation.
const LATIN: &[RangeInclusive<char>] = &[
    ' '..='~',
    '\u{a0}'..='\u{17f}',    // Latin-1 Supplement, Latin Extended-A
    '\u{2010}'..='\u{2027}', // dashes, quotes, bullets, ellipsis
    '\u{2030}'..='\u{203a}', // per mille, primes, guillemets
    '\u{20ac}'..='\u{20ac}', // euro sign
    '\u{2122}'..='\u{2122}', // trade mark
    '\u{2212}'..='\u{2212}', // minus sign
    '\u{fffd}'..='\u{fffd}', // replacement character
];

/// The ligatures ff, fi, fl, ffi and ffl, drawn in place of their letters
/// where a font has them (see `FontHandle::ligatures` in the SDK).
#[allow(
    clippy::single_range_in_vec_init,
    reason = "a set of ranges, of one here"
)]
const LIGATURES: &[RangeInclusive<char>] = &['\u{fb00}'..='\u{fb04}'];

/// Cyrillic (Russian, Ukrainian, Bulgarian, Serbian and more), for packs
/// added to fonts made for Latin.
#[allow(
    clippy::single_range_in_vec_init,
    reason = "a set of ranges, of one here"
)]
const CYRILLIC: &[RangeInclusive<char>] = &['\u{400}'..='\u{4ff}'];

/// Greek, modern and polytonic (Greek Extended, for older texts), and the
/// modifier letter apostrophe of elisions (`καθʼ`, #60).
const GREEK: &[RangeInclusive<char>] = &[
    '\u{2bc}'..='\u{2bc}',
    '\u{370}'..='\u{3ff}',
    '\u{1f00}'..='\u{1fff}',
];

/// Hangul: all 11,172 syllables of modern Korean (#61). The 2,350 of KS X
/// 1001 that most fonts and books keep to are a list of their own, for
/// smaller packs (`tools/fontgen/sets/hangul-ks.txt`).
#[allow(
    clippy::single_range_in_vec_init,
    reason = "a set of ranges, of one here"
)]
const HANGUL: &[RangeInclusive<char>] = &['\u{ac00}'..='\u{d7a3}'];

/// What Korean, Chinese and Japanese text sets around its letters: CJK
/// punctuation and symbols, Hangul's compatibility letters (jamo written
/// alone) and the full-width forms of ASCII.
const CJK_SYMBOLS: &[RangeInclusive<char>] = &[
    '\u{3000}'..='\u{303f}',
    '\u{3131}'..='\u{318e}',
    '\u{ff01}'..='\u{ff5e}',
];

/// Arabic (also Persian, Urdu and other languages written in it, #33):
/// its letters, marks and digits, the supplement, and the presentation
/// forms the joined letters are drawn as (Forms-B, and of Forms-A those
/// of Persian and Urdu letters and the ligature of Allah: the other
/// ligatures of Forms-A would triple a pack and are not drawn).
const ARABIC: &[RangeInclusive<char>] = &[
    '\u{600}'..='\u{6ff}',
    '\u{750}'..='\u{77f}',
    '\u{fb50}'..='\u{fbff}',
    '\u{fdf2}'..='\u{fdf2}',
    '\u{fe70}'..='\u{feff}',
];

/// Hebrew: its letters, points (niqqud) and cantillation marks, and the
/// presentation forms (letters with points, wide letters).
const HEBREW: &[RangeInclusive<char>] = &['\u{590}'..='\u{5ff}', '\u{fb1d}'..='\u{fb4f}'];

paperpaw_table::shared! {
/// A character of nearly the same look, for `ch` where a font lacks it,
/// tried before the replacement character: a non-breaking hyphen as a
/// hyphen, the long s as s, rare punctuation as its common cousin.
/// Followed again where the font lacks that one too (U+2011, U+2010,
/// `-`).
pub fn stand_in(ch: char) -> Option<char> {
    Some(match ch {
        '\u{2011}' => '\u{2010}', // non-breaking hyphen: hyphen
        '\u{2010}' => '-',
        '\u{17f}' => 's',                                   // long s
        '\u{2016}' => '|',                                  // double vertical line
        '\u{2017}' => '_',                                  // double low line
        '\u{201b}' => '\u{2018}',                           // reversed single quote
        '\u{201f}' => '\u{201c}',                           // reversed double quote
        '\u{2023}' => '\u{2022}',                           // triangular bullet: bullet
        '\u{2024}' => '.',                                  // one dot leader
        '\u{2025}' => '\u{2026}',                           // two dot leader: ellipsis
        '\u{2027}' => '\u{b7}',                             // hyphenation point: middle dot
        '\u{2031}' => '\u{2030}',                           // per ten thousand: per mille
        '\u{2034}' | '\u{2036}' | '\u{2037}' => '\u{2033}', // triple and reversed primes
        '\u{2035}' => '\u{2032}',                           // reversed prime
        '\u{2038}' => '^',                                  // caret
        '\u{fffd}' => '?',                                  // replacement character
        _ => return None,
    })
} @ text::CHARSET_STAND_IN
}

/// A character set by name: `latin`, `ligatures`, `cyrillic`, `greek`,
/// `arabic`, `hebrew`, `hangul` or `cjk-symbols`.
pub fn named(name: &str) -> Option<&'static [RangeInclusive<char>]> {
    match name {
        "latin" => Some(LATIN),
        "ligatures" => Some(LIGATURES),
        "cyrillic" => Some(CYRILLIC),
        "greek" => Some(GREEK),
        "arabic" => Some(ARABIC),
        "hebrew" => Some(HEBREW),
        "hangul" => Some(HANGUL),
        "cjk-symbols" => Some(CJK_SYMBOLS),
        _ => None,
    }
}

/// Code points written as `U+EA01` or `U+EA01-EA0F` (e.g. icons, which
/// have no set of their own).
pub fn code_points(text: &str) -> Option<RangeInclusive<char>> {
    let hex = text
        .strip_prefix("U+")
        .or_else(|| text.strip_prefix("u+"))?;
    let (first, last) = hex.split_once('-').unwrap_or((hex, hex));
    let char_of = |h: &str| u32::from_str_radix(h, 16).ok().and_then(char::from_u32);
    let (first, last) = (char_of(first)?, char_of(last)?);
    (first <= last).then_some(first..=last)
}

/// All characters of `ranges`.
pub fn chars(ranges: &[RangeInclusive<char>]) -> impl Iterator<Item = char> + Clone + '_ {
    ranges.iter().flat_map(|range| range.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_points_are_read() {
        assert_eq!(code_points("U+EA01"), Some('\u{ea01}'..='\u{ea01}'));
        assert_eq!(code_points("U+ea01-EA0F"), Some('\u{ea01}'..='\u{ea0f}'));
        assert_eq!(code_points("U+EA0F-EA01"), None);
        assert_eq!(code_points("U+D800"), None);
        assert_eq!(code_points("latin"), None);
    }

    #[test]
    fn arabic_and_hebrew_hold_their_letters_and_drawn_forms() {
        let has =
            |set: &str, ch: char| named(set).is_some_and(|r| r.iter().any(|r| r.contains(&ch)));
        // Beh, its final form, lam-alef, Allah, fatha; not a rare ligature.
        for ch in [
            '\u{628}', '\u{fe90}', '\u{fefb}', '\u{fdf2}', '\u{64e}', '\u{6cc}',
        ] {
            assert!(has("arabic", ch), "{ch:?}");
        }
        assert!(!has("arabic", '\u{fc00}'));
        // Alef, qamats, shin with shin dot.
        for ch in ['\u{5d0}', '\u{5b8}', '\u{fb2a}'] {
            assert!(has("hebrew", ch), "{ch:?}");
        }
    }
}
