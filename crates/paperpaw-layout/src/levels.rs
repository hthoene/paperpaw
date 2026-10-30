//! Bidirectional levels of a block's text (UAX #9) for its lines: runs
//! split where the level changes, lines of right-to-left paragraphs set
//! from the right, letters that join measured in the forms they take.

use alloc::vec::Vec;
use core::ops::Range;

use paperpaw_document::{Block, Direction, Locator};
use paperpaw_text::bidi::{self, Level};

use crate::LayoutContext;

/// Pieces a walk to the start of a paragraph passes at most (about
/// 256 KB of text); one longer resolves the pieces past it as if they
/// started it.
const MAX_WALK: usize = 64;

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
        let paragraph = paragraph_of(block);
        if !paragraph.is_rtl() && !bidi::needs_bidi(&block.text) {
            return BlockLevels::plain(paragraph);
        }
        let mut chars = alloc::vec![paragraph; block.text.chars().count()];
        bidi::resolve(&block.text, paragraph, &mut chars);
        BlockLevels::by_byte(block, paragraph, chars)
    }

    /// The levels of `block`, of `section` of `cx`'s document, as one
    /// piece of its paragraph where it is (a long paragraph split in
    /// pieces): from the state at its start, which `cx` keeps (see
    /// [`PieceStarts`](crate::PieceStarts)) or a walk from the
    /// paragraph's start finds, and with the next piece read ahead. The
    /// state at the start of the next piece is kept.
    pub(crate) fn in_context(block: &Block, cx: &LayoutContext<'_>, section: u32) -> BlockLevels {
        let own = paragraph_of(block);
        if !own.is_rtl() && !bidi::needs_bidi(&block.text) {
            return BlockLevels::plain(own);
        }
        let next = next_piece(block, cx, section);
        if !block.continues && next.is_none() {
            return BlockLevels::of(block);
        }
        let mut state = alloc::vec![0u8; bidi::state_len()];
        let found = block.continues && start_of(block, cx, section, &mut state);
        if !found {
            bidi::start_state(own, &mut state);
        }
        let paragraph = bidi::state_paragraph(&state).unwrap_or(own);
        let count = block.text.chars().count();
        let ahead = next.as_ref().map_or("", |n| n.text.as_str());
        let rest = block.text.chars().chain(ahead.chars());
        let mut chars = alloc::vec![paragraph; count];
        bidi::resolve_from(&mut state, paragraph, rest, count, &mut chars);
        if let (Some(next), Some(pieces)) = (&next, cx.pieces) {
            pieces.put(section, next.start, &state);
        }
        BlockLevels::by_byte(block, paragraph, chars)
    }

    fn plain(paragraph: Level) -> BlockLevels {
        BlockLevels {
            paragraph,
            by_byte: Vec::new(),
        }
    }

    fn by_byte(block: &Block, paragraph: Level, chars: Vec<Level>) -> BlockLevels {
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

/// A block's paragraph level: as the book says, else from its first
/// strong letter (rules P2 and P3), else left to right.
fn paragraph_of(block: &Block) -> Level {
    match block.dir {
        Direction::Rtl => Level::RTL,
        Direction::Ltr => Level::LTR,
        _ => bidi::paragraph_level(&block.text).unwrap_or(Level::LTR),
    }
}

/// The piece after `block` of its paragraph, if it has one: what the
/// levels near its end read ahead, as far as that piece's end.
fn next_piece(block: &Block, cx: &LayoutContext<'_>, section: u32) -> Option<Block> {
    if !block.continued {
        return None;
    }
    let at = Locator::new(section, block.end);
    let next = cx.document.block_at(cx.storage, at).ok().flatten()?;
    (next.continues && next.start >= block.end).then_some(next)
}

/// Writes the state at the start of `block`, a piece that continues its
/// paragraph, into `state`: the one kept, else found by walking back to
/// a kept one or the paragraph's start and on again, keeping the starts
/// passed. False where the walk fails (a document that cannot be read,
/// a paragraph of more than [`MAX_WALK`] pieces).
fn start_of(block: &Block, cx: &LayoutContext<'_>, section: u32, state: &mut [u8]) -> bool {
    if cx
        .pieces
        .is_some_and(|p| p.get(section, block.start, state))
    {
        return true;
    }
    // Back: the starts of the pieces before, nearest first.
    let mut starts: Vec<u32> = Vec::new();
    let mut at = block.start;
    let found = loop {
        if starts.len() >= MAX_WALK || at == 0 {
            return false;
        }
        let Ok(Some(piece)) = cx
            .document
            .block_at(cx.storage, Locator::new(section, at - 1))
        else {
            return false;
        };
        if piece.start >= at || piece.end > at {
            return false;
        }
        starts.push(piece.start);
        if cx
            .pieces
            .is_some_and(|p| p.get(section, piece.start, state))
        {
            break true;
        }
        if !piece.continues {
            break bidi::start_state(paragraph_of(&piece), state);
        }
        at = piece.start;
    };
    if !found {
        return false;
    }
    // On: each piece read once, with the one after it read ahead.
    let paragraph = bidi::state_paragraph(state).unwrap_or(Level::LTR);
    let Some(&first) = starts.last() else {
        return false;
    };
    let Ok(Some(mut piece)) = cx
        .document
        .block_at(cx.storage, Locator::new(section, first))
    else {
        return false;
    };
    for &next_start in starts
        .iter()
        .rev()
        .skip(1)
        .chain(core::iter::once(&block.start))
    {
        let next = if next_start == block.start {
            None
        } else {
            match cx
                .document
                .block_at(cx.storage, Locator::new(section, next_start))
            {
                Ok(Some(next)) => Some(next),
                _ => return false,
            }
        };
        // Read ahead as a page that starts in the piece reads it.
        let ahead = match (&next, piece.continued) {
            (_, false) => "",
            (Some(next), true) => next.text.as_str(),
            (None, true) => block.text.as_str(),
        };
        let count = piece.text.chars().count();
        bidi::skip_from(
            state,
            paragraph,
            piece.text.chars().chain(ahead.chars()),
            count,
        );
        if let Some(pieces) = cx.pieces {
            pieces.put(section, next_start, state);
        }
        match next {
            Some(next) => piece = next,
            None => break,
        }
    }
    true
}
