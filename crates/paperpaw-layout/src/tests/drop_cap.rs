//! Drop capitals (`Block::drop_cap`): the first letter set large over the
//! first lines, which are set in beside it. Lines are twenty units wide,
//! a character one unit (four from twice the size up: a capital), lines
//! ten high, ems ten.

use super::*;

/// [`Units`], with letters four wide at twice the size and more.
struct Scaled;

impl Typesetter for Scaled {
    fn advance(&self, face: Face, c: char) -> f32 {
        match face.size >= 200 {
            true => 4.0,
            false => Units.advance(face, c),
        }
    }

    fn line(&self, face: Face) -> (f32, f32) {
        Units.line(face)
    }
}

const TEXT: &str = "Lorem ipsum dolor sit amet consectetur";

/// The page from `start` of the one block `block`, `height` high.
fn page(block: &Block, start: u32, height: f32, style: &Style) -> Page {
    let (block, end) = (block.clone(), block.end);
    let doc = Blocks::new(move |at: Locator| (at.offset < end).then(|| block.clone()));
    let storage = paperpaw_document::NoStorage;
    let cx = LayoutContext::new(&doc, &storage, 20.0, style, &Scaled);
    layout_page(&cx, Locator::new(0, start), height).unwrap()
}

fn capped(text: &str, lines: u8) -> Block {
    block(text, 0, text.len() as u32).drop_cap(lines)
}

/// Each line's text and left edge.
fn placed(page: &Page) -> Vec<(&str, f32)> {
    page.lines.iter().map(|l| (l.text.as_str(), l.x)).collect()
}

#[test]
fn the_capital_spans_its_lines_which_are_set_in_beside_it() {
    let page = page(&capped(TEXT, 3), 0, 100.0, &STYLE);
    // The capital first: four units wide and a gap of 0.15 em beside it.
    assert_eq!(
        placed(&page),
        [
            ("L", 0.0),
            ("orem ipsum", 5.5),
            ("dolor sit amet", 5.5),
            ("consectetur", 5.5)
        ]
    );
    let cap = &page.lines[0];
    // On the third line's baseline, as tall as two lines and a capital
    // letter (0.7 em): 27 units of capitals, a size of 385 %.
    assert_eq!(cap.baseline, page.lines[3].baseline);
    assert_eq!(cap.runs.len(), 1);
    assert_eq!(cap.runs[0].face.size, 385);
    // The capital is the word's start; the line beside it has the rest.
    assert_eq!(cap.words, [(0, 1)]);
    assert_eq!(page.lines[1].words[0], (1, 5));
    assert!(page.lines[1].bidi.has(LineBidi::PARAGRAPH_START));
    // Marked as the capital; the lines beside it not.
    assert!(cap.bidi.has(LineBidi::DROP_CAP));
    assert!(
        page.lines[1..]
            .iter()
            .all(|l| !l.bidi.has(LineBidi::DROP_CAP))
    );
    // Without one asked for, the lines as they always were.
    let plain = page_plain();
    assert_eq!(
        placed(&plain),
        [("Lorem ipsum dolor", 0.0), ("sit amet consectetur", 0.0)]
    );
    assert!(plain.lines.iter().all(|l| !l.bidi.has(LineBidi::DROP_CAP)));
}

/// An annotation of the word the capital starts goes with the line beside
/// it, from that line's start.
#[test]
fn an_annotation_of_the_capitals_word_goes_on_the_line_beside_it() {
    let mut block = capped(TEXT, 3);
    let gloss = paperpaw_document::Annotation::new(0, 5, "gloss", AnnotationKind::Gloss);
    block.annotations.push(gloss);
    let page = page(&block, 0, 100.0, &STYLE);
    assert!(page.lines[0].bidi.has(LineBidi::DROP_CAP));
    let notes: Vec<(f32, &str)> = page.lines[1]
        .annotations
        .iter()
        .map(|a| (a.x, a.text.as_str()))
        .collect();
    assert_eq!(notes, [(page.lines[1].x, "gloss")]);
}

fn page_plain() -> Page {
    page(&capped(TEXT, 0), 0, 100.0, &STYLE)
}

#[test]
fn pages_from_inside_the_paragraph_set_its_lines_as_from_its_start() {
    let block = capped(TEXT, 3);
    let whole = page(&block, 0, 100.0, &STYLE);
    // From the second line: still beside where the capital stands.
    let second = whole.lines[2].words[0].0;
    let from = page(&block, second, 100.0, &STYLE);
    assert_eq!(
        placed(&from),
        [("dolor sit amet", 5.5), ("consectetur", 5.5)]
    );
    // And a position inside it goes to its start.
    let storage = paperpaw_document::NoStorage;
    let doc = Blocks::new(move |at: Locator| (at.offset < 38).then(|| block.clone()));
    let cx = LayoutContext::new(&doc, &storage, 20.0, &STYLE, &Scaled);
    let inside = line_start(&cx, Locator::new(0, second + 3)).unwrap();
    assert_eq!(inside, Locator::new(0, second));
    // Balanced lines agree with it too.
    let balanced = STYLE.balanced(true);
    let whole = page(&capped(TEXT, 3), 0, 100.0, &balanced);
    assert_eq!(whole.lines[0].text, "L");
    let cx = LayoutContext::new(&doc, &storage, 20.0, &balanced, &Scaled);
    let third = whole.lines[3].words[0].0;
    assert_eq!(
        line_start(&cx, Locator::new(0, third + 1)).unwrap(),
        Locator::new(0, third)
    );
}

