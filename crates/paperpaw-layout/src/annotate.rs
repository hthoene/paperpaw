//! A block's annotations set by its lines (ADR 0032): ruby and phonetics in
//! the room above a line, glosses in the room below it, each at the start of
//! its range on the line that has that start ([`Block::annotations`]).
//!
//! Every line of a block with annotations gets the same room, so the lines
//! keep one distance (an annotation without text is that room alone); an
//! annotation that would run into the one before it on the same side moves
//! on behind it, and what passes the line's measure goes on at its next
//! line's start (cut on the block's last line, and on the page's last: a
//! gloss does not go on over a page break). Lines that run right to left, go
//! down a column or need their runs put in order get none yet.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_document::{AnnotationKind, Block, MAX_LEADING_ABOVE, TextStyle};
use paperpaw_text::Around;

use crate::{Face, LayoutContext, LineAnnotation, LineBidi, LineRun, book_size};

/// An annotation's size where it names none: half of its text's.
const DEFAULT_SIZE: u16 = 50;
/// The sizes an annotation may take, in percent of its text's.
const SIZES: (u16, u16) = (20, 100);
/// The most annotations a block's lines look at, and the most one line sets.
const MOST: usize = 256;
const MOST_ON_LINE: usize = 32;

/// The room a block's annotations take above and below each of its lines.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rooms {
    above: f32,
    below: f32,
    em: f32,
}

impl Rooms {
    /// The room above and below each line, in pixels.
    pub(crate) fn room(&self) -> (f32, f32) {
        (self.above, self.below)
    }
}

/// Whether an annotation of `kind` goes under its line: a gloss (and a kind
/// a later build adds, shown as a gloss); ruby and phonetics go above it.
fn under(kind: AnnotationKind) -> bool {
    !matches!(kind, AnnotationKind::Ruby | AnnotationKind::Phonetic)
}

/// The size of an annotation that asks for `size`, in percent of its text.
fn percent(size: u16) -> u16 {
    if size == 0 {
        DEFAULT_SIZE
    } else {
        size.clamp(SIZES.0, SIZES.1)
    }
}

/// The face an annotation of `size` (percent of its text, 0 for the
/// default) of `block` is set in: italic for a gloss.
fn face(cx: &LayoutContext<'_>, block: &Block, kind: AnnotationKind, size: u16) -> Face {
    let size = percent(size);
    let text = book_size(block, cx.style).unwrap_or(100);
    let percent = (u32::from(text) * u32::from(size) / 100).clamp(1, u32::from(u16::MAX));
    let style = if under(kind) {
        TextStyle::ITALIC
    } else {
        TextStyle::REGULAR
    };
    Face::new(style, 0, percent as u16)
}

/// The room `block`'s annotations take, its text `em` pixels; `None` for a
/// block without any.
pub(crate) fn rooms(cx: &LayoutContext<'_>, block: &Block, em: f32) -> Option<Rooms> {
    if block.annotations.is_empty() {
        return None;
    }
    let mut rooms = Rooms {
        above: 0.0,
        below: 0.0,
        em,
    };
    for note in block.annotations.iter().take(MOST) {
        // Without text: room alone, `size` hundredths of an em.
        let height = if note.text.is_empty() {
            em * f32::from(note.size.min(MAX_LEADING_ABOVE)) / 100.0
        } else {
            cx.typesetter.line(face(cx, block, note.kind, note.size)).1
        };
        let side = if under(note.kind) {
            &mut rooms.below
        } else {
            &mut rooms.above
        };
        *side = side.max(height);
    }
    Some(rooms)
}

/// The rest of an annotation longer than the room left on its line: it goes
/// on under (or over) the next line of its block, from that line's start.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Pending {
    text: String,
    kind: AnnotationKind,
    size: u16,
}

/// The most rests carried to the next line.
const MOST_PENDING: usize = 8;

/// The annotations of `block` on its line of `range` (its text's bytes), set
/// at `x` with `word_spacing` as the line is: first the rests `carried` from
/// the line before, from the line's start, then those that start on it, each
/// where its range starts; above with their baselines at the bottom of the
/// room that ends at `above`, below in the room that starts at `below`. What
/// does not fit goes on as `carried` to the next line, at whole words; on the
/// `last` line of the block on its page it is cut.
pub(crate) fn place(
    cx: &LayoutContext<'_>,
    rooms: Option<&Rooms>,
    (block, range, runs, bidi): (&Block, core::ops::Range<usize>, &[LineRun], LineBidi),
    (x, word_spacing): (f32, f32),
    (above, below): (f32, f32),
    (carried, last): (&mut Vec<Pending>, bool),
) -> Vec<LineAnnotation> {
    let mut placed = Vec::new();
    let pending = core::mem::take(carried);
    let Some(rooms) = rooms else {
        return placed;
    };
    let flags = LineBidi::REORDER | LineBidi::VERTICAL;
    if bidi.is_rtl() || bidi.has(flags) {
        return placed;
    }
    let Some(text) = block.text.get(range.clone()) else {
        return placed;
    };
    let starting = block.annotations.iter().take(MOST).filter(|note| {
        let start = note.start as usize;
        range.contains(&start) && note.end > note.start && !note.text.is_empty()
    });
    let mut set = Setter {
        cx,
        block,
        rooms,
        line: (x, above, below, last),
        ends: [f32::NEG_INFINITY; 2],
        placed: &mut placed,
        carried,
    };
    for rest in &pending {
        set.one(&rest.text, (rest.kind, rest.size), x);
    }
    for note in starting {
        let at = x + offset(
            cx,
            text,
            runs,
            note.start as usize - range.start,
            word_spacing,
        );
        set.one(&note.text, (note.kind, note.size), at);
    }
    placed
}

