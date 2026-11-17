//! Status items: small pieces of information shown in a status line, such
//! as the time, the battery or the page.
//!
//! Any plugin adds items with
//! `Items::add_status_item`;
//! screens with a status line let the user choose which items it shows and
//! where (the reader does this for its lines above and below the text).
//! An item computes its value from the environment and from the context the
//! screen passes, e.g. the reader's `title`, `chapter`, `page` and
//! `progress` (see `context`).
//!
//! An item may do something when it is tapped ([`StatusItemSpec::tap`]:
//! the battery opens its settings), and says how much it matters
//! ([`StatusItemSpec::priority`]): where a line has no room for all its
//! items, those that matter least are left out first, never cut.
//!
//! Plugins can also add whole status lines
//! (`Items::add_status_line`),
//! which draw the line themselves: a progress bar, a large clock, anything.
//! The reader offers the lines the user chose, above and below the text,
//! and switches between them when the line is tapped.
//!
//! The places where status items show are [`Place`]s
//! (`Items::add_status_place`):
//! three settings of the items for a line's left, centre and right. The
//! reader has one above and one below the text, the home screen one in
//! its status widget; a status line drawn in a place is told which
//! ([`StatusFrame::place`]).

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use paperpaw_graphics::Canvas;
use paperpaw_platform::{Color, Rect};

use crate::fonts::FontHandle;
use crate::id::Id;
use crate::params::Params;

/// Parameters screens pass to status items. Items ignore what they do not
/// need; a screen passes what it knows.
pub(crate) mod context {}

/// Where and how a status line is drawn (see [`StatusLineSpec`]).
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct StatusFrame<'a> {
    pub area: Rect,
    /// The line above the text, else below it.
    pub top: bool,
    /// Text size in pixels, as the user set it for status lines.
    pub size: f32,
    pub color: Color,
    /// What the screen tells about its content (see `context`).
    pub context: &'a Params,
    /// The place it is drawn in ([`Place`]), whose items a line of items
    /// shows; `None` where the screen has none.
    pub place: Option<&'a str>,
    /// The font role its text is drawn in: the status lines' own
    /// ([`fonts::STATUS`](crate::builtin::fonts::STATUS)) unless the
    /// screen says another.
    pub font: &'a str,
    /// Levels (the battery) with their percentage beside the symbol, or
    /// the symbol alone.
    pub percent: bool,
}

impl<'a> StatusFrame<'a> {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(area: Rect, top: bool, size: f32, color: Color, context: &'a Params) -> Self {
        Self {
            area,
            top,
            size,
            color,
            context,
            place: None,
            font: crate::builtin::fonts::STATUS.static_str(),
            percent: true,
        }
    }

    /// Levels with their percentage, or the symbol alone.
    pub fn percent(self, percent: bool) -> Self {
        Self { percent, ..self }
    }

    /// Drawn in place `place` ([`Place::id`]).
    pub fn place(self, place: &'a str) -> Self {
        Self {
            place: Some(place),
            ..self
        }
    }

    /// Its text in the font of role `font`.
    pub fn font(self, font: &'a str) -> Self {
        Self { font, ..self }
    }
}




/// A place in a status line that does something: a tap on it runs
/// `command`, e.g. an item's [`StatusItemSpec::tap`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct StatusTarget {
    pub area: Rect,
    pub command: crate::Command,
}

impl StatusTarget {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(area: Rect, command: crate::Command) -> Self {
        Self { area, command }
    }
}




/// What a status item shows.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StatusValue {
    Text(String),
    /// A gauge with a percentage, drawn as a battery.
    Level {
        percent: u8,
        charging: bool,
    },
    /// An icon by name ([`icons`](crate::icons)), e.g. while reading
    /// aloud.
    Icon(Cow<'static, str>),
    /// A short bar filled this far, in thousandths, e.g. how far into a
    /// chapter the page is.
    Bar {
        permille: u16,
    },
    /// An icon by name with a text after it, e.g. Wi-Fi's and the
    /// network's name; shown whole or not at all.
    Labelled {
        icon: Cow<'static, str>,
        text: String,
    },
    /// A kind a later build adds, by its key, with its parameters (as
    /// [`Command::Extended`](crate::Command::Extended)); a build that
    /// doesn't know it leaves the value out.
    Extended {
        key: crate::Id,
        params: crate::Params,
    },
}


/// How much an item matters where none says ([`StatusItemSpec::priority`]).
pub(crate) const PRIORITY: u8 = 128;



