//! Shapes a widget draws that the native canvas has no call for, made of the
//! calls it has: circles and rings of rounded squares, lines, discs cut by a
//! line or an arc as spans one pixel high.
use crate::{Paint, paint};
use alloc::vec::Vec;
use paperpaw_sdk::{Color, Point, Rect, native::Error};

/// The rectangle `cx - r .. cx + r` squared round a centre, cut to what a
/// `Rect` holds (a circle further out than the screen's origin is cut there).
fn square(centre: Point, radius: u32) -> Rect {
    let r = i64::from(radius);
    let origin = |at: i32| crate::geometry::clamp_u16(i64::from(at) - r);
    let size = crate::geometry::clamp_u16(2 * r);
    Rect::new(origin(centre.x), origin(centre.y), size, size)
}

/// A disc of `radius` pixels round `centre` (the corner between four
/// pixels: `2 * radius` across).
///
/// # Errors
/// [`Error::NoMemory`] when the list cannot grow.
pub fn fill_circle(
    out: &mut Vec<Paint>,
    centre: Point,
    radius: u32,
    color: Color,
) -> Result<(), Error> {
    if radius == 0 {
        return Ok(());
    }
    paint::push(
        out,
        Paint::Focus(square(centre, radius), radius, None, color),
    )
}

/// A circle's outline of `radius` pixels round `centre`, `thickness` wide,
/// growing inwards.
///
/// # Errors
/// [`Error::NoMemory`] when the list cannot grow.
pub fn stroke_circle(
    out: &mut Vec<Paint>,
    centre: Point,
    radius: u32,
    thickness: u32,
    color: Color,
) -> Result<(), Error> {
    if radius == 0 || thickness == 0 {
        return Ok(());
    }
    paint::push(
        out,
        Paint::Focus(square(centre, radius), radius, Some(thickness), color),
    )
}

/// A straight line `width` pixels thick.
///
/// # Errors
/// [`Error::NoMemory`] when the list cannot grow.
pub fn line(
    out: &mut Vec<Paint>,
    from: Point,
    to: Point,
    width: u32,
    color: Color,
) -> Result<(), Error> {
    paint::push(
        out,
        Paint::Line {
            from,
            to,
            width: width.max(1),
            color,
        },
    )
}

/// A rectangle in `color`.
///
/// # Errors
/// [`Error::NoMemory`] when the list cannot grow.
pub fn fill(out: &mut Vec<Paint>, rect: Rect, color: Color) -> Result<(), Error> {
    if rect.width == 0 || rect.height == 0 {
        return Ok(());
    }
    paint::push(out, Paint::Fill(rect, color))
}

