//! Text in a widget fits its place rather than running out of it: a line that
//! must stay on one line is made smaller down to a least size and only then
//! shortened with an ellipsis; text that may take several lines breaks
//! between words, never inside one, and is made smaller before it is cut.
//! Sizes are measured as they are drawn, so fonts with few sizes fit as well
//! as fonts drawn at any size. Everything lands in [`Paint`] operations.
use super::look::Align;
use crate::{Paint, paint, text};
use alloc::{string::String, vec::Vec};
use core::ops::Range;
use paperpaw_sdk::{
    Color, Rect,
    native::{
        Error,
        fonts::{Font, TextOrigin},
    },
};

/// How far letters may reach past the box their advance gives them; text in a
/// widget's area is cut there, so that a slant or a slash's tail stays inside.
const OVERHANG: u16 = 2;

/// Where text placed in `area` may draw: `area`, two pixels wider on each side.
#[must_use]
pub fn ink_room(area: Rect) -> Rect {
    let left = area.x.saturating_sub(OVERHANG);
    Rect::new(
        left,
        area.y,
        area.width
            .saturating_add(area.x - left)
            .saturating_add(OVERHANG),
        area.height,
    )
}

/// The largest size from `max` down to `min` (steps of 8 %) at which `fits`.
///
/// # Errors
/// The first error `fits` returns.
pub fn largest_size(
    max: f32,
    min: f32,
    mut fits: impl FnMut(f32) -> Result<bool, Error>,
) -> Result<Option<f32>, Error> {
    let min = min.min(max).max(1.0);
    let mut size = max.max(min);
    while size > min {
        if fits(size)? {
            return Ok(Some(size));
        }
        size = (size * 0.92).max(min);
    }
    Ok(fits(min)?.then_some(min))
}

/// `text` at the largest size from `max` to `min` at which it is `width` wide
/// or less; at `min` it is shortened with an ellipsis instead.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn fit_line(
    font: &Font<'_>,
    max: f32,
    min: f32,
    text: &str,
    width: f32,
) -> Result<(f32, String), Error> {
    match largest_size(max, min, |size| Ok(font.measure_text(size, text)? <= width))? {
        Some(size) => Ok((size, text::copy(text)?)),
        None => {
            let min = min.min(max);
            Ok((min, text::fit(font, min, text, width)?))
        }
    }
}

/// The baseline that centres the ink of `text` in `band`.
///
/// # Errors
/// Font refusals and [`Error::Invalid`] for hostile bounds.
pub fn ink_baseline(font: &Font<'_>, size: f32, text: &str, band: Rect) -> Result<i32, Error> {
    let (top, bottom) = match font.ink(size, text)? {
        Some(ink) => crate::geometry::check_ink(ink.top, ink.bottom)?,
        None => {
            let (ascent, descent, _) = crate::geometry::line_metrics(font, size)?;
            (-(ascent as i32), -(descent as i32))
        }
    };
    let room = i32::from(band.height) - (bottom - top);
    Ok(i32::from(band.y) + room.div_euclid(2) - top)
}

/// One line of text in `area`, as large as fits between `max` and `min`,
/// placed by `align` with its ink centred vertically; returns the size used.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub fn line(
    out: &mut Vec<Paint>,
    font: &Font<'_>,
    area: Rect,
    align: Align,
    (max, min): (f32, f32),
    text: &str,
    color: Color,
) -> Result<f32, Error> {
    let (size, shown) = fit_line(font, max, min, text, f32::from(area.width))?;
    let width = font.measure_text(size, &shown)?;
    let x = align.x(area, width);
    let baseline = ink_baseline(font, size, &shown, area)?;
    paint::push(
        out,
        Paint::Text {
            text: shown,
            size,
            origin: TextOrigin::new(x, baseline),
            color,
            clip: Some(ink_room(area)),
        },
    )?;
    Ok(size)
}

