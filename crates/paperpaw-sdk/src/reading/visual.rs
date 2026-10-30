//! Where the words of a line that is not left to right alone go
//! ([`LineBidi::REORDER`](crate::layout::LineBidi::REORDER)): its runs in
//! the order their levels give (UAX #9, rule L2), the words of a
//! right-to-left run from its right, each set with what lies around it
//! (Arabic letters joined across a change of style).

use alloc::vec::Vec;
use core::ops::Range;

use super::typeset::ReadingFaces;
use crate::lang::Direction;
use crate::layout::PageLine;
use crate::text::Around;
use crate::text::bidi::{Level, visual_order};

/// A word or a space of a line, where it is drawn.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Piece {
    /// The run it is of, by its place in the line's runs.
    pub run: usize,
    /// Its bytes of the line's text.
    pub range: Range<usize>,
    /// The word it is (a word going on in the next run is one), from 0
    /// in reading order; a space: the word before it.
    pub ordinal: usize,
    /// A space, not a word; `true` in `decorated` where a word follows
    /// it (lines under words go on under it).
    pub space: bool,
    pub decorated: bool,
    /// From the line's start ([`PageLine::x`]), and its width.
    pub x: f32,
    pub width: f32,
}

/// How a word of `line` at `range` of its run `run` is set: in its run's
/// direction and language, after and before the letters around it.
pub(super) fn around(line: &PageLine, run: usize, range: &Range<usize>) -> Around {
    let Some(run) = line.runs.get(run) else {
        return Around::NONE;
    };
    let base = match run.is_rtl() {
        true => Direction::Rtl,
        false => Direction::Ltr,
    };
    let not_space = |c: &char| *c != ' ';
    let before = line
        .text
        .get(..range.start)
        .and_then(|t| t.chars().next_back())
        .filter(not_space);
    let after = line
        .text
        .get(range.end..)
        .and_then(|t| t.chars().next())
        .filter(not_space);
    Around::new(base, run.lang).with_context(before, after)
}

/// The words and spaces of `line` in the order they are drawn from the
/// left, each where it goes.
pub(super) fn pieces(faces: &ReadingFaces<'_>, line: &PageLine) -> Vec<Piece> {
    // In reading order first, words counted as `words_in` counts them.
    let mut by_run: Vec<Vec<Piece>> = Vec::with_capacity(line.runs.len());
    let mut count = 0usize;
    let mut joined = false;
    for (r, run) in line.runs.iter().enumerate() {
        let mut parts = Vec::new();
        let mut at = run.start;
        let text = line.text.get(run.start..run.end).unwrap_or_default();
        for (i, part) in text.split(' ').enumerate() {
            if i > 0 {
                parts.push(Piece {
                    run: r,
                    range: at - 1..at,
                    ordinal: count.saturating_sub(1),
                    space: true,
                    decorated: !part.is_empty(),
                    x: 0.0,
                    width: faces.space(run.face) + line.word_spacing,
                });
                joined = false;
            }
            if !part.is_empty() {
                if !joined {
                    count += 1;
                }
                joined = true;
                let range = at..at + part.len();
                let width = faces.measure_around(run.face, part, &around(line, r, &range));
                parts.push(Piece {
                    run: r,
                    range,
                    ordinal: count - 1,
                    space: false,
                    decorated: false,
                    x: 0.0,
                    width,
                });
            }
            at += part.len() + 1;
        }
        by_run.push(parts);
    }
    // Then the runs in the order of their levels, a right-to-left one's
    // pieces from its end.
    let levels: Vec<Level> = line
        .runs
        .iter()
        .map(|run| Level::new(run.level).unwrap_or(Level::LTR))
        .collect();
    let mut order = alloc::vec![0u16; levels.len()];
    let shown = visual_order(&levels, &mut order);
    let mut placed = Vec::new();
    let mut x = 0.0;
    for &r in &order[..shown] {
        let r = usize::from(r);
        let Some(parts) = by_run.get_mut(r) else {
            continue;
        };
        if line.runs[r].is_rtl() {
            parts.reverse();
        }
        for mut piece in parts.drain(..) {
            piece.x = x;
            x += piece.width;
            placed.push(piece);
        }
    }
    placed
}
