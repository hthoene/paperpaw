use super::*;
use crate::native_typeset::Measure;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::Cell;
use paperpaw_sdk::{
    NoStorage, Storage,
    document::{Block, BlockKind, Document, DocumentError, Locator, Metadata, TextStyle},
    layout::{LayoutContext, Style, layout_page},
};

/// Every character is 10 pixels wide at size 20; a line is 1.25 of the size.
#[derive(Debug, Default)]
struct Fixed;

impl Measure for Fixed {
    fn width(&self, role: Role, size: f32, text: &str) -> Result<f32, Error> {
        let bold = if matches!(role, Role::Bold | Role::BoldItalic) {
            2.0
        } else {
            0.0
        };
        Ok((size * 0.5 + bold) * text.chars().count() as f32)
    }
    fn metrics(&self, _: Role, size: f32) -> Result<(f32, f32), Error> {
        Ok((size * 0.8, size * 0.2))
    }
}

struct Book {
    blocks: Vec<Block>,
    metadata: Metadata,
}

impl Document for Book {
    fn metadata(&self) -> &Metadata {
        &self.metadata
    }
    fn sections(&self) -> u32 {
        1
    }
    fn section_len(&self, _: u32) -> u32 {
        self.blocks.last().map_or(0, |b| b.end)
    }
    fn block_at(&self, _: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
        Ok(self.blocks.iter().find(|b| b.end > at.offset).cloned())
    }
}

#[derive(Default)]
struct Recorder {
    calls: Vec<(Role, i32, i32, String)>,
    fills: Vec<(i32, i32, u32, u32)>,
    fail_at: Cell<Option<usize>>,
}

impl Ink for Recorder {
    fn run(
        &mut self,
        role: Role,
        size: f32,
        x: f32,
        baseline: i32,
        text: &str,
    ) -> Result<f32, Error> {
        if self.fail_at.get() == Some(self.calls.len()) {
            return Err(Error::State);
        }
        self.calls.push((role, x as i32, baseline, text.into()));
        Fixed.width(role, size, text)
    }

    fn fill(&mut self, x: i32, y: i32, width: u32, height: u32) -> Result<(), Error> {
        self.fills.push((x, y, width, height));
        Ok(())
    }
}

fn page(text: &str, style: TextStyle) -> (Page, Faces<Fixed>) {
    let mut block = Block::new(BlockKind::Paragraph, 0, text.len() as u32, text.into());
    block.runs = alloc::vec![paperpaw_sdk::document::Run::new(
        0,
        text.len() as u32,
        style
    )];
    let book = Book {
        blocks: alloc::vec![block],
        metadata: Metadata::default(),
    };
    let faces = Faces::new(Fixed, 20.0, 1.0).unwrap();
    let style = Style::new(0.0, 0.0, 20.0, false);
    let cx = LayoutContext::new(&book, &NoStorage, 400.0, &style, &faces);
    (layout_page(&cx, Locator::START, 400.0).unwrap(), faces)
}

#[test]
fn words_are_set_one_after_the_other_with_the_gaps() {
    let (page, faces) = page("one two three", TextStyle::REGULAR);
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &page, (30.0, 50.0)).unwrap();
    let words: Vec<&str> = ink.calls.iter().map(|c| c.3.as_str()).collect();
    assert_eq!(words, ["one", "two", "three"]);
    // 10 px a letter and a space: "one" 30, a gap of 10, "two" at 70, "three" at 110.
    let xs: Vec<i32> = ink.calls.iter().map(|c| c.1).collect();
    assert_eq!(xs, [30, 70, 110]);
    // One line: one baseline, below the top.
    assert!(ink.calls.iter().all(|c| c.2 == ink.calls[0].2 && c.2 > 50));
    assert!(ink.calls.iter().all(|c| c.0 == Role::Reading));
}

#[test]
fn a_bold_run_is_drawn_in_the_bold_role() {
    let (page, faces) = page("bold", TextStyle::BOLD);
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &page, (0.0, 0.0)).unwrap();
    assert_eq!(ink.calls[0].0, Role::Bold);
}

