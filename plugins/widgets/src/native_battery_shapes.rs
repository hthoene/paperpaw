//! The shapes of the battery widget: the battery as drawn to scale, the
//! ring and the bolt.
use alloc::vec::Vec;
use paperpaw_sdk::{Color, Point, Rect, native::Error, round::point_at};
use paperpaw_ui_kit::{
    Paint, paint,
    widget::shape::{fill_circle, fill_ring, spans, stroke_circle},
};

/// The shade of a charge that is low: so the outline stands out with little
/// in it.
const LOW_SHADE: u8 = 110;

/// A battery drawn in `body`: rounded, with its knob (to the right, or on top
/// where standing), filled as far as charged.
///
/// # Errors
/// [`Error::NoMemory`] when the paint list cannot grow.
pub fn battery(
    out: &mut Vec<Paint>,
    (ink, paper): (Color, Color),
    body: Rect,
    (percent, low, standing): (u8, bool, bool),
    line: u16,
) -> Result<(), Error> {
    let short = body.width.min(body.height);
    if short < 6 {
        return Ok(());
    }
    let knob = (short * 12 / 100).max(2);
    let (shell, knob_at) = if standing {
        let shell = Rect::new(body.x, body.y + knob, body.width, body.height - knob);
        let w = body.width * 45 / 100;
        (
            shell,
            Rect::new(body.x + (body.width - w) / 2, body.y, w, knob + 1),
        )
    } else {
        let shell = Rect::new(body.x, body.y, body.width - knob, body.height);
        let h = body.height * 45 / 100;
        (
            shell,
            Rect::new(
                body.x + body.width - knob - 1,
                body.y + (body.height - h) / 2,
                knob + 1,
                h,
            ),
        )
    };
    let stroke = u32::from(short / 11).max(u32::from(line) * 2).max(2);
    let radius = u32::from(short / 5);
    paint::push(out, Paint::Focus(knob_at, u32::from(knob / 2), None, ink))?;
    paint::push(out, Paint::Focus(shell, radius, None, paper))?;
    paint::push(out, Paint::Focus(shell, radius, Some(stroke), ink))?;
    // The charge, inside with a little room.
    let pad = stroke as u16 + (short / 14).max(1);
    let inner = Rect::new(
        shell.x + pad,
        shell.y + pad,
        shell.width.saturating_sub(2 * pad),
        shell.height.saturating_sub(2 * pad),
    );
    let percent = u32::from(percent.min(100));
    let level = if standing {
        let h = (u32::from(inner.height) * percent / 100) as u16;
        Rect::new(inner.x, inner.y + inner.height - h, inner.width, h)
    } else {
        let w = (u32::from(inner.width) * percent / 100) as u16;
        Rect::new(inner.x, inner.y, w, inner.height)
    };
    if level.width > 0 && level.height > 0 {
        let radius = radius.saturating_sub(u32::from(pad)).max(1);
        let color = if low { Color::gray(LOW_SHADE) } else { ink };
        paint::push(out, Paint::Focus(level, radius, None, color))?;
    }
    Ok(())
}

/// A ring round `centre`: a fine line all round and over it the part
/// charged, from twelve o'clock, with round ends.
///
/// # Errors
/// [`Error::NoMemory`] when the paint list cannot grow.
pub fn ring(
    out: &mut Vec<Paint>,
    ink: Color,
    (centre, radius, thickness): (Point, u32, u32),
    percent: u8,
    line: u16,
) -> Result<(), Error> {
    let thickness = thickness.min(radius);
    if thickness == 0 {
        return Ok(());
    }
    let fine = u32::from(line).clamp(1, thickness);
    // The fine line in the middle of the ring's width.
    let track = radius - (thickness - fine) / 2;
    stroke_circle(out, centre, track, fine, ink)?;
    let percent = u32::from(percent.min(100));
    if percent == 0 {
        return Ok(());
    }
    let sweep = percent * 360 / 100;
    fill_ring(out, centre, radius, thickness, (0, sweep.max(1)), ink)?;
    if sweep < 360 && thickness >= 4 {
        // Round ends.
        let middle = (radius - thickness / 2) as i32;
        let end = (thickness / 2).max(1);
        for degrees in [0, sweep as i32] {
            fill_circle(out, point_at(centre, middle, degrees), end, ink)?;
        }
    }
    Ok(())
}

