//! The layout's tests.

use super::*;
use alloc::vec;
use paperpaw_document::{MAX_INSERTED, MAX_INSERTED_TEXT, Metadata, NoStorage, OffsetMap, Run};

/// Paragraphs separated by single newlines; offsets are bytes. Lines
/// starting with `#` are headings; `*word*` is bold.
struct Text {
    text: &'static str,
    metadata: Metadata,
}

impl Text {
    fn new(text: &'static str) -> Self {
        Self {
            text,
            metadata: Metadata::default(),
        }
    }
}

impl Document for Text {
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn sections(&self) -> u32 {
        1
    }

    fn section_len(&self, _: u32) -> u32 {
        self.text.len() as u32
    }

    fn block_at(&self, _: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
        let offset = at.offset as usize;
        if offset >= self.text.len() {
            return Ok(None);
        }
        let start = self.text[..offset].rfind('\n').map_or(0, |i| i + 1);
        let end = self.text[offset..]
            .find('\n')
            .map_or(self.text.len(), |i| offset + i + 1);
        let line = self.text[start..end].trim_end_matches('\n');
        let runs = line
            .match_indices('*')
            .collect::<Vec<_>>()
            .chunks(2)
            .filter(|pair| pair.len() == 2)
            .map(|pair| Run {
                start: pair[0].0 as u32,
                end: pair[1].0 as u32 + 1,
                style: TextStyle::BOLD,
            })
            .collect();
        let kind = if line.starts_with('#') {
            BlockKind::Heading(1)
        } else {
            BlockKind::Paragraph
        };
        let align = if line.starts_with('>') {
            Align::Center
        } else if line.starts_with('=') {
            Align::Justify
        } else {
            Align::Auto
        };
        let spaced = line.starts_with('~');
        Ok(Some(
            Block::new(kind, start as u32, end as u32, String::from(line))
                .runs(runs)
                .align(align)
                .indent(spaced.then_some(0))
                .space(spaced.then_some(200))
                .size(spaced.then_some(150)),
        ))
    }
}

/// One block that begins at offset 8 of its section, as the first of a
/// converted chapter does: the text, or the picture.
struct Late {
    image: Option<ImageRef>,
    metadata: Metadata,
}

impl Document for Late {
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn sections(&self) -> u32 {
        1
    }

    fn section_len(&self, _: u32) -> u32 {
        18
    }

    fn block_at(&self, _: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
        let kind = match self.image {
            Some(_) => BlockKind::Image,
            None => BlockKind::Paragraph,
        };
        let block = Block::new(kind, 8, 18, String::from("aaaa bbbb"));
        Ok((at.offset < 18).then(|| block.image(self.image.clone())))
    }
}

/// Body text: 1 unit per character (bold 2), lines of 10 with ascent 8;
/// headings: 2 units, lines of 20.
struct Units;

impl Typesetter for Units {
    fn advance(&self, face: Face, _: char) -> f32 {
        if face.heading > 0 || face.style.bold() {
            2.0
        } else {
            1.0
        }
    }

    fn line(&self, face: Face) -> (f32, f32) {
        if face.heading > 0 {
            (16.0, 20.0)
        } else {
            (8.0, 10.0)
        }
    }
}

const STYLE: Style = Style {
    paragraph_spacing: 0.0,
    indent: 0.0,
    em: 10.0,
    justify: false,
    widows: 1,
    orphans: 1,
    book: BookStyles::ALL,
    balanced: false,
};

#[test]
fn widows_and_orphans_move_lines_on() {
    // A line, then a paragraph of three; pages of three lines.
    let doc = Text::new("aaaa bbbb\ncccc dddd eeee ffff gggg hhhh\n");
    let page = |style: &Style| {
        let first = layout(&doc, 0, 30.0, style);
        let next = layout(&doc, first.end.offset, 30.0, style);
        (texts(&first).len(), texts(&next).len())
    };
    // As they fall: the paragraph's last line alone on the next page.
    assert_eq!(page(&STYLE), (3, 1));
    // Two at the top: a line more goes on.
    assert_eq!(page(&STYLE.lines_kept(2, 1)), (2, 2));
    // And two at the bottom: the paragraph goes whole.
    assert_eq!(page(&STYLE.lines_kept(2, 2)), (1, 3));
    // A paragraph that starts the page stays, whatever it leaves.
    let long = Text::new("aaaa bbbb cccc dddd eeee ffff gggg hhhh\n");
    let first = layout(&long, 0, 30.0, &STYLE.lines_kept(3, 3));
    assert_eq!(texts(&first).len(), 3);
}

