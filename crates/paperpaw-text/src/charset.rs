//! Character sets for bitmap font packs.

use core::ops::RangeInclusive;

/// Latin script for Western, Central and Northern European languages,
/// plus common typographic punctuation.
pub const LATIN: &[RangeInclusive<char>] = &[
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
pub const LIGATURES: &[RangeInclusive<char>] = &['\u{fb00}'..='\u{fb04}'];

/// Cyrillic (Russian, Ukrainian, Bulgarian, Serbian and more), for packs
/// added to fonts made for Latin.
#[allow(
    clippy::single_range_in_vec_init,
    reason = "a set of ranges, of one here"
)]
pub const CYRILLIC: &[RangeInclusive<char>] = &['\u{400}'..='\u{4ff}'];

/// Greek, modern and polytonic (Greek Extended, for older texts).
pub const GREEK: &[RangeInclusive<char>] = &['\u{370}'..='\u{3ff}', '\u{1f00}'..='\u{1fff}'];

paperpaw_native::shared! {
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
} @ CHARSET_STAND_IN
}

/// A character set by name: `latin`, `ligatures`, `cyrillic` or `greek`.
pub fn named(name: &str) -> Option<&'static [RangeInclusive<char>]> {
    match name {
        "latin" => Some(LATIN),
        "ligatures" => Some(LIGATURES),
        "cyrillic" => Some(CYRILLIC),
        "greek" => Some(GREEK),
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
}
