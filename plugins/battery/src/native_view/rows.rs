use super::{Paint, Style, text};
use alloc::vec::Vec;
use paperpaw_sdk::{
    Rect,
    native::{Error, fonts::Font},
};
pub(super) fn prepare(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    s: &Style,
    rect: Rect,
    title: &str,
    value: &str,
) -> Result<(), Error> {
    let margin = f32::from(s.margin);
    let width = f32::from(rect.width) - 2.0 * margin;
    let room = (width - margin).max(0.0);
    let title_width = font.measure_text(s.text, title)?.min(room * 0.6);
    let shown = text::fit(font, s.text, value, room - title_width + 1.0)?;
    let value_width = font.measure_text(s.text, &shown)?;
    let title_room = (room - value_width).max(0.0);
    let mut size = s.text;
    if shown == value {
        while size > s.small && font.measure_text(size, title)? > title_room + 1.0 {
            size = (size - 0.25).max(s.small);
        }
    }
    let title = text::fit(font, size, title, title_room + 1.0)?;
    let title_area = Rect::new(
        rect.x,
        rect.y,
        (margin + title_room).max(0.0) as u16,
        rect.height,
    );
    text::label(
        paint,
        font,
        &title,
        size,
        f32::from(rect.x) + margin,
        title_area,
        s.ink,
        Some(rect),
    )?;
    text::label(
        paint,
        font,
        &shown,
        s.text,
        f32::from(rect.x) + f32::from(rect.width) - margin - value_width,
        rect,
        s.ink,
        Some(rect),
    )
}
