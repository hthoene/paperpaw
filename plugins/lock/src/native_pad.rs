//! The lock screen's geometry: the keypad of a PIN,
//! the three by three dots of a pattern, and the code's dots. Pure: sizes in,
//! rectangles out.
use paperpaw_sdk::{Point, Rect};

/// What the layout is made from, in pixels.
#[derive(Debug, Clone, Copy)]
pub struct Room {
    pub width: u16,
    pub height: u16,
    /// Pixels in `mm` millimetres: the screen's dots per inch.
    pub dpi: u16,
    pub margin: u16,
    pub gap: u16,
    pub row: u16,
    /// The header's height where there is one (setting up), else the clock is
    /// drawn.
    pub header: Option<u16>,
    /// Heights of the clock's two lines.
    pub clock_lines: (u16, u16),
    /// A button's height (the pattern's *OK*).
    pub button: u16,
    pub touch: bool,
}

impl Room {
    /// Pixels for `mm` millimetres, as the SDK's `Metrics::mm` has them:
    /// rounded, at least 1.
    fn mm(&self, mm: u32) -> u32 {
        let tenths = u64::from(mm) * 10;
        let px = (tenths * u64::from(self.dpi) + 127) / 254;
        let px = u32::try_from(px).unwrap_or(u32::MAX);
        if px == 0 && tenths > 0 { 1 } else { px }
    }
}

/// A length for a rectangle: saturated to what a rectangle holds.
fn px(length: u32) -> u16 {
    u16::try_from(length).unwrap_or(u16::MAX)
}

/// Where things are: as on a phone, the clock (when unlocking) or the header
/// (when setting up) at the top, then what is asked for, the code's dots and
/// a message, and the keypad or the pattern below.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub clock: Rect,
    pub title: Rect,
    /// The dots that stand for a PIN's digits.
    pub dots: Rect,
    pub message: Rect,
    /// Where the kinds are listed.
    pub list_top: u16,
    pub keys: [Rect; 12],
    /// The centres of the pattern's dots, and their radius.
    pub points: [Point; 9],
    pub radius: i32,
    pub ok: Rect,
    /// The password's button.
    pub password: Rect,
}

impl Layout {
    /// The layout, worked out in the order of the interface-6 screen (in 32
    /// bits, so that no size overflows) and cut to what a rectangle holds.
    #[must_use]
    pub fn new(room: &Room) -> Self {
        let (w, h) = (u32::from(room.width), u32::from(room.height));
        let (row, gap, margin) = (
            u32::from(room.row),
            u32::from(room.gap),
            u32::from(room.margin),
        );
        let header = room.header.is_some();
        let mut y = match room.header {
            Some(header) => u32::from(header) + gap * 2,
            None => h / 12,
        };
        let list_top = y;
        let clock_height = if header {
            0
        } else {
            u32::from(room.clock_lines.0) + u32::from(room.clock_lines.1)
        };
        let clock = Rect::new(0, px(y), room.width, px(clock_height));
        y += clock_height + if header { 0 } else { gap * 3 };
        let title_height = if header { 0 } else { row };
        let title = Rect::new(0, px(y), room.width, px(title_height));
        y += title_height;
        let dots = Rect::new(0, px(y), room.width, px(row));
        y += row;
        let message = Rect::new(0, px(y), room.width, px(row));
        y += row + gap;
        // The keypad at the bottom: round keys, at most 15 mm across, three a
        // row with room between them.
        let bottom = h.saturating_sub((h / 14).max(margin * 2));
        let free = bottom.saturating_sub(y);
        let width = w.saturating_sub(margin * 4);
        let d = room.mm(15).min(free * 4 / 19).min(width / 4).max(8);
        let (across, down) = (d * 3 / 2, d * 5 / 4);
        let pad_top = bottom.saturating_sub(down * 3 + d);
        let keys = core::array::from_fn(|i| {
            let (col, line) = ((i % 3) as u32, (i / 3) as u32);
            Rect::new(
                px((w / 2).saturating_sub(d / 2).saturating_sub(across) + col * across),
                px(pad_top + line * down),
                px(d),
                px(d),
            )
        });
        // The pattern: three by three dots in the same room, OK below where
        // there is no touch.
        let button = u32::from(room.button);
        let ok_room = if room.touch { 0 } else { button + row };
        let step = room
            .mm(22)
            .min(width / 3)
            .min(free.saturating_sub(ok_room) / 3)
            .max(3);
        let x0 = w.saturating_sub(step * 3) / 2;
        let y0 = y + free.saturating_sub(ok_room + step * 3) / 2;
        let points = core::array::from_fn(|i| {
            Point::new(
                i32::from(px(x0 + (i % 3) as u32 * step + step / 2)),
                i32::from(px(y0 + (i / 3) as u32 * step + step / 2)),
            )
        });
        let ok_w = w / 3;
        let ok = Rect::new(
            px(w.saturating_sub(ok_w) / 2),
            px(y0 + step * 3 + row / 2),
            px(ok_w),
            px(button),
        );
        let password = Rect::new(
            px(w / 6),
            px(y + free.saturating_sub(button) / 3),
            px(w * 2 / 3),
            px(button),
        );
        Self {
            clock,
            title,
            dots,
            message,
            list_top: px(list_top),
            keys,
            points,
            radius: i32::try_from(step / 10).unwrap_or(i32::MAX).max(4),
            ok,
            password,
        }
    }

