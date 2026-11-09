//! Named icons: looked up in the host (`icon.glyph`) and drawn with the bound
//! icon font as [`Paint::Icon`] runs.
use crate::{Paint, geometry, paint, widget::text::largest_size};
use alloc::vec::Vec;
use paperpaw_sdk::{
    Color, Rect,
    native::{
        Error, Host,
        fonts::{Font, IconGlyph, TextOrigin},
    },
};

/// The key of the icon font: the font the reader bound to the icons' role.
pub const ROLE: &str = "role:icons";

/// The character the host has for the icon `name`; `None` where it has none (no
/// icon font is bound, the set lacks the name, the slot is not wired): the
/// caller then shows words instead.
///
/// # Errors
/// [`Error::NoMemory`] only; every other refusal is "no icon".
pub fn glyph(host: &Host<'_>, name: &str) -> Result<Option<IconGlyph>, Error> {
    match IconGlyph::named(host, name) {
        Ok(glyph) => Ok(Some(glyph)),
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Err(_) => Ok(None),
    }
}

/// The icon font for a draw, or `None` where none is bound.
///
/// # Errors
/// [`Error::NoMemory`] only; every other refusal is "no icon font".
pub fn font<'a>(host: &'a Host<'a>) -> Result<Option<Font<'a>>, Error> {
    match Font::acquire(host, ROLE) {
        Ok(font) => Ok(Some(font)),
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Err(_) => Ok(None),
    }
}

/// How large icons are in a place and where their ink is: every icon in the
/// same frame (the ink of a circle, which fills the icons' grid), so that names
/// under icons side by side line up.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Frame {
    /// The size the icons are drawn at.
    pub size: f32,
    /// The frame's top above the baseline (negative) and its bottom.
    pub top: i32,
    pub bottom: i32,
}

impl Frame {
    /// The height of the frame.
    #[must_use]
    pub fn height(&self) -> i32 {
        self.bottom.saturating_sub(self.top)
    }
}

/// The frame of icons in `room` pixels of height: as large as `title * 1.6`
/// allows, down to half of that, the circle's ink (or the icon's own where the
/// font has no circle) within `room`.
///
/// # Errors
/// Font refusals.
pub fn frame(
    icons: &Font<'_>,
    (circle, own): (Option<&IconGlyph>, &IconGlyph),
    title: f32,
    room: f32,
) -> Result<Frame, Error> {
    let reference = circle.unwrap_or(own).as_str();
    let ink = |size: f32| -> Result<Option<(i32, i32)>, Error> {
        match icons.ink(size, reference)? {
            Some(ink) => geometry::check_ink(ink.top, ink.bottom).map(Some),
            None => Ok(None),
        }
    };
    let largest = (title * 1.6).min(room);
    let size = largest_size(largest, largest * 0.5, |size| {
        Ok(ink(size)?.is_none_or(|(top, bottom)| (bottom - top) as f32 <= room))
    })?
    .unwrap_or(largest * 0.5);
    let (top, bottom) = ink(size)?.unwrap_or((-(size as i32), 0));
    Ok(Frame { size, top, bottom })
}

/// Draws `glyph` in `frame` with the middle of its ink at `middle` and the top
/// of the frame at `y`; where its ink is.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn draw(
    out: &mut Vec<Paint>,
    icons: &Font<'_>,
    glyph: &IconGlyph,
    frame: &Frame,
    (middle, y): (f32, i32),
    color: Color,
) -> Result<Rect, Error> {
    // Centred by its ink, as icons are everywhere.
    let (first, last) = match icons.ink(frame.size, glyph.as_str())? {
        Some(ink) => (ink.left, ink.right),
        None => (0, icons.measure_text(frame.size, glyph.as_str())? as i32),
    };
    let x = middle - (first.saturating_add(last)) as f32 / 2.0;
    paint::push(
        out,
        Paint::Icon {
            glyph: *glyph,
            size: frame.size,
            origin: TextOrigin::new(x, y.saturating_sub(frame.top)),
            color,
        },
    )?;
    Ok(Rect::new(
        geometry::clamp_u16(i64::from(x as i32 + first)),
        geometry::clamp_u16(i64::from(y)),
        geometry::clamp_u16(i64::from(last.saturating_sub(first))),
        geometry::clamp_u16(i64::from(frame.height())),
    ))
}

/// Draws `glyph` at `size` centred in `area` by its ink, both ways.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn draw_centred(
    out: &mut Vec<Paint>,
    icons: &Font<'_>,
    glyph: &IconGlyph,
    size: f32,
    area: Rect,
    color: Color,
) -> Result<(), Error> {
    let baseline = crate::widget::text::ink_baseline(icons, size, glyph.as_str(), area)?;
    let (first, last) = match icons.ink(size, glyph.as_str())? {
        Some(ink) => (ink.left, ink.right),
        None => (0, icons.measure_text(size, glyph.as_str())? as i32),
    };
    let middle = f32::from(area.x) + f32::from(area.width) / 2.0;
    let x = middle - (first.saturating_add(last)) as f32 / 2.0;
    paint::push(
        out,
        Paint::Icon {
            glyph: *glyph,
            size,
            origin: TextOrigin::new(x, baseline),
            color,
        },
    )
}