#[test]
fn a_heading_goes_on_with_the_paragraph_after_it() {
    // A line, a heading, a paragraph of two; pages 50 high.
    let doc = Text::new(
        "aaaa bbbb
#Hd
cccc dddd eeee ffff
",
    );
    let page = |style: &Style| {
        let first = layout(&doc, 0, 50.0, style);
        let next = layout(&doc, first.end.offset, 50.0, style);
        (texts(&first).len(), texts(&next).len())
    };
    // As they fall: the paragraph's last line on the next page.
    assert_eq!(page(&STYLE), (3, 1));
    // The paragraph goes whole, and its heading with it.
    assert_eq!(page(&STYLE.lines_kept(2, 1)), (1, 3));
    assert_eq!(page(&STYLE.lines_kept(2, 2)), (1, 3));
    // Under a heading that starts the page, it stays as it falls.
    let top = Text::new(
        "#Hd
cccc dddd eeee ffff
",
    );
    let first = layout(&top, 0, 35.0, &STYLE.lines_kept(2, 2));
    assert_eq!(texts(&first).len(), 2);
}

fn texts(page: &Page) -> Vec<&str> {
    page.lines.iter().map(|l| l.text.as_str()).collect()
}

fn layout(doc: &Text, start: u32, height: f32, style: &Style) -> Page {
    let storage = paperpaw_document::NoStorage;
    let cx = LayoutContext {
        document: doc,
        storage: &storage,
        width: 10.0,
        style,
        typesetter: &Units,
        hyphenate: None,
    };
    layout_page(&cx, Locator::new(0, start), height).unwrap()
}

#[test]
fn fills_pages_and_continues_mid_paragraph() {
    let doc = Text::new("aaaa bbbb cccc dddd\nee ff\n");
    let first = layout(&doc, 0, 30.0, &STYLE);
    assert_eq!(texts(&first), vec!["aaaa bbbb", "cccc dddd", "ee ff"]);
    assert_eq!(first.end, Locator::new(0, 26));

    let small = layout(&doc, 0, 10.0, &STYLE);
    assert_eq!(texts(&small), vec!["aaaa bbbb"]);
    assert_eq!(small.end.offset, 10);
    let next = layout(&doc, small.end.offset, 10.0, &STYLE);
    assert_eq!(texts(&next), vec!["cccc dddd"]);
    assert_eq!(next.lines[0].baseline, 8.0);
}

#[test]
fn positions_inside_a_line_go_to_its_start() {
    let doc = Text::new(
        "aaaa bbbb cccc dddd
ee ff
",
    );
    let storage = paperpaw_document::NoStorage;
    let start = |offset, width| {
        let cx = LayoutContext {
            document: &doc,
            storage: &storage,
            width,
            style: &STYLE,
            typesetter: &Units,
            hyphenate: None,
        };
        line_start(&cx, Locator::new(0, offset)).unwrap().offset
    };
    // Lines of 10 units: "aaaa bbbb", "cccc dddd".
    assert_eq!(start(12, 10.0), 10, "inside \"cccc\"");
    assert_eq!(start(10, 10.0), 10, "a line's start stays");
    assert_eq!(start(3, 10.0), 0);
    // Wider lines (e.g. turned): "aaaa bbbb cccc", "dddd".
    assert_eq!(start(12, 15.0), 0);
    assert_eq!(start(16, 15.0), 15);
    assert_eq!(start(20, 15.0), 20, "another paragraph's start");
    assert_eq!(start(22, 15.0), 20);
}

#[test]
fn indents_and_justifies() {
    let doc = Text::new("aa bb cc dd ee\n");
    let style = Style {
        indent: 3.0,
        em: 10.0,
        justify: true,
        ..STYLE
    };
    let page = layout(&doc, 0, 100.0, &style);
    assert_eq!(texts(&page), vec!["aa bb", "cc dd ee"]);
    assert_eq!(page.lines[0].x, 3.0);
    // 7 px available, 5 used, one space: +2 per space.
    assert_eq!(page.lines[0].word_spacing, 2.0);
    assert_eq!(
        page.lines[1].word_spacing, 0.0,
        "last line is not justified"
    );
}

