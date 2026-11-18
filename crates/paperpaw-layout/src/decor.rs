//! The rules beside a block quote's lines and the box around an aside's (a
//! callout, a note shown in place): blocks of kind
//! [`BlockKind::Extended`] [`QUOTE`] and [`ASIDE`]. Their lines are set
//! in far enough for them; the rules are drawn once a page is laid out,
//! along the lines it kept, so they never change where a page breaks.

use alloc::vec::Vec;

use paperpaw_document::{Block, BlockKind};

use crate::{Page, PageRule};

/// The kind number of an aside: a callout, a sidebar, a note shown in
/// place, drawn in a box.
pub(crate) const ASIDE: u16 = 5;
/// The kind number of a paragraph of a block quote, drawn with a rule at
/// the start of its lines.
pub(crate) const QUOTE: u16 = 8;
/// The least room at a side of a quote's or an aside's lines that has a
/// rule, in ems.
const ROOM: f32 = 1.0;
/// How far a rule is from the lines beside it, in ems.
const GAP: f32 = 0.5;
/// The thickness of a rule, in pixels.
pub(crate) const RULE: f32 = 1.0;

/// What is drawn beside a block's lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Quote,
    Aside,
}

/// What `block` has drawn beside its lines, if anything.
pub(crate) fn kind(block: &Block) -> Option<Kind> {
    match block.kind {
        BlockKind::Extended(QUOTE) => Some(Kind::Quote),
        BlockKind::Extended(ASIDE) => Some(Kind::Aside),
        _ => None,
    }
}

/// The room at the start and the end of `block`'s lines, `(start, end)`,
/// with what its rules need, in a line `width` wide and ems of `em`.
pub(crate) fn inset(block: &Block, (start, end): (f32, f32), em: f32, width: f32) -> (f32, f32) {
    let room = (ROOM * em).min(width / 4.0);
    match kind(block) {
        Some(Kind::Quote) => (start.max(room), end),
        Some(Kind::Aside) => (start.max(room), end.max(room)),
        None => (start, end),
    }
}

/// A line of a quote or an aside on the page: its number among the page's
/// lines, its top and bottom, and where the rules at its left and right
/// are.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Mark {
    pub(crate) line: usize,
    pub(crate) kind: Kind,
    pub(crate) top: f32,
    pub(crate) bottom: f32,
    pub(crate) left: f32,
    pub(crate) right: f32,
}

/// Where the rules of `block`'s lines are, left and right, its lines set
/// from `start` (`measure.rest`) `available` wide in a line `width` wide,
/// starting at the right in a right-to-left paragraph; `None` for a block
/// without.
pub(crate) fn sides(
    block: &Block,
    (start, available): (f32, f32),
    (width, em, rtl): (f32, f32, bool),
) -> Option<(Kind, f32, f32)> {
    let kind = kind(block)?;
    let gap = GAP * em;
    // Where the lines start and end, from the left.
    let (left, right) = match rtl {
        true => (width - start - available, width - start),
        false => (start, start + available),
    };
    let clamp = |x: f32| x.min(width - RULE).max(0.0);
    let (left, right) = (clamp(left - gap), clamp(right + gap - RULE));
    Some(match (kind, rtl) {
        // A quote's rule is at the start of its lines.
        (Kind::Quote, true) => (kind, right, right),
        _ => (kind, left, right),
    })
}

/// The rules of the quotes and asides whose lines `marks` lists, along the
/// lines `page` kept: one rule beside lines that follow each other, one box
/// around an aside's.
pub(crate) fn rules(page: &mut Page, marks: &[Mark]) {
    let kept = page.lines.len();
    let mut group: Option<Mark> = None;
    for mark in marks.iter().filter(|m| m.line < kept) {
        group = match group {
            Some(open)
                if open.line + 1 == mark.line
                    && open.kind == mark.kind
                    && open.left == mark.left
                    && open.right == mark.right =>
            {
                Some(Mark {
                    top: open.top,
                    ..*mark
                })
            }
            Some(open) => {
                draw(&mut page.rules, open);
                Some(*mark)
            }
            None => Some(*mark),
        };
    }
    if let Some(open) = group {
        draw(&mut page.rules, open);
    }
}

/// The rules of the lines from `group.top` to `group.bottom`.
fn draw(rules: &mut Vec<PageRule>, group: Mark) {
    let height = (group.bottom - group.top).max(RULE);
    rules.push(PageRule::new(group.left, group.top, RULE, height));
    if group.kind == Kind::Aside {
        let width = group.right - group.left + RULE;
        rules.push(PageRule::new(group.right, group.top, RULE, height));
        rules.push(PageRule::new(group.left, group.top, width, RULE));
        rules.push(PageRule::new(
            group.left,
            group.top + height - RULE,
            width,
            RULE,
        ));
    }
}
