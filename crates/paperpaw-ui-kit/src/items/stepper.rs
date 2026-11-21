//! The round − and + buttons of a stepper row, with its value tight between them.
use super::RowItem;
use crate::Paint;
use crate::{Style, paint, text};
use alloc::vec::Vec;
use paperpaw_sdk::{
    Color, Rect,
    native::{Error, canvas::PartRole, fonts::Font},
};

/// Draws the − and + buttons at the end of a stepper row's first line `band`,
/// round, with the row's value between them; where the title goes, and the
/// buttons.
pub(super) fn draw_stepper(
    paint: &mut Vec<Paint>,
    (font, style): (&Font<'_>, &Style),
    band: Rect,
    row: &RowItem,
    ink: Color,
) -> Result<(Rect, (Rect, Rect)), Error> {
    // Square tap targets at the right end of the row's first line; the value
    // sits between them, so the number is read where the finger is.
    let side = band.height.min(band.width / 4);
    let wide = font.measure_text(style.text, &row.value)?.max(0.0) as u16 + 1;
    let room = wide.saturating_add(style.gap);
    let right = band
        .x
        .saturating_add(band.width)
        .saturating_sub(style.margin / 2);
    let plus = Rect::new(right.saturating_sub(side), band.y, side, band.height);
    let minus = Rect::new(
        plus.x.saturating_sub(room).saturating_sub(side),
        band.y,
        side,
        band.height,
    );
    let between = Rect::new(
        minus.x.saturating_add(side),
        band.y,
        plus.x.saturating_sub(minus.x.saturating_add(side)),
        band.height,
    );
    // Mirrored as a whole where the interface runs right to left: the buttons
    // at the left end and the value to their right.
    let free = Rect::new(band.x, band.y, minus.x.saturating_sub(band.x), band.height);
    let cols = style.flip(band, free);
    let (minus, plus) = (style.flip(band, minus), style.flip(band, plus));
    let between = style.flip(band, between);
    let thickness = u32::from(style.line).max(1);
    for (rect, glyph) in [(minus, "\u{2212}"), (plus, "+")] {
        // Named by its sign: a sign alone is not told as text.
        paint::describe_named(
            paint,
            style.describing,
            PartRole::Button,
            rect,
            glyph,
            false,
            row.disabled,
        )?;
        // A circle as wide as the row's first line is high, less a little air.
        let diameter = rect.height.saturating_sub(rect.height / 4).min(rect.width);
        let disc = Rect::new(
            rect.x
                .saturating_add(rect.width.saturating_sub(diameter) / 2),
            rect.y
                .saturating_add(rect.height.saturating_sub(diameter) / 2),
            diameter,
            diameter,
        );
        paint::push(
            paint,
            Paint::Focus(disc, u32::from(diameter / 2), Some(thickness), ink),
        )?;
        let size = style.text;
        let x = f32::from(rect.x) + (f32::from(rect.width) - font.measure_text(size, glyph)?) / 2.0;
        text::label(paint, font, glyph, size, x, rect, ink, Some(rect))?;
    }
    let x = f32::from(between.x)
        + (f32::from(between.width) - font.measure_text(style.text, &row.value)?) / 2.0;
    text::label(
        paint,
        font,
        &row.value,
        style.text,
        x,
        between,
        ink,
        Some(between),
    )?;
    Ok((cols, (minus, plus)))
}
