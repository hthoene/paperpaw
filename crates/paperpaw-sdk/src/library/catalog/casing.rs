//! The lowercase of a text as `str::to_lowercase` makes it, into a string
//! reserved fallibly (`str::to_lowercase` allocates without a way to refuse).
//!
//! `str::to_lowercase` is `char::to_lowercase` for each char but for the
//! capital sigma, which is a final `ς` at the end of a word: after a cased
//! letter and before none, skipping the case-ignorable characters (an
//! apostrophe, a combining mark, a dot…) on both sides. The standard
//! library keeps `Case_Ignorable` and `Cased` private, so [`IGNORABLE`] is
//! a copy of the first, and the second is built from what `char` offers;
//! the tests compare both with the standard library for every char, so a
//! toolchain with newer Unicode data fails there, not silently in the
//! order of a library.

use alloc::string::String;

use crate::data::Error;

/// The `Case_Ignorable` chars as ranges: the first char in the low 21 bits
/// and the range's length less one above them, ordered by first char.
const IGNORABLE: [u32; 464] = [
    0x00000027, 0x0000002E, 0x0000003A, 0x0000005E, 0x00000060, 0x000000A8, 0x000000AD, 0x000000AF,
    0x000000B4, 0x002000B7, 0x17E002B0, 0x00200374, 0x0000037A, 0x00200384, 0x00000387, 0x00C00483,
    0x00000559, 0x0000055F, 0x05800591, 0x000005BF, 0x002005C1, 0x002005C4, 0x000005C7, 0x000005F4,
    0x00A00600, 0x01400610, 0x0000061C, 0x00000640, 0x0280064B, 0x00000670, 0x00E006D6, 0x012006DF,
    0x006006EA, 0x0000070F, 0x00000711, 0x03400730, 0x014007A6, 0x014007EB, 0x000007FA, 0x000007FD,
    0x02E00816, 0x00400859, 0x00000888, 0x00200890, 0x01000897, 0x072008C9, 0x0000093A, 0x0000093C,
    0x00E00941, 0x0000094D, 0x00C00951, 0x00200962, 0x00000971, 0x00000981, 0x000009BC, 0x006009C1,
    0x000009CD, 0x002009E2, 0x000009FE, 0x00200A01, 0x00000A3C, 0x00200A41, 0x00200A47, 0x00400A4B,
    0x00000A51, 0x00200A70, 0x00000A75, 0x00200A81, 0x00000ABC, 0x00800AC1, 0x00200AC7, 0x00000ACD,
    0x00200AE2, 0x00A00AFA, 0x00000B01, 0x00000B3C, 0x00000B3F, 0x00600B41, 0x00000B4D, 0x00200B55,
    0x00200B62, 0x00000B82, 0x00000BC0, 0x00000BCD, 0x00000C00, 0x00000C04, 0x00000C3C, 0x00400C3E,
    0x00400C46, 0x00600C4A, 0x00200C55, 0x00200C62, 0x00000C81, 0x00000CBC, 0x00000CBF, 0x00000CC6,
    0x00200CCC, 0x00200CE2, 0x00200D00, 0x00200D3B, 0x00600D41, 0x00000D4D, 0x00200D62, 0x00000D81,
    0x00000DCA, 0x00400DD2, 0x00000DD6, 0x00000E31, 0x00C00E34, 0x01000E46, 0x00000EB1, 0x01000EB4,
    0x00000EC6, 0x00C00EC8, 0x00200F18, 0x00000F35, 0x00000F37, 0x00000F39, 0x01A00F71, 0x00800F80,
    0x00200F86, 0x01400F8D, 0x04600F99, 0x00000FC6, 0x0060102D, 0x00A01032, 0x00201039, 0x0020103D,
    0x00201058, 0x0040105E, 0x00601071, 0x00001082, 0x00201085, 0x0000108D, 0x0000109D, 0x000010FC,
    0x0040135D, 0x00401712, 0x00201732, 0x00201752, 0x00201772, 0x002017B4, 0x00C017B7, 0x000017C6,
    0x014017C9, 0x000017D7, 0x000017DD, 0x0080180B, 0x00001843, 0x00201885, 0x000018A9, 0x00401920,
    0x00201927, 0x00001932, 0x00401939, 0x00201A17, 0x00001A1B, 0x00001A56, 0x00C01A58, 0x00001A60,
    0x00001A62, 0x00E01A65, 0x01201A73, 0x00001A7F, 0x00001AA7, 0x05A01AB0, 0x01601AE0, 0x00601B00,
    0x00001B34, 0x00801B36, 0x00001B3C, 0x00001B42, 0x01001B6B, 0x00201B80, 0x00601BA2, 0x00201BA8,
    0x00401BAB, 0x00001BE6, 0x00201BE8, 0x00001BED, 0x00401BEF, 0x00E01C2C, 0x00201C36, 0x00A01C78,
    0x00401CD0, 0x01801CD4, 0x00C01CE2, 0x00001CED, 0x00001CF4, 0x00201CF8, 0x07C01D2C, 0x00001D78,
    0x0C801D9B, 0x00001FBD, 0x00401FBF, 0x00401FCD, 0x00401FDD, 0x00401FED, 0x00201FFD, 0x0080200B,
    0x00202018, 0x00002024, 0x00002027, 0x0080202A, 0x00802060, 0x01202066, 0x00002071, 0x0000207F,
    0x01802090, 0x040020D0, 0x00202C7C, 0x00402CEF, 0x00002D6F, 0x00002D7F, 0x03E02DE0, 0x00002E2F,
    0x00003005, 0x0060302A, 0x00803031, 0x0000303B, 0x00A03099, 0x004030FC, 0x0000A015, 0x00A0A4F8,
    0x0000A60C, 0x0060A66F, 0x0120A674, 0x0000A67F, 0x0060A69C, 0x0020A6F0, 0x0420A700, 0x0000A770,
    0x0040A788, 0x0060A7F1, 0x0020A7F8, 0x0000A802, 0x0000A806, 0x0000A80B, 0x0020A825, 0x0000A82C,
    0x0020A8C4, 0x0220A8E0, 0x0000A8FF, 0x00E0A926, 0x0140A947, 0x0040A980, 0x0000A9B3, 0x0060A9B6,
    0x0020A9BC, 0x0000A9CF, 0x0020A9E5, 0x00A0AA29, 0x0020AA31, 0x0020AA35, 0x0000AA43, 0x0000AA4C,
    0x0000AA70, 0x0000AA7C, 0x0000AAB0, 0x0040AAB2, 0x0020AAB7, 0x0020AABE, 0x0000AAC1, 0x0000AADD,
    0x0020AAEC, 0x0020AAF3, 0x0000AAF6, 0x0080AB5B, 0x0040AB69, 0x0000ABE5, 0x0000ABE8, 0x0000ABED,
    0x0000FB1E, 0x0200FBB2, 0x01E0FE00, 0x0000FE13, 0x01E0FE20, 0x0000FE52, 0x0000FE55, 0x0000FEFF,
    0x0000FF07, 0x0000FF0E, 0x0000FF1A, 0x0000FF3E, 0x0000FF40, 0x0000FF70, 0x0020FF9E, 0x0000FFE3,
    0x0040FFF9, 0x000101FD, 0x000102E0, 0x00810376, 0x00A10780, 0x05210787, 0x010107B2, 0x00410A01,
    0x00210A05, 0x00610A0C, 0x00410A38, 0x00010A3F, 0x00210AE5, 0x00610D24, 0x00010D4E, 0x00810D69,
    0x00010D6F, 0x00210EAB, 0x00010EC5, 0x00A10EFA, 0x01410F46, 0x00610F82, 0x00011001, 0x01C11038,
    0x00011070, 0x00211073, 0x0041107F, 0x006110B3, 0x002110B9, 0x000110BD, 0x000110C2, 0x000110CD,
    0x00411100, 0x00811127, 0x00E1112D, 0x00011173, 0x00211180, 0x010111B6, 0x006111C9, 0x000111CF,
    0x0041122F, 0x00011234, 0x00211236, 0x0001123E, 0x00011241, 0x000112DF, 0x00E112E3, 0x00211300,
    0x0021133B, 0x00011340, 0x00C11366, 0x00811370, 0x00A113BB, 0x000113CE, 0x000113D0, 0x000113D2,
    0x002113E1, 0x00E11438, 0x00411442, 0x00011446, 0x0001145E, 0x00A114B3, 0x000114BA, 0x002114BF,
    0x002114C2, 0x006115B2, 0x002115BC, 0x002115BF, 0x002115DC, 0x00E11633, 0x0001163D, 0x0021163F,
    0x000116AB, 0x000116AD, 0x00A116B0, 0x000116B7, 0x0001171D, 0x0001171F, 0x00611722, 0x00811727,
    0x0101182F, 0x00211839, 0x0021193B, 0x0001193E, 0x00011943, 0x006119D4, 0x002119DA, 0x000119E0,
    0x01211A01, 0x00A11A33, 0x00611A3B, 0x00011A47, 0x00A11A51, 0x00411A59, 0x01811A8A, 0x00211A98,
    0x00011B60, 0x00411B62, 0x00011B66, 0x00C11C30, 0x00A11C38, 0x00011C3F, 0x02A11C92, 0x00C11CAA,
    0x00211CB2, 0x00211CB5, 0x00A11D31, 0x00011D3A, 0x00211D3C, 0x00C11D3F, 0x00011D47, 0x00211D90,
    0x00011D95, 0x00011D97, 0x00011DD9, 0x00211EF3, 0x00211F00, 0x00811F36, 0x00011F40, 0x00011F42,
    0x00011F5A, 0x02013430, 0x01C13447, 0x0161611E, 0x0041612D, 0x00816AF0, 0x00C16B30, 0x00616B40,
    0x00416D40, 0x00216D6B, 0x00016F4F, 0x02016F8F, 0x00216FE0, 0x00216FE3, 0x00216FF2, 0x0061AFF0,
    0x00C1AFF5, 0x0021AFFD, 0x0021BC9D, 0x0061BCA0, 0x05A1CF00, 0x02C1CF30, 0x0041D167, 0x01E1D173,
    0x00C1D185, 0x0061D1AA, 0x0041D242, 0x06C1DA00, 0x0621DA3B, 0x0001DA75, 0x0001DA84, 0x0081DA9B,
    0x01C1DAA1, 0x00C1E000, 0x0201E008, 0x00C1E01B, 0x0021E023, 0x0081E026, 0x07A1E030, 0x0001E08F,
    0x01A1E130, 0x0001E2AE, 0x0061E2EC, 0x0081E4EB, 0x0021E5EE, 0x0001E6E3, 0x0001E6E6, 0x0021E6EE,
    0x0001E6F5, 0x0001E6FF, 0x00C1E8D0, 0x00E1E944, 0x0081F3FB, 0x000E0001, 0x0BEE0020, 0x1DEE0100,
];