/// Where the words of `text` break at `width`, and whether a word is wider
/// than a line.
fn break_words(
    font: &Font<'_>,
    size: f32,
    text: &str,
    width: f32,
) -> Result<(Vec<Range<usize>>, bool), Error> {
    let mut lines: Vec<Range<usize>> = Vec::new();
    let mut too_wide = false;
    let mut start = 0;
    for paragraph in text.split('\n') {
        let base = start;
        start += paragraph.len() + 1;
        let mut current: Option<Range<usize>> = None;
        let mut at = 0;
        for word in paragraph.split(' ') {
            let range = base + at..base + at + word.len();
            at += word.len() + 1;
            if word.is_empty() {
                continue;
            }
            if font.measure_text(size, word)? > width {
                too_wide = true;
            }
            current = match current {
                Some(held)
                    if text
                        .get(held.start..range.end)
                        .map(|joined| font.measure_text(size, joined))
                        .transpose()?
                        .is_some_and(|measured| measured <= width) =>
                {
                    Some(held.start..range.end)
                }
                Some(held) => {
                    lines.try_reserve(1).map_err(|_| Error::NoMemory)?;
                    lines.push(held);
                    Some(range)
                }
                None => Some(range),
            };
        }
        lines.try_reserve(1).map_err(|_| Error::NoMemory)?;
        lines.push(current.unwrap_or(base..base));
    }
    Ok((lines, too_wide))
}

/// The distance between lines of `size`: text, or a closer heading.
fn step(font: &Font<'_>, size: f32, heading: bool) -> Result<u16, Error> {
    let line = crate::geometry::line_px(font, size)?;
    Ok(if heading {
        crate::geometry::clamp_u16((f32::from(line) * 0.9) as i64)
    } else {
        line
    })
}

fn lines_height(font: &Font<'_>, size: f32, heading: bool, count: usize) -> Result<u16, Error> {
    if count == 0 {
        return Ok(0);
    }
    let others = u16::try_from(count - 1).unwrap_or(u16::MAX);
    Ok(crate::geometry::line_px(font, size)?
        .saturating_add(step(font, size, heading)?.saturating_mul(others)))
}

/// Text broken into lines that fit a place; one that does not fit whole is
/// cut with an ellipsis on its last line.
#[derive(Debug, Default, PartialEq)]
pub struct Wrapped {
    size: f32,
    lines: Vec<String>,
    whole: bool,
    heading: bool,
}

impl Wrapped {
    /// `text` at `size` in `width` by `height`.
    ///
    /// # Errors
    /// Font refusals and [`Error::NoMemory`].
    pub fn new(
        font: &Font<'_>,
        text: &str,
        size: f32,
        (width, height): (u16, u16),
        heading: bool,
    ) -> Result<Self, Error> {
        let mut wrapped = Self {
            size,
            lines: Vec::new(),
            whole: true,
            heading,
        };
        let room = f32::from(width);
        let (ranges, _) = break_words(font, size, text, room)?;
        let line = crate::geometry::line_px(font, size)?.max(1);
        let apart = step(font, size, heading)?.max(1);
        let fits = 1 + usize::from(height.saturating_sub(line) / apart);
        let count = ranges.len();
        for (n, range) in ranges.into_iter().enumerate() {
            let own = text.get(range.clone()).unwrap_or("").trim_end();
            wrapped.lines.try_reserve(1).map_err(|_| Error::NoMemory)?;
            if n + 1 == fits && n + 1 < count {
                // The last line there is room for: the rest on it, its
                // breaks as spaces.
                let rest = text.get(range.start..).unwrap_or("").trim_end();
                let mut joined = String::new();
                joined
                    .try_reserve_exact(rest.len() + 4)
                    .map_err(|_| Error::NoMemory)?;
                for c in rest.chars() {
                    joined.push(if c == '\n' { ' ' } else { c });
                }
                joined.push_str(" …");
                wrapped.lines.push(text::fit(font, size, &joined, room)?);
                wrapped.whole = false;
                break;
            }
            let shown = text::fit(font, size, own, room)?;
            let shortened = shown != own;
            wrapped.lines.push(shown);
            if shortened {
                wrapped.whole = false;
                break;
            }
        }
        Ok(wrapped)
    }

