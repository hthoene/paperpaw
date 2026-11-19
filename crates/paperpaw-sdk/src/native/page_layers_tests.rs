use super::*;

fn text_ask(record: text::Record<'_>) -> alloc::vec::Vec<u8> {
    let mut out = alloc::vec![0; text::MAX_INPUT];
    let len = text::input_encode(&[record], &mut out).unwrap();
    out.truncate(len);
    out
}

#[test]
fn a_block_is_read_and_answered_with_room_replacements_glosses_and_blocks() {
    let input = text_ask(text::Record::TextBlock(text::TextBlock::new(
        2,
        40,
        0,
        "Alice was here.",
    )));
    let TextAsk::Block(block) = TextAsk::decode(&input).unwrap() else {
        panic!("a block");
    };
    assert_eq!(
        (block.section, block.offset, block.kind),
        (2, 40, BlockKind::Paragraph)
    );
    assert_eq!(block.text, "Alice was here.");
    let mut out = [0; MAX_ANSWER];
    let mut answer = TextAnswer::new(&mut out).unwrap();
    answer.room(50, 25).unwrap();
    answer.replace(0..5, "Alicia").unwrap();
    answer.gloss(6..9, GlossKind::Gloss, 0, "war").unwrap();
    answer.insert("Alice war hier.", 80, true).unwrap();
    assert_eq!(
        answer.replace(0..1, &"x".repeat(MAX_REPLACEMENT + 1)),
        Err(Error::Invalid)
    );
    let len = answer.finish().unwrap();
    let rows: alloc::vec::Vec<_> = text::reply_decode(&out[..len])
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(rows.len(), 4, "the refused one left out");
    assert_eq!(rows[0], text::Record::TextRoom(text::TextRoom::new(50, 25)));
    assert_eq!(
        rows[2],
        text::Record::TextGloss(text::TextGloss::new(6, 9, 2, 0, "war"))
    );
}

#[test]
fn a_book_is_asked_and_answered_with_a_signature() {
    let input = text_ask(text::Record::TextBook(text::TextBook::new("a.epub", "en")));
    assert_eq!(
        TextAsk::decode(&input),
        Ok(TextAsk::Book {
            path: "a.epub",
            language: "en"
        })
    );
    assert_eq!(TextAsk::decode(&[]), Err(Error::Invalid));
    let mut out = [0; 32];
    let mut answer = TextAnswer::new(&mut out).unwrap();
    answer.uses(7).unwrap();
    let len = answer.finish().unwrap();
    let mut rows = text::reply_decode(&out[..len]).unwrap();
    assert_eq!(
        rows.next(),
        Some(Ok(text::Record::TextUses(text::TextUses::new(7))))
    );
}

#[test]
fn a_page_shows_its_lines_and_words_and_is_answered_with_drawing() {
    let words = [3, 0, 20, 0, 0, 0, 30, 0, 12, 0, 6, 0];
    let rows = [
        layer::Record::LayerPage(layer::LayerPage::new(400, 600, 30, 0)),
        layer::Record::LayerLine(layer::LayerLine::new(
            3,
            18,
            39,
            12,
            0,
            8,
            1,
            100,
            0,
            "Alice was",
            &words,
        )),
    ];
    let mut input = alloc::vec![0; layer::MAX_INPUT];
    let len = layer::input_encode(&rows, &mut input).unwrap();
    let LayerAsk::Page(page) = LayerAsk::decode(&input[..len]).unwrap() else {
        panic!("a page");
    };
    assert_eq!(
        (page.width, page.height, page.lines, page.first),
        (400, 600, 30, 0)
    );
    let lines: alloc::vec::Vec<_> = page.lines().collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(
        (lines[0].baseline, lines[0].top, lines[0].text),
        (18, 8, "Alice was")
    );
    let words: alloc::vec::Vec<_> = lines[0].words().collect();
    assert_eq!(
        words,
        [
            LayerWord {
                x: 3,
                width: 20,
                at: 0
            },
            LayerWord {
                x: 30,
                width: 12,
                at: 6
            }
        ]
    );
    let mut out = [0; MAX_ANSWER];
    let mut answer = LayerAnswer::new(&mut out).unwrap();
    answer
        .rect((3, 20), (39, 2), (Tone::gray(128), Tone::NONE), 0)
        .unwrap();
    answer.segment((3, 22), (42, 22), Tone::BLACK, 1).unwrap();
    answer
        .text((3, 30), (8, Tone::BLACK, true), "Alice war")
        .unwrap();
    let len = answer.finish().unwrap();
    assert_eq!(layer::reply_decode(&out[..len]).unwrap().len(), 3);
}

#[test]
fn a_page_read_aloud_says_its_words_to_a_plugin_that_follows_it() {
    let rows = [
        layer::Record::LayerPage(layer::LayerPage::new(400, 600, 30, 4)),
        layer::Record::LayerAloud(layer::LayerAloud::new(3, 2, 5, 1)),
    ];
    let mut input = alloc::vec![0; layer::MAX_INPUT];
    let len = layer::input_encode(&rows, &mut input).unwrap();
    let LayerAsk::Page(page) = LayerAsk::decode(&input[..len]).unwrap() else {
        panic!("a page");
    };
    let spoken = page.spoken.unwrap();
    assert!(spoken.holds(3, 2) && spoken.holds(4, 40) && spoken.holds(5, 0));
    assert!(!spoken.holds(3, 1) && !spoken.holds(5, 1));
    assert_eq!(page.lines().count(), 0);
    let mut out = [0; 64];
    let mut answer = LayerAnswer::new(&mut out).unwrap();
    answer.uses_reading_aloud(7).unwrap();
    answer.region((0, 0), (1000, 500)).unwrap();
    let len = answer.finish().unwrap();
    let mut rows = layer::reply_decode(&out[..len]).unwrap();
    assert_eq!(
        rows.next(),
        Some(Ok(layer::Record::LayerUses(layer::LayerUses::new(7, 1))))
    );
    assert_eq!(
        rows.next(),
        Some(Ok(layer::Record::LayerRegion(layer::LayerRegion::new(
            0, 0, 1000, 500
        ))))
    );
}
