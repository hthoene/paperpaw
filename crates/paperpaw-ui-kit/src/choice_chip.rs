//! A chip that opens a choice list ("All books ⌄") with, at the other end of
//! its row, a lens that opens a search: the header row of a list on a touch
//! screen, as Beta 3 showed the library's filter. One tap on the chip shows
//! every choice (the host's choice dialog, `Question::choice`), instead of a
//! row that steps from one choice to the next.
//!
//! Like the [`slider`](crate::slider) this holds the geometry (where the chip
//! and the lens go, so that a tap can be told from the other) and the paint of
//! both; what a tap does is the screen's. Nothing is kept: lay the row out when
//! the screen is drawn ([`layout`]), keep the two rectangles for the taps, and
//! paint ([`draw`]). Where the interface runs right to left ([`Style::rtl`])
//! the row is mirrored.

use crate::{
    Paint, Style,
    geometry::clamp_u16,
    paint, text,
    widget::shape::{line as stroke_line, stroke_circle},
};
use alloc::vec::Vec;
use paperpaw_sdk::{
    Color, Point, Rect,
    native::{Error, fonts::Font},
};

/// Where the chip and the lens of a row are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Parts {
    pub chip: Rect,
    /// The lens, where the row has one.
    pub lens: Option<Rect>,
}

impl Parts {
    /// Whether `point` is on the lens.
    #[must_use]
    pub fn on_lens(&self, point: Point) -> bool {
        self.lens.is_some_and(|lens| lens.contains(point))
    }

    /// Whether `point` is on the chip.
    #[must_use]
    pub fn on_chip(&self, point: Point) -> bool {
        self.chip.contains(point)
    }
}

/// The height of the chip and the side of the lens in a row `row` high: a row
/// less a gap, so that two rows never touch.
fn side(row: &Rect, style: &Style) -> u16 {
    row.height.saturating_sub(style.gap).max(1)
}

/// The room the chevron takes at the right end of the chip.
fn chevron_room(high: u16) -> u16 {
    high / 2
}

/// The parts of a row of `row`, the chip's label being `label_width` pixels
/// wide: the chip as wide as its label, its chevron and a margin at each end
/// need (never wider than what the lens leaves), the lens a square at the
/// row's end. Laid out from the left; [`Style::rtl`] mirrors it.
#[must_use]
pub fn place(style: &Style, row: Rect, label_width: f32, lens: bool) -> Parts {
    let high = side(&row, style);
    let top = row.y.saturating_add(row.height.saturating_sub(high) / 2);
    let pad = style.margin;
    let lens_rect = lens.then(|| {
        Rect::new(
            row.x
                .saturating_add(row.width)
                .saturating_sub(style.margin.saturating_add(high)),
            top,
            high,
            high,
        )
    });
    let after = lens_rect.map_or(row.width, |l| l.x.saturating_sub(row.x));
    let room = after.saturating_sub(style.margin.saturating_mul(2));
    let want = clamp_u16(
        (label_width + f32::from(chevron_room(high)) + 2.0 * f32::from(pad) + 1.0) as i64,
    );
    let chip = Rect::new(
        row.x.saturating_add(style.margin),
        top,
        want.min(room),
        high,
    );
    Parts {
        chip: style.flip(row, chip),
        lens: lens_rect.map(|l| style.flip(row, l)),
    }
}

/// Lays the row out: measures `label` (shortened where it is wider than the
/// row leaves) and places the parts.
///
/// # Errors
/// A refused font reply.
pub fn layout(
    font: &Font<'_>,
    style: &Style,
    row: Rect,
    label: &str,
    lens: bool,
) -> Result<Parts, Error> {
    let wide = font.measure_text(style.text, label)?;
    Ok(place(style, row, wide, lens))
}