/// A place where status items show: the settings of the items listed for
/// its left, centre and right (ids separated by commas, see
/// [`item_ids`]). A status line of items drawn there shows these
/// ([`StatusFrame::place`]); the settings' editors may list the places.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Place {
    pub id: Id,
    /// Translation key of its name, e.g. "Below the text".
    pub title: Cow<'static, str>,
    /// The settings of its left, centre and right.
    pub parts: [Id; 3],
    /// The setting of the status line it shows (the first of the ids it
    /// lists), where the place lets the reader choose: without one, or
    /// where it lists none, its items
    /// ([`status_lines::ITEMS`](crate::builtin::status_lines::ITEMS)).
    pub line: Option<Id>,
}

impl Place {
    /// Made of what it needs; the rest as set with its methods.
    pub const fn new(id: Id, title: &'static str, parts: [Id; 3]) -> Self {
        Self {
            id,
            title: Cow::Borrowed(title),
            parts,
            line: None,
        }
    }






}





/// The items of a list as stored in a setting: ids separated by commas.
pub fn item_ids(list: &str) -> impl Iterator<Item = &str> {
    list.split(',').map(str::trim).filter(|id| !id.is_empty())
}

/// A status line: the items listed (comma-separated ids) for its left,
/// centre and right, drawn in `area` with text of `size` in `color`.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct StatusLine<'a> {
    pub area: Rect,
    pub lists: [&'a str; 3],
    pub size: f32,
    pub color: Color,
    /// Levels (the battery) with their percentage beside the symbol, or
    /// the symbol alone.
    pub percent: bool,
}

/// An item as a line places it: where it starts, how wide it is, and
/// what it shows there (a text shortened to its room).
#[derive(Debug)]
struct Laid<'a> {
    id: &'a str,
    value: StatusValue,
    x: f32,
    width: f32,
    percent: bool,
}

/// An item listed in a part of a line: its value now, measured once.
struct Listed<'a> {
    id: &'a str,
    value: StatusValue,
    priority: u8,
    /// Its width with a level's percentage and without it, at least its
    /// least width.
    widths: [f32; 2],
    /// Its least width, in pixels.
    least: f32,
}

impl Listed<'_> {
    fn width(&self, percent: bool) -> f32 {
        self.widths[usize::from(!percent)]
    }

    fn text(&self) -> bool {
        matches!(self.value, StatusValue::Text(_))
    }
}

