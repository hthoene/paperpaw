//! Checked geometry: the host's font replies are data and may be extreme.
use paperpaw_sdk::native::{Error, fonts::Font};

/// The largest length in pixels the kit accepts from a font reply.
pub const MAX_PX: f32 = 2048.0;

/// `value` as whole pixels.
///
/// # Errors
/// [`Error::Invalid`] when it is not finite, negative or beyond [`MAX_PX`].
pub fn px(value: f32) -> Result<u16, Error> {
    if value.is_finite() && (0.0..=MAX_PX).contains(&value) {
        Ok(value as u16)
    } else {
        Err(Error::Invalid)
    }
}

/// A font length that must be finite and within [`MAX_PX`] in either direction.
fn bounded(value: f32) -> Result<f32, Error> {
    if value.is_finite() && (-MAX_PX..=MAX_PX).contains(&value) {
        Ok(value)
    } else {
        Err(Error::Invalid)
    }
}

/// Ascent, descent and line height of `font` at `size`, each checked.
///
/// # Errors
/// [`Error::Invalid`] for a reply that is not finite, beyond [`MAX_PX`], or has a
/// negative line height; the font's own refusals.
pub fn line_metrics(font: &Font<'_>, size: f32) -> Result<(f32, f32, f32), Error> {
    let m = font.read_line_metrics(size)?;
    let line = bounded(m.line_height)?;
    if line < 0.0 {
        return Err(Error::Invalid);
    }
    Ok((bounded(m.ascent)?, bounded(m.descent)?, line))
}

/// The line height of `font` at `size`, checked.
///
/// # Errors
/// As [`line_metrics`].
pub fn line_height(font: &Font<'_>, size: f32) -> Result<f32, Error> {
    Ok(line_metrics(font, size)?.2)
}

/// The line height in whole pixels, checked.
///
/// # Errors
/// As [`line_metrics`].
pub fn line_px(font: &Font<'_>, size: f32) -> Result<u16, Error> {
    px(line_height(font, size)?)
}

/// Where the top and bottom of capital ink lie, as `(top, bottom)` relative to the baseline.
///
/// # Errors
/// As [`line_metrics`], and [`Error::Invalid`] for ink bounds beyond [`MAX_PX`].
pub fn cap_bounds(font: &Font<'_>, size: f32) -> Result<(i32, i32), Error> {
    let max = MAX_PX as i32;
    if let Some(ink) = font.ink(size, "H")? {
        if ink.top.abs() > max || ink.bottom.abs() > max {
            return Err(Error::Invalid);
        }
        Ok((ink.top, ink.bottom))
    } else {
        let (ascent, descent, _) = line_metrics(font, size)?;
        Ok((-(ascent as i32), -(descent as i32)))
    }
}

/// `value` widened and clamped into a `u16`.
#[must_use]
pub fn clamp_u16(value: i64) -> u16 {
    value.clamp(0, i64::from(u16::MAX)) as u16
}