/// Fills, row by row, the pixels of the box `x .. x + width`, `y .. y + height`
/// for which `inside(column, row)` holds (offsets from the box's corner), as
/// spans one pixel high: a mask without a mask call. The spans of a row are
/// joined; at most `width * height / 2` spans are made.
///
/// # Errors
/// [`Error::NoMemory`] when the list cannot grow.
pub fn spans(
    out: &mut Vec<Paint>,
    (x, y): (i32, i32),
    (width, height): (u32, u32),
    color: Color,
    mut inside: impl FnMut(u32, u32) -> bool,
) -> Result<(), Error> {
    for row in 0..height {
        let mut start: Option<u32> = None;
        for column in 0..=width {
            let on = column < width && inside(column, row);
            match (start, on) {
                (None, true) => start = Some(column),
                (Some(from), false) => {
                    let left = i64::from(x) + i64::from(from);
                    let top = i64::from(y) + i64::from(row);
                    if left >= 0 && top >= 0 {
                        fill(
                            out,
                            Rect::new(
                                crate::geometry::clamp_u16(left),
                                crate::geometry::clamp_u16(top),
                                crate::geometry::clamp_u16(i64::from(column - from)),
                                1,
                            ),
                            color,
                        )?;
                    }
                    start = None;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// A line from `a` to `b`, `width` pixels thick, with round ends: the pixels
/// whose middle lies within half the width of the segment, as spans. (The
/// canvas pen is square; the interface-6 capsule had smooth edges, the spans
/// have none.)
///
/// # Errors
/// [`Error::NoMemory`] when the list cannot grow.
pub fn capsule(
    out: &mut Vec<Paint>,
    a: Point,
    b: Point,
    width: u32,
    color: Color,
) -> Result<(), Error> {
    // In eighths of a pixel.
    let half = i64::from(width.max(1)) * 4;
    let (ax, ay) = (i64::from(a.x) * 8 + 4, i64::from(a.y) * 8 + 4);
    let (bx, by) = (i64::from(b.x) * 8 + 4, i64::from(b.y) * 8 + 4);
    let (dx, dy) = (bx - ax, by - ay);
    let length2 = dx * dx + dy * dy;
    // How far a point is from the segment, squared.
    let distance2 = |px: i64, py: i64| -> i64 {
        let (qx, qy) = (px - ax, py - ay);
        let along = qx * dx + qy * dy;
        if length2 == 0 || along <= 0 {
            qx * qx + qy * qy
        } else if along >= length2 {
            let (rx, ry) = (px - bx, py - by);
            rx * rx + ry * ry
        } else {
            let across = qx * dy - qy * dx;
            across * across / length2
        }
    };
    let pad = i32::try_from(width / 2 + 2).unwrap_or(i32::MAX / 4);
    let left = a.x.min(b.x).saturating_sub(pad);
    let top = a.y.min(b.y).saturating_sub(pad);
    let w = (a.x.abs_diff(b.x)).saturating_add(2 * pad.unsigned_abs()) + 1;
    let h = (a.y.abs_diff(b.y)).saturating_add(2 * pad.unsigned_abs()) + 1;
    spans(out, (left, top), (w, h), color, |x, y| {
        let px = (i64::from(left) + i64::from(x)) * 8 + 4;
        let py = (i64::from(top) + i64::from(y)) * 8 + 4;
        distance2(px, py) <= half * half
    })
}

/// Whether direction `b` lies less than half a turn clockwise after
/// direction `a` (both east and north parts).
const fn clockwise_after(a: (i64, i64), b: (i64, i64)) -> bool {
    a.1 * b.0 - a.0 * b.1 > 0
}

/// The east and north parts of the direction `degrees` clockwise from twelve
/// o'clock.
fn direction(degrees: i32) -> (i64, i64) {
    let (sin, cos) = paperpaw_sdk::round::sin_cos(degrees);
    (i64::from(sin), i64::from(cos))
}

/// Whether the direction east `x`, north `y` lies on the arc of `sweep`
/// degrees clockwise from `from`.
fn on_arc(from: i32, sweep: u32, (x, y): (i64, i64)) -> bool {
    let sweep = sweep.min(360);
    if sweep >= 360 {
        return true;
    }
    if sweep == 0 {
        return false;
    }
    let (a, to) = (
        direction(from),
        direction(from.rem_euclid(360) + sweep as i32),
    );
    let p = (x, y);
    if sweep == 180 {
        // The half plane clockwise of `from` and the direction of `from`
        // itself, so that two halves of a ring share no edge.
        let along = a.1 * p.0 == a.0 * p.1 && a.0 * p.0 + a.1 * p.1 > 0;
        return clockwise_after(a, p) || along;
    }
    let between = |a, b| !clockwise_after(p, a) && clockwise_after(p, b);
    if sweep <= 180 {
        between(a, to)
    } else {
        !(!clockwise_after(p, to) && clockwise_after(p, a))
    }
}

/// An arc of a ring of `radius` pixels round `centre`, `thickness` wide,
/// growing inwards: `sweep` degrees clockwise from `from` (degrees clockwise
/// from twelve o'clock), e.g. a progress ring. Made of spans one pixel high.
///
/// # Errors
/// [`Error::NoMemory`] when the list cannot grow.
pub fn fill_ring(
    out: &mut Vec<Paint>,
    centre: Point,
    radius: u32,
    thickness: u32,
    (from, sweep): (i32, u32),
    color: Color,
) -> Result<(), Error> {
    let radius = radius.min(1024);
    if radius == 0 || thickness == 0 || sweep == 0 {
        return Ok(());
    }
    if sweep >= 360 {
        return if thickness >= radius {
            fill_circle(out, centre, radius, color)
        } else {
            stroke_circle(out, centre, radius, thickness, color)
        };
    }
    let outer = i64::from(2 * radius);
    let inner = i64::from(2 * radius.saturating_sub(thickness));
    let r = i64::from(radius);
    spans(
        out,
        (centre.x - radius as i32, centre.y - radius as i32),
        (2 * radius, 2 * radius),
        color,
        |column, row| {
            // The pixel's middle against the centre, in half pixels.
            let dx = 2 * i64::from(column) + 1 - 2 * r;
            let dy = 2 * i64::from(row) + 1 - 2 * r;
            let d2 = dx * dx + dy * dy;
            d2 <= outer * outer && d2 >= inner * inner && on_arc(from, sweep, (dx, -dy))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_circle_is_a_rounded_square_round_its_centre() {
        let mut out = Vec::new();
        fill_circle(&mut out, Point::new(50, 40), 10, Color::BLACK).unwrap();
        stroke_circle(&mut out, Point::new(50, 40), 10, 2, Color::BLACK).unwrap();
        fill_circle(&mut out, Point::new(5, 5), 0, Color::BLACK).unwrap();
        assert_eq!(out.len(), 2);
        match &out[0] {
            Paint::Focus(rect, radius, None, _) => {
                assert_eq!((rect.x, rect.y, rect.width, rect.height), (40, 30, 20, 20));
                assert_eq!(*radius, 10);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_circle_off_the_corner_is_cut_not_wrapped() {
        let rect = square(Point::new(-5, 3), 10);
        assert_eq!((rect.x, rect.y), (0, 0));
    }

    #[test]
    fn an_arc_covers_its_quarter_and_not_the_rest() {
        // A quarter from twelve to three: the upper right, nothing else.
        let mut out = Vec::new();
        fill_ring(&mut out, Point::new(30, 30), 20, 20, (0, 90), Color::BLACK).unwrap();
        assert!(!out.is_empty());
        for op in &out {
            let Paint::Fill(rect, _) = op else {
                panic!("{op:?}")
            };
            // Right of the centre column, above the centre row.
            assert!(rect.x >= 30 && u32::from(rect.y) < 30, "{rect:?}");
        }
        // A full sweep is a rounded square, not spans.
        let mut full = Vec::new();
        fill_ring(
            &mut full,
            Point::new(30, 30),
            20,
            20,
            (0, 360),
            Color::BLACK,
        )
        .unwrap();
        assert!(matches!(full.as_slice(), [Paint::Focus(..)]));
        // Nothing for an empty sweep or thickness.
        let mut none = Vec::new();
        fill_ring(&mut none, Point::new(30, 30), 20, 5, (0, 0), Color::BLACK).unwrap();
        fill_ring(&mut none, Point::new(30, 30), 20, 0, (0, 90), Color::BLACK).unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn the_two_halves_of_a_ring_leave_no_pixel_out_and_none_twice() {
        for (a, b) in [(0, 180), (90, 270)] {
            for (x, y) in [(5, 3), (-4, 7), (0, 9), (0, -9), (9, 0), (-9, 0), (-3, -3)] {
                let first = on_arc(a, 180, (x, y));
                let second = on_arc(b, 180, (x, y));
                assert!(first != second, "{a} {x},{y}");
            }
        }
    }

    #[test]
    fn a_capsule_covers_its_line_and_round_ends_only() {
        let mut out = Vec::new();
        capsule(
            &mut out,
            Point::new(20, 20),
            Point::new(40, 20),
            4,
            Color::BLACK,
        )
        .unwrap();
        let rects: Vec<_> = out
            .iter()
            .map(|op| match op {
                Paint::Fill(r, _) => *r,
                other => panic!("{other:?}"),
            })
            .collect();
        assert!(!rects.is_empty());
        // Four thick: rows 18 to 21, from the left end's round cap to the right.
        assert!(rects.iter().all(|r| (18..22).contains(&r.y)), "{rects:?}");
        let (left, right) = (
            rects.iter().map(|r| r.x).min().unwrap(),
            rects.iter().map(|r| r.x + r.width).max().unwrap(),
        );
        assert!(left <= 18 && right >= 42, "{left} {right}");
    }

    #[test]
    fn spans_join_a_rows_runs() {
        let mut out = Vec::new();
        // A mask of two runs in the first row and one in the second.
        let rows = [[1, 1, 0, 1], [0, 1, 1, 1]];
        spans(&mut out, (10, 20), (4, 2), Color::BLACK, |c, r| {
            rows[r as usize][c as usize] == 1
        })
        .unwrap();
        let rects: Vec<_> = out
            .iter()
            .map(|op| match op {
                Paint::Fill(r, _) => (r.x, r.y, r.width, r.height),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(rects, [(10, 20, 2, 1), (13, 20, 1, 1), (11, 21, 3, 1)]);
    }
}