impl<'a> StatusLine<'a> {
    /// The items of `lists` in `area`, with text of `size` in `color`;
    /// levels with their percentage.
    pub fn new(area: Rect, lists: [&'a str; 3], size: f32, color: Color) -> Self {
        Self {
            area,
            lists,
            size,
            color,
            percent: true,
        }
    }

    /// Levels with their percentage beside the symbol, or the symbol
    /// alone.
    pub fn percent(self, percent: bool) -> Self {
        Self { percent, ..self }
    }

    /// Whether no item is listed.
    pub fn is_empty(&self) -> bool {
        self.lists
            .iter()
            .all(|list| item_ids(list).next().is_none())
    }



}


/// Leaves out of `parts` what does not fit `room` whole, and says whether
/// levels show their percentage (where `percent` wants it): the
/// percentage goes first, then the items of the lowest priority, of
/// equal ones the centre's, then the left's, then the right's, each
/// part's from its end; after each, the percentage is tried again. Texts
/// count with a few letters, as they are shortened to their room.
fn fit(size: f32, room: f32, parts: &mut [Vec<Listed<'_>>; 3], percent: bool) -> bool {
    loop {
        let levels = parts
            .iter()
            .flatten()
            .any(|item| matches!(item.value, StatusValue::Level { .. }));
        let tries: &[bool] = if percent && levels {
            &[true, false]
        } else {
            &[false]
        };
        for &with in tries {
            let count: usize = parts.iter().map(Vec::len).sum();
            let mut need = size * 0.4 * count.saturating_sub(1) as f32;
            for item in parts.iter().flatten() {
                let width = item.width(with);
                need += if item.text() {
                    width.min(size * 3.0)
                } else {
                    width
                };
            }
            // A pixel of slack for rounding, as when the parts are placed.
            if need <= room + 1.0 {
                return with && percent;
            }
        }
        let mut least: Option<(u8, usize, usize)> = None;
        for (rank, at) in [1, 0, 2].into_iter().enumerate() {
            for (index, item) in parts[at].iter().enumerate().rev() {
                if least.is_none_or(|(p, r, _)| (item.priority, rank) < (p, r)) {
                    least = Some((item.priority, rank, index));
                }
            }
        }
        let Some((_, rank, index)) = least else {
            return false;
        };
        parts[[1, 0, 2][rank]].remove(index);
    }
}


/// A bar's length: about as long as a short word.
fn bar_width(size: f32) -> f32 {
    size * 4.0
}

/// Between a labelled item's icon and its text.
fn label_gap(size: f32) -> f32 {
    size * 0.25
}


fn battery_width(size: f32) -> f32 {
    size * 1.3
}

/// Places the `items` of a part from `x` to `end`, with `gap`s between
/// them: what is not text whole or not at all, a gauge without its
/// percentage where only the gauge fits, the longest text shortened
/// where they do not all fit.
fn place<'a>(
    out: &mut Vec<Laid<'a>>,
    font: &FontHandle<'_>,
    (size, gap, with_percent): (f32, f32, bool),
    items: Vec<Listed<'a>>,
    (mut x, end): (f32, f32),
) {
    let widths: Vec<f32> = items.iter().map(|item| item.width(with_percent)).collect();
    let texts: Vec<bool> = items.iter().map(Listed::text).collect();
    let shrunk = shrink(&widths, &texts, gap, end - x);
    let budget = |i: usize| match shrunk {
        Some((at, width)) if at == i => width,
        _ => f32::MAX,
    };
    for (i, item) in items.into_iter().enumerate() {
        if x >= end {
            break;
        }
        // A pixel of slack for rounding, as for texts.
        let fits = |width: f32| x + width <= end + 1.0;
        let percent = with_percent && fits(widths[i]);
        let (value, width) = match item.value {
            StatusValue::Text(text) => {
                // A pixel of slack for rounding: the parts were measured to fit.
                let room = (end - x + 1.0).min(budget(i));
                let text = crate::ui::fit(font, size, &text, room);
                let width = font.measure(size, &text).max(item.least.min(room));
                (StatusValue::Text(text), width)
            }
            value => {
                let width = item.widths[usize::from(!percent)];
                if !fits(width) {
                    break;
                }
                (value, width)
            }
        };
        out.push(Laid {
            id: item.id,
            value,
            x,
            width,
            percent,
        });
        x += width + gap;
    }
}

/// The one line a status line's items are centred on: the middle of its
/// figures' ink (the digits of the time, the page, the percentage), in
/// the middle of the line's band, top and bottom alike. Text stands on
/// `baseline`; shapes and icons are centred on `twice` / 2 (twice the y,
/// so that a centre between two pixels is said exactly).
#[derive(Debug, Clone, Copy)]
struct Middle {
    baseline: i32,
    twice: i32,
    /// The figures' height, in pixels.
    figures: i32,
}

/// What a status line's figures are measured by.
const FIGURES: &str = "0123456789";

impl Middle {
    fn of(font: &FontHandle<'_>, area: Rect, size: f32) -> Self {
        let (top, bottom) = font.ink(size, FIGURES).unwrap_or_else(|| {
            let metrics = font.line_metrics(size);
            (-(metrics.ascent as i32), -(metrics.descent as i32))
        });
        let room = i32::from(area.height) - (bottom - top);
        // An odd pixel goes below, where the eye expects it.
        let baseline = i32::from(area.y) + room.div_euclid(2) - top;
        Middle {
            baseline,
            twice: 2 * baseline + top + bottom,
            figures: bottom - top,
        }
    }

