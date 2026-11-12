#![allow(clippy::unwrap_used, reason = "test assertions")]
use super::*;
use crate::fail_alloc::{exempt, sweep};
use alloc::{format, string::ToString, vec};
use paperpaw_plugin_aids::{pictures, reader_filter as aids};
use paperpaw_sdk::filters::Verdict;
use proptest::prelude::*;

/// What aids answers for these settings (its own encoder: the bytes the stage
/// carries).
fn reply(bold: bool, list: &str, policy: &str, word: &str) -> Vec<u8> {
    let mut bytes = vec![0; wire::MAX_REPLY];
    let len = aids::encode(bold, list, policy, word, &mut bytes).unwrap();
    bytes.truncate(len);
    bytes
}

/// aids' ask of the stage for a book in `language`.
fn ask(language: &str, image_only: u32) -> Vec<u8> {
    let mut bytes = vec![0; 256];
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

fn shown(verdict: Verdict) -> Shown {
    match verdict {
        Verdict::Keep => Shown::Keep,
        Verdict::Drop => Shown::Drop,
        // `Verdict` is `#[non_exhaustive]`; aids answers only these two.
        _ => Shown::Keep,
    }
}

/// The reader's application and aids' (which aids' tests pin to the
/// interface-6 filters) make the same block of every block, rules and book.
#[test]
fn the_rules_change_a_block_as_aids_did() {
    let lists = [
        "",
        "alpha = one; one = longer; βήτα = b",
        "Mr. = Mister; e.g. = for example",
    ];
    let policies = [pictures::SHOW, pictures::CAPTION, pictures::HIDE, "future"];
    let paths = ["book.epub", "book.CBZ", "book.cbr", "book.xtc", "book.xtch"];
    let blocks = [
        Block::new(
            BlockKind::Paragraph,
            10,
            100,
            "alpha alpha2 βήτα".to_string(),
        ),
        Block::new(
            BlockKind::Paragraph,
            0,
            40,
            "über café a, e.g. Mr. Smith".to_string(),
        ),
        Block::new(BlockKind::Paragraph, 0, 0, String::new()),
        Block::new(BlockKind::Heading(1), 0, 9, "one heading".to_string()),
        Block::new(BlockKind::Image, 4, 90, " A ship ".to_string()),
        Block::new(BlockKind::Image, 4, 90, String::new()),
    ];
    for bold in [false, true] {
        for list in lists {
            for policy in policies {
                for word in ["Picture", "Bild"] {
                    let bytes = reply(bold, list, policy, word);
                    let filter = Filter::from_reply(&bytes).unwrap();
                    for path in paths {
                        for original in &blocks {
                            let mut styled = original.clone();
                            if let Some(ch) = styled.text.chars().next() {
                                let end = ch.len_utf8() as u32;
                                styled.runs.push(Run::new(0, end, TextStyle::ITALIC));
                            }
                            for block in [original.clone(), styled] {
                                let (mut ours, mut theirs) = (block.clone(), block.clone());
                                let verdict = aids::apply(&bytes, path, &mut theirs).unwrap().1;
                                assert_eq!(
                                    filter.apply(path, &mut ours).unwrap(),
                                    shown(verdict),
                                    "{bold} {list} {policy} {path} {block:?}"
                                );
                                assert_eq!(ours, theirs, "{bold} {list} {policy} {path}");
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn a_caption_takes_the_pictures_place_in_the_language_asked_for() {
    for (language, word) in [("en", "Picture"), ("de", "Bild")] {
        let mut bytes = vec![0; wire::MAX_REPLY];
        let len = aids::answer(
            &ask(language, 0),
            false,
            "",
            pictures::CAPTION,
            None,
            &mut bytes,
        )
        .unwrap();
        let filter = Filter::from_reply(&bytes[..len]).unwrap();
        let mut block = Block::new(BlockKind::Image, 4, 90, " A ship ".to_string());
        assert_eq!(filter.apply("book.epub", &mut block).unwrap(), Shown::Keep);
        assert_eq!(block.text, format!("[{word}: A ship]"));
        assert_eq!(block.kind, BlockKind::Paragraph);
        assert_eq!(block.align, Align::Center);
        assert_eq!(
            block.runs,
            [Run::new(0, block.text.len() as u32, TextStyle::ITALIC)]
        );
        assert!(block.image.is_none());
        assert_eq!(block.source_offset(0), 4);
        assert_eq!(block.source_offset(block.text.len()), 90);
    }
}

#[test]
fn a_book_of_pictures_keeps_them_whatever_the_rule() {
    let mut bytes = vec![0; wire::MAX_REPLY];
    let len = aids::answer(&ask("en", 1), false, "", pictures::HIDE, None, &mut bytes).unwrap();
    let filter = Filter::from_reply(&bytes[..len]).unwrap();
    let mut block = Block::new(BlockKind::Image, 0, 9, "page".to_string());
    let before = block.clone();
    assert_eq!(filter.apply("book.epub", &mut block).unwrap(), Shown::Keep);
    assert_eq!(block, before);
}

#[test]
fn the_reply_is_kept_exactly_and_nothing_more() {
    let bytes = reply(true, "a=b; c=d", pictures::CAPTION, "Picture");
    let filter = Filter::from_reply(&bytes).unwrap();
    assert_eq!(filter.reply, bytes);
    assert_eq!(filter.reply.capacity(), bytes.len());
}

#[test]
fn a_cut_reply_an_unknown_record_or_a_stale_signature_is_refused() {
    let bytes = reply(true, "hello=world", pictures::SHOW, "Picture");
    for end in 0..bytes.len() {
        assert_eq!(
            Filter::from_reply(&bytes[..end]),
            Err(Error::Invalid),
            "{end}"
        );
    }
    let mut unknown = bytes.clone();
    unknown[1] = 4; // local tag 4 is not part of schema 1
    assert_eq!(Filter::from_reply(&unknown), Err(Error::Invalid));
    // Other rules under the first reply's signature: settings changed since.
    let other = reply(false, "hello=world", pictures::SHOW, "Picture");
    let mut rows = wire::reply_decode(&other).unwrap();
    let Some(Ok(wire::Record::FilterRules(mut rules))) = rows.next() else {
        panic!("no rules");
    };
    rules.signature = Filter::from_reply(&bytes).unwrap().signature();
    let mut records = vec![wire::Record::FilterRules(rules)];
    records.extend(rows.map(Result::unwrap));
    let mut stale = vec![0; wire::MAX_REPLY];
    let len = wire::reply_encode(&records, &mut stale).unwrap();
    assert_eq!(Filter::from_reply(&stale[..len]), Err(Error::Invalid));
}

#[test]
fn the_signature_follows_the_rules() {
    let a = Filter::from_reply(&reply(false, "x=y", pictures::SHOW, "Picture")).unwrap();
    let b = Filter::from_reply(&reply(true, "x=y", pictures::SHOW, "Picture")).unwrap();
    let c = Filter::from_reply(&reply(false, "x=z", pictures::SHOW, "Picture")).unwrap();
    assert_ne!(a.signature(), b.signature());
    assert_ne!(a.signature(), c.signature());
    let both = signature(&[a.clone(), b.clone()]);
    assert_ne!(both, signature(&[b.clone(), a.clone()]));
    assert_ne!(both, signature(core::slice::from_ref(&a)));
    assert_eq!(signature(&[]), 0);
}

#[test]
fn filters_apply_in_turn_and_a_dropped_block_goes_to_no_other() {
    let hide = Filter::from_reply(&reply(false, "", pictures::HIDE, "Picture")).unwrap();
    let caption = Filter::from_reply(&reply(false, "", pictures::CAPTION, "Picture")).unwrap();
    let mut picture = Block::new(BlockKind::Image, 0, 9, "ship".to_string());
    let before = picture.clone();
    assert_eq!(
        apply(&[hide, caption.clone()], "b.epub", &mut picture),
        Ok(Shown::Drop)
    );
    assert_eq!(picture, before);
    let swap = Filter::from_reply(&reply(false, "one=two", pictures::SHOW, "Picture")).unwrap();
    let mut text = Block::new(BlockKind::Paragraph, 0, 9, "one one".to_string());
    assert_eq!(
        apply(&[swap, caption], "b.epub", &mut text),
        Ok(Shown::Keep)
    );
    assert_eq!(text.text, "two two");
}

#[test]
fn a_block_over_the_budget_is_left_as_it_is() {
    let filter = Filter::from_reply(&reply(false, "a=aaaaaaaa", pictures::SHOW, "P")).unwrap();
    let text = "a ".repeat(BUDGET / 4);
    let mut block = Block::new(BlockKind::Paragraph, 0, 1, text);
    let before = block.clone();
    assert_eq!(filter.apply("b.epub", &mut block), Err(Error::NoSpace));
    assert_eq!(block, before);
}

#[test]
fn a_refused_allocation_is_no_memory_and_changes_nothing() {
    let bytes = reply(true, "alpha=one; one=longer", pictures::CAPTION, "Picture");
    sweep(|| Filter::from_reply(&bytes).map(|f| f.signature()));
    let filter = Filter::from_reply(&bytes).unwrap();
    for original in [
        Block::new(
            BlockKind::Paragraph,
            0,
            30,
            "alpha one beta gamma".to_string(),
        ),
        Block::new(BlockKind::Image, 0, 9, "ship".to_string()),
    ] {
        sweep(|| {
            let mut block = exempt(|| original.clone());
            let applied = filter.apply("book.epub", &mut block);
            if applied.is_err() {
                assert_eq!(block, original);
            }
            applied
        });
    }
}

proptest! {
    #[test]
    fn any_bytes_are_a_reply_or_refused_without_a_panic(
        bytes in proptest::collection::vec(any::<u8>(), 0..4000)
    ) {
        // A reply taken is kept exactly; anything else is refused.
        if let Ok(filter) = Filter::from_reply(&bytes) {
            prop_assert_eq!(filter.reply, bytes);
        }
    }
}
