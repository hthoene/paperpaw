#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use alloc::string::ToString;
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
                crate::bold_beginnings(&mut expected);
            }
            crate::replace(
                &mut expected,
                &crate::pairs("alpha = one; one = longer; βήτα = b"),
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
                let verdict = crate::pictures::apply(policy, path, &translator, &mut expected);
                let bytes = encoded(false, "", policy, word);
                assert_eq!(apply(&bytes, path, &mut actual).unwrap().1, verdict);
                if policy == crate::pictures::CAPTION && path == "book.epub" {
                    // Deliberate bug fix: the whole caption covers the original
                    // image span, not the alternative text's stale checkpoints.
                    expected.map = paperpaw_sdk::document::OffsetMap::IDENTITY;
                    expected.map.checkpoint(0, expected.start);
                    expected.map.checkpoint(expected.text.len(), expected.end);
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
    unknown[1] = 3; // local tag 3 is not part of schema 1
    let mut block = Block::new(BlockKind::Paragraph, 0, 5, "hello".to_string());
    assert!(matches!(
        apply(&unknown, "book.epub", &mut block),
        Err(Error::Wire(-1))
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
        let len = wire::records_encode(&rows, &mut bytes).unwrap();
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
    crate::replace(&mut expected, &crate::pairs(".=x"));
    let bytes = encoded(false, ".=x", crate::pictures::SHOW, "Picture");
    apply(&bytes, "book.epub", &mut actual).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn stale_cache_signature_refuses_before_mutation() {
    let bytes = encoded(true, "a=b", crate::pictures::SHOW, "Picture");
    let batch = wire::records_decode(&bytes).unwrap();
    let mut records: Vec<_> = batch.iter().collect();
    if let wire::Record::FilterPair(pair) = &mut records[1] {
        pair.to = "changed";
    }
    let mut altered = alloc::vec![0; 3840];
    let len = wire::records_encode(&records, &mut altered).unwrap();
    let original = Block::new(BlockKind::Paragraph, 0, 1, "a".to_string());
    let mut block = original.clone();
    assert_eq!(
        apply(&altered[..len], "book.epub", &mut block),
        Err(Error::Invalid)
    );
    assert_eq!(block, original);
}
