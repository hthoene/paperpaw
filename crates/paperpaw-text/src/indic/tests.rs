//! Indic shaping tests.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "tests")]

extern crate std;

use alloc::vec::Vec;

use super::*;

/// The keys of `text` shaped run by run with `shaper`.
fn shaped<B: Bytes + ?Sized>(shaper: &Shaper<'_, B>, text: &str) -> Vec<char> {
    let mut keys = Vec::new();
    let mut glyphs = Glyphs::default();
    let mut at = 0;
    while at < text.len() {
        let end = run_end(text, at);
        shaper.shape(&text[at..end], at == 0, &mut glyphs);
        keys.extend(glyphs.keys());
        at = end;
    }
    keys
}

#[test]
fn without_substitutions_vowel_signs_move_left_and_split() {
    let shaper: Shaper<'_, [u8]> = Shaper::new(None);
    // Bengali i and e before their consonant, o as e and aa around it.
    assert_eq!(shaped(&shaper, "কি"), ['\u{9BF}', 'ক']);
    assert_eq!(shaped(&shaper, "কো"), ['\u{9C7}', 'ক', '\u{9BE}']);
    assert_eq!(shaped(&shaper, "কৌ"), ['\u{9C7}', 'ক', '\u{9D7}']);
    // With no conjunct to draw, the sign goes after the virama, before
    // the consonant it is read with.
    assert_eq!(shaped(&shaper, "স্কি"), ['স', '\u{9CD}', '\u{9BF}', 'ক']);
    // Devanagari i; other signs stay where they are.
    assert_eq!(
        shaped(&shaper, "हिन्दी"),
        ['\u{93F}', 'ह', 'न', '\u{94D}', 'द', '\u{940}']
    );
    // Joiners draw nothing; other text is left as it is.
    assert_eq!(shaped(&shaper, "a\u{200D}b"), ['a', 'b']);
}

/// Khmer, Lao and Tibetan without a pack's substitutions (#314): coeng and
/// ro, and Khmer's vowel signs drawn left, move before the consonant, its
/// split vowels are e and their own sign; Lao's sara am is niggahita (over
/// the tone mark) and sara aa; Tibetan's composed letters are their parts.
#[test]
fn khmer_lao_and_tibetan_are_ordered_and_decomposed() {
    let shaper: Shaper<'_, [u8]> = Shaper::new(None);
    // Khmer srei: sa, coeng, ro, ii; ro and coeng go first.
    assert_eq!(shaped(&shaper, "ស្រី"), ['\u{17D2}', 'រ', 'ស', '\u{17B8}']);
    // Kaet: oe is e before the consonant and oe's own glyph after it.
    assert_eq!(shaped(&shaper, "កើត"), ['\u{17C1}', 'ក', '\u{17BE}', 'ត']);
    // E after a subscript: before the whole cluster.
    assert_eq!(shaped(&shaper, "ខ្មែ"), ['\u{17C2}', 'ខ', '\u{17D2}', 'ម']);
    // Lao nam: niggahita before the tone mark, then sara aa.
    assert_eq!(
        shaped(&shaper, "ນ້ຳ"),
        ['ນ', '\u{ECD}', '\u{EC9}', '\u{EB2}']
    );
    // Tibetan gha and the long ii as their parts.
    assert_eq!(shaped(&shaper, "གྷཱི"), ['ག', '\u{FB7}', '\u{F71}', '\u{F72}']);
    for text in ["ស្រី", "ນ້ຳ", "སྐད"] {
        assert!(has_indic(text) && has_letters(text), "{text}");
    }
    assert!(!has_indic("ภาษาไทย"));
    // A Khmer cluster with its subscripts, Lao and Tibetan ones with their
    // signs; a tsheg alone.
    assert_eq!(syllable_end("ស្ត្រីក", 0), "ស្ត្រី".len());
    assert_eq!(syllable_end("ເຂົ້າ", "ເ".len()), "ເຂົ້າ".len());
    assert_eq!(syllable_end("སྒྲུབས", 0), "སྒྲུ".len());
    assert_eq!(syllable_end("་ཀ", 0), "་".len());
    // Shaped as a run (a word, or up to a space), measured by syllables.
    assert_eq!(run_end("བོད་ཀྱི", 0), "བོད་ཀྱི".len());
    let lao = "ຂ້ອຍຮັກ";
    assert_eq!(shaped_at(lao, "ຂ້ອ".len()), "ຂ້ອ".len().."ຂ້ອຍ".len());
    assert_eq!(shaped_at(lao, "ຂ".len()), 0.."ຂ້".len());
}

