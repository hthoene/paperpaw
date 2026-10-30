//! Bidirectional levels of a block's text (UAX #9) for its lines: runs
//! split where the level changes, lines of right-to-left paragraphs set
//! from the right, letters that join measured in the forms they take.

use alloc::vec::Vec;
use core::ops::Range;

use paperpaw_document::{Block, Direction};
use paperpaw_text::bidi::{self, Level};

/// The paragraph level of a block and, where its text needs the
/// algorithm, the level of each of its bytes (a character's on all of
/// its bytes); none where it is left to right alone, as most text is.
#[derive(Debug, Clone)]
pub(crate) struct BlockLevels {
    pub(crate) paragraph: Level,
    by_byte: Vec<Level>,
}

impl BlockLevels {
    /// The levels of `block`: its paragraph's direction as the book says,
    /// else from its first strong letter (rules P2 and P3), else left to
    /// right.
    pub(crate) fn of(block: &Block) -> BlockLevels {
        let paragraph = match block.dir {
            Direction::Rtl => Level::RTL,
            Direction::Ltr => Level::LTR,
            _ => bidi::paragraph_level(&block.text).unwrap_or(Level::LTR),
        };
        if !paragraph.is_rtl() && !bidi::needs_bidi(&block.text) {
            return BlockLevels {
                paragraph,
                by_byte: Vec::new(),
            };
        }
        let mut chars = alloc::vec![paragraph; block.text.chars().count()];
        bidi::resolve(&block.text, paragraph, &mut chars);
        let mut by_byte = Vec::with_capacity(block.text.len());
        for (c, level) in block.text.chars().zip(chars) {
            by_byte.extend(core::iter::repeat_n(level, c.len_utf8()));
        }
        BlockLevels { paragraph, by_byte }
    }

    /// Whether all of the block is left to right, at level 0.
    pub(crate) fn is_plain(&self) -> bool {
        self.by_byte.is_empty()
    }

    /// The level of the character at byte `index`, before rule L1 (the
    /// paragraph's past the text: a hyphen).
    pub(crate) fn at(&self, index: usize) -> Level {
        self.by_byte.get(index).copied().unwrap_or(self.paragraph)
    }

    /// The levels of the characters of the line `range` of `text` (the
    /// block's), one each, rule L1 applied.
    pub(crate) fn line(&self, text: &str, range: Range<usize>) -> Vec<Level> {
        let Some(line) = text.get(range.clone()) else {
            return Vec::new();
        };
        let mut levels: Vec<Level> = line
            .char_indices()
            .map(|(i, _)| self.at(range.start + i))
            .collect();
        if !self.is_plain() {
            bidi::reset_line_end(line, &mut levels, self.paragraph);
        }
        levels
    }

    /// Whether the character at byte `index` runs right to left.
    pub(crate) fn is_rtl_at(&self, index: usize) -> bool {
        !self.is_plain() && self.at(index).is_rtl()
    }
}

/// Whether `c` is of the scripts whose letters join their neighbours
/// (Arabic, Syriac, Thaana, NKo and the Arabic supplements): measured in
/// the form it takes there.
pub(crate) fn may_join(c: char) -> bool {
    matches!(c, '\u{0600}'..='\u{08FF}')
}
