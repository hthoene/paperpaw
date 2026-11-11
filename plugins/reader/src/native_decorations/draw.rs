//! A page's marks drawn: backgrounds behind its text, lines and frames over
//! it, each over the word boxes a mark covers joined on a line. The first
//! subscriber's look wins where marks meet: marks are drawn last to first.
use paperpaw_sdk::{
    Color, decorations::LineKind, document::LineStyle, native::Error, reading::WordBox,
};

use super::{Covered, Decorations, Look};
use crate::native_draw::Ink;
use crate::native_marks_codes as codes;
use crate::native_typeset::Role;

/// Where the marks are drawn: rectangles in a colour.
pub trait Paint {
    /// Fills the rectangle at `(x, y)`, `width` by `height` pixels.
    ///
    /// # Errors
    /// The host's refusal.
    fn fill(&mut self, x: i32, y: i32, width: u32, height: u32, color: Color) -> Result<(), Error>;
}

/// The marks' backgrounds, drawn before the page's text; the word boxes
/// moved by `(dx, dy)` (the frame's corner).
///
/// # Errors
/// What `paint` refuses.
pub fn draw_behind(
    paint: &mut impl Paint,
    decorations: &Decorations,
    words: &[WordBox],
    (dx, dy): (i32, i32),
) -> Result<(), Error> {
    for (look, covered) in decorations.marked(words).rev() {
        let Some(tone) = codes::tone(u32::from(look.background)) else {
            continue;
        };
        for (x, y, width, height) in lines(&covered) {
            paint.fill(x + dx, y + dy, width, height, tone.color())?;
        }
    }
    Ok(())
}

/// The marks' lines, struck lines and frames, drawn after the page's text;
/// `dpi` sizes a line's thickness.
///
/// # Errors
/// What `paint` refuses.
pub fn draw_over(
    paint: &mut impl Paint,
    decorations: &Decorations,
    words: &[WordBox],
    (dx, dy): (i32, i32),
    dpi: u16,
) -> Result<(), Error> {
    for (look, covered) in decorations.marked(words).rev() {
        let flags = u32::from(look.flags);
        let line = codes::line(u32::from(look.line));
        if line.is_none() && flags & (codes::FRAME | codes::STRUCK) == 0 {
            continue;
        }
        let color = codes::tone(u32::from(look.line_tone)).map_or(Color::BLACK, |t| t.color());
        let mut ink = Tinted { paint, color };
        for (x, y, width, height) in lines(&covered) {
            let (x, y) = (x + dx, y + dy);
            let thick = thickness(look, height, dpi);
            if let Some(kind) = line {
                let under = y + height as i32 - thick as i32;
                crate::native_decor::line_at(&mut ink, style(kind), (x, width), under, thick)?;
            }
            if flags & codes::STRUCK != 0 {
                ink.fill(x, y + (height / 2) as i32, width, thick)?;
            }
            if flags & codes::FRAME != 0 {
                ink.fill(x, y, width, thick)?;
                ink.fill(x, y + height as i32 - thick as i32, width, thick)?;
                ink.fill(x, y, thick, height)?;
                ink.fill(x + width as i32 - thick as i32, y, thick, height)?;
            }
        }
    }
    Ok(())
}

/// A line's thickness in pixels: the look's tenths of a millimetre, else a
/// sixteenth of the line's height; at least one.
fn thickness(look: &Look, height: u32, dpi: u16) -> u32 {
    let px = if look.thickness == 0 {
        height / 16
    } else {
        u32::from(look.thickness) * u32::from(dpi) / 254
    };
    px.max(1)
}

const fn style(kind: LineKind) -> LineStyle {
    match kind {
        LineKind::Double => LineStyle::Double,
        LineKind::Dotted | LineKind::Dashed => LineStyle::Dotted,
        LineKind::Wavy => LineStyle::Wavy,
        _ => LineStyle::Solid,
    }
}

/// The boxes of the covered words joined where they share a line:
/// `(x, y, width, height)`.
fn lines<'a>(covered: &'a Covered<'a>) -> impl Iterator<Item = (i32, i32, u32, u32)> + 'a {
    let mut words = covered.words().peekable();
    core::iter::from_fn(move || {
        let first = words.next()?;
        let a = first.area;
        let (y, height) = (a.y, a.height);
        let (mut left, mut right) = (a.x, a.x.saturating_add(a.width));
        while let Some(next) = words.peek() {
            let b = next.area;
            if b.y != y || b.height != height {
                break;
            }
            left = left.min(b.x);
            right = right.max(b.x.saturating_add(b.width));
            words.next();
        }
        Some((
            i32::from(left),
            i32::from(y),
            u32::from(right - left),
            u32::from(height),
        ))
    })
}

/// [`Paint`] in one colour as the reader's [`Ink`] (for its lines).
struct Tinted<'p, P> {
    paint: &'p mut P,
    color: Color,
}

impl<P: Paint> Ink for Tinted<'_, P> {
    fn run(&mut self, _: Role, _: f32, _: f32, _: i32, _: &str) -> Result<f32, Error> {
        Ok(0.0)
    }

    fn fill(&mut self, x: i32, y: i32, width: u32, height: u32) -> Result<(), Error> {
        self.paint.fill(x, y, width, height, self.color)
    }

    fn image(&mut self, _: u32, _: i32, _: i32, _: u32, _: u32) -> Result<(), Error> {
        Ok(())
    }
}
