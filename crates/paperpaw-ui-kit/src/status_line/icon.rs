//! A status item drawn by its plugin (`DrawPurpose::StatusItem`): a sign in
//! the place the line gives the item, as interface 6 drew
//! `StatusValue::Icon`. The sign is the character of an icon in the bound icon
//! font; where there is no icon font the item's name stands for it, small, as
//! the menus show text for an icon they lack. The Wi-Fi and Bluetooth items
//! are drawn so.
use crate::{Paint, Style, paint, text};
use alloc::vec::Vec;
use paperpaw_sdk::{
    Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, FrameRect},
        fonts::{Font, FontRole, IconGlyph},
        formatting::copy_message,
        snapshot,
    },
};

/// The place as a screen rectangle: what cannot be one (a negative corner,
/// more than 65,535 pixels) is cut, never wrapped.
fn area_of(rect: FrameRect) -> Rect {
    let edge = |value: i32| u16::try_from(value.max(0)).unwrap_or(u16::MAX);
    let size = |value: u32| u16::try_from(value).unwrap_or(u16::MAX);
    Rect::new(
        edge(rect.x),
        edge(rect.y),
        size(rect.width),
        size(rect.height),
    )
}

/// The size the sign is drawn at: nine tenths of the place's shorter side.
fn sign_size(area: Rect) -> f32 {
    f32::from(area.width.min(area.height)) * 0.9
}

/// Draws the status item in `frame`: the sign of the icon `icon`, else the
/// message `name` (the item's name). Another purpose is not a status item's:
/// nothing is drawn.
///
/// # Errors
/// Font, catalog and drawing errors; `NoMemory`.
pub fn draw(
    host: &Host<'_>,
    frame: &mut DrawFrame<'_>,
    icon: &str,
    name: &str,
) -> Result<(), Error> {
    if !matches!(frame.purpose, DrawPurpose::StatusItem) {
        return Ok(());
    }
    let area = area_of(frame.rect);
    if area.is_empty() {
        return Ok(());
    }
    let font = Font::acquire_role(host, FontRole::Ui)?;
    // The font goes back on every path, an error too.
    let drawn = draw_with(host, frame, &font, area, (icon, name));
    let released = font.release();
    drawn.and(released)
}

fn draw_with(
    host: &Host<'_>,
    frame: &mut DrawFrame<'_>,
    font: &Font<'_>,
    area: Rect,
    (icon, name): (&str, &str),
) -> Result<(), Error> {
    let display = snapshot::capture_snapshot(host)?;
    let style = Style::read(host, &display, font)?;
    match sign(host, frame, &style, area, icon) {
        Err(Error::NotFound | Error::Unsupported) => named(host, frame, font, &style, (area, name)),
        other => other,
    }
}

/// The sign, centred in the place.
fn sign(
    host: &Host<'_>,
    frame: &mut DrawFrame<'_>,
    style: &Style,
    area: Rect,
    icon: &str,
) -> Result<(), Error> {
    let glyph = IconGlyph::named(host, icon)?;
    let icons = Font::acquire(host, crate::icons::ROLE)?;
    let size = sign_size(area);
    let drawn = (|| {
        let width = icons.measure_text(size, glyph.as_str())?;
        let x = f32::from(area.x) + ((f32::from(area.width) - width) / 2.0).max(0.0);
        let mut body: Vec<Paint> = Vec::new();
        text::label(
            &mut body,
            &icons,
            glyph.as_str(),
            size,
            x,
            area,
            style.ink,
            None,
        )?;
        paint::replay(frame, &icons, body.iter())
    })();
    let released = icons.release();
    drawn.and(released)
}

/// The item's name (the message `name`), small, left in the place.
fn named(
    host: &Host<'_>,
    frame: &mut DrawFrame<'_>,
    font: &Font<'_>,
    style: &Style,
    (area, name): (Rect, &str),
) -> Result<(), Error> {
    let mut label = [0; 64];
    let words = copy_message(host, name, &mut label)?;
    let mut body: Vec<Paint> = Vec::new();
    text::label(
        &mut body,
        font,
        words,
        style.small,
        f32::from(area.x),
        area,
        style.ink,
        None,
    )?;
    paint::replay(frame, font, body.iter())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_place_is_a_screen_rectangle_and_the_sign_fits_it() {
        let area = area_of(FrameRect::new(-5, 12, 20, 18));
        assert_eq!((area.x, area.y, area.width, area.height), (0, 12, 20, 18));
        assert!((sign_size(area) - 16.2).abs() < 0.01, "the shorter side");
        let huge = area_of(FrameRect::new(0, 0, 70_000, 4));
        assert_eq!(huge.width, u16::MAX);
        assert!(area_of(FrameRect::new(0, 0, 0, 9)).is_empty());
    }
}
