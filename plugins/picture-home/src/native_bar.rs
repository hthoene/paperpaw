//! The status bar of the picture home screen without the host: what it is on
//! (a plate of one of five looks, or the picture itself), where its plate is
//! in the strip the page keeps free (the whole width, or a compact pill at the
//! left, in the middle or at the right), and the room the items get inside it.
//! The items and their texts are `paperpaw_ui_kit::status_line`; what shows
//! them is `native_dashboard`. When it shows (always, never, after a press) is
//! `native_picture::Visible`.
use paperpaw_sdk::{Color, Rect};

use crate::settings;

/// What the bar is on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Look {
    /// A plate as light as the page.
    #[default]
    Normal,
    /// A black plate.
    Inverted,
    /// A light gray plate.
    Light,
    /// A dark gray plate.
    Dark,
    /// No plate: the items on the picture.
    Clear,
}

impl Look {
    /// The look a value of the look setting names; the page's own for any other.
    #[must_use]
    pub fn of(value: &str) -> Self {
        match settings::BAR_LOOKS.iter().position(|l| *l == value) {
            Some(1) => Look::Inverted,
            Some(2) => Look::Light,
            Some(3) => Look::Dark,
            Some(4) => Look::Clear,
            _ => Look::Normal,
        }
    }

    /// The plate's gray and the ink on it; no plate for [`Look::Clear`].
    #[must_use]
    pub const fn plate(self) -> Option<(Color, Color)> {
        match self {
            Look::Normal => Some((Color::WHITE, Color::BLACK)),
            Look::Inverted => Some((Color::BLACK, Color::WHITE)),
            Look::Light => Some((Color::gray(165), Color::BLACK)),
            Look::Dark => Some((Color::gray(85), Color::WHITE)),
            Look::Clear => None,
        }
    }
}

/// Where the plate sits in its strip.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Placing {
    /// A compact pill instead of the strip's whole width.
    pub compact: bool,
    /// 0 left, 1 middle, 2 right (of a compact pill).
    pub align: u8,
}

impl Placing {
    /// The place the shape and align settings name (the right for an unknown
    /// align, as interface 6).
    #[must_use]
    pub fn of(shape: &str, align: &str) -> Self {
        Self {
            compact: shape == settings::BAR_SHAPES[1],
            align: settings::BAR_ALIGNS
                .iter()
                .position(|a| *a == align)
                .map_or(2, |at| at as u8),
        }
    }
}

/// Where the bar's plate is in `strip` (the whole width of the screen): the
/// strip less a margin each side, or in it a pill 55 % as wide.
#[must_use]
pub fn plate(strip: Rect, (margin, gap): (u16, u16), placing: Placing) -> Rect {
    let margin = margin.max(gap / 2);
    let x = strip.x.saturating_add(margin);
    let full = Rect::new(
        x,
        strip.y,
        strip.width.saturating_sub(margin.saturating_mul(2)),
        strip.height,
    );
    if !placing.compact {
        return full;
    }
    let width = u16::try_from(u32::from(full.width) * 55 / 100).unwrap_or(full.width);
    let left = match placing.align {
        0 => 0,
        1 => (full.width - width) / 2,
        _ => full.width - width,
    };
    Rect::new(full.x.saturating_add(left), full.y, width, full.height)
}

/// The items' room inside the plate: `pad` in from each side.
#[must_use]
pub fn inner(plate: Rect, pad: u16) -> Rect {
    Rect::new(
        plate.x.saturating_add(pad),
        plate.y,
        plate.width.saturating_sub(pad.saturating_mul(2)),
        plate.height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const STRIP: Rect = Rect::new(0, 400, 480, 32);

    #[test]
    fn looks_are_read_by_their_values_and_default_to_the_page() {
        for (i, look) in [
            Look::Normal,
            Look::Inverted,
            Look::Light,
            Look::Dark,
            Look::Clear,
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(Look::of(settings::BAR_LOOKS[i]), look);
        }
        assert_eq!(Look::of("anything"), Look::Normal);
    }

    #[test]
    fn plates_have_ink_that_reads_on_them() {
        for look in [Look::Normal, Look::Inverted, Look::Light, Look::Dark] {
            let (paper, ink) = look.plate().unwrap();
            let gap = i32::from(paper.luma()) - i32::from(ink.luma());
            assert!(gap.abs() > 120, "{look:?}");
        }
        assert!(Look::Clear.plate().is_none());
    }

    #[test]
    fn the_full_plate_is_the_strip_less_a_margin_each_side() {
        let full = Placing::of(settings::BAR_SHAPES[0], "");
        assert_eq!(plate(STRIP, (16, 8), full), Rect::new(16, 400, 448, 32));
        // The margin is at least half a gap.
        assert_eq!(plate(STRIP, (2, 8), full), Rect::new(4, 400, 472, 32));
    }

    #[test]
    fn a_compact_pill_sits_at_the_left_the_middle_or_the_right() {
        let at = |align: &str| plate(STRIP, (16, 8), Placing::of(settings::BAR_SHAPES[1], align));
        let (left, middle, right) = (
            at(settings::BAR_ALIGNS[0]),
            at(settings::BAR_ALIGNS[1]),
            at(settings::BAR_ALIGNS[2]),
        );
        assert_eq!((left.x, left.width), (16, 246));
        assert_eq!(middle.x, 16 + (448 - 246) / 2);
        assert_eq!(right.x + right.width, 464);
        // An align it does not know is the right.
        assert_eq!(at("anything"), right);
    }

    #[test]
    fn the_items_room_is_the_plate_less_its_padding_and_never_underflows() {
        assert_eq!(
            inner(Rect::new(10, 20, 100, 30), 6),
            Rect::new(16, 20, 88, 30)
        );
        assert_eq!(inner(Rect::new(0, 0, 4, 30), 6).width, 0);
    }
}
