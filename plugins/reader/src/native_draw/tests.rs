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