    /// The pattern's dot at `point`, if near one.
    #[must_use]
    pub fn dot_at(&self, point: Point) -> Option<usize> {
        let reach = i64::from(self.radius) * 3;
        self.points.iter().position(|p| {
            let (dx, dy) = (
                i64::from(p.x) - i64::from(point.x),
                i64::from(p.y) - i64::from(point.y),
            );
            dx * dx + dy * dy <= reach * reach
        })
    }

    /// The key of the keypad at `point`.
    #[must_use]
    pub fn key_at(&self, point: Point) -> Option<usize> {
        self.keys.iter().position(|rect| rect.contains(point))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(w: u16, h: u16, touch: bool, header: bool) -> Room {
        Room {
            width: w,
            height: h,
            dpi: 220,
            margin: 16,
            gap: 8,
            row: 56,
            header: header.then_some(48),
            clock_lines: (80, 24),
            button: 44,
            touch,
        }
    }

    #[test]
    fn the_keypad_is_three_columns_of_four_rows_inside_the_screen() {
        let l = Layout::new(&room(480, 800, true, false));
        for (i, key) in l.keys.iter().enumerate() {
            assert!(
                key.x + key.width <= 480 && key.y + key.height <= 800,
                "{i}: {key:?}"
            );
            assert_eq!(key.width, key.height);
        }
        assert!(l.keys[0].x < l.keys[1].x && l.keys[1].x < l.keys[2].x);
        assert!(l.keys[0].y < l.keys[3].y && l.keys[9].y > l.keys[6].y);
        assert!(l.keys[0].y > l.message.y, "below the message");
    }

    #[test]
    fn the_pattern_is_a_square_of_dots_and_each_is_found_by_a_touch_near_it() {
        let l = Layout::new(&room(480, 800, true, false));
        for (i, p) in l.points.iter().enumerate() {
            assert_eq!(l.dot_at(*p), Some(i));
            assert_eq!(l.dot_at(Point::new(p.x + l.radius, p.y)), Some(i));
        }
        assert_eq!(l.dot_at(Point::new(-50, -50)), None);
        assert!(l.points[0].x < l.points[1].x && l.points[0].y < l.points[3].y);
    }

    #[test]
    fn a_header_moves_everything_down_and_leaves_no_clock() {
        let a = Layout::new(&room(480, 800, true, false));
        let b = Layout::new(&room(480, 800, true, true));
        assert_eq!(b.clock.height, 0);
        assert_eq!(b.title.height, 0);
        assert!(a.clock.height > 0 && a.title.height > 0);
        assert_eq!(b.list_top, 48 + 16);
    }

    #[test]
    fn without_touch_the_ok_button_sits_below_the_pattern() {
        let l = Layout::new(&room(480, 800, false, false));
        assert!(l.ok.y >= l.points[8].y as u16);
        assert!(l.ok.y + l.ok.height <= 800);
    }

    #[test]
    fn tiny_screens_do_not_overflow() {
        for (w, h) in [(0, 0), (1, 1), (20, 20), (100, 60), (u16::MAX, u16::MAX)] {
            let _ = Layout::new(&room(w, h, false, false));
            let _ = Layout::new(&room(w, h, true, true));
        }
    }

    /// The interface-6 screen's arithmetic, in 16 bits as it was written.
    fn reference(room: &Room) -> ([Rect; 12], [Point; 9], Rect, Rect, i32) {
        let (w, h) = (room.width, room.height);
        let (row, gap, margin) = (room.row, room.gap, room.margin);
        let mm = |mm: u32| {
            let tenths = mm * 10;
            let px = (tenths * u32::from(room.dpi) + 127) / 254;
            (if px == 0 && tenths > 0 { 1 } else { px }) as u16
        };
        let header = room.header.is_some();
        let mut y = room.header.map_or(h / 12, |header| header + gap * 2);
        let clock = if header {
            0
        } else {
            room.clock_lines.0 + room.clock_lines.1
        };
        y += clock + if header { 0 } else { gap * 3 };
        y += if header { 0 } else { row };
        y += row;
        y += row;
        y += gap;
        let bottom = h.saturating_sub((h / 14).max(margin * 2));
        let free = bottom.saturating_sub(y);
        let width = w.saturating_sub(margin * 4);
        let d = mm(15).min(free * 4 / 19).min(width / 4).max(8);
        let (across, down) = (d * 3 / 2, d * 5 / 4);
        let pad_top = bottom.saturating_sub(down * 3 + d);
        let keys = core::array::from_fn(|i| {
            let (col, line) = ((i % 3) as u16, (i / 3) as u16);
            Rect::new(
                w / 2 - d / 2 - across + col * across,
                pad_top + line * down,
                d,
                d,
            )
        });
        let ok_room = if room.touch { 0 } else { room.button + row };
        let step = mm(22)
            .min(width / 3)
            .min(free.saturating_sub(ok_room) / 3)
            .max(3);
        let x0 = (w - step * 3) / 2;
        let y0 = y + (free.saturating_sub(ok_room + step * 3)) / 2;
        let points = core::array::from_fn(|i| {
            Point::new(
                i32::from(x0 + (i % 3) as u16 * step + step / 2),
                i32::from(y0 + (i / 3) as u16 * step + step / 2),
            )
        });
        let ok_w = w / 3;
        let ok = Rect::new((w - ok_w) / 2, y0 + step * 3 + row / 2, ok_w, room.button);
        let password = Rect::new(
            w / 6,
            y + free.saturating_sub(room.button) / 3,
            w * 2 / 3,
            room.button,
        );
        (keys, points, ok, password, i32::from(step / 10).max(4))
    }

    #[test]
    fn the_layout_is_the_interface_6_one_to_the_pixel() {
        for (w, h, dpi) in [
            (480, 800, 220),
            (800, 480, 220),
            (1200, 1600, 300),
            (600, 1024, 167),
        ] {
            for (touch, header) in [(true, false), (false, false), (true, true), (false, true)] {
                let mut r = room(w, h, touch, header);
                r.dpi = dpi;
                let l = Layout::new(&r);
                let (keys, points, ok, password, radius) = reference(&r);
                let which = alloc::format!("{w}x{h} dpi {dpi} touch {touch} header {header}");
                assert_eq!(l.keys, keys, "{which}");
                assert_eq!(l.points, points, "{which}");
                assert_eq!(l.ok, ok, "{which}");
                assert_eq!(l.password, password, "{which}");
                assert_eq!(l.radius, radius, "{which}");
            }
        }
    }
}
