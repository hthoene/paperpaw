use super::*;
use crate::stages::{reader_layer_v1 as layer, reader_text_v1 as text};
use alloc::vec;
use paperpaw_sdk::document::Locator;
use paperpaw_sdk::layout::{LayoutContext, Page, Style, layout_page};
use paperpaw_sdk::{NoStorage, Rect, reading::WordBox};

/// An answer of the text stage made of `rows`.
fn answer(rows: &[text::Record<'_>]) -> Vec<u8> {
    let mut out = vec![0; text::MAX_REPLY];
    let len = text::reply_encode(rows, &mut out).unwrap();
    out.truncate(len);
    out
}

fn block(text: &str) -> Block {
    Block::new(
        BlockKind::Paragraph,
        100,
        100 + text.len() as u32,
        text.into(),
    )
}

#[test]
fn an_answer_gives_the_block_room_words_glosses_and_blocks_after_it() {
    let reply = answer(&[
        text::Record::TextRoom(text::TextRoom::new(50, 900)),
        text::Record::TextReplace(text::TextReplace::new(0, 5, "Alicia")),
        text::Record::TextGloss(text::TextGloss::new(6, 9, 2, 60, "war")),
        text::Record::TextGloss(text::TextGloss::new(0, 5, 0, 0, "on the replaced part")),
        text::Record::TextGloss(text::TextGloss::new(3, 99, 2, 0, "off the text")),
        text::Record::TextInsert(text::TextInsert::new("Alice war hier.", 80, 1)),
    ]);
    let mut b = block("Alice was here.");
    apply(&reply, &mut b).unwrap();
    assert_eq!(b.text, "Alicia was here.");
    assert_eq!(b.leading_above, Some(50));
    // The room below: an annotation without text, at most four ems.
    let room = &b.annotations[0];
    assert_eq!((room.text.as_str(), room.size), ("", 400));
    // Moved with the replacement before it; the one on it and the one off the
    // text left out.
    assert_eq!(b.annotations.len(), 2);
    let gloss = &b.annotations[1];
    assert_eq!(
        (gloss.start, gloss.end, gloss.text.as_str()),
        (7, 10, "war")
    );
    assert_eq!((gloss.kind, gloss.size), (AnnotationKind::Gloss, 60));
    assert_eq!(b.inserted.len(), 1);
    assert_eq!(b.inserted[0].text, "Alice war hier.");
    assert_eq!(b.inserted[0].size, Some(80));
    assert_eq!(b.inserted[0].runs[0].style, TextStyle::ITALIC);
    // The book's places stay: the block starts and ends where it did.
    assert_eq!((b.start, b.end), (100, 115));
}

#[test]
fn overlapping_or_broken_replacements_are_left_out_and_nothing_is_nothing() {
    let reply = answer(&[
        text::Record::TextReplace(text::TextReplace::new(0, 3, "A")),
        text::Record::TextReplace(text::TextReplace::new(2, 4, "B")),
        text::Record::TextReplace(text::TextReplace::new(7, 8, "C")),
    ]);
    // "é" takes bytes 6..8: 7 is inside it.
    let mut b = block("abcdefé");
    apply(&reply, &mut b).unwrap();
    assert_eq!(b.text, "Adefé");
    let mut plain = block("abc");
    apply(&[], &mut plain).unwrap();
    apply(&answer(&[]), &mut plain).unwrap();
    assert_eq!(plain, block("abc"));
    assert_eq!(apply(&[1, 2, 3], &mut plain), Err(Error::Invalid));
}

#[test]
fn a_plugin_is_chosen_only_where_it_says_it_adds_to_the_book() {
    let uses = answer(&[text::Record::TextUses(text::TextUses::new(5))]);
    let chosen = Chosen::from_reply(Kind::Text, (2, "interlinear", 9), &uses).unwrap();
    assert_eq!(
        chosen,
        Some(Chosen {
            cursor: 2,
            id: "interlinear".into(),
            signature: 5_u64.rotate_left(7) ^ 9
        })
    );
    let none = answer(&[text::Record::TextUses(text::TextUses::new(0))]);
    assert_eq!(Chosen::from_reply(Kind::Text, (0, "a", 0), &none), Ok(None));
    assert_eq!(
        Chosen::from_reply(Kind::Text, (0, "a", 0), &answer(&[])),
        Ok(None)
    );
    assert_eq!(
        Chosen::from_reply(Kind::Layer, (0, "a", 0), &uses),
        Ok(None)
    );
    let input = book_input(Kind::Layer, "a.epub", "en").unwrap();
    assert!(matches!(
        layer::input_decode(&input).unwrap().next(),
        Some(Ok(layer::Record::LayerBook(_)))
    ));
    let input = block_input(3, &block("é".repeat(2000).as_str())).unwrap();
    let Some(Ok(text::Record::TextBlock(asked))) = text::input_decode(&input).unwrap().next()
    else {
        panic!("a block");
    };
    assert_eq!((asked.section, asked.offset, asked.kind), (3, 100, 0));
    assert_eq!(asked.text.len(), 3200, "cut at a character");
}

#[test]
fn answers_are_kept_by_block_and_the_oldest_go_past_their_bounds() {
    let mut answers = Answers::default();
    let big = vec![7_u8; 5000];
    for n in 0..3_u32 {
        let mut b = block("text");
        b.start = n * 10;
        b.end = n * 10 + 10;
        answers.keep(0, &b, &big).unwrap();
    }
    // 15,000 bytes do not fit 12 KiB: the first went.
    assert_eq!(answers.reply(0, 0), None);
    assert_eq!(answers.reply(0, 10).map(<[u8]>::len), Some(5000));
    assert_eq!(answers.around(0, 25), Some((30, 4)));
    assert_eq!(answers.around(1, 25), None);
    // An empty answer keeps only the place.
    let mut b = block("text");
    b.start = 40;
    b.end = 50;
    answers.keep(0, &b, &[0]).unwrap();
    assert_eq!(answers.reply(0, 40), Some(&[][..]));
    for n in 0..(KEPT_BLOCKS as u32 + 5) {
        let mut b = block("t");
        (b.start, b.end) = (1000 + n, 1001 + n);
        answers.keep(0, &b, &[]).unwrap();
    }
    assert!(answers.kept.len() <= KEPT_BLOCKS);
    answers.clear();
    assert_eq!(answers.around(0, 1001), None);
}

/// A drawing pen that notes what it is asked.
#[derive(Default)]
struct Noted(Vec<alloc::string::String>);

impl page::Pen for Noted {
    fn fill(&mut self, at: (i32, i32), size: (u32, u32), gray: u8) -> Result<(), Error> {
        self.0.push(alloc::format!("fill {at:?} {size:?} {gray}"));
        Ok(())
    }
    fn segment(
        &mut self,
        from: (i32, i32),
        to: (i32, i32),
        width: u32,
        gray: u8,
    ) -> Result<(), Error> {
        self.0
            .push(alloc::format!("segment {from:?} {to:?} {width} {gray}"));
        Ok(())
    }
    fn text(&mut self, at: (i32, i32), look: (f32, u8, bool), text: &str) -> Result<(), Error> {
        self.0.push(alloc::format!("text {at:?} {look:?} {text}"));
        Ok(())
    }
}

/// A page of one block laid out 10 px a letter.
fn laid(text: &str) -> Page {
    struct Units;
    impl paperpaw_sdk::layout::Typesetter for Units {
        fn advance(&self, _: paperpaw_sdk::layout::Face, _: char) -> f32 {
            10.0
        }
        fn line(&self, _: paperpaw_sdk::layout::Face) -> (f32, f32) {
            (16.0, 20.0)
        }
    }
    struct One(Block, paperpaw_sdk::document::Metadata);
    impl paperpaw_sdk::document::Document for One {
        fn metadata(&self) -> &paperpaw_sdk::document::Metadata {
            &self.1
        }
        fn sections(&self) -> u32 {
            1
        }
        fn section_len(&self, _: u32) -> u32 {
            self.0.end
        }
        fn block_at(
            &self,
            _: &dyn paperpaw_sdk::Storage,
            at: Locator,
        ) -> Result<Option<Block>, paperpaw_sdk::document::DocumentError> {
            Ok((at.offset < self.0.end).then(|| self.0.clone()))
        }
    }
    let book = One(
        Block::new(BlockKind::Paragraph, 0, text.len() as u32, text.into()),
        paperpaw_sdk::document::Metadata::default(),
    );
    let style = Style::new(0.0, 0.0, 20.0, false);
    let cx = LayoutContext::new(&book, &NoStorage, 100.0, &style, &Units);
    layout_page(&cx, Locator::START, 400.0).unwrap()
}

#[test]
fn a_page_is_shown_in_lines_with_its_words_and_its_answer_drawn_over_it() {
    let page = laid("one two three four");
    assert_eq!(page.lines.len(), 2);
    // The reader's boxes, the text area 30 px from the left.
    let boxes: Vec<WordBox> = [(0, "one"), (4, "two"), (8, "three"), (14, "four")]
        .iter()
        .enumerate()
        .map(|(n, &(at, word))| {
            let x = if n < 2 {
                30 + at as u16 * 10
            } else {
                30 + (at as u16 - 8) * 10
            };
            let y = if n < 2 { 0 } else { 20 };
            let area = Rect::new(x, y, word.len() as u16 * 10, 20);
            WordBox::new(
                area,
                word,
                Locator::new(0, at),
                Locator::new(0, at + word.len() as u32),
            )
        })
        .collect();
    let rows = page::rows(&page, &boxes, (30, 0)).unwrap();
    let (input, next) = page::fire(&page, &rows, 0, (100, 400)).unwrap();
    assert_eq!(next, 2);
    let mut asked = layer::input_decode(&input).unwrap();
    let Some(Ok(layer::Record::LayerPage(head))) = asked.next() else {
        panic!("the page first");
    };
    assert_eq!(
        (head.width, head.height, head.lines, head.first),
        (100, 400, 2, 0)
    );
    let Some(Ok(layer::Record::LayerLine(first))) = asked.next() else {
        panic!("a line");
    };
    assert_eq!(
        (first.text, first.baseline, first.offset),
        ("one two", 16, 0)
    );
    // "two": 40 px in, 30 wide, at byte 4 of the line.
    assert_eq!(&first.words[6..], &[40, 0, 30, 0, 4, 0]);
    assert_eq!(first.width, 70);
    // What the plugin answers is drawn at the text area's place.
    let mut out = vec![0; layer::MAX_REPLY];
    let rows = [
        layer::Record::LayerBox(layer::LayerBox::new(40, 18, 30, 4, 128, 256, 0)),
        layer::Record::LayerBox(layer::LayerBox::new(0, 0, 10, 10, 256, 0, 2)),
        layer::Record::LayerSegment(layer::LayerSegment::new(0, 30, 70, 30, 0, 0)),
        layer::Record::LayerText(layer::LayerText::new(40, 34, 8, 0, 1, "zwei")),
        layer::Record::LayerText(layer::LayerText::new(0, 0, 8, 300, 0, "no ink")),
    ];
    let len = layer::reply_encode(&rows, &mut out).unwrap();
    out.truncate(len);
    let mut pen = Noted::default();
    page::draw(&mut pen, &[out], (30, 50)).unwrap();
    assert_eq!(
        pen.0,
        [
            "fill (70, 68) (30, 4) 128",
            "fill (30, 50) (10, 2) 0",
            "fill (30, 58) (10, 2) 0",
            "fill (30, 50) (2, 10) 0",
            "fill (38, 50) (2, 10) 0",
            "segment (30, 80) (100, 80) 1 0",
            "text (70, 84) (8.0, 0, true) zwei",
        ]
    );
}

#[test]
fn the_answers_of_two_pages_are_kept() {
    let mut pages = page::Pages::default();
    for n in 0..3 {
        pages.keep((0, n), vec![vec![n as u8]]).unwrap();
    }
    assert!(pages.get((0, 0)).is_none());
    assert_eq!(pages.get((0, 2)), Some(&[vec![2_u8]][..]));
    pages.keep((0, 1), Vec::new()).unwrap();
    assert_eq!(pages.get((0, 1)), Some(&[][..]));
    pages.clear();
    assert!(pages.get((0, 2)).is_none());
}
