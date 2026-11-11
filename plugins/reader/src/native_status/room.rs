//! The room the status lines take, from the settings and the display alone:
//! the height of a place depends only on the text size of status lines, so
//! switching lines never changes a page. Whole numbers throughout.
use paperpaw_sdk::Rect;

/// The text sizes the setting allows, in points (`reader.status.size`).
const SIZE_MIN: i64 = 5;
const SIZE_MAX: i64 = 16;
/// The edge distances the setting allows, in tenths of a millimetre.
const EDGE_MAX: i64 = 150;
/// What keeps the text from a status line, in tenths of a millimetre.
const GAP_TENTH_MM: u32 = 20;

/// Where status lines are and how high, in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Room {
    /// There is a line above the text.
    pub top: bool,
    /// There is a line below the text.
    pub bottom: bool,
    /// The text size of the lines.
    pub size: u16,
    /// A line's height: 2.1 times the text size.
    pub height: u16,
    /// A line's distance from the display's edge.
    pub edge: u16,
    /// What keeps the text from a line.
    gap: u16,
}

fn tenth_mm(tenths: u32, dpi: u16) -> u32 {
    (tenths * u32::from(dpi) + 127) / 254
}

impl Room {
    /// No status lines.
    pub const NONE: Self = Self {
        top: false,
        bottom: false,
        size: 0,
        height: 0,
        edge: 0,
        gap: 0,
    };

    /// The room of lines of `size_pt` points, `edge` tenths of a millimetre
    /// from the edge, on a display of `dpi`; `top` and `bottom` say which places
    /// have lines. A value the setting does not allow is kept to what it does.
    #[must_use]
    pub fn new(top: bool, bottom: bool, (size_pt, edge): (i64, i64), dpi: u16) -> Self {
        let pt = u32::try_from(size_pt.clamp(SIZE_MIN, SIZE_MAX)).unwrap_or(6);
        let size = (pt * u32::from(dpi) + 36) / 72;
        let height = (size * 21 + 5) / 10;
        let edge = u32::try_from(edge.clamp(0, EDGE_MAX)).unwrap_or(0);
        let narrow = |value: u32| u16::try_from(value).unwrap_or(u16::MAX);
        Self {
            top,
            bottom,
            size: narrow(size),
            height: narrow(height),
            edge: narrow(tenth_mm(edge, dpi)),
            gap: narrow(tenth_mm(GAP_TENTH_MM, dpi)),
        }
    }

    /// How far from the display's top and bottom edge the text must stay for the
    /// lines there (0 for a place without a line).
    #[must_use]
    pub fn reserved(&self) -> (u16, u16) {
        let one = self
            .edge
            .saturating_add(self.height)
            .saturating_add(self.gap);
        (
            if self.top { one } else { 0 },
            if self.bottom { one } else { 0 },
        )
    }

    /// The line at the top (`true`) or the bottom of a display `height` high,
    /// between `x` and `x + width`; none for a place without a line.
    #[must_use]
    pub fn strip(&self, top: bool, (x, width): (u16, u16), display_height: u16) -> Option<Rect> {
        let shown = if top { self.top } else { self.bottom };
        if !shown || self.height == 0 {
            return None;
        }
        let y = if top {
            self.edge
        } else {
            display_height.checked_sub(self.edge.saturating_add(self.height))?
        };
        Some(Rect::new(x, y, width, self.height))
    }

    /// Where a tap counts for the line at the top or the bottom: its strip
    /// reaching to the edge of the display, so a thumb at the edge hits it.
    #[must_use]
    pub fn zone(&self, top: bool, display: (u16, u16)) -> Option<Rect> {
        let strip = self.strip(top, (0, display.0), display.1)?;
        Some(if top {
            Rect::new(0, 0, display.0, strip.y.saturating_add(strip.height))
        } else {
            Rect::new(0, strip.y, display.0, display.1 - strip.y)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_height_depends_on_the_text_size_alone() {
        // 254 dpi: 10 pixels a millimetre; 6 pt is 21 px, a line 44 px high.
        let a = Room::new(true, false, (6, 15), 254);
        let b = Room::new(false, true, (6, 15), 254);
        assert_eq!((a.size, a.height, a.edge), (21, 44, 15));
        assert_eq!((b.size, b.height, b.edge), (21, 44, 15));
        assert_eq!(Room::new(true, true, (12, 15), 254).height, 88);
    }

    #[test]
    fn a_place_without_a_line_keeps_nothing() {
        let room = Room::new(false, true, (6, 15), 254);
        // Edge 15, height 44, the text's 2 mm of 20.
        assert_eq!(room.reserved(), (0, 79));
        assert_eq!(Room::new(true, false, (6, 15), 254).reserved(), (79, 0));
        assert_eq!(Room::NONE.reserved(), (0, 0));
        assert_eq!(room.strip(true, (40, 400), 800), None);
    }

    #[test]
    fn the_strips_sit_at_the_edges() {
        let room = Room::new(true, true, (6, 15), 254);
        assert_eq!(
            room.strip(true, (40, 400), 800),
            Some(Rect::new(40, 15, 400, 44))
        );
        assert_eq!(
            room.strip(false, (40, 400), 800),
            Some(Rect::new(40, 741, 400, 44))
        );
        // Their zones reach to the edge.
        assert_eq!(room.zone(true, (480, 800)), Some(Rect::new(0, 0, 480, 59)));
        assert_eq!(
            room.zone(false, (480, 800)),
            Some(Rect::new(0, 741, 480, 59))
        );
        // A display too small for the line has none.
        assert_eq!(room.strip(false, (0, 10), 30), None);
    }

    #[test]
    fn values_the_settings_do_not_allow_are_kept_to_what_they_do() {
        let wild = Room::new(true, false, (500, 9999), 254);
        assert_eq!(wild, Room::new(true, false, (16, 150), 254));
        let tiny = Room::new(true, false, (-3, -9), 254);
        assert_eq!(tiny, Room::new(true, false, (5, 0), 254));
        // A display with no density has lines of no height: none shown.
        assert_eq!(
            Room::new(true, true, (6, 15), 0).strip(true, (0, 10), 100),
            None
        );
    }
}