#[test]
fn justification_is_continuous_at_the_gap_limit() {
    struct Spaced(f32);
    impl Typesetter for Spaced {
        fn advance(&self, _: Face, c: char) -> f32 {
            if c == ' ' { self.0 } else { 1.0 }
        }
        fn line(&self, _: Face) -> (f32, f32) {
            (8.0, 10.0)
        }
    }
    let doc = Text::new("aa bb abcdefghi");
    for balanced in [false, true] {
        let style = Style::new(0.0, 0.0, 10.0, true).balanced(balanced);
        let mut previous = None;
        for space in [0.5, 0.9, 1.0, 1.1, 1.5] {
            let face = Spaced(space);
            let cx = LayoutContext::new(&doc, &NoStorage, 11.0, &style, &face);
            let page = layout_page(&cx, Locator::START, 100.0).unwrap();
            assert_eq!(texts(&page), ["aa bb", "abcdefghi"]);
            let gap = space + page.lines[0].word_spacing;
            if let Some((before_space, before_gap)) = previous {
                assert!(gap >= before_gap);
                assert!(gap - before_gap <= space - before_space + 0.001);
            }
            assert!(page.lines[0].word_spacing <= 6.0);
            assert_eq!(page.lines[1].word_spacing, 0.0);
            previous = Some((space, gap));
        }
    }
}

#[test]
fn justified_breaks_share_the_spare_width_between_more_words() {
    let doc = Text::new("aaa bbbbb c dddddddd eeee ffffff ggggggggg hhh iiiiii");
    let style = Style::new(0.0, 0.0, 10.0, true).balanced(true);
    let cx = LayoutContext::new(&doc, &NoStorage, 20.0, &style, &Units);
    let page = layout_page(&cx, Locator::START, 100.0).unwrap();
    assert_eq!(
        texts(&page),
        [
            "aaa bbbbb c",
            "dddddddd eeee ffffff",
            "ggggggggg hhh iiiiii"
        ]
    );
    assert_eq!(page.lines[0].word_spacing, 4.5);
    // Page continuations and search must use the same paragraph breaks.
    for line in &page.lines {
        let start = Locator::new(0, line.words[0].0);
        let inside = Locator::new(0, start.offset + 1);
        assert_eq!(line_start(&cx, inside).unwrap(), start);
        let continued = layout_page(&cx, start, 10.0).unwrap();
        assert_eq!(continued.lines[0].text, line.text);
    }
    let mut ragged = style;
    ragged.justify = false;
    let cx = LayoutContext::new(&doc, &NoStorage, 20.0, &ragged, &Units);
    let page = layout_page(&cx, Locator::START, 100.0).unwrap();
    assert_eq!(page.lines[0].text, "aaa bbbbb c dddddddd");
    assert!(page.lines.iter().all(|line| line.word_spacing == 0.0));
}

#[test]
fn the_books_indents_and_spaces_win() {
    let doc = Text::new(
        "aa
~bb
cc
",
    );
    let style = Style {
        indent: 3.0,
        em: 10.0,
        ..STYLE
    };
    let page = layout(&doc, 0, 100.0, &style);
    assert_eq!(texts(&page), vec!["aa", "~bb", "cc"]);
    assert_eq!(page.lines[0].x, 3.0);
    assert_eq!(page.lines[1].x, 0.0, "the book's indent");
    assert_eq!(page.lines[2].x, 3.0);
    // 2 em above "~bb", which is set at 150 % (15 px ems); the reader's
    // spacing elsewhere.
    assert_eq!(page.lines[1].baseline - page.lines[0].baseline, 40.0);
    assert_eq!(page.lines[1].runs[0].face.size, 150);
    assert_eq!(page.lines[0].runs[0].face.size, 0);
    assert_eq!(page.lines[2].baseline - page.lines[1].baseline, 10.0);
    // At the top of a chapter's first page the space stays.
    let doc = Text::new(
        "~aa
",
    );
    let page = layout(&doc, 0, 100.0, &style);
    assert_eq!(page.lines[0].baseline, 38.0);
}

#[test]
fn the_readers_choice_wins_over_justified_books() {
    // "=" marks a block the book justifies.
    let doc = Text::new(
        "=a bb cc dd ee
",
    );
    let page = layout(&doc, 0, 100.0, &STYLE);
    assert_eq!(texts(&page), vec!["=a bb cc", "dd ee"]);
    assert_eq!(page.lines[0].word_spacing, 0.0);
    let style = Style {
        justify: true,
        ..STYLE
    };
    let page = layout(&doc, 0, 100.0, &style);
    assert_eq!(page.lines[0].word_spacing, 1.0);
}

