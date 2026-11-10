#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use alloc::string::{String, ToString};
use paperpaw_sdk::document::{BlockKind, Run, TextStyle};
use paperpaw_sdk::filters::Verdict;
use proptest::prelude::*;

fn encoded(bold: bool, list: &str, policy: &str, word: &str) -> Vec<u8> {
    let mut bytes = alloc::vec![0; 3840];
    let len = encode(bold, list, policy, word, &mut bytes).unwrap();
    bytes.truncate(len);
    bytes
}

#[test]
fn preserves_existing_order_styles_unicode_and_offsets() {
    for text in ["alpha alpha2 βήτα", "über café a", "one two one", ""] {
        let original = Block::new(BlockKind::Paragraph, 10, 100, text.to_string());
        for bold in [false, true] {
            let mut expected = original.clone();
            if let Some(ch) = expected.text.chars().next() {
                expected
                    .runs
                    .push(Run::new(0, ch.len_utf8() as u32, TextStyle::ITALIC));
            }
            let mut actual = expected.clone();
            if bold {
                crate::oracle::bold_beginnings(&mut expected);
            }
            crate::oracle::replace(
                &mut expected,
                &crate::oracle::pairs("alpha = one; one = longer; βήτα = b"),
            );
            let bytes = encoded(
                bold,
                "alpha = one; one = longer; βήτα = b",
                "unknown",
                "Picture",
            );
            assert_eq!(
                apply(&bytes, "book.epub", &mut actual).unwrap().1,
                Verdict::Keep
            );
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn image_parity_in_both_languages_and_image_only_books() {
    for (language, catalog, word) in [
        ("en", include_str!("../../i18n/en.ini"), "Picture"),
        ("de", include_str!("../../i18n/de.ini"), "Bild"),
    ] {
        let mut translator = Translator::new();
        translator.add(paperpaw_i18n::Catalog::parse(catalog).unwrap());
        translator.set_language(language);
        assert_eq!(translator.tr("aids.picture"), word);
        for policy in [
            crate::pictures::SHOW,
            crate::pictures::HIDE,
            crate::pictures::CAPTION,
            "future",
        ] {
            for path in ["book.epub", "book.CBZ", "book.cbr", "book.xtc", "book.xtch"] {
                let mut expected = Block::new(BlockKind::Image, 4, 90, " A ship ".to_string());
                let mut actual = expected.clone();
                let verdict = crate::oracle::apply(policy, path, &translator, &mut expected);
                let bytes = encoded(false, "", policy, word);
                assert_eq!(apply(&bytes, path, &mut actual).unwrap().1, verdict);
                if policy == crate::pictures::CAPTION && path == "book.epub" {
                    // Deliberate bug fix: the whole caption covers the original
                    // image span, not the alternative text's stale checkpoints.
                    expected.map = paperpaw_sdk::document::OffsetMap::IDENTITY;
                    assert_eq!(actual.source_offset(0), 4);
                    assert_eq!(actual.source_offset(actual.text.len()), 90);
                }
                assert_eq!(actual, expected);
            }
        }
    }
}

#[test]
fn truncations_and_unknown_tags_never_change_the_block() {
    let bytes = encoded(true, "hello=world", crate::pictures::SHOW, "Picture");
    for end in 0..bytes.len() {
        let original = Block::new(BlockKind::Paragraph, 0, 5, "hello".to_string());
        let mut block = original.clone();
        assert!(apply(&bytes[..end], "book.epub", &mut block).is_err());
        assert_eq!(block, original);
    }
    let mut unknown = bytes;
    unknown[1] = 4; // local tag 4 is not part of schema 1
    let mut block = Block::new(BlockKind::Paragraph, 0, 5, "hello".to_string());
    assert!(matches!(
        apply(&unknown, "book.epub", &mut block),
        Err(Error::Wire(_))
    ));
}

#[test]
fn refuses_caps_atomically_and_signatures_change() {
    let mut out = alloc::vec![0x55; 3840];
    for list in [
        "x=y;".repeat(33),
        alloc::format!("{}=y", "x".repeat(97)),
        alloc::format!("{}={};", "x".repeat(96), "y".repeat(96)).repeat(32),
    ] {
        assert!(encode(false, &list, crate::pictures::SHOW, "Picture", &mut out).is_err());
        assert!(out.iter().all(|b| *b == 0x55));
    }
    let a = encoded(false, "x=y", crate::pictures::SHOW, "Picture");
    let b = encoded(true, "x=y", crate::pictures::SHOW, "Picture");
    assert_ne!(a, b);
}

proptest! {
    #[test]
    fn malformed_bytes_do_not_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4000)) {
        let mut block = Block::new(BlockKind::Paragraph, 0, 5, "hello".to_string());
        let _ = apply(&bytes, "book.epub", &mut block);
    }
}

#[test]
fn semantic_refusal_is_atomic_and_short_output_is_untouched() {
    let rules = wire::FilterRules {
        signature: 1,
        bold: 1,
        pictures: 0,
        caption_word: "Picture",
    };
    let mut bytes = alloc::vec![0; 3840];
    for rows in [
        alloc::vec![
            wire::Record::FilterRules(rules),
            wire::Record::FilterRules(rules)
        ],
        alloc::vec![
            wire::Record::FilterRules(rules),
            wire::Record::FilterPair(wire::FilterPair {
                from: "",
                to: "x",
                whole_word: 1
            })
        ],
        alloc::vec![wire::Record::FilterRules(wire::FilterRules {
            bold: 2,
            ..rules
        })],
    ] {
        let len = wire::reply_encode(&rows, &mut bytes).unwrap();
        let original = Block::new(BlockKind::Paragraph, 0, 5, "hello".to_string());
        let mut actual = original.clone();
        assert_eq!(
            apply(&bytes[..len], "book.epub", &mut actual),
            Err(Error::Invalid)
        );
        assert_eq!(actual, original);
    }
    let mut short = [0x55; 2];
    assert!(encode(false, "x=y", crate::pictures::SHOW, "Picture", &mut short).is_err());
    assert_eq!(short, [0x55; 2]);
}

#[test]
fn expansion_refuses_without_changing_original() {
    let bytes = encoded(
        false,
        &"a=a a;".repeat(32),
        crate::pictures::SHOW,
        "Picture",
    );
    let original = Block::new(BlockKind::Paragraph, 0, 1, "a".to_string());
    let mut actual = original.clone();
    assert_eq!(apply(&bytes, "book.epub", &mut actual), Err(Error::Limit));
    assert_eq!(actual, original);
}

#[test]
fn punctuation_matches_use_boundaries_before_the_rule() {
    let mut actual = Block::new(BlockKind::Paragraph, 0, 3, "...".to_string());
    let mut expected = actual.clone();
    crate::oracle::replace(&mut expected, &crate::oracle::pairs(".=x"));
    let bytes = encoded(false, ".=x", crate::pictures::SHOW, "Picture");
    apply(&bytes, "book.epub", &mut actual).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn caption_maps_to_picture_start_with_empty_alt() {
    assert_caption_source_span("");
}

#[test]
fn caption_maps_to_picture_start_with_nonidentity_alt() {
    assert_caption_source_span("ship");
}

fn assert_caption_source_span(alt: &str) {
    let mut block = Block::new(BlockKind::Image, 4, 90, alt.to_string());
    if !alt.is_empty() {
        block.map.checkpoint(0, 40);
        block.map.checkpoint(2, 70);
    }
    let bytes = encoded(false, "", crate::pictures::CAPTION, "Picture");
    apply(&bytes, "book.epub", &mut block).unwrap();
    assert_eq!(block.source_offset(0), 4, "alt {alt:?}");
    assert_eq!(block.source_offset(1), 5, "alt {alt:?}");
    assert_eq!(block.source_offset(block.text.len()), 90, "alt {alt:?}");
}

#[test]
fn stale_cache_signature_refuses_before_mutation() {
    let bytes = encoded(true, "a=b", crate::pictures::SHOW, "Picture");
    let batch = wire::reply_decode(&bytes).unwrap();
    let mut records: Vec<_> = batch.clone().map(Result::unwrap).collect();
    if let wire::Record::FilterPair(pair) = &mut records[1] {
        pair.to = "changed";
    }
    let mut altered = alloc::vec![0; 3840];
    let len = wire::reply_encode(&records, &mut altered).unwrap();
    let original = Block::new(BlockKind::Paragraph, 0, 1, "a".to_string());
    let mut block = original.clone();
    assert_eq!(
        apply(&altered[..len], "book.epub", &mut block),
        Err(Error::Invalid)
    );
    assert_eq!(block, original);
}

fn ask(language: &str, image_only: u32) -> Vec<u8> {
    let mut bytes = alloc::vec![0; 256];
    let len = wire::input_encode(
        &[wire::Record::FilterAsk(wire::FilterAsk::new(
            language, "epub", image_only,
        ))],
        &mut bytes,
    )
    .unwrap();
    bytes.truncate(len);
    bytes
}

fn answered(language: &str, image_only: u32, pictures: &str) -> Vec<u8> {
    let mut bytes = alloc::vec![0; 3840];
    let len = answer(
        &ask(language, image_only),
        true,
        "a=b",
        pictures,
        None,
        &mut bytes,
    )
    .unwrap();
    bytes.truncate(len);
    bytes
}

fn caption_of(bytes: &[u8]) -> (u32, String) {
    let mut rows = wire::reply_decode(bytes).unwrap();
    let Some(Ok(wire::Record::FilterRules(rules))) = rows.next() else {
        panic!("no rules");
    };
    (rules.pictures, rules.caption_word.to_string())
}

#[test]
fn the_answer_holds_the_rules_in_the_language_asked_for() {
    let de = answered("de-CH", 0, crate::pictures::CAPTION);
    assert_eq!(caption_of(&de), (1, "Bild".to_string()));
    let en = answered("en", 0, crate::pictures::HIDE);
    assert_eq!(caption_of(&en), (2, "Picture".to_string()));
    // Another language than the two texts: the English one.
    assert_eq!(
        caption_of(&answered("fr", 0, crate::pictures::CAPTION)).1,
        "Picture"
    );
    // The answer is a reply the application takes as it is.
    let mut block = Block::new(BlockKind::Paragraph, 0, 1, "hello a".to_string());
    apply(&de, "book.epub", &mut block).unwrap();
    assert_eq!(block.text, "hello b");
}

#[test]
fn a_book_of_pictures_keeps_them() {
    let bytes = answered("en", 1, crate::pictures::HIDE);
    assert_eq!(caption_of(&bytes).0, 0);
}

#[test]
fn the_caption_words_are_the_texts_of_the_catalogs() {
    for (file, language) in [
        (include_str!("../../i18n/en.ini"), "en"),
        (include_str!("../../i18n/de.ini"), "de"),
    ] {
        let line = file
            .lines()
            .find_map(|l| l.strip_prefix("aids.picture = "))
            .unwrap();
        assert_eq!(caption_word(language), line, "{language}");
    }
}

#[test]
fn a_hostile_ask_is_refused_and_an_oversize_one_leaves_the_output_alone() {
    let mut out = alloc::vec![7u8; 3840];
    for bad in [&[][..], &[0xff; 9][..], &[1, 2][..]] {
        assert!(answer(bad, true, "", crate::pictures::SHOW, None, &mut out).is_err());
    }
    let list: String = (0..33).map(|n| alloc::format!("w{n}=x;")).collect();
    assert!(
        answer(
            &ask("en", 0),
            false,
            &list,
            crate::pictures::SHOW,
            None,
            &mut out
        )
        .is_err()
    );
    assert!(out.iter().all(|&b| b == 7));
}

#[test]
fn the_catalogs_word_is_taken_first_whatever_the_language() {
    // A language pack's text (Arabic, Spanish, ...) is the host catalog's.
    for word in ["صورة", "Imagen"] {
        let mut bytes = alloc::vec![0; 3840];
        let len = answer(
            &ask("ar", 0),
            false,
            "",
            crate::pictures::CAPTION,
            Some(word),
            &mut bytes,
        )
        .unwrap();
        assert_eq!(caption_of(&bytes[..len]).1, word);
    }
    // No text (or an empty one): the plugin's own by language.
    for none in [None, Some("")] {
        let mut bytes = alloc::vec![0; 3840];
        let len = answer(
            &ask("de", 0),
            false,
            "",
            crate::pictures::CAPTION,
            none,
            &mut bytes,
        )
        .unwrap();
        assert_eq!(caption_of(&bytes[..len]).1, "Bild");
    }
}