/// Whether `c` is case-ignorable (a sigma next to it looks past it).
fn is_ignorable(c: char) -> bool {
    let c = u32::from(c);
    let at = IGNORABLE.partition_point(|&range| (range & 0x1F_FFFF) <= c);
    at.checked_sub(1).is_some_and(|at| {
        let range = IGNORABLE[at];
        c <= (range & 0x1F_FFFF) + (range >> 21)
    })
}

/// Whether `c` is cased (lower, upper or title case). Only asked of chars
/// that are not case-ignorable, for which the standard library never asks.
fn is_cased(c: char) -> bool {
    // The 31 titlecase letters are neither lower nor upper case: they are
    // the chars whose upper and lower case differ and whose title case is
    // neither, which the standard library has no name for. They are listed.
    c.is_lowercase()
        || c.is_uppercase()
        || matches!(u32::from(c),
            0x01C5 | 0x01C8 | 0x01CB | 0x01F2 | 0x1F88..=0x1F8F | 0x1F98..=0x1F9F
            | 0x1FA8..=0x1FAF | 0x1FBC | 0x1FCC | 0x1FFC)
}

/// Whether the first char of `chars` that is not case-ignorable is cased.
fn cased_then(mut chars: impl Iterator<Item = char>) -> bool {
    chars.find(|&c| !is_ignorable(c)).is_some_and(is_cased)
}

