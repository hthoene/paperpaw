//! Reading fonts keep their gaps stable when a line crosses the old
//! justification cutoff. Settings previews and book pages use the same
//! layout and drawing, including at the reported 9 pt size.

#![cfg(feature = "unstable")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::Harness;
use paperpaw_sdk::builtin::{fonts, groups, settings};
use paperpaw_sdk::document::{Block, BlockKind, Document, DocumentError, Locator, Metadata};
use paperpaw_sdk::layout::{Face, LayoutContext, layout_page};
use paperpaw_sdk::reading::{PageType, ReadingFaces, StatusRoom};
use paperpaw_sdk::{
    Font, FontSpec, Id, NoStorage, Priority, SettingKind, SettingSpec, SettingValue, Storage,
};
use std::sync::OnceLock;

const LITERATA: &[u8] = include_bytes!("../../../plugins/fonts-default/fonts/Literata.ttf");
const FIRST_LINE: &str = "Alice was beginning to";
const BOUNDARY_TEXT: &str = "Alice was beginning to conversations";
const ENGLISH: &str = "Alice was beginning to get very tired of sitting by her sister on the bank, and of having nothing to do: once or twice she had peeped into the book her sister was reading, but it had no pictures or conversations in it.";
const GERMAN: &str = "Als Gregor Samsa eines Morgens aus unruhigen Träumen erwachte, fand er sich in seinem Bett zu einem ungeheueren Ungeziefer verwandelt.";
const FONT_ID: Id = Id::new_static("test.literata");

#[derive(Clone, Copy, Debug)]
enum Kind {
    Outline,
    Pack,
}

impl Kind {
    fn font(self) -> Font {
        match self {
            Self::Outline => Font::from_data(LITERATA, &[("wght", 400.0)]).unwrap(),
            Self::Pack => {
                // The real font and the strikes shipped on small readers.
                // Built once in memory, with only the characters these tests use.
                static PACK: OnceLock<Vec<u8>> = OnceLock::new();
                let data = PACK.get_or_init(|| {
                    let outline =
                        paperpaw_text::OutlineFont::new(LITERATA, &[("wght", 400.0)]).unwrap();
                    let mut chars: Vec<char> = ENGLISH.chars().chain(GERMAN.chars()).collect();
                    chars.extend("-ﬁﬂﬀﬃﬄ".chars());
                    chars.sort_unstable();
                    chars.dedup();
                    paperpaw_text::build_pack(&outline, &[24.0, 28.0, 32.0], chars)
                });
                Font::from_data(data.as_slice(), &[]).unwrap()
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Options {
    size: i64,
    spacing: i64,
    balanced: bool,
    justify: bool,
}

fn harness(kind: Kind, options: Options) -> Harness {
    let mut h = Harness::new();
    for (id, value) in [
        (settings::FONT_SIZE.static_str(), options.size),
        (settings::WORD_SPACING.static_str(), options.spacing),
        (settings::LINE_SPACING.static_str(), 135),
        (settings::INDENT_SIZE.static_str(), 150),
        (settings::MARGIN.static_str(), 4),
        (settings::MARGIN_ENDS.static_str(), 4),
        (settings::WIDOWS.static_str(), 1),
        (settings::ORPHANS.static_str(), 1),
    ] {
        h.settings
            .define(SettingSpec::number(id, "x", 0, 300, value, groups::OTHER))
            .unwrap();
    }
    for (id, value) in [
        (settings::JUSTIFY.static_str(), options.justify),
        (settings::INDENT.static_str(), true),
        (settings::KERNING.static_str(), true),
        (settings::LIGATURES.static_str(), true),
    ] {
        h.settings
            .define(SettingSpec::switch(id, "x", value, groups::OTHER))
            .unwrap();
    }
    let breaking = if options.balanced {
        settings::LINE_BREAKING_BALANCED
    } else {
        settings::LINE_BREAKING_FAST
    };
    h.settings
        .define(SettingSpec::new(
            Id::new_static(settings::LINE_BREAKING.static_str()),
            "x",
            SettingKind::Text,
            SettingValue::Text(breaking.into()),
            groups::OTHER,
        ))
        .unwrap();
    h.fonts.add(FontSpec::new(FONT_ID, kind.font())).unwrap();
    h.fonts.propose(fonts::READING, FONT_ID, Priority::DEFAULT);
    h
}

struct Paragraph<'a> {
    text: &'a str,
    metadata: Metadata,
}

impl<'a> Paragraph<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            metadata: Metadata::default(),
        }
    }
}

impl Document for Paragraph<'_> {
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
        Ok(
            (at.section == 0 && at.offset < self.text.len() as u32).then(|| {
                Block::new(
                    BlockKind::Paragraph,
                    0,
                    self.text.len() as u32,
                    self.text.to_string(),
                )
            }),
        )
    }
}

/// A fixed set of words around the old cutoff, independent of later
/// paragraph break choices. A changed line break may legitimately change
/// the gap; this test requires the first line to retain the same words.
#[test]
fn real_fonts_do_not_collapse_gaps_at_the_old_cutoff() {
    for kind in [Kind::Outline, Kind::Pack] {
        for size in [9, 8, 10] {
            for balanced in [false, true] {
                let mut available = None;
                let mut before: Option<f32> = None;
                for spacing in [80, 90] {
                    let h = harness(
                        kind,
                        Options {
                            size,
                            spacing,
                            balanced,
                            justify: true,
                        },
                    );
                    let env = h.env();
                    let font = env.fonts().role(fonts::READING.as_str()).unwrap();
                    let t = PageType::new(&env, &font, "en", false, StatusRoom::NONE);
                    let faces = ReadingFaces::new(&env, &t, None).unwrap();
                    let face = Face::default();
                    let base = faces.space(face);
                    let words: f32 = FIRST_LINE.split(' ').map(|w| faces.measure(face, w)).sum();
                    // At 80%, each gap needs just over the former cutoff.
                    // At 90%, the same line needs just under it.
                    let width = *available.get_or_insert(
                        words + 3.0 * (base + t.line_height * 0.6 + 0.2) + t.style.indent,
                    );
                    let doc = Paragraph::new(BOUNDARY_TEXT);
                    let cx = LayoutContext::new(&doc, &NoStorage, width, &t.style, &faces);
                    let page = layout_page(&cx, Locator::START, 500.0).unwrap();
                    assert_eq!(
                        page.lines[0].text, FIRST_LINE,
                        "{kind:?}, {size} pt, {spacing}%"
                    );
                    let gap = base + page.lines[0].word_spacing;
                    if let Some(before) = before {
                        assert!(
                            (gap - before).abs() < 1.0,
                            "gap jumped from {before} to {gap}: {kind:?}, {size} pt, balanced={balanced}"
                        );
                    }
                    before = Some(gap);
                    assert_eq!(page.lines.last().unwrap().word_spacing, 0.0);
                }
            }
        }
    }
}
