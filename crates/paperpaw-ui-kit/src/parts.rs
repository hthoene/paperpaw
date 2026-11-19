//! The small parts every screen draws the same way: a progress bar, a pill, a
//! cover's placeholder and a checkbox.
//!
//! Each takes the [`Style`] and the room it is drawn in and queues its
//! [`Paint`]; none keeps state, and none allocates beyond the paint list.
//! They are the one look of these parts: a screen that draws its own variant
//! shows the user two looks for one thing.
use crate::{
    Paint, Style,
    geometry::{clamp_u16, line_px},
    paint,
    widget::{
        Align,
        shape::{fill_circle, line as stroke_line, stroke_circle},
        text::Wrapped,
    },
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Point, Rect,
    native::{Error, fonts::Font},
};

/// The height of a [`progress_bar`] in `style`: twice the line, at least 4.
#[must_use]
pub fn bar_height(style: &Style) -> u16 {
    style.line.max(2).saturating_mul(2)
}

/// A bar `permille` thousandths full in `rect`: a track in the style's rule
/// colour and, from its start, the part done in `ink`. Both have the style's
/// corners (the ends of a pill theme are round), the part done never rounder
/// than it is wide.
///
/// # Errors
/// [`Error::NoMemory`] when the paint list cannot grow.
pub fn progress_bar(
    out: &mut Vec<Paint>,
    style: &Style,
    rect: Rect,
    permille: u16,
    ink: Color,
) -> Result<(), Error> {
    if rect.width == 0 || rect.height == 0 {
        return Ok(());
    }
    let radius = if style.pill {
        u32::from(rect.height) / 2
    } else {
        style.corner.min(u32::from(rect.height) / 2)
    };
    paint::push(out, Paint::Focus(rect, radius, None, style.rule))?;
    let done = u32::from(rect.width) * u32::from(permille.min(1000)) / 1000;
    let done = clamp_u16(i64::from(done));
    if done > 0 {
        let part = Rect::new(rect.x, rect.y, done, rect.height);
        paint::push(
            out,
            Paint::Focus(part, radius.min(u32::from(done) / 2), None, ink),
        )?;
    }
    Ok(())
}

/// The width a [`pill`] with `text` needs in `style`: the text and a gap on
/// each side.
///
/// # Errors
/// A refused font reply.
pub fn pill_width(font: &Font<'_>, style: &Style, text: &str) -> Result<u16, Error> {
    let wide = font.measure_text(style.small, text)?;
    Ok(clamp_u16((wide + f32::from(style.gap) * 2.0 + 1.0) as i64))
}

/// A tag: `text` in small letters in a capsule round as high as `at`, an
/// outline in `ink`, or filled with `ink` and the text in the paper's colour
/// where `filled` (a chosen or active tag). The text is shortened to fit.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn pill(
    out: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    at: Rect,
    text: &str,
    (ink, filled): (Color, bool),
) -> Result<(), Error> {
    let radius = u32::from(at.height) / 2;
    let (shape, tone) = if filled {
        (None, style.paper)
    } else {
        (Some(u32::from(style.line).max(1)), ink)
    };
    paint::push(out, Paint::Focus(at, radius, shape, ink))?;
    let room = at.width.saturating_sub(style.gap);
    let shown = crate::text::fit(font, style.small, text, f32::from(room))?;
    let wide = font.measure_text(style.small, &shown)?;
    let x = f32::from(at.x) + (f32::from(at.width) - wide).max(0.0) / 2.0;
    crate::text::label(out, font, &shown, style.small, x, at, tone, Some(at))
}

/// The first letter of `title` in capitals, up to three characters of what
/// it uppercases to: what stands for a book with a title too long for a
/// small place.
fn initial(title: &str) -> Result<String, Error> {
    let mut letter = String::new();
    letter.try_reserve(12).map_err(|_| Error::NoMemory)?;
    if let Some(c) = title.chars().find(|c| c.is_alphanumeric()) {
        for upper in c.to_uppercase().take(3) {
            letter.push(upper);
        }
    }
    Ok(letter)
}