/// The lowercase of `text` appended to `out`, as `str::to_lowercase`;
/// [`Error::OutOfMemory`] when `out` cannot grow (`out` is then partial).
pub(super) fn push_lower(out: &mut String, text: &str) -> Result<(), Error> {
    out.try_reserve(text.len())
        .map_err(|_| Error::OutOfMemory)?;
    if text.is_ascii() {
        for byte in text.bytes() {
            out.push(char::from(byte.to_ascii_lowercase()));
        }
        return Ok(());
    }
    for (at, c) in text.char_indices() {
        if c == '\u{3A3}' {
            let last = cased_then(text[..at].chars().rev())
                && !cased_then(text[at + c.len_utf8()..].chars());
            out.try_reserve(2).map_err(|_| Error::OutOfMemory)?;
            out.push(if last { '\u{3C2}' } else { '\u{3C3}' });
            continue;
        }
        for lower in c.to_lowercase() {
            out.try_reserve(lower.len_utf8())
                .map_err(|_| Error::OutOfMemory)?;
            out.push(lower);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        reason = "tests panic on purpose when something is wrong"
    )]

    use alloc::format;
    use alloc::string::String;
    use alloc::vec::Vec;

    use super::{is_cased, is_ignorable, push_lower};

    fn lowered_by_std(text: &str) -> String {
        let mut out = String::new();
        push_lower(&mut out, text).unwrap();
        out
    }

    /// Every char, as the standard library decides a sigma by it: after
    /// `AΣ` the sigma is final if the char is ignorable or not cased; and
    /// before `B` it is not if the char is ignorable or cased.
    #[test]
    fn predicates_are_the_standard_librarys_for_every_char() {
        let sigma = |text: &str| text.to_lowercase().chars().nth(1).unwrap();
        for code in 0..=0x10_FFFF_u32 {
            let Some(c) = char::from_u32(code) else {
                continue;
            };
            let after = sigma(&format!("A\u{3A3}{c}")) == '\u{3C2}';
            let before_b = sigma(&format!("A\u{3A3}{c}B")) == '\u{3C3}';
            let ignorable = after && before_b;
            assert_eq!(is_ignorable(c), ignorable, "case-ignorable {code:#X}");
            if !ignorable {
                assert_eq!(is_cased(c), !after, "cased {code:#X}");
            }
        }
    }

    #[test]
    fn lowercase_is_the_standard_librarys_on_a_mixed_corpus() {
        let corpus = [
            "",
            "Hello World",
            "ÄÖÜ ẞtraße STRASSE",
            "Straße",
            "ΣΟΦΟΣ",
            "ΟΔΥΣΣΕΥΣ",
            "Σ",
            "ΑΣ",
            "ΑΣ.",
            "Σ.Α",
            "ΑΣ'Α",
            "ΑΣ’",
            "ΑΣ\u{301}",
            "ΑΣ\u{301}Α",
            "\u{301}ΑΣ",
            "Α\u{301}Σ",
            "ΑΣ ΑΣ",
            "ΑΣ-ΑΣ",
            "ΑΣ1",
            "1Σ",
            "Σ\u{2B0}",
            "ΑΣ\u{2B0}",
            "ΑΣ\u{2B0}Α",
            "ǅΣ",
            "ΑΣǅ",
            "İstanbul",
            "İ",
            "I\u{307}",
            "ISTANBUL",
            "ı",
            "Ǆ ǅ ǆ",
            "ΐ ΰ",
            "ᾈ",
            "ÀÉÎ",
            "e\u{301}",
            "A\u{300}\u{301}\u{302}Σ",
            "Σ\u{300}\u{301}\u{302}A",
            "日本語 ΣΑΣ",
            "Ⓐ Ⓑ",
            "Ꭰ",
            "𐐀",
            "Σ\u{200D}",
            "ΑΣ\u{200D}",
            "ΑΣ\u{AD}",
            "ΑΣ\u{E0001}",
            "Aa\u{3A3}",
            "\u{3A3}\u{3A3}",
            "ΑΣΣ",
            "ΑΣ ΣΑ",
            "ΑΣΑ",
            "ΣΑΣΑΣ.",
            "Ω Σ Α",
        ];
        for text in corpus {
            assert_eq!(lowered_by_std(text), text.to_lowercase(), "{text:?}");
        }
    }

    /// A sigma between every pair of chars of a small alphabet, as the
    /// standard library lowercases the whole.
    #[test]
    fn a_sigma_between_any_two_chars_of_a_small_alphabet() {
        let alphabet = [
            'a', 'Α', 'α', 'Σ', '1', ' ', '.', '\'', '\u{301}', '\u{2B0}', '\u{1C5}', '\u{AD}',
        ];
        let mut texts: Vec<String> = Vec::new();
        for first in alphabet {
            for second in alphabet {
                for third in alphabet {
                    texts.push([first, 'Σ', second, third, 'Σ'].iter().collect());
                    texts.push([first, second, 'Σ', third].iter().collect());
                }
            }
        }
        for text in &texts {
            assert_eq!(lowered_by_std(text), text.to_lowercase(), "{text:?}");
        }
    }
}