#[test]
fn words_know_where_they_come_from() {
    // Second line of the second block: "cc dd" at 3..8 of "aa bb cc dd".
    let doc = Text::new(
        "x
aa bb cc dd
",
    );
    let page = layout(&doc, 0, 100.0, &STYLE);
    assert_eq!(texts(&page), vec!["x", "aa bb cc", "dd"]);
    assert_eq!(page.lines[1].words, [(2, 4), (5, 7), (8, 10)]);
    assert_eq!(page.lines[2].words, [(11, 13)]);
    assert_eq!(page.lines[2].section, 0);
}

#[test]
fn past_the_end_is_empty() {
    let doc = Text::new("x\n");
    let page = layout(&doc, 2, 100.0, &STYLE);
    assert!(page.is_past_end());
    assert_eq!(page.end, page.start);
}

#[test]
fn paragraph_spacing_between_blocks() {
    let doc = Text::new("a\nb\n");
    let style = Style {
        paragraph_spacing: 5.0,
        ..STYLE
    };
    let page = layout(&doc, 0, 100.0, &style);
    assert_eq!(page.lines[1].baseline - page.lines[0].baseline, 15.0);
}

#[test]
fn styled_runs_are_measured_and_reported() {
    // "*bb*" is bold: 4 characters of 2 units, so "a *bb*" just fits.
    let doc = Text::new("a *bb* c\n");
    let page = layout(&doc, 0, 100.0, &STYLE);
    assert_eq!(texts(&page), vec!["a *bb*", "c"]);
    let faces: Vec<(usize, usize, bool)> = page.lines[0]
        .runs
        .iter()
        .map(|r| (r.start, r.end, r.face.style.bold()))
        .collect();
    assert_eq!(faces, vec![(0, 2, false), (2, 6, true)]);
}

#[test]
fn headings_use_their_face_and_centred_blocks_are_centred() {
    let doc = Text::new("#ab\n>xy\n");
    let style = Style {
        indent: 3.0,
        em: 10.0,
        justify: true,
        ..STYLE
    };
    let page = layout(&doc, 0, 100.0, &style);
    // Heading: 3 characters of 2 units, line box of 20 with ascent 16.
    assert_eq!(page.lines[0].baseline, 16.0);
    assert_eq!(page.lines[0].x, 0.0, "headings are not indented");
    assert_eq!(page.lines[0].runs[0].face.heading, 1);
    // ">xy" is 3 units in a 10-unit line, centred and not indented.
    assert_eq!(page.lines[1].x, 3.5);
    assert_eq!(page.lines[1].baseline, 20.0 + 8.0);
}

/// A document whose blocks come from a function, for documents that
/// break the rules. Fails the test when laid out without end.
struct Blocks<F> {
    block_at: F,
    calls: core::cell::Cell<u32>,
    metadata: Metadata,
}

impl<F: Fn(Locator) -> Option<Block>> Blocks<F> {
    fn new(block_at: F) -> Self {
        Self {
            block_at,
            calls: core::cell::Cell::new(0),
            metadata: Metadata::default(),
        }
    }
}

impl<F: Fn(Locator) -> Option<Block>> Document for Blocks<F> {
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn sections(&self) -> u32 {
        1
    }

    fn section_len(&self, _: u32) -> u32 {
        u32::MAX
    }

    fn block_at(&self, _: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
        self.calls.set(self.calls.get() + 1);
        assert!(self.calls.get() < 10_000, "the layout does not end");
        Ok((self.block_at)(at))
    }
}

fn block(text: &str, start: u32, end: u32) -> Block {
    Block::new(BlockKind::Paragraph, start, end, String::from(text))
        .map(OffsetMap::IDENTITY)
        .align(Align::Auto)
}

/// Lines without height, as from a broken font.
struct Flat;

impl Typesetter for Flat {
    fn advance(&self, _: Face, _: char) -> f32 {
        1.0
    }

    fn line(&self, _: Face) -> (f32, f32) {
        (0.0, 0.0)
    }
}