/// Paints the row's parts in `ink`: the chip as an outline with `label` and a
/// chevron, the lens as a ring with a handle.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn draw(
    out: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    parts: &Parts,
    label: &str,
    ink: Color,
) -> Result<(), Error> {
    let chip = parts.chip;
    let thick = u32::from(style.line).max(1);
    paint::push(
        out,
        Paint::Focus(chip, u32::from(chip.height) / 2, Some(thick), ink),
    )?;
    let chevron = chevron_room(chip.height);
    let pad = f32::from(style.margin);
    let room = (f32::from(chip.width) - 2.0 * pad - f32::from(chevron)).max(1.0);
    let shown = text::fit(font, style.text, label, room)?;
    let x = style.flip_x(
        chip,
        f32::from(chip.x) + pad,
        font.measure_text(style.text, &shown)?,
    );
    text::label(out, font, &shown, style.text, x, chip, ink, Some(chip))?;
    // The chevron, in the room at the chip's end.
    let half = i32::from(chevron) / 3;
    let cx = if style.rtl {
        i32::from(chip.x) + i32::from(style.margin) / 2 + i32::from(chevron) / 2
    } else {
        i32::from(chip.x) + i32::from(chip.width)
            - i32::from(style.margin) / 2
            - i32::from(chevron) / 2
    };
    let cy = i32::from(chip.y) + i32::from(chip.height) / 2;
    let width = u32::from(style.line).max(2);
    let tip = Point::new(cx, cy + half / 2);
    stroke_line(out, Point::new(cx - half, cy - half / 2), tip, width, ink)?;
    stroke_line(out, tip, Point::new(cx + half, cy - half / 2), width, ink)?;
    if let Some(lens) = parts.lens {
        draw_lens(out, style, lens, ink)?;
    }
    Ok(())
}

/// A lens in the square `at`: a ring and its handle at the lower right.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn draw_lens(out: &mut Vec<Paint>, style: &Style, at: Rect, ink: Color) -> Result<(), Error> {
    let side = i32::from(at.width.min(at.height));
    let (x, y) = (i32::from(at.x), i32::from(at.y));
    let width = u32::from(style.line).max(2);
    let radius = (side * 22 / 100).max(2);
    let centre = Point::new(x + side * 44 / 100, y + side * 44 / 100);
    stroke_circle(out, centre, radius as u32, width, ink)?;
    // The handle runs from the ring's edge (at 45 degrees) to the corner.
    let edge = radius * 7 / 10;
    stroke_line(
        out,
        Point::new(centre.x + edge, centre.y + edge),
        Point::new(x + side * 76 / 100, y + side * 76 / 100),
        width,
        ink,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    fn style() -> Style {
        Style {
            corner: 4,
            pill: false,
            ring: false,
            soft: false,
            shadow: 0,
            frame: None,
            button: crate::style::ButtonLook::Outlined,
            mono: false,
            roles: crate::style::Roles::default(),
            separator_dither: false,
            separator_ink: Color::BLACK,
            text: 22.0,
            small: 17.0,
            title: 26.0,
            head: 28.0,
            row: 64,
            header: 56,
            margin: 12,
            gap: 8,
            line: 2,
            dark: false,
            plain: false,
            separators: "core.separators.lines".to_string(),
            ink: Color::BLACK,
            paper: Color::WHITE,
            disabled: Color::gray(160),
            rule: Color::gray(190),
            muted: Color::gray(100),
            note: Color::gray(80),
            heading: 30,
            card: Color::gray(230),
            on_dark_muted: Color::gray(200),
            strong: false,
            rtl: false,
            describing: false,
        }
    }

    #[test]
    fn the_lens_is_a_square_at_the_end_and_the_chip_stops_before_it() {
        let row = Rect::new(0, 100, 480, 64);
        let parts = place(&style(), row, 90.0, true);
        let lens = parts.lens.unwrap();
        assert_eq!((lens.width, lens.height), (56, 56));
        assert_eq!(lens.x + lens.width, 480 - 12);
        assert!(parts.chip.x + parts.chip.width + 12 <= lens.x);
        assert_eq!(parts.chip.x, 12);
        assert_eq!(parts.chip.height, 56);
        // Both sit in the middle of the row.
        assert_eq!(lens.y, 104);
        assert_eq!(parts.chip.y, 104);
    }

    #[test]
    fn a_tap_is_told_apart_and_a_long_label_is_held_in() {
        let row = Rect::new(0, 0, 200, 60);
        let parts = place(&style(), row, 900.0, true);
        let lens = parts.lens.unwrap();
        assert!(parts.chip.x + parts.chip.width <= lens.x);
        assert!(parts.on_lens(Point::new(i32::from(lens.x) + 1, 5)));
        assert!(!parts.on_chip(Point::new(i32::from(lens.x) + 1, 5)));
        assert!(parts.on_chip(Point::new(20, 5)));
        let alone = place(&style(), row, 10.0, false);
        assert!(alone.lens.is_none());
        assert!(!alone.on_lens(Point::new(190, 5)));
    }

    #[test]
    fn a_right_to_left_row_is_mirrored() {
        let row = Rect::new(0, 0, 480, 64);
        let flat = place(&style(), row, 90.0, true);
        let mirrored = place(
            &Style {
                rtl: true,
                ..style()
            },
            row,
            90.0,
            true,
        );
        assert_eq!(mirrored.chip.x, 480 - flat.chip.x - flat.chip.width,);
        assert_eq!(mirrored.lens.unwrap().x, 12);
    }
}
