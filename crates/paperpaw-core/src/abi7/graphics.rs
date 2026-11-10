//! Borrowed host operations for the first graphics ABI slots.
//!
//! Context and handle lookup belong to the caller. These functions validate
//! inputs before drawing and keep the caller's clip, origin and semantic sink.

use paperpaw_graphics::Canvas;
use paperpaw_platform::{Color, Point};
use paperpaw_sdk::FontHandle;

/// An input cannot be represented by the existing drawing API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidInput;

const MAX_TEXT_BYTES: usize = 4096;

fn coordinate(value: i64) -> Result<i32, InvalidInput> {
    i32::try_from(value).map_err(|_| InvalidInput)
}

fn extent(value: u64) -> Result<u32, InvalidInput> {
    u32::try_from(value).map_err(|_| InvalidInput)
}

/// Fills a signed rectangle, clipped by the borrowed canvas.
pub fn fill_rect(
    canvas: &mut Canvas<'_>,
    x: i64,
    y: i64,
    width: u64,
    height: u64,
    color: Color,
) -> Result<(), InvalidInput> {
    let (x, y) = (coordinate(x)?, coordinate(y)?);
    let (width, height) = (extent(width)?, extent(height)?);
    canvas.fill_rect(x, y, width, height, color);
    Ok(())
}

/// Draws a line; width zero retains Canvas's one-pixel pen semantics.
#[allow(
    clippy::too_many_arguments,
    reason = "matches the drawing operation's scalar inputs"
)]
pub fn draw_line(
    canvas: &mut Canvas<'_>,
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
    width: u64,
    color: Color,
) -> Result<(), InvalidInput> {
    let from = Point::new(coordinate(x0)?, coordinate(y0)?);
    let to = Point::new(coordinate(x1)?, coordinate(y1)?);
    let width = extent(width)?;
    canvas.draw_line(from, to, width, color);
    Ok(())
}

/// Fills rounded corners without narrowing signed positions to Rect.
#[allow(
    clippy::too_many_arguments,
    reason = "matches the drawing operation's scalar inputs"
)]
pub fn fill_round(
    canvas: &mut Canvas<'_>,
    x: i64,
    y: i64,
    width: u64,
    height: u64,
    radius: u64,
    color: Color,
) -> Result<(), InvalidInput> {
    let (x, y) = (coordinate(x)?, coordinate(y)?);
    let (width, height) = (extent(width)?, extent(height)?);
    let radius = extent(radius)?;
    canvas.fill_round_rect(x, y, width, height, radius, color);
    Ok(())
}

/// Strokes rounded corners after checking the inner-size arithmetic.
#[allow(
    clippy::too_many_arguments,
    reason = "matches the drawing operation's scalar inputs"
)]
pub fn stroke_round(
    canvas: &mut Canvas<'_>,
    x: i64,
    y: i64,
    width: u64,
    height: u64,
    thickness: u64,
    radius: u64,
    color: Color,
) -> Result<(), InvalidInput> {
    let (x, y) = (coordinate(x)?, coordinate(y)?);
    let (width, height) = (extent(width)?, extent(height)?);
    let (thickness, radius) = (extent(thickness)?, extent(radius)?);
    let rounded = radius.min(width / 2).min(height / 2);
    let effective = thickness.min(width / 2 + 1).min(height / 2 + 1);
    // Canvas doubles this value only in its rounded-corner path.
    if rounded != 0 && effective.checked_mul(2).is_none() {
        return Err(InvalidInput);
    }
    canvas.stroke_round_rect(x, y, width, height, thickness, radius, color);
    Ok(())
}

fn nonnegative(value: f32) -> Result<f32, InvalidInput> {
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err(InvalidInput)
    }
}

/// Measures borrowed UTF-8 using the resolved font's fallback and ink rules.
pub fn text_measure(font: &FontHandle<'_>, size: f32, text: &str) -> Result<f32, InvalidInput> {
    nonnegative(size)?;
    if text.len() > MAX_TEXT_BYTES {
        return Err(InvalidInput);
    }
    nonnegative(font.measure(size, text))
}

/// Draws at most 4096 UTF-8 bytes without replacing the canvas or semantic sink.
/// Input errors fail before drawing. A bad returned advance is reported after
/// drawing, so this operation does not promise atomic effects on output errors.
#[allow(
    clippy::too_many_arguments,
    reason = "matches the font drawing operation's scalar inputs"
)]
pub fn text_draw(
    canvas: &mut Canvas<'_>,
    font: &FontHandle<'_>,
    size: f32,
    x: f32,
    baseline: i64,
    text: &str,
    color: Color,
) -> Result<f32, InvalidInput> {
    // i32::MAX rounds up to 2^31 in f32, so the upper edge is exclusive.
    if !x.is_finite() || !(-2_147_483_648.0..2_147_483_648.0).contains(&x) {
        return Err(InvalidInput);
    }
    let baseline = coordinate(baseline)?;
    nonnegative(size)?;
    if text.len() > MAX_TEXT_BYTES {
        return Err(InvalidInput);
    }
    // Keep FontHandle's mode-dependent return and use only one shaping pass.
    nonnegative(font.draw(canvas, size, x, baseline, text, color))
}

/// Draws like [`text_draw`] with glyphs widened by `extra` pixels and the
/// coverage drawn in `tone` (slot 41, `text.draw_inked`). Every argument is
/// checked before the first pixel, so a refused call draws nothing; the text
/// is shaped once, as `text_draw` does, and the advance it returns is the
/// one the draw gives (widening does not change it), checked as `text_draw`'s
/// is.
#[allow(
    clippy::too_many_arguments,
    reason = "matches the font drawing operation's scalar inputs"
)]
pub fn text_draw_toned(
    canvas: &mut Canvas<'_>,
    font: &FontHandle<'_>,
    size: f32,
    x: f32,
    baseline: i64,
    text: &str,
    color: Color,
    extra: u16,
    tone: paperpaw_sdk::text::ToneCurve,
) -> Result<f32, InvalidInput> {
    if !x.is_finite() || !(-2_147_483_648.0..2_147_483_648.0).contains(&x) {
        return Err(InvalidInput);
    }
    let baseline = coordinate(baseline)?;
    nonnegative(size)?;
    if text.len() > MAX_TEXT_BYTES {
        return Err(InvalidInput);
    }
    nonnegative(font.draw_toned(canvas, size, x, baseline, text, color, extra, tone))
}