#[test]
fn a_short_paragraph_has_a_smaller_capital_or_none() {
    // Two lines: a capital of two.
    let two = page(&capped("Lorem ipsum dolor sit amet", 3), 0, 100.0, &STYLE);
    assert_eq!(two.lines[0].text, "L");
    assert_eq!(two.lines[0].baseline, two.lines[2].baseline);
    assert_eq!(two.lines[0].runs[0].face.size, 242);
    // One line: none.
    let one = page(&capped("Lorem ipsum", 3), 0, 100.0, &STYLE);
    assert_eq!(placed(&one), [("Lorem ipsum", 0.0)]);
}

#[test]
fn the_capitals_lines_go_to_the_next_page_together() {
    // A line, then the paragraph with a capital of three; pages of three
    // lines: it goes whole.
    let text = "aaaa\nLorem ipsum dolor sit amet consectetur\n";
    let doc = Blocks::new(move |at: Locator| {
        let (start, end) = if at.offset < 5 { (0, 5) } else { (5, 44) };
        (at.offset < 44).then(|| {
            let b = block(&text[start..end - 1], start as u32, end as u32);
            if start == 5 { b.drop_cap(3) } else { b }
        })
    });
    let storage = paperpaw_document::NoStorage;
    let cx = LayoutContext::new(&doc, &storage, 20.0, &STYLE, &Scaled);
    let first = layout_page(&cx, Locator::START, 30.0).unwrap();
    assert_eq!(texts(&first), ["aaaa"]);
    let next = layout_page(&cx, first.end, 30.0).unwrap();
    assert_eq!(texts(&next)[..2], ["L", "orem ipsum"]);
    // Starting a page too short for it, it stays as it falls.
    let short = page(&capped(TEXT, 3), 0, 20.0, &STYLE);
    assert_eq!(texts(&short), ["L", "orem ipsum", "dolor sit amet"]);
    // As large as the two lines there.
    assert_eq!(short.lines[0].runs[0].face.size, 242);
    assert_eq!(short.lines[0].baseline, short.lines[2].baseline);
}

#[test]
fn only_paragraphs_left_to_right_starting_with_a_letter_have_capitals() {
    let first_line = |block: Block| page(&block, 0, 100.0, &STYLE).lines[0].text.clone();
    // An opening quote goes with the letter.
    assert_eq!(
        first_line(capped("\u{201c}Lorem ipsum dolor sit amet", 2)),
        "\u{201c}L"
    );
    // No letter near the start, a heading, right to left: none.
    assert_eq!(
        first_line(capped("... ipsum dolor sit amet", 2)),
        "... ipsum dolor sit"
    );
    let heading = Block::new(
        BlockKind::Heading(1),
        0,
        26,
        String::from("Lorem ipsum dolor sit amet"),
    )
    .map(OffsetMap::IDENTITY)
    .drop_cap(2);
    // (Headings are two units a letter.)
    assert_eq!(first_line(heading), "Lorem");
    let hebrew = "\u{5d0}\u{5d1} \u{5d2}\u{5d3} \u{5d4}\u{5d5}";
    let rtl = capped(hebrew, 2).dir(Direction::Rtl);
    assert_eq!(first_line(rtl), hebrew);
    // A capital of more lines than allowed spans five, where the font
    // grows as asked; Scaled's letters grow four times only: three lines.
    let long = capped(
        "Lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor",
        255,
    );
    let (doc, storage) = (
        Blocks::new(move |at: Locator| (at.offset < 76).then(|| long.clone())),
        paperpaw_document::NoStorage,
    );
    let spans = |typesetter: &dyn Typesetter| {
        let cx = LayoutContext::new(&doc, &storage, 20.0, &STYLE, typesetter);
        let page = layout_page(&cx, Locator::START, 100.0).unwrap();
        let cap = page.lines[0].baseline;
        page.lines[1..]
            .iter()
            .position(|l| l.baseline == cap)
            .map(|i| i + 1)
    };
    assert_eq!(spans(&Proportional), Some(5));
    assert_eq!(spans(&Scaled), Some(3));
}

/// Letters as wide as their size in hundredths: as fonts drawn at any
/// size grow.
struct Proportional;

impl Typesetter for Proportional {
    fn advance(&self, face: Face, _: char) -> f32 {
        f32::from(face.size.max(100)) / 100.0
    }

    fn line(&self, face: Face) -> (f32, f32) {
        Units.line(face)
    }
}

#[test]
fn a_font_that_cannot_be_drawn_large_has_no_capital() {
    // Units sets every size as wide: a font of one size, as a bitmap pack
    // past its largest.
    let block = capped(TEXT, 3);
    let (doc, storage) = (
        Blocks::new(move |at: Locator| (at.offset < 38).then(|| block.clone())),
        paperpaw_document::NoStorage,
    );
    let cx = LayoutContext::new(&doc, &storage, 20.0, &STYLE, &Units);
    let page = layout_page(&cx, Locator::START, 100.0).unwrap();
    assert_eq!(placed(&page), placed(&page_plain()));
}