    /// `height` made as long as it can be centred on the line to the
    /// pixel: as odd or as even as the figures' height (a pixel less
    /// where it is not), and the top of a shape that tall on it.
    fn fit(&self, height: i32) -> (i32, i32) {
        let height = if (height - self.figures) % 2 == 0 {
            height
        } else {
            height - 1
        }
        .max(1);
        (height, (self.twice - height).div_euclid(2))
    }
}


/// When values of `widths` (with `gap`s between them) do not fit `room`:
/// the longest text (`texts`) and the width it may take, so short ones
/// (the page, the time) stay whole. A pixel of slack for rounding: parts
/// are placed by their own width.
fn shrink(widths: &[f32], texts: &[bool], gap: f32, room: f32) -> Option<(usize, f32)> {
    let total = widths.iter().sum::<f32>() + gap * widths.len().saturating_sub(1) as f32;
    if total <= room + 1.0 {
        return None;
    }
    let longest = (0..widths.len())
        .filter(|&i| texts[i])
        .max_by(|&a, &b| widths[a].total_cmp(&widths[b]))?;
    Some((longest, (widths[longest] - (total - room)).max(0.0)))
}

/// A battery filled to `percent`, in the style of the UI's shapes: a rounded
/// body, a small rounded nub, the level as a rounded bar inside with a
/// gap as wide as the line all round, and a bolt when charging, in ink
/// with a rim of paper, so that it reads on the level, beside it
/// and across its edge alike. Crisp at every size: whole pixels only,
/// nothing blended.
fn draw_battery(
    canvas: &mut Canvas<'_>,
    area: Rect,
    size: f32,
    percent: u8,
    charging: bool,
    color: Color,
) {
    let (x, y, height) = (i32::from(area.x), i32::from(area.y), u32::from(area.height));
    let line = ((size / 10.0) as u32).max(1);
    let nub = ((size / 8.0) as u32).max(2);
    let width = u32::from(area.width).saturating_sub(nub);
    let radius = (height / 4).max(1);
    canvas.stroke_round_rect(x, y, width, height, line, radius, color);
    // The nub, half as tall as the body and centred on it to the pixel,
    // rounded on its outer side (its inner side lies on the body's line).
    let mut nub_height = height / 2;
    if (height - nub_height) % 2 == 1 {
        nub_height += 1;
    }
    let nub_radius = line.min(nub_height / 2).max(1);
    canvas.fill_round_rect(
        x + width as i32 - nub_radius as i32,
        y + ((height - nub_height) / 2) as i32,
        nub + nub_radius,
        nub_height,
        nub_radius,
        color,
    );
    // The level: a bar inside, as far from the line as the line is wide.
    let inset = 2 * line;
    let bar_height = height.saturating_sub(2 * inset);
    let full = width.saturating_sub(2 * inset);
    let fill = full * u32::from(percent.min(100)) / 100;
    let bar_radius = radius.saturating_sub(inset).max(1).min(bar_height / 2);
    let (bar_x, bar_y) = (x + inset as i32, y + inset as i32);
    if fill > 0 && bar_height > 0 {
        canvas.fill_round_rect(bar_x, bar_y, fill, bar_height, bar_radius, color);
    }
    if !charging {
        return;
    }
    // The bolt in the middle of the inside: in ink, with a rim of paper
    // a pixel wide inside the body.
    let mark_height = height.saturating_sub(2 * line + 2).max(3);
    let mark_width = (mark_height * 2 / 3).max(4);
    let left = x + (width as i32 - mark_width as i32) / 2;
    let top = y + (height as i32 - mark_height as i32) / 2;
    let in_mark = |column: i32, row: i32| {
        if column < 0 || row < 0 || column >= mark_width as i32 || row >= mark_height as i32 {
            return false;
        }
        in_bolt(column as u32, row as u32, mark_width, mark_height, line + 1)
    };
    let (inside_left, inside_right) = (x + line as i32, x + (width - line) as i32);
    let (inside_top, inside_bottom) = (y + line as i32, y + (height - line) as i32);
    for row in -1..=mark_height as i32 {
        for column in -1..=mark_width as i32 {
            let (px, py) = (left + column, top + row);
            let ink = if in_mark(column, row) {
                color
            } else if (-1..=1).any(|dy| (-1..=1).any(|dx| in_mark(column + dx, row + dy))) {
                Color::WHITE
            } else {
                continue;
            };
            if px >= inside_left && px < inside_right && py >= inside_top && py < inside_bottom {
                canvas.fill_rect(px, py, 1, 1, ink);
            }
        }
    }
}

/// Whether the pixel at `column` and `row` of a box `width` by `height`
/// is a lightning bolt's, drawn as pixels are at small sizes: a stroke
/// `thickness` wide from the top right down to the middle's left, a bar
/// across the middle, and a stroke from the middle's right down to the
/// bottom left.
fn in_bolt(column: u32, row: u32, width: u32, height: u32, thickness: u32) -> bool {
    let thickness = thickness.min(width / 2).max(1);
    let middle = height / 2;
    let run = width - thickness;
    let left = if row < middle {
        // Rounded to the nearest pixel, as the steps fall.
        (run * (middle - row) + middle / 2) / middle.max(1)
    } else if row == middle {
        return column < width;
    } else {
        let rest = height - 1 - middle;
        (run * (height - 1 - row) + rest / 2) / rest.max(1)
    };
    column >= left && column < left + thickness
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_lines_too_long_give_way() {
        // Battery and time placed by their own width: rounding noise is
        // no reason to cut the time.
        let widths = [60.0, 70.1];
        let room = 70.1 + 60.0 + 20.0 - 0.000_01;
        assert_eq!(shrink(&widths, &[false, true], 20.0, room), None);
        // Too long: the longest text gives way, the page stays whole.
        let widths = [300.0, 80.0];
        assert_eq!(
            shrink(&widths, &[true, true], 20.0, 250.0),
            Some((0, 150.0))
        );
        // Nothing to shorten without text.
        assert_eq!(shrink(&[100.0], &[false], 0.0, 50.0), None);
    }
}
