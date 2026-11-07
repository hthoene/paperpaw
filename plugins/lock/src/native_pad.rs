//! The lock screen's geometry and what is entered on it: the keypad of a PIN,
//! the three by three dots of a pattern, and the code's dots. Pure: sizes in,
//! rectangles and digits out.
use alloc::string::String;
use paperpaw_sdk::{Point, Rect};

/// The keypad of a PIN, row by row.
pub const KEYS: [&str; 12] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", DELETE, "0", OK];
/// The key that deletes the last digit.
pub const DELETE: &str = "delete";
/// The key that submits.
pub const OK: &str = "OK";
/// The button focus of a pattern's *OK* where there is no touch.
pub const PATTERN_OK: usize = 9;

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
    fn mm(&self, mm: u32) -> u16 {
        let px = u32::from(self.dpi).saturating_mul(mm) * 10 / 254;
        u16::try_from(px).unwrap_or(u16::MAX)
    }
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
    #[must_use]
    pub fn new(room: &Room) -> Self {
        let (w, h) = (room.width, room.height);
        let (row, gap) = (room.row, room.gap);
        let header = room.header.is_some();
        let mut y = match room.header {
            Some(header) => header.saturating_add(gap.saturating_mul(2)),
            None => h / 12,
        };
        let list_top = y;
        let clock_height = if header {
            0
        } else {
            room.clock_lines.0.saturating_add(room.clock_lines.1)
        };
        let clock = Rect::new(0, y, w, clock_height);
        y = y.saturating_add(clock_height).saturating_add(if header {
            0
        } else {
            gap.saturating_mul(3)
        });
        let title = Rect::new(0, y, w, if header { 0 } else { row });
        y = y.saturating_add(title.height);
        let dots = Rect::new(0, y, w, row);
        y = y.saturating_add(row);
        let message = Rect::new(0, y, w, row);
        y = y.saturating_add(row).saturating_add(gap);
        // The keypad at the bottom: round keys, at most 15 mm across, three a
        // row with room between them.
        let bottom = h.saturating_sub((h / 14).max(room.margin.saturating_mul(2)));
        let free = bottom.saturating_sub(y);
        let width = w.saturating_sub(room.margin.saturating_mul(4));
        let d = room.mm(15).min(free / 19 * 4).min(width / 4).max(8);
        let (across, down) = (d / 2 * 3, d / 4 * 5);
        let pad_top = bottom.saturating_sub(down.saturating_mul(3).saturating_add(d));
        let keys = core::array::from_fn(|i| {
            let (col, line) = ((i % 3) as u16, (i / 3) as u16);
            Rect::new(
                (w / 2)
                    .saturating_sub(d / 2)
                    .saturating_sub(across)
                    .saturating_add(col.saturating_mul(across)),
                pad_top.saturating_add(line.saturating_mul(down)),
                d,
                d,
            )
        });
        // The pattern: three by three dots in the same room, OK below where
        // there is no touch.
        let ok_room = if room.touch {
            0
        } else {
            room.button.saturating_add(row)
        };
        let step = room
            .mm(22)
            .min(width / 3)
            .min(free.saturating_sub(ok_room) / 3)
            .max(3);
        let x0 = w.saturating_sub(step.saturating_mul(3)) / 2;
        let y0 = y.saturating_add(
            free.saturating_sub(ok_room.saturating_add(step.saturating_mul(3))) / 2,
        );
        let points = core::array::from_fn(|i| {
            Point::new(
                i32::from(x0) + (i % 3) as i32 * i32::from(step) + i32::from(step / 2),
                i32::from(y0) + (i / 3) as i32 * i32::from(step) + i32::from(step / 2),
            )
        });
        let ok_w = w / 3;
        let ok = Rect::new(
            w.saturating_sub(ok_w) / 2,
            y0.saturating_add(step.saturating_mul(3))
                .saturating_add(row / 2),
            ok_w,
            room.button,
        );
        let password = Rect::new(
            w / 6,
            y.saturating_add(free.saturating_sub(room.button) / 3),
            w / 3 * 2,
            room.button,
        );
        Self {
            clock,
            title,
            dots,
            message,
            list_top,
            keys,
            points,
            radius: i32::from(step / 10).max(4),
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

/// What is entered: the digits of a PIN or the dots of a pattern (`0` to `8`).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Entered(pub String);

impl Entered {
    /// A key of the keypad: what it asks for.
    pub fn press(&mut self, key: usize) -> Pressed {
        match KEYS.get(key) {
            Some(&DELETE) => {
                self.0.pop();
                Pressed::Changed
            }
            Some(&OK) => Pressed::Submit(core::mem::take(&mut self.0)),
            Some(digit) if self.0.len() < crate::native_core::MAX_PIN => {
                if self.0.try_reserve(1).is_err() {
                    return Pressed::Nothing;
                }
                self.0.push_str(digit);
                Pressed::Changed
            }
            _ => Pressed::Nothing,
        }
    }

    /// Adds dot `dot` to the pattern, once. Whether it was added.
    pub fn add_dot(&mut self, dot: usize) -> bool {
        let Some(c) = u8::try_from(dot).ok().filter(|d| *d < 9) else {
            return false;
        };
        let c = char::from(b'0' + c);
        if self.0.contains(c) || self.0.try_reserve(1).is_err() {
            return false;
        }
        self.0.push(c);
        true
    }

    /// The dots of the pattern so far, in order.
    pub fn dots(&self) -> impl Iterator<Item = usize> + '_ {
        self.0
            .bytes()
            .map(|b| usize::from(b.saturating_sub(b'0')).min(8))
    }
}

/// What a key came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pressed {
    Nothing,
    Changed,
    Submit(String),
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

    #[test]
    fn digits_are_entered_deleted_and_submitted() {
        let mut e = Entered::default();
        assert_eq!(e.press(0), Pressed::Changed);
        assert_eq!(e.press(10), Pressed::Changed);
        assert_eq!(e.0, "10");
        assert_eq!(e.press(9), Pressed::Changed);
        assert_eq!(e.0, "1");
        assert_eq!(e.press(11), Pressed::Submit("1".into()));
        assert_eq!(e.0, "");
        assert_eq!(e.press(99), Pressed::Nothing);
    }

    #[test]
    fn a_pin_stops_at_sixteen_digits() {
        let mut e = Entered::default();
        for _ in 0..40 {
            e.press(4);
        }
        assert_eq!(e.0.len(), crate::native_core::MAX_PIN);
    }

    #[test]
    fn a_dot_of_the_pattern_counts_once() {
        let mut e = Entered::default();
        assert!(e.add_dot(4));
        assert!(!e.add_dot(4));
        assert!(e.add_dot(0));
        assert!(!e.add_dot(9));
        assert!(!e.add_dot(usize::MAX));
        assert_eq!(e.dots().collect::<alloc::vec::Vec<_>>(), [4, 0]);
    }
}