/// What stands in the place of a cover the book has none of (or not yet): a
/// frame with the style's corners and, in it, the title in small letters
/// broken between words as far as it fits (the last line ends in "…"); where
/// the place is too small for words, the title's first letter.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn placeholder(
    out: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    at: Rect,
    title: &str,
    ink: Color,
) -> Result<(), Error> {
    let radius = style.corner.min(u32::from(at.width.min(at.height)) / 8);
    paint::push(
        out,
        Paint::Focus(at, radius, Some(u32::from(style.line).max(2)), ink),
    )?;
    let pad = style.gap / 2;
    let (width, height) = (
        at.width.saturating_sub(2 * pad),
        at.height.saturating_sub(2 * pad),
    );
    if f32::from(width) >= style.small * 2.0 && height >= line_px(font, style.small)? {
        let words = Wrapped::new(font, title, style.small, (width, height), false)?;
        let room = Rect::new(
            at.x.saturating_add(pad),
            at.y.saturating_add(pad),
            width,
            words.height(font)?,
        );
        words.draw(out, font, room, Align::Left, ink)?;
        return Ok(());
    }
    let letter = initial(title)?;
    if letter.is_empty() {
        return Ok(());
    }
    let size = style.title * 1.3;
    let wide = font.measure_text(size, &letter)?;
    let x = f32::from(at.x) + (f32::from(at.width) - wide) / 2.0;
    crate::text::label(out, font, &letter, size, x, at, ink, Some(at))
}

/// The shape of a [`checkbox`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BoxShape {
    /// A circle (a to-do's tick).
    Round,
    /// A square with the corners of a third of its radius.
    Square,
}

/// The colours of a [`checkbox`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct BoxTones {
    /// The outline of an empty box.
    pub outline: Color,
    /// What a ticked box is filled with.
    pub fill: Color,
    /// The tick in it.
    pub tick: Color,
}

impl BoxTones {
    /// The colours: the outline, the fill of a ticked box and its tick.
    #[must_use]
    pub const fn new(outline: Color, fill: Color, tick: Color) -> Self {
        Self {
            outline,
            fill,
            tick,
        }
    }
}

/// A box in `place` (square: the box is as wide as `place`, as high as its
/// width): an outline, or filled and ticked where `checked`.
///
/// # Errors
/// [`Error::NoMemory`] when the paint list cannot grow.
pub fn checkbox(
    out: &mut Vec<Paint>,
    style: &Style,
    place: Rect,
    shape: BoxShape,
    checked: bool,
    tones: BoxTones,
) -> Result<(), Error> {
    let line = (u32::from(place.width) / 10)
        .max(u32::from(style.line))
        .max(1);
    let r = u32::from(place.width / 2);
    let radius = i32::try_from(r).unwrap_or(0);
    let centre = Point::new(
        i32::from(place.x).saturating_add(radius),
        i32::from(place.y).saturating_add(radius),
    );
    match (shape, checked) {
        (BoxShape::Round, false) => stroke_circle(out, centre, r, line, tones.outline)?,
        (BoxShape::Round, true) => fill_circle(out, centre, r, tones.fill)?,
        (_, false) => paint::push(out, Paint::Focus(place, r / 3, Some(line), tones.outline))?,
        (_, true) => paint::push(out, Paint::Focus(place, r / 3, None, tones.fill))?,
    }
    if checked {
        let s = i32::from(place.width);
        let (x, y) = (i32::from(place.x), i32::from(place.y));
        let width = (s as u32 / 9).max(2);
        let a = Point::new(x + s * 28 / 100, y + s * 50 / 100);
        let b = Point::new(x + s * 44 / 100, y + s * 66 / 100);
        let c = Point::new(x + s * 72 / 100, y + s * 36 / 100);
        stroke_line(out, a, b, width, tones.tick)?;
        stroke_line(out, b, c, width, tones.tick)?;
    }
    Ok(())
}
