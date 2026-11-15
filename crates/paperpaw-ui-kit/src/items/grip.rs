//! The grips of rows that can be moved, and the frame and bar drawn while one is.
use super::{RowItem, draw_row};
use crate::{
    Paint, Style,
    list::{backdrop, ink},
    paint,
};
use alloc::vec::Vec;
use paperpaw_sdk::{
    Rect,
    native::{Error, fonts::Font},
};

/// Draws a row, with a grip at its end where `grip` (the list the grips are
/// noted in and the row's index) is given; the steppers' buttons it has.
pub(super) fn draw_row_with_grip(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    rect: Rect,
    row: &RowItem,
    focused: bool,
    grip: Option<(&mut Vec<(Rect, usize)>, usize)>,
) -> Result<Option<(Rect, Rect)>, Error> {
    let content = match grip {
        Some((grips, index)) => grip_row(paint, style, rect, focused, grips, index)?,
        None => rect,
    };
    draw_row(paint, font, style, content, row, focused)
}

/// The grip of a row in `rect`: as wide as the row is high, at its end (the left
/// where the interface runs right to left).
fn grip_strip(style: &Style, rect: Rect) -> Rect {
    let side = rect.height.min(rect.width);
    style.flip(
        rect,
        Rect::new(
            rect.x.saturating_add(rect.width.saturating_sub(side)),
            rect.y,
            side,
            rect.height,
        ),
    )
}

/// Draws the grip (three bars) of a row and notes it for [`ItemList::grip_at`];
/// what is left of the row for its content.
fn grip_row(
    paint: &mut Vec<Paint>,
    style: &Style,
    rect: Rect,
    focused: bool,
    grips: &mut Vec<(Rect, usize)>,
    index: usize,
) -> Result<Rect, Error> {
    let strip = grip_strip(style, rect);
    // The focus fills the whole row, under its content.
    backdrop(paint, style, rect, focused)?;
    let bar = style.line.max(2);
    let pitch = bar.saturating_mul(3);
    let width = (strip.width / 2).max(bar);
    let x = strip
        .x
        .saturating_add(strip.width.saturating_sub(width) / 2);
    let top = strip.y.saturating_add(
        strip
            .height
            .saturating_sub(bar.saturating_add(pitch.saturating_mul(2)))
            / 2,
    );
    let ink = ink(style, focused);
    for n in 0..3u16 {
        let y = top.saturating_add(n.saturating_mul(pitch));
        paint::push(paint, Paint::Fill(Rect::new(x, y, width, bar), ink))?;
    }
    grips.try_reserve(1).map_err(|_| Error::NoMemory)?;
    grips.push((strip, index));
    let side = strip.width.min(rect.width);
    let x = if style.rtl {
        rect.x.saturating_add(side)
    } else {
        rect.x
    };
    Ok(Rect::new(
        x,
        rect.y,
        rect.width.saturating_sub(side),
        rect.height,
    ))
}

/// A frame round the row taken along and a bar where it would land: below the
/// row it passes when moving down, above it when moving up.
pub(super) fn draw_drag(
    paint: &mut Vec<Paint>,
    style: &Style,
    hits: &[(Rect, usize)],
    from: usize,
    to: usize,
) -> Result<(), Error> {
    let rect_of = |item: usize| hits.iter().find(|(_, i)| *i == item).map(|(r, _)| *r);
    if let Some(r) = rect_of(from) {
        let t = style.line.saturating_mul(2).max(2).min(r.height / 2);
        for edge in [
            Rect::new(r.x, r.y, r.width, t),
            Rect::new(
                r.x,
                r.y.saturating_add(r.height.saturating_sub(t)),
                r.width,
                t,
            ),
            Rect::new(r.x, r.y, t, r.height),
            Rect::new(
                r.x.saturating_add(r.width.saturating_sub(t)),
                r.y,
                t,
                r.height,
            ),
        ] {
            paint::push(paint, Paint::Fill(edge, style.ink))?;
        }
    }
    if to == from {
        return Ok(());
    }
    // Not on this page (it was turned meanwhile): no bar.
    let Some(r) = rect_of(to) else {
        return Ok(());
    };
    let thick = style.line.saturating_mul(4).max(4);
    let y = if to > from {
        r.y.saturating_add(r.height).saturating_sub(thick / 2)
    } else {
        r.y.saturating_sub(thick / 2)
    };
    let margin = style.margin;
    let bar = Rect::new(
        r.x.saturating_add(margin),
        y,
        r.width.saturating_sub(margin.saturating_mul(2)),
        thick,
    );
    paint::push(paint, Paint::Fill(bar, style.ink))
}