#[test]
fn a_long_text_wraps_and_every_line_has_its_own_baseline() {
    let text = "word ".repeat(40);
    let (page, faces) = page(&text, TextStyle::REGULAR);
    assert!(page.lines.len() > 1);
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &page, (0.0, 0.0)).unwrap();
    let mut baselines: Vec<i32> = ink.calls.iter().map(|c| c.2).collect();
    baselines.dedup();
    assert_eq!(baselines.len(), page.lines.len());
    assert!(baselines.windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn the_first_refusal_of_the_host_ends_the_drawing() {
    let (page, faces) = page("one two three", TextStyle::REGULAR);
    let mut ink = Recorder::default();
    ink.fail_at.set(Some(1));
    assert_eq!(
        draw_lines(&mut ink, &faces, &page, (0.0, 0.0)),
        Err(Error::State)
    );
    assert_eq!(ink.calls.len(), 1);
}

#[test]
fn characters_that_are_not_drawn_are_left_out_of_a_word() {
    let (page, faces) = page("zero\u{200b}width soft\u{feff}", TextStyle::REGULAR);
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &page, (0.0, 0.0)).unwrap();
    let words: Vec<&str> = ink.calls.iter().map(|c| c.3.as_str()).collect();
    assert_eq!(words, ["zerowidth", "soft"]);
}

#[test]
fn an_underlined_run_has_a_line_below_the_whole_run() {
    let (page, faces) = page("one two", TextStyle::UNDERLINE);
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &page, (30.0, 50.0)).unwrap();
    let base = ink.calls[0].2;
    // Size 20: a pixel thick, two below the baseline, the run's 70 pixels wide.
    assert_eq!(ink.fills, [(30, base + 2, 70, 1)]);
}

#[test]
fn a_strike_through_and_an_overline_are_drawn_where_beta_3_drew_them() {
    let style = TextStyle::STRIKE.union(TextStyle::OVERLINE);
    let (page, faces) = page("abc", style);
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &page, (0.0, 0.0)).unwrap();
    let base = ink.calls[0].2;
    assert_eq!(ink.fills, [(0, base - 15, 30, 1), (0, base - 5, 30, 1)]);
}

#[test]
fn a_double_line_has_a_second_line_and_a_dotted_one_has_pieces() {
    let double = TextStyle::UNDERLINE.with_line_style(paperpaw_sdk::document::LineStyle::Double);
    let (double_page, faces) = page("abc", double);
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &double_page, (0.0, 0.0)).unwrap();
    let base = ink.calls[0].2;
    assert_eq!(ink.fills, [(0, base + 2, 30, 1), (0, base + 4, 30, 1)]);
    let dotted = TextStyle::UNDERLINE.with_line_style(paperpaw_sdk::document::LineStyle::Dotted);
    let (dotted_page, faces) = page("abc", dotted);
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &dotted_page, (0.0, 0.0)).unwrap();
    // One pixel dots with one pixel gaps over 30 pixels.
    assert_eq!(ink.fills.len(), 15);
    assert!(ink.fills.iter().all(|f| f.2 == 1 && f.3 == 1));
}

#[test]
fn a_text_without_lines_and_a_page_without_rules_fill_nothing() {
    let (page, faces) = page("plain", TextStyle::REGULAR);
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &page, (0.0, 0.0)).unwrap();
    assert!(ink.fills.is_empty());
}

#[test]
fn a_tables_rules_are_filled_at_the_content_box() {
    let (mut page, faces) = page("x", TextStyle::REGULAR);
    page.rules
        .push(paperpaw_sdk::layout::PageRule::new(5.0, 6.0, 100.0, 1.0));
    page.rules
        .push(paperpaw_sdk::layout::PageRule::new(5.0, 6.0, 0.2, 40.0));
    let mut ink = Recorder::default();
    draw_lines(&mut ink, &faces, &page, (30.0, 50.0)).unwrap();
    // A rule is a pixel thick at least.
    assert_eq!(ink.fills, [(35, 56, 100, 1), (35, 56, 1, 40)]);
}