#[test]
fn syllables_runs_and_what_is_drawn_as_one() {
    let word = "কর্ম";
    assert_eq!(syllable_end(word, 0), "ক".len());
    assert_eq!(syllable_end(word, "ক".len()), word.len());
    assert_eq!(run_end(word, 0), word.len());
    // A word is drawn as one, from any of its characters; a space and
    // Latin letters each alone.
    let text = "ab কর্ম দে";
    let start = "ab ".len();
    assert_eq!(shaped_at(text, start + 3), start..start + word.len());
    assert_eq!(shaped_at(text, 1), 1..2);
    assert_eq!(shaped_at(text, 2), 2..3);
    assert!(has_indic(text) && has_indic("हि") && !has_indic("abc") && !has_indic("日本"));
    // A danda or digits alone have nothing for a pack's substitutions.
    assert!(has_letters(text) && !has_letters("।") && !has_letters("১২ ।"));
    // Signs without a letter stay together; a long word is cut.
    assert_eq!(syllable_end("\u{9BF}\u{981}k", 0), "\u{9BF}\u{981}".len());
    let long = "ক".repeat(40);
    assert_eq!(run_end(&long, 0), MAX_RUN * "ক".len());
}

#[test]
fn which_keys_are_marks() {
    for mark in [
        '\u{9C1}',
        '\u{981}',
        '\u{9CD}',
        '\u{941}',
        '\u{902}',
        '\u{F_8010}',
    ] {
        assert!(is_mark_key(mark), "{mark:?}");
    }
    for letter in ['ক', '\u{9BE}', '\u{9BF}', '\u{982}', 'a', '\u{F_1010}'] {
        assert!(!is_mark_key(letter), "{letter:?}");
    }
    // Khmer coeng and vowels above and below, Lao's and Tibetan's marks;
    // Khmer reahmuk, aa and Tibetan's visarga take room.
    for mark in [
        '\u{17D2}', '\u{17B7}', '\u{17BB}', '\u{17C6}', '\u{EB4}', '\u{EC8}', '\u{F72}', '\u{FB1}',
    ] {
        assert!(is_mark_key(mark), "{mark:?}");
    }
    for letter in [
        'ក', '\u{17B6}', '\u{17C7}', 'ກ', '\u{EB2}', 'ཀ', '\u{F7F}', '\u{F0B}',
    ] {
        assert!(!is_mark_key(letter), "{letter:?}");
    }
    assert_eq!(glyph_of_key('\u{F_1010}'), Some(0x10));
    assert_eq!(glyph_of_key('\u{F_8010}'), Some(0x10));
    assert_eq!(glyph_of_key('ক'), None);
}

#[cfg(feature = "outline")]
mod fonts {
    use alloc::rc::Rc;
    use alloc::string::String;
    use alloc::vec::Vec;
    use std::sync::OnceLock;

    use proptest::prelude::*;

    use super::*;
    use crate::{Around, BitmapFont, Font, OutlineFont, PackSource, StoredPack};

    const BENGALI: &[u8] = include_bytes!("../../../../tools/fonts/NotoSerifBengali.ttf");
    const DEVANAGARI: &[u8] = include_bytes!("../../../../tools/fonts/NotoSerifDevanagari.ttf");

    const KHMER: &[u8] = include_bytes!("../../../../tools/fonts/NotoSerifKhmer.ttf");
    const LAO: &[u8] = include_bytes!("../../../../tools/fonts/NotoSerifLao.ttf");
    const TIBETAN: &[u8] = include_bytes!("../../../../tools/fonts/NotoSerifTibetan.ttf");

    /// A pack of `font` at 20 px with its script's letters (and their
    /// forms), made as the store's packs are.
    fn pack(font: &'static [u8], set: &str) -> Vec<u8> {
        let ranges = crate::charset::named(set).unwrap();
        pack_of(font, crate::charset::chars(ranges))
    }

    /// [`pack`] of the characters `chars`.
    fn pack_of(font: &'static [u8], chars: impl IntoIterator<Item = char> + Clone) -> Vec<u8> {
        let outline = OutlineFont::new(font, &[]).unwrap();
        crate::build_pack(&outline, &[20.0], chars)
    }