fn layout_with(
    doc: &dyn Document,
    typesetter: &dyn Typesetter,
    start: Locator,
    height: f32,
) -> Result<Page, DocumentError> {
    let storage = paperpaw_document::NoStorage;
    let cx = LayoutContext {
        document: doc,
        storage: &storage,
        width: 10.0,
        style: &STYLE,
        typesetter,
        hyphenate: None,
    };
    layout_page(&cx, start, height)
}

#[test]
fn blocks_that_do_not_go_on_are_malformed() {
    // Each block ends where it was asked for, or before.
    for back in [0, 1] {
        let doc = Blocks::new(|at: Locator| Some(block("", 0, at.offset.saturating_sub(back))));
        for typesetter in [&Units as &dyn Typesetter, &Flat] {
            let page = layout_with(&doc, typesetter, Locator::new(0, 1), 100.0);
            assert!(matches!(page, Err(DocumentError::Malformed(_))));
        }
    }
    // A page with lines that ends where it starts would come again and
    // again.
    let mut map = OffsetMap::default();
    map.checkpoint(10, 0);
    let doc = Blocks::new(move |_| Some(block("aaaa bbbb cccc", 100, 200).map(map.clone())));
    let page = layout_with(&doc, &Units, Locator::new(0, 100), 10.0);
    assert!(matches!(page, Err(DocumentError::Malformed(_))));
}

#[test]
fn pictures_without_size_are_left_out() {
    let doc = Blocks::new(|at: Locator| match at.offset {
        0..10 => {
            let mut picture = block("", 0, 10).image(ImageRef {
                path: String::from("a.gray"),
                width: 0,
                height: 0,
            });
            picture.kind = BlockKind::Image;
            Some(picture)
        }
        10..20 => Some(block("aa", 10, 20)),
        _ => None,
    });
    let page = layout_with(&doc, &Units, Locator::START, 100.0).unwrap();
    assert!(page.images.is_empty());
    assert_eq!(texts(&page), vec!["aa"]);
    assert_eq!(page.lines[0].baseline, 8.0);
}

#[test]
fn lines_without_height_still_fill_pages() {
    let doc =
        Blocks::new(|at: Locator| (at.offset < 5000).then(|| block("a", at.offset, at.offset + 1)));
    let page = layout_with(&doc, &Flat, Locator::START, 100.0).unwrap();
    assert!(page.lines.len() <= 100, "{} lines", page.lines.len());
    assert!(page.end > page.start);
}

#[test]
fn the_last_section_number_does_not_overflow() {
    let doc = Text::new("x\n");
    let page = layout_with(&doc, &Units, Locator::new(u32::MAX, 0), 100.0).unwrap();
    assert_eq!(texts(&page), vec!["x"]);
}

#[test]
fn headings_are_not_left_alone_at_the_bottom() {
    let doc = Text::new("aa\n#h\nbb\n");
    // Room for "aa" (10) and the heading (20), but not for "bb".
    let page = layout(&doc, 0, 35.0, &STYLE);
    assert_eq!(texts(&page), vec!["aa"]);
    assert_eq!(page.end, Locator::new(0, 3));
    let next = layout(&doc, page.end.offset, 35.0, &STYLE);
    assert_eq!(texts(&next), vec!["#h", "bb"]);
}

/// One paragraph of 4-letter words, laid out 40 wide in `style`.
fn styled_page(block: Block, style: &Style) -> (Page, Locator) {
    let doc = Blocks::new(move |at: Locator| (at.offset < 79).then(|| block.clone()));
    let storage = paperpaw_document::NoStorage;
    let cx = LayoutContext::new(&doc, &storage, 40.0, style, &Units);
    let page = layout_page(&cx, Locator::new(0, 0), 100.0).unwrap();
    // Where the second line starts, as found from inside it.
    let second = Locator::new(0, page.lines[1].words[0].0);
    let inside = Locator::new(0, second.offset + 2);
    assert_eq!(
        line_start(&cx, inside).unwrap(),
        second,
        "line starts agree"
    );
    (page, second)
}

fn words() -> Block {
    let text = "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk llll mmmm nnnn oooo pppp";
    block(text, 0, 79)
}