#[test]
fn a_refused_fill_ends_the_drawing() {
    struct Refuses;
    impl Ink for Refuses {
        fn run(&mut self, _: Role, _: f32, _: f32, _: i32, text: &str) -> Result<f32, Error> {
            Ok(text.len() as f32 * 10.0)
        }
        fn fill(&mut self, _: i32, _: i32, _: u32, _: u32) -> Result<(), Error> {
            Err(Error::State)
        }
    }
    let (page, faces) = page("abc", TextStyle::UNDERLINE);
    assert!(draw_lines(&mut Refuses, &faces, &page, (0.0, 0.0)).is_err());
}

fn marked(page: &Page, faces: &Faces<Fixed>, words: core::ops::Range<usize>) -> Recorder {
    let mut ink = Recorder::default();
    draw_lines_marked(&mut ink, faces, page, (30.0, 50.0), Some(words)).unwrap();
    ink
}

#[test]
fn the_words_of_the_sentence_read_aloud_have_one_line_under_them() {
    let (page, faces) = page("one two three", TextStyle::REGULAR);
    let ink = marked(&page, &faces, 1..3);
    let base = ink.calls[0].2;
    // "two" from 70 to 100, a gap of 10, "three" to 160: one line, as the book's own.
    assert_eq!(ink.fills, [(70, base + 2, 90, 1)]);
    // Nothing marked: nothing under it, and the words are drawn as always.
    assert!(marked(&page, &faces, 0..0).fills.is_empty());
    assert!(marked(&page, &faces, 5..9).fills.is_empty());
    assert_eq!(ink.calls.len(), 3);
    // Without a mark it is draw_lines.
    let mut plain = Recorder::default();
    draw_lines_marked(&mut plain, &faces, &page, (30.0, 50.0), None).unwrap();
    assert!(plain.fills.is_empty());
}

#[test]
fn a_marked_stretch_ends_with_its_line_and_goes_on_on_the_next() {
    let text = "word ".repeat(20);
    let (page, faces) = page(text.trim_end(), TextStyle::REGULAR);
    assert!(page.lines.len() > 1);
    let first_line = page.lines[0].text.split(' ').count();
    // The last word of the first line and the first of the second: two lines.
    let ink = marked(&page, &faces, first_line - 1..first_line + 1);
    assert_eq!(ink.fills.len(), 2);
    assert!(ink.fills[0].1 < ink.fills[1].1);
}

#[test]
fn a_word_in_two_runs_is_one_word() {
    let mut block = Block::new(BlockKind::Paragraph, 0, 8, "abcd efg".into());
    block.runs = alloc::vec![
        paperpaw_sdk::document::Run::new(0, 2, TextStyle::BOLD),
        paperpaw_sdk::document::Run::new(2, 8, TextStyle::REGULAR),
    ];
    let book = Book {
        blocks: alloc::vec![block],
        metadata: Metadata::default(),
    };
    let faces = Faces::new(Fixed, 20.0, 1.0).unwrap();
    let style = Style::new(0.0, 0.0, 20.0, false);
    let cx = LayoutContext::new(&book, &NoStorage, 400.0, &style, &faces);
    let page = layout_page(&cx, Locator::START, 400.0).unwrap();
    // "abcd" is word 0 (the bold "ab" and the regular "cd" pieces): both underlined
    // in one line over its 44 pixels (a bold letter is 2 wider in this measure).
    let ink = marked(&page, &faces, 0..1);
    assert_eq!(ink.fills.len(), 1);
    assert_eq!((ink.fills[0].0, ink.fills[0].2), (30, 44));
    // "efg" is word 1: from its own start, after a space.
    let second = marked(&page, &faces, 1..2);
    assert_eq!(second.fills, [(84, second.calls[0].2 + 2, 30, 1)]);
}