/// What sets the annotations of one line.
struct Setter<'s, 'c> {
    cx: &'s LayoutContext<'c>,
    block: &'s Block,
    rooms: &'s Rooms,
    /// The line's left edge, the bottom of its room above, the top of its
    /// room below, and whether it is its block's last on the page.
    line: (f32, f32, f32, bool),
    /// Where the last annotation on each side ends: above, below.
    ends: [f32; 2],
    placed: &'s mut Vec<LineAnnotation>,
    carried: &'s mut Vec<Pending>,
}

impl Setter<'_, '_> {
    /// Sets `text` from `at` on, behind the one before it on its side.
    fn one(&mut self, text: &str, (kind, size): (AnnotationKind, u16), at: f32) {
        let (cx, (x, above, below, last)) = (self.cx, self.line);
        if self.placed.len() >= MOST_ON_LINE || self.placed.try_reserve(1).is_err() {
            return;
        }
        let face = face(cx, self.block, kind, size);
        let side = usize::from(under(kind));
        let gap = cx.typesetter.advance(face, ' ');
        let left = at.max(self.ends[side] + gap);
        let fresh = left <= x + 0.5;
        let (shown, rest) = split(cx, face, text, cx.width - left, fresh || last);
        if let Some(rest) = rest.filter(|r| !r.is_empty() && !last) {
            let mut kept = String::new();
            if self.carried.len() < MOST_PENDING && kept.try_reserve_exact(rest.len()).is_ok() {
                kept.push_str(rest);
                self.carried.push(Pending {
                    text: kept,
                    kind,
                    size,
                });
            }
        }
        let Some((shown, width)) = shown else {
            return;
        };
        self.ends[side] = left + width;
        let (ascent, height) = cx.typesetter.line(face);
        let baseline = if side == 1 {
            below + ascent
        } else {
            above - (height - ascent)
        };
        self.placed.push(LineAnnotation {
            x: left,
            width,
            baseline,
            text: shown,
            kind,
            size: self.rooms.em * f32::from(percent(size)) / 100.0,
        });
    }
}

/// `text` in `face` as far as it fits `room` pixels, at whole words, with
/// its width, and the rest. Where not one word fits, nothing is shown and all
/// of it is the rest, unless `cut`: then as many characters as fit.
fn split<'t>(
    cx: &LayoutContext<'_>,
    face: Face,
    text: &'t str,
    room: f32,
    cut: bool,
) -> (Option<(String, f32)>, Option<&'t str>) {
    let space = cx.typesetter.advance(face, ' ');
    let (mut end, mut width, mut at) = (0, 0.0, 0);
    for word in text.split(' ') {
        let wide = cx.typesetter.advance_text(face, word, &Around::NONE);
        let next = if at == 0 { wide } else { width + space + wide };
        if next > room {
            break;
        }
        (width, at) = (next, at + word.len() + 1);
        end = (at - 1).min(text.len());
    }
    if end == 0 && cut {
        // Not one word: as many characters as fit.
        let mut w = 0.0;
        for (i, c) in text.char_indices() {
            let next = w + cx.typesetter.advance(face, c);
            if next > room {
                break;
            }
            (w, end) = (next, i + c.len_utf8());
        }
        width = w;
    }
    if end == 0 {
        return (None, Some(text));
    }
    let mut shown = String::new();
    if shown.try_reserve_exact(end).is_err() {
        return (None, None);
    }
    shown.push_str(&text[..end]);
    let rest = text[end..].trim_start_matches(' ');
    (Some((shown, width)), (!rest.is_empty()).then_some(rest))
}

/// How far from the line's left edge byte `at` of its `text` is drawn: the
/// words before it with the line's gaps, as the reader sets them.
fn offset(
    cx: &LayoutContext<'_>,
    text: &str,
    runs: &[LineRun],
    at: usize,
    word_spacing: f32,
) -> f32 {
    let mut x = 0.0;
    for run in runs {
        let Some(part) = text.get(run.start..run.end.min(text.len())) else {
            continue;
        };
        let space = cx.typesetter.advance(run.face, ' ') + word_spacing;
        let mut here = run.start;
        for (n, word) in part.split(' ').enumerate() {
            if n > 0 {
                if here >= at {
                    return x;
                }
                x += space;
                here += 1;
            }
            if here >= at {
                return x;
            }
            if here + word.len() > at {
                let before = word.get(..at - here).unwrap_or(word);
                return x + cx.typesetter.advance_text(run.face, before, &Around::NONE);
            }
            x += cx.typesetter.advance_text(run.face, word, &Around::NONE);
            here += word.len();
        }
    }
    x
}
