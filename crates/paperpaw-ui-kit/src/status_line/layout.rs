//! Where the items of a status line go: the left, the centre and the right
//! part of a line, as interface 6 placed them (`paperpaw_sdk::status`). Pure
//! arithmetic on widths, so that it is tested without a font: what does not
//! fit is left out, the items that matter least first.
use alloc::vec::Vec;

/// An item listed in a part of a line, measured once.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Listed {
    /// Where it stands in the items asked for (to find it again).
    pub index: usize,
    /// How much it matters, 0 to 255: the least goes first.
    pub priority: u8,
    /// Its width, at least its least width.
    pub width: f32,
    /// A text may be shortened to its room; a drawn value is whole or not there.
    pub text: bool,
    /// Its least width.
    pub least: f32,
}

/// An item as the line places it: where it starts and how wide it is (a text
/// is shortened to that).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Laid {
    pub index: usize,
    pub x: f32,
    pub width: f32,
}

/// The three parts, left, centre, right.
pub type Parts = [Vec<Listed>; 3];

/// Leaves out of `parts` what does not fit `room` whole: the items of the
/// lowest priority, of equal ones the centre's, then the left's, then the
/// right's, each part's from its end. Texts count with a few letters, as they
/// are shortened to their room.
pub fn fit(size: f32, room: f32, parts: &mut Parts) {
    loop {
        let count: usize = parts.iter().map(Vec::len).sum();
        let mut need = size * 0.4 * count.saturating_sub(1) as f32;
        for item in parts.iter().flatten() {
            need += if item.text {
                item.width.min(size * 3.0)
            } else {
                item.width
            };
        }
        // A pixel of slack for rounding, as when the parts are placed.
        if need <= room + 1.0 {
            return;
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
            return;
        };
        parts[[1, 0, 2][rank]].remove(index);
    }
}

/// When the widths (with `gap`s between them) do not fit `room`: the longest
/// text and the width it may take, so short ones (the time) stay whole.
fn shrink(items: &[Listed], gap: f32, room: f32) -> Option<(usize, f32)> {
    let total =
        items.iter().map(|i| i.width).sum::<f32>() + gap * items.len().saturating_sub(1) as f32;
    if total <= room + 1.0 {
        return None;
    }
    let longest = (0..items.len())
        .filter(|&i| items[i].text)
        .max_by(|&a, &b| items[a].width.total_cmp(&items[b].width))?;
    Some((longest, (items[longest].width - (total - room)).max(0.0)))
}

/// Places the `items` of a part from `x` to `end`, with `gap`s between them.
fn place(out: &mut Vec<Laid>, items: &[Listed], gap: f32, (mut x, end): (f32, f32)) {
    let shrunk = shrink(items, gap, end - x);
    for (at, item) in items.iter().enumerate() {
        if x >= end {
            break;
        }
        let width = match shrunk {
            Some((longest, room)) if longest == at => room.min(item.width),
            _ => item.width,
        };
        // A pixel of slack for rounding.
        if !item.text && x + width > end + 1.0 {
            break;
        }
        let width = if item.text {
            width.min(end - x + 1.0).max(item.least.min(end - x + 1.0))
        } else {
            width
        };
        if out.try_reserve(1).is_err() {
            break;
        }
        out.push(Laid {
            index: item.index,
            x,
            width,
        });
        x += width + gap;
    }
}

