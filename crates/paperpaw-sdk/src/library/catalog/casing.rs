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
