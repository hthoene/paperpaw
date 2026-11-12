//! A page's marks drawn as Beta 3 drew decorations, through the reader's own
//! mark drawing (`native_marks`: shading on panels of few grays, half a
//! millimetre round a background or frame, a default thickness of four
//! tenths of a millimetre): backgrounds behind the text, lines, struck lines
//! and frames over it, over the word boxes a mark covers joined on a line.
//! The first subscriber's look wins where marks meet: marks are drawn last
//! to first.
use paperpaw_sdk::{Rect, decorations::Decoration, native::Error, reading::WordBox};

use super::{Covered, Decorations, Look};
use crate::native_marks::{self, Gauge, Pen};
use crate::native_marks_codes as codes;

/// The marks' backgrounds, drawn before the page's text; the word boxes
/// moved by `at` (the frame's corner).
///
/// # Errors
/// What `pen` refuses.
pub fn draw_behind(
    pen: &mut impl Pen,
    gauge: Gauge,
    decorations: &Decorations,
    words: &[WordBox],
    at: (i32, i32),
) -> Result<(), Error> {
    for (look, covered) in decorations.marked(words).rev() {
        let deco = decoration(look);
        if deco.background.is_none() {
            continue;
        }
        for line in lines(&covered, at) {
            native_marks::draw_behind(pen, gauge, core::slice::from_ref(&line), &deco)?;
        }
    }
    Ok(())
}

/// The marks' lines, struck lines and frames, drawn after the page's text.
///
/// # Errors
/// What `pen` refuses.
pub fn draw_over(
    pen: &mut impl Pen,
    gauge: Gauge,
    decorations: &Decorations,
    words: &[WordBox],
    at: (i32, i32),
) -> Result<(), Error> {
    for (look, covered) in decorations.marked(words).rev() {
        let deco = decoration(look);
        if deco.line.is_none() && !deco.frame && !deco.struck {
            continue;
        }
        for line in lines(&covered, at) {
            native_marks::draw_over(pen, gauge, core::slice::from_ref(&line), &deco)?;
        }
    }
    Ok(())
}

/// A look as the SDK's decoration, by the stage's fixed codes.
#[must_use]
pub fn decoration(look: &Look) -> Decoration {
    let mut deco = Decoration::NONE.thickness(look.thickness);
    if let Some(tone) = codes::tone(u32::from(look.background)) {
        deco = deco.background(tone);
    }
    if let Some(tone) = codes::tone(u32::from(look.ink)) {
        deco = deco.ink(tone);
    }
    if let Some(kind) = codes::line(u32::from(look.line)) {
        deco = deco.line(kind);
    }
    if let Some(tone) = codes::tone(u32::from(look.line_tone)) {
        deco = deco.line_tone(tone);
    }
    let flags = u32::from(look.flags);
    if flags & codes::FRAME != 0 {
        deco = deco.frame();
    }
    if flags & codes::STRUCK != 0 {
        deco = deco.struck();
    }
    deco
}

/// The boxes of the covered words joined where they share a line, moved by
/// `at`.
fn lines<'a>(covered: &'a Covered<'a>, (dx, dy): (i32, i32)) -> impl Iterator<Item = Rect> + 'a {
    let mut words = covered.words().peekable();
    core::iter::from_fn(move || {
        let first = words.next()?;
        let a = first.area;
        let (mut left, mut right) = (a.x, a.x.saturating_add(a.width));
        while let Some(next) = words.peek() {
            let b = next.area;
            if b.y != a.y || b.height != a.height {
                break;
            }
            left = left.min(b.x);
            right = right.max(b.x.saturating_add(b.width));
            words.next();
        }
        Some(Rect::new(
            moved(left, dx),
            moved(a.y, dy),
            right - left,
            a.height,
        ))
    })
}

fn moved(at: u16, by: i32) -> u16 {
    u16::try_from((i32::from(at) + by).max(0)).unwrap_or(u16::MAX)
}