/// Where each item of `parts` goes in the line from `x0` for `room` pixels,
/// at text size `size`; what does not fit is left out.
#[must_use]
pub fn layout(size: f32, (x0, room): (f32, f32), mut parts: Parts) -> Vec<Laid> {
    fit(size, room, &mut parts);
    let [left, center, right] = parts;
    // A gap as wide as the text is high; narrower, down to 40 %, where the items
    // would not fit whole with it (wide margins, large text).
    let all = [&left, &center, &right];
    let inked: f32 = all.iter().flat_map(|p| p.iter()).map(|i| i.width).sum();
    let gaps = all.iter().map(|p| p.len()).sum::<usize>().saturating_sub(1);
    let gap = if gaps == 0 {
        size
    } else {
        ((room - inked) / gaps as f32).max(size * 0.4).min(size)
    };
    let width = |items: &[Listed]| -> f32 {
        items.iter().map(|i| i.width).sum::<f32>() + gap * items.len().saturating_sub(1) as f32
    };
    let x1 = x0 + room;
    // The left keeps a few letters, the centre all it needs; the right gets the rest.
    let left_room = if left.is_empty() {
        0.0
    } else {
        width(&left).min(size * 3.0)
    };
    let center_need = if center.is_empty() {
        0.0
    } else {
        width(&center) + gap
    };
    let beside = left_room + if left.is_empty() { 0.0 } else { gap } + center_need;
    let right_room = (room - beside).max(room / 2.0);
    let right_x = x1 - width(&right).min(right_room);
    let center_room = (right_x - gap - x0 - left_room).max(0.0);
    let center_w = width(&center).min(center_room);
    // Centred, unless that runs into the right part.
    let center_x = (x0 + (room - center_w) / 2.0)
        .min(right_x - gap - center_w)
        .max(x0 + left_room);
    let left_end = if center.is_empty() { right_x } else { center_x } - gap;
    let mut laid = Vec::new();
    place(&mut laid, &center, gap, (center_x, right_x - gap));
    place(&mut laid, &right, gap, (right_x, x1));
    place(&mut laid, &left, gap, (x0, left_end));
    laid
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(index: usize, priority: u8, width: f32, text: bool) -> Listed {
        Listed {
            index,
            priority,
            width,
            text,
            least: 0.0,
        }
    }

    #[test]
    fn items_that_fit_stay_and_the_parts_keep_their_sides() {
        let parts = [
            alloc::vec![item(0, 100, 40.0, true)],
            Vec::new(),
            alloc::vec![item(1, 100, 30.0, false), item(2, 100, 20.0, false)],
        ];
        let laid = layout(10.0, (0.0, 300.0), parts);
        let by = |index: usize| laid.iter().find(|l| l.index == index).unwrap();
        assert_eq!(by(0).x, 0.0);
        // The right part ends at the line's end.
        let last = laid.iter().map(|l| l.x + l.width).fold(0.0, f32::max);
        assert!((last - 300.0).abs() < 0.01, "{last}");
        assert!(by(1).x < by(2).x);
    }

    #[test]
    fn what_does_not_fit_leaves_by_priority_then_the_centre_first() {
        let mut parts: Parts = [
            alloc::vec![item(0, 50, 40.0, false)],
            alloc::vec![item(1, 50, 40.0, false)],
            alloc::vec![item(2, 200, 40.0, false)],
        ];
        // Room for two of the three whole (with 40 % gaps).
        fit(10.0, 90.0, &mut parts);
        let left: Vec<usize> = parts.iter().flatten().map(|l| l.index).collect();
        // Equal priority: the centre's goes before the left's; the dear one stays.
        assert_eq!(left, [0, 2]);
    }

    #[test]
    fn a_long_text_is_shortened_before_a_short_one_or_a_drawn_value() {
        let parts = [
            alloc::vec![item(0, 100, 400.0, true)],
            Vec::new(),
            alloc::vec![item(1, 100, 40.0, true), item(2, 100, 20.0, false)],
        ];
        let laid = layout(10.0, (0.0, 200.0), parts);
        let by = |index: usize| laid.iter().find(|l| l.index == index).unwrap();
        assert!(by(0).width < 400.0, "the long text is cut");
        assert_eq!(by(1).width, 40.0);
        assert_eq!(by(2).width, 20.0);
    }

    #[test]
    fn nothing_is_placed_past_the_line_and_nothing_panics_on_no_room() {
        let parts = [
            alloc::vec![item(0, 1, f32::MAX, true)],
            alloc::vec![item(1, 1, 5.0, false)],
            alloc::vec![item(2, 1, 5.0, false)],
        ];
        for room in [0.0, 1.0, 10.0, 1000.0] {
            let laid = layout(10.0, (3.0, room), parts.clone());
            for l in &laid {
                assert!(
                    l.x >= 3.0 - 0.01 && l.x + l.width <= 3.0 + room + 1.5,
                    "{l:?}"
                );
            }
        }
        assert!(layout(10.0, (0.0, 100.0), [Vec::new(), Vec::new(), Vec::new()]).is_empty());
    }
}
