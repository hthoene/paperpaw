//! Right-to-left lines on the page: the words of a Hebrew line drawn from
//! the right, a number in it left to right, word boxes in reading order
//! where the words are drawn.

#![cfg(feature = "unstable")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::Harness;
use paperpaw_graphics::Framebuffer;
use paperpaw_sdk::builtin::{fonts, groups, settings};
use paperpaw_sdk::document::{Block, BlockKind, Document, DocumentError, Locator, Metadata};
use paperpaw_sdk::lang::Direction;
use paperpaw_sdk::layout::{LayoutContext, LineBidi, layout_page};
use paperpaw_sdk::reading::{PageType, ReadingFaces, StatusRoom, draw_lines, word_boxes};
use paperpaw_sdk::{
    Font, FontSpec, Id, LineMetrics, NoStorage, PixelFormat, Priority, SettingSpec, Storage,
};
use paperpaw_text::{Glyph, PackBuilder};

const FONT_ID: Id = Id::new_static("test.hebrew");
/// Three words of Hebrew and a number.
const TEXT: &str = "\u{5d0}\u{5d1}\u{5d2} \u{5d3}\u{5d4} 12 \u{5d5}\u{5d6}";

/// Letters, digits and spaces 5 wide, drawing nothing.
fn font() -> Font {
    let metrics = LineMetrics::new(16.0, 4.0, 24.0);
    let mut builder = PackBuilder::new();
    let glyphs = TEXT
        .chars()
        .filter(|c| *c != ' ')
        .chain(['1', '2', ' '])
        .map(|c| (c, Glyph::empty(5.0)))
        .collect();
    builder.add_strike(20.0, metrics, glyphs);
    Font::from_data(builder.build(), &[]).unwrap()
}

fn harness() -> Harness {
    let mut h = Harness::new();
    for (id, value) in [
        (settings::FONT_SIZE.static_str(), 15),
        (settings::WORD_SPACING.static_str(), 100),
        (settings::LINE_SPACING.static_str(), 120),
        (settings::MARGIN.static_str(), 0),
        (settings::MARGIN_ENDS.static_str(), 0),
    ] {
        h.settings
            .define(SettingSpec::number(id, "x", 0, 300, value, groups::OTHER))
            .unwrap();
    }
    for (id, value) in [
        (settings::JUSTIFY.static_str(), false),
        (settings::INDENT.static_str(), false),
    ] {
        h.settings
            .define(SettingSpec::switch(id, "x", value, groups::OTHER))
            .unwrap();
    }
    h.fonts.add(FontSpec::new(FONT_ID, font())).unwrap();
    h.fonts.propose(fonts::READING, FONT_ID, Priority::DEFAULT);
    h
}

struct Hebrew(Metadata);

impl Document for Hebrew {
    fn metadata(&self) -> &Metadata {
        &self.0
    }

    fn sections(&self) -> u32 {
        1
    }

    fn section_len(&self, _: u32) -> u32 {
        TEXT.len() as u32
    }

    fn block_at(&self, _: &dyn Storage, at: Locator) -> Result<Option<Block>, DocumentError> {
        let len = TEXT.len() as u32;
        Ok((at.offset < len)
            .then(|| Block::new(BlockKind::Paragraph, 0, len, TEXT.into()).dir(Direction::Rtl)))
    }
}

#[test]
fn a_hebrew_line_is_drawn_from_the_right_its_words_boxed_in_reading_order() {
    let h = harness();
    let env = h.env();
    let font = env.fonts().role(fonts::READING.as_str()).unwrap();
    let t = PageType::new(&env, &font, "he", false, StatusRoom::NONE);
    let faces = ReadingFaces::new(&env, &t, None).unwrap();
    let doc = Hebrew(Metadata::default());
    let cx = LayoutContext::new(&doc, &NoStorage, 400.0, &t.style, &faces);
    let page = layout_page(&cx, Locator::START, 500.0).unwrap();
    let line = &page.lines[0];
    assert!(line.bidi.is_rtl() && line.bidi.has(LineBidi::REORDER));
    let words = word_boxes(&faces, &t, &page);
    let texts: Vec<&str> = words.iter().map(|w| w.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "\u{5d0}\u{5d1}\u{5d2}",
            "\u{5d3}\u{5d4}",
            "12",
            "\u{5d5}\u{5d6}"
        ]
    );
    // From the right: each word left of the one before it, the line's
    // end at the right edge of the text box.
    let xs: Vec<u16> = words.iter().map(|w| w.area.x).collect();
    assert!(xs.windows(2).all(|w| w[0] > w[1]), "{xs:?}");
    let right = words[0].area.x + words[0].area.width;
    assert!((f32::from(right) - (t.x + 400.0)).abs() <= 1.0, "{right}");
    let rtl: Vec<bool> = words.iter().map(|w| w.rtl).collect();
    assert_eq!(rtl, [true, true, false, true]);
    // Drawn where boxed (nothing to see in empty glyphs: it runs).
    let mut frame = Framebuffer::new(480, 100, PixelFormat::Gray8).unwrap();
    draw_lines(
        &mut frame.canvas(),
        &faces,
        &t,
        (t.x, t.y),
        &page.lines,
        &[],
    );
}
