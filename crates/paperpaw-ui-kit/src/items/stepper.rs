//! The − and + buttons of a stepper row.
use super::RowItem;
use crate::Paint;
use crate::{Style, paint, text};
use alloc::vec::Vec;
use paperpaw_sdk::{
    Color, Rect,
    native::{Error, canvas::PartRole, fonts::Font},
};

/// Draws the − and + buttons at the end of a stepper row's first line `band`:
/// where the title and the value go, and the buttons.
pub(super) fn draw_stepper(
    paint: &mut Vec<Paint>,
    (font, style): (&Font<'_>, &Style),
    band: Rect,
    row: &RowItem,
    ink: Color,
) -> Result<(Rect, (Rect, Rect)), Error> {
    // Square buttons at the right end of the row's first line; the value
    // sits to their left.
    let side = band.height.min(band.width / 4);
    let right = band
        .x
        .saturating_add(band.width)
        .saturating_sub(style.margin / 2);
    let plus = Rect::new(right.saturating_sub(side), band.y, side, band.height);
    let minus = Rect::new(plus.x.saturating_sub(side), band.y, side, band.height);
    // Mirrored as a whole where the interface runs right to left: the buttons
    // at the left end and the value to their right.
    let free = Rect::new(band.x, band.y, minus.x.saturating_sub(band.x), band.height);
    let cols = style.flip(band, free);
    let (minus, plus) = (style.flip(band, minus), style.flip(band, plus));
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
        let size = style.text;
        let x = f32::from(rect.x) + (f32::from(rect.width) - font.measure_text(size, glyph)?) / 2.0;
        text::label(paint, font, glyph, size, x, rect, ink, Some(rect))?;
    }
    Ok((cols, (minus, plus)))
}