    /// `text` at the largest size from `max` to `min` at which it breaks into
    /// lines no wider than `width` within `height`; a heading's lines sit
    /// closer.
    ///
    /// # Errors
    /// Font refusals and [`Error::NoMemory`].
    pub fn fitting(
        font: &Font<'_>,
        text: &str,
        (max, min): (f32, f32),
        (width, height): (u16, u16),
        heading: bool,
    ) -> Result<Self, Error> {
        let room = f32::from(width);
        let size = largest_size(max, min, |size| {
            let (lines, too_wide) = break_words(font, size, text, room)?;
            Ok(!too_wide && lines_height(font, size, heading, lines.len())? <= height)
        })?;
        Self::new(
            font,
            text,
            size.unwrap_or_else(|| min.min(max)),
            (width, height),
            heading,
        )
    }

    /// The size the text is drawn at.
    #[must_use]
    pub const fn size(&self) -> f32 {
        self.size
    }

    /// Whether all of it is shown.
    #[must_use]
    pub const fn whole(&self) -> bool {
        self.whole
    }

    /// The lines.
    #[must_use]
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// The height the lines take.
    ///
    /// # Errors
    /// Font refusals.
    pub fn height(&self, font: &Font<'_>) -> Result<u16, Error> {
        lines_height(font, self.size, self.heading, self.lines.len())
    }

    /// Draws the lines centred vertically in `area`, placed by `align`;
    /// returns the height taken.
    ///
    /// # Errors
    /// Font refusals and [`Error::NoMemory`].
    pub fn draw(
        &self,
        out: &mut Vec<Paint>,
        font: &Font<'_>,
        area: Rect,
        align: Align,
        color: Color,
    ) -> Result<u16, Error> {
        let size = self.size;
        let line = f32::from(crate::geometry::line_px(font, size)?).max(1.0);
        let apart = f32::from(step(font, size, self.heading)?).max(1.0);
        let total = self.height(font)?;
        let top = area.y.saturating_add(area.height.saturating_sub(total) / 2);
        let (ascent, descent, _) = crate::geometry::line_metrics(font, size)?;
        // Cut at the sides only: an area lower than a line still shows it.
        let tall = Rect::new(
            area.x,
            area.y,
            area.width,
            area.height.max(top - area.y + total),
        );
        for (n, shown) in self.lines.iter().enumerate() {
            let y = f32::from(top) + apart * n as f32;
            let baseline = y + (line + ascent - descent) / 2.0;
            let x = align.x(area, font.measure_text(size, shown)?);
            paint::push(
                out,
                Paint::Text {
                    text: text::copy(shown)?,
                    size,
                    origin: TextOrigin::new(x, baseline as i32),
                    color,
                    clip: Some(ink_room(tall)),
                },
            )?;
        }
        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_largest_size_that_fits_is_found_and_none_when_nothing_does() {
        let found = largest_size(40.0, 10.0, |size| Ok(size <= 25.0))
            .unwrap()
            .unwrap();
        assert!(found <= 25.0 && found > 20.0);
        assert_eq!(largest_size(40.0, 10.0, |_| Ok(false)).unwrap(), None);
        assert_eq!(largest_size(40.0, 10.0, |_| Ok(true)).unwrap(), Some(40.0));
        // A least above the greatest: the greatest is the least.
        assert_eq!(largest_size(10.0, 40.0, |_| Ok(true)).unwrap(), Some(10.0));
    }

    #[test]
    fn a_failing_measure_stops_the_search() {
        assert_eq!(
            largest_size(40.0, 10.0, |_| Err(Error::Invalid)),
            Err(Error::Invalid)
        );
    }

    #[test]
    fn ink_room_widens_the_area_without_wrapping_at_the_edge() {
        let room = ink_room(Rect::new(1, 0, 10, 5));
        assert_eq!((room.x, room.width), (0, 13));
        let room = ink_room(Rect::new(0, 0, u16::MAX, 5));
        assert_eq!(room.width, u16::MAX);
    }
}