#[test]
fn book_margins_narrow_the_lines_within_a_quarter() {
    // 2 and 9 ems at 10 px: 20 px kept to a quarter of 40, 10 px.
    let book = words().margins(200, 900);
    let (page, _) = styled_page(book.clone(), &STYLE);
    for line in &page.lines {
        assert_eq!(line.x, 10.0);
        assert!(line.text.len() <= 20, "{:?}", line.text);
    }
    // Left to the reader: the whole width.
    let reader = STYLE.book(BookStyles::ALL.without(BookStyles::MARGINS));
    let (page, _) = styled_page(book, &reader);
    assert!(page.lines.iter().all(|l| l.x == 0.0));
    assert_eq!(
        page.lines[0].text,
        "aaaa bbbb cccc dddd eeee ffff gggg hhhh"
    );
}

#[test]
fn a_negative_indent_hangs_into_the_margin() {
    let hanging = words().margins(100, 0).indent(-100);
    let (page, _) = styled_page(hanging.clone(), &STYLE);
    assert_eq!(page.lines[0].x, 0.0);
    assert!(page.lines[1..].iter().all(|l| l.x == 10.0));
    // Without a margin it cannot hang out of the page.
    let (page, _) = styled_page(words().indent(-100), &STYLE);
    assert!(page.lines.iter().all(|l| l.x == 0.0));
    // The book's indent left to the reader: the margin stays.
    let reader = STYLE.book(BookStyles::ALL.without(BookStyles::INDENT));
    let (page, _) = styled_page(hanging, &reader);
    assert!(page.lines.iter().all(|l| l.x == 10.0));
}

#[test]
fn the_books_line_height_only_where_followed() {
    let tall = words().line_height(200);
    let (page, _) = styled_page(tall.clone(), &STYLE);
    assert_eq!(page.lines[1].baseline - page.lines[0].baseline, 20.0);
    let reader = STYLE.book(BookStyles::ALL.without(BookStyles::LINE_HEIGHT));
    let (page, _) = styled_page(tall, &reader);
    assert_eq!(page.lines[1].baseline - page.lines[0].baseline, 10.0);
}

#[test]
fn room_above_the_lines_moves_them_down_and_breaks_the_page_sooner() {
    let (plain, _) = styled_page(words(), &STYLE);
    // Half an em at 10 px: 5 px above every line, part of its height,
    // whether the reader follows the book or not.
    for style in [STYLE, STYLE.book(BookStyles::NONE)] {
        let (spaced, _) = styled_page(words().leading_above(50), &style);
        assert_eq!(spaced.lines[0].baseline - plain.lines[0].baseline, 5.0);
        assert_eq!(spaced.lines[1].baseline - spaced.lines[0].baseline, 15.0);
        assert!(spaced.lines.iter().all(|l| l.leading == 5.0));
        assert_eq!(spaced.lines[0].text, plain.lines[0].text);
    }
    assert!(plain.lines.iter().all(|l| l.leading == 0.0));
    // Four ems at most: on a page 60 px high, a line of 50 px leaves
    // no room for the second, which comes on the next page.
    let page = |block: Block| {
        let doc = Blocks::new(move |at: Locator| (at.offset < 79).then(|| block.clone()));
        let storage = paperpaw_document::NoStorage;
        let cx = LayoutContext::new(&doc, &storage, 40.0, &STYLE, &Units);
        layout_page(&cx, Locator::new(0, 0), 60.0).unwrap()
    };
    let (plain, most) = (page(words()), page(words().leading_above(5000)));
    assert_eq!(plain.lines.len(), 2);
    assert_eq!(most.lines.len(), 1);
    assert_eq!(most.lines[0].leading, 40.0);
    assert!(most.end < plain.end, "the page breaks sooner");
    assert_eq!(most.end.offset, plain.lines[1].words[0].0);
}

/// A page `height` high from `start`, of the one block `block`.
fn page_of(block: Block, start: u32, height: f32) -> Page {
    let end = block.end;
    let doc = Blocks::new(move |at: Locator| (at.offset < end).then(|| block.clone()));
    let storage = paperpaw_document::NoStorage;
    let cx = LayoutContext::new(&doc, &storage, 40.0, &STYLE, &Units);
    layout_page(&cx, Locator::new(0, start), height).unwrap()
}

/// `words()` with a filter's translation inserted after it.
fn translated() -> Block {
    let mut block = words();
    assert!(block.insert_after(block_of("xxxx yyyy")));
    block
}

fn block_of(text: &str) -> Block {
    Block::new(
        BlockKind::Paragraph,
        0,
        text.len() as u32,
        String::from(text),
    )
}