    /// The packs of the store's Khmer, Lao and Tibetan letters (the blocks
    /// `tools/script-pack.py` gives them).
    fn khmer_pack() -> &'static [u8] {
        static PACK: OnceLock<Vec<u8>> = OnceLock::new();
        PACK.get_or_init(|| pack_of(KHMER, '\u{1780}'..='\u{17FF}'))
    }

    fn lao_pack() -> &'static [u8] {
        static PACK: OnceLock<Vec<u8>> = OnceLock::new();
        PACK.get_or_init(|| pack_of(LAO, '\u{E80}'..='\u{EFF}'))
    }

    fn tibetan_pack() -> &'static [u8] {
        static PACK: OnceLock<Vec<u8>> = OnceLock::new();
        PACK.get_or_init(|| pack_of(TIBETAN, '\u{F00}'..='\u{FFF}'))
    }

    fn bengali() -> &'static [u8] {
        static PACK: OnceLock<Vec<u8>> = OnceLock::new();
        PACK.get_or_init(|| pack(BENGALI, "bengali"))
    }

    fn devanagari() -> &'static [u8] {
        static PACK: OnceLock<Vec<u8>> = OnceLock::new();
        PACK.get_or_init(|| pack(DEVANAGARI, "devanagari"))
    }

    /// The glyph numbers of `text` shaped with the pack, as the font's.
    fn glyphs(pack: &[u8], font: &[u8], text: &str) -> Vec<u16> {
        let font = read_fonts::FontRef::new(font).unwrap();
        let table = BitmapFont::new(pack.to_vec())
            .unwrap()
            .table(BitmapFont::SUBSTITUTIONS)
            .map(<[u8]>::to_vec)
            .unwrap();
        let shaper = Shaper::new(Substitutions::new(&table));
        shaped(&shaper, text)
            .into_iter()
            .map(|k| crate::marks::glyph_of(&font, k).unwrap().to_u16())
            .collect()
    }

    /// Words with conjuncts, reph, ya- and ra-phala, split vowels and
    /// vowel signs drawn left, as HarfBuzz 11 shapes them with these
    /// fonts (`uharfbuzz`, kerning off): glyph by glyph.
    #[test]
    fn known_words_are_shaped_as_harfbuzz_shapes_them() {
        let words: [(&str, &[u16]); 7] = [
            ("বাংলা", &[47, 59, 11, 52, 478]),
            ("রবীন্দ্রনাথ", &[51, 47, 61, 303, 44, 59, 41]),
            ("কর্ম", &[25, 49, 124]),
            ("স্বাধীনতা", &[434, 59, 43, 61, 44, 40, 478]),
            ("ক্ষমা", &[142, 49, 478]),
            ("কৌতুক", &[479, 25, 72, 40, 62, 25]),
            ("ব্যবহার", &[47, 125, 47, 56, 59, 51]),
        ];
        for (word, want) in words {
            assert_eq!(glyphs(bengali(), BENGALI, word), want, "{word}");
        }
        let words_hi: [(&str, &[u16]); 4] = [
            ("कर्म", &[57, 82, 514]),
            ("क्षमा", &[92, 82, 31]),
            ("प्रेम", &[312, 40, 82]),
            ("राष्ट्र", &[84, 31, 472, 516]),
        ];
        for (word, want) in words_hi {
            assert_eq!(glyphs(devanagari(), DEVANAGARI, word), want, "{word}");
        }
    }

    /// Khmer's coeng subscripts, ro before its consonant, split vowels and
    /// ligatures of a consonant and aa across a subscript (a lookup that
    /// passes over marks); Lao's tone marks over vowels above (forms the
    /// letter before chooses); Tibetan's stacks: as HarfBuzz 11 shapes them
    /// with Noto Serif Khmer, Lao and Tibetan.
    #[test]
    fn khmer_lao_and_tibetan_are_shaped_as_harfbuzz_shapes_them() {
        let khmer: [(&str, &[u16]); 6] = [
            ("ឆ្នាំ", &[221, 182, 112]),
            ("ឈ្មោះ", &[106, 225, 189, 114]),
            ("សង្គ្រាម", &[58, 194, 217, 158, 51]),
            ("ស្រី", &[193, 58, 84]),
            ("កើត", &[106, 24, 84, 40]),
            ("ខ្មែរ", &[107, 25, 189, 53]),
        ];
        for (word, want) in khmer {
            assert_eq!(glyphs(khmer_pack(), KHMER, word), want, "{word}");
        }
        let lao: [(&str, &[u16]); 4] = [
            ("ເຂົ້າ", &[81, 10, 66, 96, 60]),
            ("ນ້ຳ", &[30, 115, 60]),
            ("ຢູ່", &[39, 78, 91]),
            ("ພາສາລາວ", &[35, 60, 45, 60, 41, 60, 42]),
        ];
        for (word, want) in lao {
            assert_eq!(glyphs(lao_pack(), LAO, word), want, "{word}");
        }
        let tibetan: [(&str, &[u16]); 5] = [
            ("ཀྱི", &[176, 1328]),
            ("རྒྱལ", &[849, 40]),
            ("སྐད", &[1088, 22]),
            ("བསྒྲུབས", &[27, 1103, 27, 43]),
            ("བོད", &[27, 1341, 22]),
        ];
        for (word, want) in tibetan {
            assert_eq!(glyphs(tibetan_pack(), TIBETAN, word), want, "{word}");
        }
    }

    #[test]
    fn the_tables_fit_what_a_card_pack_keeps_in_memory() {
        for pack in [
            bengali(),
            devanagari(),
            khmer_pack(),
            lao_pack(),
            tibetan_pack(),
        ] {
            let pack = BitmapFont::new(pack.to_vec()).unwrap();
            let table = pack.table(BitmapFont::SUBSTITUTIONS).unwrap();
            let len = u32::from_le_bytes(table[4..8].try_into().unwrap()) as usize;
            assert!(len <= crate::substitutions::MAX_HELD, "{len}");
        }
    }

    struct Memory(Vec<u8>, core::cell::Cell<usize>);

    impl PackSource for Memory {
        fn read_at(&self, at: u64, buf: &mut [u8]) -> bool {
            self.1.set(self.1.get() + 1);
            let at = at as usize;
            match self.0.get(at..at + buf.len()) {
                Some(src) => {
                    buf.copy_from_slice(src);
                    true
                }
                None => false,
            }
        }
    }

    /// A pack read from a card sets a word as the pack in memory does (its
    /// advances kept in 1/16 px, the pack's in 1/64), its substitutions
    /// read from the card once.
    #[test]
    fn a_pack_on_a_card_shapes_as_one_in_memory() {
        let source = Rc::new(Memory(bengali().to_vec(), core::cell::Cell::new(0)));
        let stored = Font::Stored(StoredPack::open(source.clone()).unwrap());
        let memory = Font::Bitmap(BitmapFont::new(bengali().to_vec()).unwrap());
        let text = "আমার সোনার বাংলা আমি তোমায় ভালোবাসি।";
        let width = crate::measure_in(&memory, 20.0, text, &Around::NONE);
        assert!(width > 0.0);
        let on_card = crate::measure_in(&stored, 20.0, text, &Around::NONE);
        assert!((on_card - width).abs() < 2.0, "{on_card} {width}");
        let reads = source.1.get();
        assert_eq!(
            crate::measure_in(&stored, 20.0, text, &Around::NONE),
            on_card
        );
        // The second time only advances (kept too): no table reads.
        assert_eq!(source.1.get(), reads);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// Any mix of the scripts' characters, joiners and Latin shapes
        /// without a panic, within the keys a run may have.
        #[test]
        fn any_text_shapes(text in "[\\u{980}-\\u{9FF}\\u{900}-\\u{97F}\\u{E80}-\\u{FFF}\\u{1780}-\\u{17FF}\\u{200C}\\u{200D}a ]{0,80}") {
            for pack in [bengali(), devanagari(), khmer_pack(), lao_pack(), tibetan_pack()] {
                let pack = BitmapFont::new(pack.to_vec()).unwrap();
                let table = pack.table(BitmapFont::SUBSTITUTIONS).unwrap();
                let shaper = Shaper::new(Substitutions::new(table));
                let mut glyphs = Glyphs::default();
                let mut at = 0;
                while at < text.len() {
                    let end = run_end(&text, at);
                    prop_assert!(end > at);
                    shaper.shape(&text[at..end], at == 0, &mut glyphs);
                    prop_assert!(glyphs.keys().count() <= MAX_SHAPED);
                    at = end;
                }
                let font = Font::Bitmap(pack);
                let _ = crate::measure_in(&font, 20.0, &text, &Around::NONE);
            }
        }
    }

    /// Prints each word of `$INDIC_WORDS` shaped with a pack made from
    /// `$INDIC_FONT`, as glyph numbers, to `$INDIC_OUT` (for comparing
    /// with HarfBuzz).
    #[test]
    #[ignore = "a tool: compares with HarfBuzz"]
    fn print_shaped() {
        let (Ok(font), Ok(words), Ok(out)) = (
            std::env::var("INDIC_FONT"),
            std::env::var("INDIC_WORDS"),
            std::env::var("INDIC_OUT"),
        ) else {
            return;
        };
        let data: &'static [u8] = std::fs::read(font).unwrap().leak();
        let pack = match std::env::var("INDIC_BLOCK").ok().as_deref() {
            Some("khmer") => khmer_pack().to_vec(),
            Some("lao") => lao_pack().to_vec(),
            Some("tibetan") => tibetan_pack().to_vec(),
            _ => [pack(data, "bengali"), pack(data, "devanagari")]
                .into_iter()
                .max_by_key(Vec::len)
                .unwrap(),
        };
        let mut lines = String::new();
        for word in std::fs::read_to_string(words).unwrap().split_whitespace() {
            let gids: Vec<String> = glyphs(&pack, data, word)
                .iter()
                .map(|g| alloc::format!("{g}"))
                .collect();
            lines.push_str(&alloc::format!("{word}\t{}\n", gids.join(" ")));
        }
        std::fs::write(out, lines).unwrap();
    }
}
