use super::*;
use crate::native_typeset::Role;
use alloc::vec;
use core::cell::Cell;
use paperpaw_sdk::{
    NoStorage, Storage,
    document::{Block, BlockKind, Document, DocumentError, Metadata, Run, TextStyle},
    layout::{LayoutContext, Style, layout_page},
};

/// Every character is 10 pixels wide at size 20 (bold 14); a line is 1.25 of the size.
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

const BOX: native_pages::Box = native_pages::Box {
    x: 30,
    y: 50,
    width: 400,
    height: 400,
};

/// `text` laid out as one paragraph with `runs` (start, end, style), 400 px wide.
fn page(text: &str, runs: &[(u32, u32, TextStyle)]) -> (Page, Faces<Fixed>) {
    let mut block = Block::new(BlockKind::Paragraph, 0, text.len() as u32, text.into());
    block.runs = runs
        .iter()
        .map(|&(start, end, style)| Run::new(start, end, style))
        .collect();
    let book = Book {
        blocks: vec![block],
        metadata: Metadata::default(),
    };
    let faces = Faces::new(Fixed, 20.0, 1.0).unwrap();
    let style = Style::new(0.0, 0.0, 20.0, false);
    let cx = LayoutContext::new(&book, &NoStorage, 400.0, &style, &faces);
    (layout_page(&cx, Locator::START, 400.0).unwrap(), faces)
}

fn plain(text: &str) -> (Page, Faces<Fixed>) {
    page(text, &[(0, text.len() as u32, TextStyle::REGULAR)])
}

#[test]
fn the_words_of_a_line_are_boxed_where_they_are_drawn() {
    let (page, faces) = plain("one two three");
    let words = word_boxes(&faces, &page, BOX).unwrap();
    let texts: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
    assert_eq!(texts, ["one", "two", "three"]);
    // 10 px a letter and a space, from the box's left edge (the drawing test's numbers).
    let xs: Vec<u16> = words.iter().map(|w| w.area.x).collect();
    assert_eq!(xs, [30, 70, 110]);
    let widths: Vec<u16> = words.iter().map(|w| w.area.width).collect();
    assert_eq!(widths, [30, 30, 50]);
    // The same row for all, below the box's top, 20 px high (the font's own: 16 + 4).
    assert!(words.iter().all(|w| w.area.y == words[0].area.y));
    assert!(words[0].area.y >= 50);
    assert_eq!(words[0].area.height, 20);
    // Their places in the book.
    assert_eq!(
        (words[1].start, words[1].end),
        (Locator::new(0, 4), Locator::new(0, 7))
    );
}

#[test]
fn a_word_in_two_runs_is_one_box() {
    // `bold` in bold and `face rest` after it: the word `boldface` goes on across runs.
    let (page, faces) = page(
        "boldface rest",
        &[(0, 4, TextStyle::BOLD), (4, 13, TextStyle::REGULAR)],
    );
    let words = word_boxes(&faces, &page, BOX).unwrap();
    let texts: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
    assert_eq!(texts, ["boldface", "rest"]);
    // 4 bold letters of 12 and 4 plain of 10.
    assert_eq!(words[0].area.width, 4 * 12 + 4 * 10);
}

#[test]
fn lines_a_filter_inserted_have_no_words() {
    let (mut page, faces) = plain("[Picture]");
    page.lines[0].inserted = true;
    assert!(word_boxes(&faces, &page, BOX).unwrap().is_empty());
}

#[test]
fn a_touch_hits_a_word_with_a_little_slack_at_the_sides() {
    let (page, faces) = plain("one two");
    let words = word_boxes(&faces, &page, BOX).unwrap();
    let y = i32::from(words[0].area.y) + 3;
    // Inside, in the gap beside either word, and past the slack.
    assert_eq!(word_at(&words, 40, y), Some(0));
    assert_eq!(word_at(&words, 62, y), Some(0));
    assert_eq!(word_at(&words, 66, y), Some(1));
    assert_eq!(word_at(&words, 400, y), None);
    // Above and below the line: none.
    assert_eq!(word_at(&words, 40, 10), None);
    assert_eq!(word_at(&words, 40, y + 40), None);
}

#[test]
fn a_refused_allocation_is_no_memory_not_a_panic() {
    let (page, faces) = plain("one two three");
    let refused = Cell::new(0);
    let total = crate::fail_alloc::sweep(|| {
        refused.set(refused.get() + 1);
        word_boxes(&faces, &page, BOX).map(|w| w.len())
    });
    assert!(total > 0);
}
