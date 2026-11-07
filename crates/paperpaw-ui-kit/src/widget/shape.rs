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
    paint::push(out, Paint::Focus(square(centre, radius), radius, None, color))
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
