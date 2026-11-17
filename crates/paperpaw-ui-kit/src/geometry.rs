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
    if let Some(ink) = font.ink(size, "H")? {
        check_ink(ink.top, ink.bottom)
    } else {
        estimated_ink(size, "H")
    }
}

/// How high capitals reach above the baseline, in parts of the size, where a
/// host does not tell the ink.
pub const CAP_HEIGHT: f32 = 0.7;
/// How far descenders reach below it.
pub const DESCENT: f32 = 0.2;

/// The ink of `text` at `size` as `(top, bottom)` from its baseline (above
/// negative), estimated where the host does not tell it (the X4's,
/// [`Font::ink`]): capitals and ascenders reach [`CAP_HEIGHT`] above the
/// baseline, and [`DESCENT`] below it where a letter has a tail. Not the
/// font's ascent and descent: taken for the ink's top and bottom they put
/// text 7 to 10 px too low (#303).
///
/// # Errors
/// [`Error::Invalid`] for a size that is not finite or beyond [`MAX_PX`].
pub fn estimated_ink(size: f32, text: &str) -> Result<(i32, i32), Error> {
    bounded(size)?;
    let tails = text.chars().any(|c| {
        matches!(
            c,
            'g' | 'j' | 'p' | 'q' | 'y' | 'Q' | ',' | ';' | '(' | ')' | '[' | ']'
        )
    });
    let below = if tails { (size * DESCENT) as i32 } else { 0 };
    Ok((-((size * CAP_HEIGHT) as i32), below))
}

/// The ink bounds of a font reply, each within [`MAX_PX`] in either direction.
///
/// Signed range checks: no `abs()`, which overflows for `i32::MIN`.
///
/// # Errors
/// [`Error::Invalid`] for a bound beyond [`MAX_PX`].
pub fn check_ink(top: i32, bottom: i32) -> Result<(i32, i32), Error> {
    let max = MAX_PX as i32;
    let range = -max..=max;
    if range.contains(&top) && range.contains(&bottom) {
        Ok((top, bottom))
    } else {
        Err(Error::Invalid)
    }
}

/// `value` widened and clamped into a `u16`.
#[must_use]
pub fn clamp_u16(value: i64) -> u16 {
    value.clamp(0, i64::from(u16::MAX)) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ink_bounds_refuse_hostile_values_without_panicking() {
        let max = MAX_PX as i32;
        assert_eq!(check_ink(-max, max), Ok((-max, max)));
        for bad in [i32::MIN, i32::MIN + 1, i32::MAX, max + 1, -max - 1] {
            assert_eq!(check_ink(bad, 0), Err(Error::Invalid), "top {bad}");
            assert_eq!(check_ink(0, bad), Err(Error::Invalid), "bottom {bad}");
        }
    }
}
