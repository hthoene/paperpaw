use super::Paint;
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Rect,
    native::{
        Error,
        fonts::{Font, TextOrigin},
    },
};
pub(super) fn fit(font: &Font<'_>, size: f32, text: &str, width: f32) -> Result<String, Error> {
    if font.measure_text(size, text)? <= width {
        return Ok(String::from(text));
    }
    let mut result = String::new();
    for c in text.chars() {
        result.push(c);
        result.push('…');
        if font.measure_text(size, &result)? > width {
            result.pop();
            result.pop();
            break;
        }
        result.pop();
    }
    result.push('…');
    Ok(result)
}
#[allow(
    clippy::too_many_arguments,
    reason = "one measured text run and its target geometry"
)]
pub(super) fn label(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    text: &str,
    size: f32,
    x: f32,
    area: Rect,
    color: Color,
    clip: Option<Rect>,
) -> Result<(), Error> {
    let room = f32::from(area.x) + f32::from(area.width) - x;
    let text = if room > 0.0 && font.measure_text(size, text)? > room + 1.0 {
        fit(font, size, text, room)?
    } else {
        String::from(text)
    };
    let metrics = font.read_line_metrics(size)?;
    let baseline = i32::from(area.y)
        + ((f32::from(area.height) + metrics.ascent - metrics.descent) / 2.0) as i32;
    paint.push(Paint::Text {
        text,
        size,
        origin: TextOrigin::new(x, baseline),
        color,
        clip,
    });
    Ok(())
}
pub(super) fn cap_baseline(font: &Font<'_>, size: f32, area: Rect) -> Result<i32, Error> {
    let (top, bottom) = if let Some(ink) = font.ink(size, "H")? {
        (ink.top, ink.bottom)
    } else {
        let m = font.read_line_metrics(size)?;
        (-(m.ascent as i32), -(m.descent as i32))
    };
    Ok(i32::from(area.y) + (i32::from(area.height) - (bottom - top)).div_euclid(2) - top)
}