/// A bolt filling `at`: two triangles meeting in the middle.
///
/// # Errors
/// [`Error::NoMemory`] when the paint list cannot grow.
pub fn bolt(out: &mut Vec<Paint>, at: Rect, color: Color) -> Result<(), Error> {
    let (w, h) = (i64::from(at.width), i64::from(at.height));
    if w < 3 || h < 4 {
        return Ok(());
    }
    // Corners in hundredths of the width and the height; a pixel's middle is
    // taken in the same units, doubled.
    let top = [(70, 0), (10, 58), (56, 58)];
    let bottom = [(44, 42), (90, 42), (30, 100)];
    spans(
        out,
        (i32::from(at.x), i32::from(at.y)),
        (u32::from(at.width), u32::from(at.height)),
        color,
        |column, row| {
            let p = (
                (2 * i64::from(column) + 1) * 100,
                (2 * i64::from(row) + 1) * 100,
            );
            let scaled = |t: [(i64, i64); 3]| t.map(|(x, y)| (x * w * 2, y * h * 2));
            inside(scaled(top), p) || inside(scaled(bottom), p)
        },
    )
}

/// Whether `p` is in the triangle `t` (edges included).
fn inside(t: [(i64, i64); 3], p: (i64, i64)) -> bool {
    let side = |a: (i64, i64), b: (i64, i64)| (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
    let (x, y, z) = (side(t[0], t[1]), side(t[1], t[2]), side(t[2], t[0]));
    (x >= 0 && y >= 0 && z >= 0) || (x <= 0 && y <= 0 && z <= 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::Rect;

    const COLOURS: (Color, Color) = (Color::BLACK, Color::WHITE);

    #[test]
    fn a_battery_is_drawn_with_its_charge_in_proportion() {
        let mut full = Vec::new();
        let mut half = Vec::new();
        battery(
            &mut full,
            COLOURS,
            Rect::new(0, 0, 106, 50),
            (100, false, false),
            1,
        )
        .unwrap();
        battery(
            &mut half,
            COLOURS,
            Rect::new(0, 0, 106, 50),
            (50, false, false),
            1,
        )
        .unwrap();
        let level = |paints: &[Paint]| match paints.last() {
            Some(Paint::Focus(rect, _, None, _)) => rect.width,
            other => panic!("{other:?}"),
        };
        assert!(level(&full) > level(&half));
        assert!(level(&half) > 0);
    }

    #[test]
    fn a_low_charge_is_shaded_and_an_empty_one_has_no_level() {
        let mut low = Vec::new();
        battery(
            &mut low,
            COLOURS,
            Rect::new(0, 0, 106, 50),
            (10, true, false),
            1,
        )
        .unwrap();
        assert!(matches!(
            low.last(),
            Some(Paint::Focus(_, _, None, c)) if *c == Color::gray(LOW_SHADE)
        ));
        let mut empty = Vec::new();
        battery(
            &mut empty,
            COLOURS,
            Rect::new(0, 0, 106, 50),
            (0, false, false),
            1,
        )
        .unwrap();
        // The knob, the shell, its outline: no level.
        assert_eq!(empty.len(), 3);
    }

    #[test]
    fn a_battery_too_small_to_draw_draws_nothing() {
        let mut out = Vec::new();
        battery(
            &mut out,
            COLOURS,
            Rect::new(0, 0, 5, 5),
            (50, false, true),
            1,
        )
        .unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn a_standing_battery_fills_from_below() {
        let mut out = Vec::new();
        battery(
            &mut out,
            COLOURS,
            Rect::new(0, 0, 50, 100),
            (50, false, true),
            1,
        )
        .unwrap();
        let Some(Paint::Focus(level, _, None, _)) = out.last() else {
            panic!("{out:?}");
        };
        let Some(Paint::Focus(shell, _, Some(_), _)) = out.get(2) else {
            panic!("{out:?}");
        };
        assert!(level.y > shell.y);
        assert!(level.y + level.height < shell.y + shell.height);
    }

    #[test]
    fn a_ring_has_a_track_and_a_part_with_round_ends() {
        let mut out = Vec::new();
        ring(&mut out, Color::BLACK, (Point::new(40, 40), 30, 6), 25, 1).unwrap();
        assert!(out.len() > 3);
        let mut none = Vec::new();
        ring(&mut none, Color::BLACK, (Point::new(40, 40), 30, 6), 0, 1).unwrap();
        assert!(none.len() < out.len());
        let mut nothing = Vec::new();
        ring(
            &mut nothing,
            Color::BLACK,
            (Point::new(40, 40), 30, 0),
            50,
            1,
        )
        .unwrap();
        assert!(nothing.is_empty());
    }

    #[test]
    fn a_bolt_covers_the_middle_and_not_the_corners() {
        let mut out = Vec::new();
        bolt(&mut out, Rect::new(0, 0, 20, 40), Color::BLACK).unwrap();
        let covered = |x: u16, y: u16| {
            out.iter().any(|paint| match paint {
                Paint::Fill(r, _) => {
                    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
                }
                _ => false,
            })
        };
        assert!(covered(10, 20));
        assert!(!covered(0, 0));
        assert!(!covered(19, 39));
        let mut tiny = Vec::new();
        bolt(&mut tiny, Rect::new(0, 0, 2, 40), Color::BLACK).unwrap();
        assert!(tiny.is_empty());
    }
}
