//! What an empty list says: a picture, a title and a few lines, centred.
use crate::{Paint, Style, paint, text};
use alloc::vec::Vec;
use paperpaw_sdk::{
    Rect,
    native::{
        Error,
        fonts::{Font, IconGlyph, TextOrigin},
    },
};

pub(super) fn draw_empty(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    area: Rect,
    (title, text): (&str, &str),
    icon: Option<IconGlyph>,
) -> Result<(), Error> {
    let room = f32::from(area.width.saturating_sub(4 * style.margin)).max(1.0);
    let lines = text::wrap_with(|line| font.measure_text(style.small, line), text, room, 6)?;
    let head = crate::geometry::line_px(font, style.title)?;
    let line = crate::geometry::line_px(font, style.small)?;
    // The icon over the title, twice as large as it, with a gap under it.
    let size = style.title * 2.0;
    let above = icon.map_or(0, |_| (size as u16).saturating_add(style.gap));
    let total = crate::geometry::clamp_u16(
        i64::from(above)
            + i64::from(head)
            + i64::from(style.gap)
            + i64::from(line) * lines.len() as i64,
    );
    let mut y = area.y.saturating_add(area.height.saturating_sub(total) / 2);
    if let Some(glyph) = icon {
        let side = size as u16;
        let x = area.x.saturating_add(area.width.saturating_sub(side) / 2);
        paint::push(
            paint,
            Paint::Icon {
                glyph,
                size,
                origin: TextOrigin::new(f32::from(x), i32::from(y) + (size * 0.86) as i32),
                color: style.ink,
            },
        )?;
        y = y.saturating_add(above);
    }
    let centred = |font: &Font<'_>, size: f32, text: &str| -> Result<f32, Error> {
        Ok((f32::from(area.width) - font.measure_text(size, text)?).max(0.0) / 2.0)
    };
    let fitted = text::fit(font, style.title, title, room)?;
    let x = centred(font, style.title, &fitted)?;
    text::label(
        paint,
        font,
        &fitted,
        style.title,
        x,
        Rect::new(0, y, area.width, head),
        style.ink,
        None,
    )?;
    y = y.saturating_add(head.saturating_add(style.gap));
    for text in &lines {
        let x = centred(font, style.small, text)?;
        text::label(
            paint,
            font,
            text,
            style.small,
            x,
            Rect::new(0, y, area.width, line),
            style.muted,
            None,
        )?;
        y = y.saturating_add(line);
    }
    Ok(())
}