#[test]
fn blocks_a_filter_inserts_follow_the_block_without_places() {
    let page = page_of(translated(), 0, 100.0);
    let (anchor, inserted): (Vec<_>, Vec<_>) = page.lines.iter().partition(|l| !l.inserted);
    assert_eq!(anchor.len(), 2);
    assert_eq!(inserted.len(), 1);
    assert_eq!(inserted[0].text, "xxxx yyyy");
    assert!(inserted[0].words.is_empty(), "no places of its own");
    assert!(inserted[0].baseline > anchor[1].baseline);
    // The page ends where the book's block does.
    assert_eq!(page.end, Locator::new(0, 79));
    // Bounded: no more than four, nor more than 4 KiB of text.
    let mut block = words();
    for _ in 0..MAX_INSERTED {
        assert!(block.insert_after(block_of("z")));
    }
    assert!(!block.insert_after(block_of("z")));
    let mut big = words();
    assert!(!big.insert_after(block_of(&"z".repeat(MAX_INSERTED_TEXT + 1))));
}

#[test]
fn the_last_line_of_a_block_goes_with_what_was_inserted_after_it() {
    // Two lines of 10 px fit in 25, the translation's third does not:
    // the second line goes on with it.
    let first = page_of(translated(), 0, 25.0);
    assert_eq!(first.lines.len(), 1);
    assert!(!first.lines[0].inserted);
    let second_start = page_of(translated(), 0, 100.0).lines[1].words[0].0;
    assert_eq!(first.end, Locator::new(0, second_start));
    let next = page_of(translated(), first.end.offset, 25.0);
    assert_eq!(next.lines.len(), 2);
    assert!(!next.lines[0].inserted && next.lines[1].inserted);
    assert_eq!(next.end, Locator::new(0, 79));
    // A block's only line at the top of a page with no room for what
    // follows it: what fits stays, the rest goes, the page goes on.
    let mut short = block_of("aaaa");
    short.end = 4;
    assert!(short.insert_after(block_of("xxxx yyyy")));
    let page = page_of(short, 0, 15.0);
    assert_eq!(page.lines.len(), 1);
    assert_eq!(page.end, Locator::new(0, 4));
    // Taller than any page: no line moves on (a page of nothing but
    // the last line would not show it whole either); what fits stays.
    let mut long = words();
    assert!(long.insert_after(block_of(&"xxxx ".repeat(100))));
    let page = page_of(long, 0, 50.0);
    assert_eq!(page.lines.iter().filter(|l| !l.inserted).count(), 2);
    assert_eq!(page.lines.len(), 5);
    assert_eq!(page.end, Locator::new(0, 79));
}

#[test]
fn the_books_alignment_size_and_fonts_can_be_left_to_the_reader() {
    let centred = words().align(Align::Center).size(200).runs(vec![Run {
        start: 0,
        end: 4,
        style: TextStyle::REGULAR.with_family(3),
    }]);
    let (page, _) = styled_page(centred.clone(), &STYLE);
    assert!(page.lines[0].x > 0.0);
    assert_eq!(page.lines[0].runs[0].face.size, 200);
    assert_eq!(page.lines[0].runs[0].face.style.family(), 3);
    let (page, _) = styled_page(centred, &STYLE.book(BookStyles::NONE));
    assert_eq!(page.lines[0].x, 0.0);
    assert_eq!(page.lines[0].runs[0].face.size, 0);
    assert_eq!(page.lines[0].runs[0].face.style.family(), 0);
}

#[test]
fn before_a_block_the_line_start_is_where_a_page_from_there_starts() {
    let picture = ImageRef {
        path: String::from("p.gray"),
        width: 10,
        height: 10,
    };
    for image in [None, Some(picture)] {
        let doc = Late {
            image,
            metadata: Metadata::default(),
        };
        let storage = paperpaw_document::NoStorage;
        let cx = LayoutContext {
            document: &doc,
            storage: &storage,
            width: 100.0,
            style: &STYLE,
            typesetter: &Units,
            hyphenate: None,
        };
        let at = Locator::new(0, 0);
        let start = line_start(&cx, at).unwrap();
        assert_eq!(start, Locator::new(0, 8));
        assert_eq!(start, layout_page(&cx, at, 100.0).unwrap().start);
        assert_eq!(line_start(&cx, start).unwrap(), start, "it stays");
    }
}
