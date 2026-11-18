//! Breaking a block's text into lines, each set where its measure says.

use alloc::vec::Vec;

use paperpaw_document::{Block, Direction};
use paperpaw_text::{Around, Line, wrap_lines_balanced_opts, wrap_lines_kerned_opts};

use crate::drop_cap::{self, Cap};
use crate::levels::{self, BlockLevels};
use crate::measure::Measure;
use crate::{Face, LayoutContext, face_at};

/// Lines of a block's text and where each is set: its start and width.
pub(crate) type SetLines = Vec<(Line, (f32, f32))>;

/// The lines of `block` from byte `from` of its text on (a line's start),
/// their ranges in its text, and where they are set: broken line by line
/// from there, or (balanced) over the whole block, from where it starts
/// (`indent` is its first line's), and those from `from` on taken; and
/// its drop capital, if it has one (its lines are broken over the whole
/// block too, beside it).
#[allow(
    clippy::too_many_arguments,
    reason = "what a block's lines need, each of it different; a struct would be built for one call"
)]
pub(crate) fn block_lines(
    block: &Block,
    from: usize,
    base: Face,
    cx: &LayoutContext<'_>,
    heading: u8,
    indent: f32,
    justify_limit: Option<f32>,
    levels: &BlockLevels,
) -> (SetLines, Option<Cap>) {
    let (style, width) = (cx.style, cx.width);
    let advance = |i: usize, c: char| advance_at(block, base, cx, levels, i, c);
    let kern = |i: usize, left: char, right: char| kern_at(block, base, cx, levels, i, left, right);
    let hyphenate = cx.hyphenate.filter(|_| heading == 0);
    // Broken from `from` on, or (balanced, or beside a drop capital) over
    // the whole block, its line `n` set where `measure` says.
    let lay = |cap: Option<Cap>| -> SetLines {
        // The capital takes no room in the text: its lines' measure has it.
        let skip = cap.map_or(0, |cap| cap.len);
        let advance = |i: usize, c: char| if i < skip { 0.0 } else { advance(i, c) };
        let kern = |i: usize, left: char, right: char| match i <= skip {
            true => 0.0,
            false => kern(i, left, right),
        };
        if style.balanced || cap.is_some() {
            // A drop capital takes the indent's place.
            let indent = if cap.is_some() { 0.0 } else { indent };
            let measure = Measure::of(block, style, width, indent).beside(cap);
            let lines = match style.balanced {
                true => wrap_lines_balanced_opts(
                    &block.text,
                    |n| measure.line(n).1,
                    advance,
                    kern,
                    hyphenate,
                    |free, spaces| justification_cost(free, spaces, justify_limit),
                    style.script_options.bits(),
                ),
                false => wrap_lines_kerned_opts(
                    &block.text,
                    |n| measure.line(n).1,
                    advance,
                    kern,
                    hyphenate,
                    style.script_options.bits(),
                ),
            };
            return lines
                .into_iter()
                .enumerate()
                .map(|(n, line)| (line, measure.line(n)))
                .collect();
        }
        // A page that goes on with a block sets its lines as wide as those
        // after a first line.
        let measure = match from == 0 && !block.continues {
            true => Measure::of(block, style, width, indent),
            false => Measure::of(block, style, width, 0.0).continued(),
        };
        let lines = wrap_lines_kerned_opts(
            &block.text[from..],
            |n| measure.line(n).1,
            |i, c| advance(from + i, c),
            |i, left, right| kern(from + i, left, right),
            hyphenate,
            style.script_options.bits(),
        );
        lines
            .into_iter()
            .enumerate()
            .map(|(n, mut line)| {
                line.range = from + line.range.start..from + line.range.end;
                (line, measure.line(n))
            })
            .collect()
    };
    // A drop capital kept to half a line's width.
    let half = Measure::of(block, style, width, 0.0).rest.1 / 2.0;
    let capital = |lines: u8| drop_cap::of(cx, block, base, heading, levels, lines, half);
    let (mut lines, cap) = drop_cap::fitted(lay, capital, block.drop_cap);
    if style.balanced || cap.is_some() {
        lines.retain(|(line, _)| line.range.start >= from);
    }
    (lines, cap)
}

/// The advance of `c` at byte `i` of `block`'s text: letters that join are
/// as wide as the form they take between their neighbours (marks left out),
/// a Bengali or Devanagari word as wide as it is drawn (on its first
/// character), as drawing sets them.
pub(crate) fn advance_at(
    block: &Block,
    base: Face,
    cx: &LayoutContext<'_>,
    levels: &BlockLevels,
    i: usize,
    c: char,
) -> f32 {
    if paperpaw_text::is_invisible(c) {
        // Direction controls an EPUB sets text apart with.
        return 0.0;
    }
    let face = face_at(block, base, i, cx.style.book);
    // A Bengali or Devanagari word: as wide as drawn, shaped, on its first
    // character (its syllables take forms by those around them).
    if matches!(c, '\u{900}'..='\u{9FF}') {
        let part = paperpaw_text::shaped_at(&block.text, i);
        if part.start != i {
            return 0.0;
        }
        let around = Around::new(Direction::Ltr, block.lang_of(face.style));
        let word = block.text.get(part).unwrap_or_default();
        return cx.typesetter.advance_text(face, word, &around);
    }
    if levels.is_plain() || !levels::may_join(c) {
        return cx.typesetter.advance(face, c);
    }
    let not_mark = |c: &char| !paperpaw_text::is_mark(*c);
    let before = block
        .text
        .get(..i)
        .and_then(|t| t.chars().rev().find(not_mark));
    let after = block
        .text
        .get(i + c.len_utf8()..)
        .and_then(|t| t.chars().find(not_mark));
    let around = Around::new(Direction::Rtl, block.lang_of(face.style)).with_context(before, after);
    let mut letter = [0u8; 4];
    cx.typesetter
        .advance_text(face, c.encode_utf8(&mut letter), &around)
}

/// How a pair in one face moves: the character before byte `i` of
/// `block`'s text and `right`, the text's own there or a hyphen set like
/// the character before it. Pairs that run right to left are not kerned.
pub(crate) fn kern_at(
    block: &Block,
    base: Face,
    cx: &LayoutContext<'_>,
    levels: &BlockLevels,
    i: usize,
    left: char,
    right: char,
) -> f32 {
    if levels.is_rtl_at(i) || levels.is_rtl_at(i - left.len_utf8()) {
        return 0.0;
    }
    // Shaped syllables are not kerned (see `advance_at`).
    if [left, right]
        .iter()
        .any(|c| matches!(c, '\u{900}'..='\u{9FF}'))
    {
        return 0.0;
    }
    let book = cx.style.book;
    let face = face_at(block, base, i - left.len_utf8(), book);
    let own = block.text[i..].starts_with(right);
    if own && face_at(block, base, i, book) != face {
        return 0.0;
    }
    cx.typesetter.kern(face, left, right)
}

/// Prefer gaps that can take the spare width, while retaining the
/// ordinary paragraph and hyphen costs. Beyond the drawing limit, the
/// undistributed width also counts: a shorter line should be exceptional.
fn justification_cost(free: f32, spaces: usize, limit: Option<f32>) -> f32 {
    let Some(limit) = limit.filter(|_| spaces > 0) else {
        return 0.0;
    };
    let extra = free / spaces as f32;
    let left = (free - spaces as f32 * limit).max(0.0);
    extra * extra + left * left
}
