//! PaperPaw's logo, the paw on a sheet with a dog-ear, where screens show
//! it (the sleep screen, the logo widget): drawn as shapes in any size,
//! sharp on every panel, nothing kept in memory.
//!
//! A picture takes its place where [`LOGO`] names one (a JPEG, PNG or GIF
//! on the card), e.g. one a data plugin brings with
//! `[core] logo = plugins/mine/logo.png` in its `settings.ini`.

use crate::setting::{Setting, Text};
use crate::{Canvas, Color, Rect};

/// A picture shown in place of the drawn logo; none (empty) for that.
pub const LOGO: Setting<Text> = Setting::new("core.logo");

/// The drawing's units: the sheet is 96 across, as in the logo's own
/// drawing (its SVG, less a margin of 12).
const UNITS: i64 = 96;
/// Fractions of a unit the shapes are tested at.
const FINE: i64 = 256;

/// Draws PaperPaw's logo in the square `area` in `color`: the sheet, its
/// dog-ear apart, the paw cut out of it. Smoothed where the panel has
/// grays.
pub fn draw(canvas: &mut Canvas<'_>, area: Rect, color: Color) {
    let side = i64::from(area.width.min(area.height));
    if side < 8 {
        return;
    }
    // Two by two samples a pixel; the pad's width for the two sample rows
    // of the pixel row being drawn, worked out once a row (only for the
    // rows drawn: nothing kept for the whole logo, band by band).
    let samples = side * 2;
    let to_units = |s: i64| (2 * s + 1) * UNITS * FINE / (2 * samples);
    let row = core::cell::Cell::new(None::<(u32, [Span; 2])>);
    let pads = |dy: u32| match row.get() {
        Some((at, pads)) if at == dy => pads,
        _ => {
            let sv = i64::from(dy) * 2;
            let pads = [pad_span(to_units(sv)), pad_span(to_units(sv + 1))];
            row.set(Some((dy, pads)));
            pads
        }
    };
    let x = i32::from(area.x) + (i32::from(area.width) - side as i32) / 2;
    let y = i32::from(area.y) + (i32::from(area.height) - side as i32) / 2;
    canvas.draw_mask(x, y, side as u32, side as u32, color, |dx, dy| {
        let (su, sv) = (i64::from(dx) * 2, i64::from(dy) * 2);
        let pads = pads(dy);
        let inside = |a: i64, b: usize| {
            let (u, v) = (to_units(su + a), to_units(sv + b as i64));
            (sheet(u, v) && !paw(u, v, pads[b])) || dog_ear(u, v)
        };
        let hits = [(0, 0), (1, 0), (0, 1), (1, 1)]
            .iter()
            .filter(|&&(a, b)| inside(a, b))
            .count();
        (hits * 255 / 4) as u8
    });
}

/// Where the pad is across one sample row: from, to (in fine steps).
type Span = (i64, i64);

/// Units in the drawing's fine steps.
const fn at(units: i64) -> i64 {
    units * FINE
}

/// Whether `(u, v)` is within `r` of `(cu, cv)` (all in fine steps).
fn near(u: i64, v: i64, cu: i64, cv: i64, r: i64) -> bool {
    (u - cu) * (u - cu) + (v - cv) * (v - cv) <= r * r
}

/// The sheet: rounded corners, the top right one folded away.
fn sheet(u: i64, v: i64) -> bool {
    let r = at(12);
    if !(0..at(UNITS)).contains(&u) || !(0..at(UNITS)).contains(&v) {
        return false;
    }
    // Round corners but the folded one.
    for (cu, cv) in [(r, r), (r, at(UNITS) - r), (at(UNITS) - r, at(UNITS) - r)] {
        let (outside_u, outside_v) = (
            if cu == r { u < cu } else { u > cu },
            if cv == r { v < cv } else { v > cv },
        );
        if outside_u && outside_v && !near(u, v, cu, cv, r) {
            return false;
        }
    }
    // Where the dog-ear was: cut away, its inner corner round.
    let fold =
        u > at(68) && v < at(18) || u > at(78) && v < at(28) || near(u, v, at(78), at(18), at(10));
    !fold
}

/// The dog-ear: a triangle in the cut corner, its inner corner round.
fn dog_ear(u: i64, v: i64) -> bool {
    if u < at(72) || v > at(24) || u - at(72) > v - at(2) {
        return false;
    }
    !(u < at(78) && v > at(18)) || near(u, v, at(78), at(18), at(6))
}

/// The paw: three toes and the pad, `pad` being the pad's span on this
/// row (empty where the row misses it).
fn paw(u: i64, v: i64, pad: (i64, i64)) -> bool {
    let toe = at(15) / 2;
    near(u, v, at(28), at(42), toe)
        || near(u, v, at(46), at(33), toe)
        || near(u, v, at(64), at(42), toe)
        || (pad.0..pad.1).contains(&u)
}

/// The pad's span on row `v`: from the logo's curves (quadratic, from the
/// widest row 67 up to the top at 53 and down to the bottom at 80), 36
/// across at most around 46.
fn pad_span(v: i64) -> (i64, i64) {
    let (mid, top, widest, bottom) = (at(46), at(53), at(67), at(80));
    if v < top || v > bottom {
        return (0, 0);
    }
    // Along a curve t runs 0..1 from the widest row; the half width is
    // 18 (1 - t²), with t = 1 - sqrt(distance from the end / its height).
    let (gone, height) = if v <= widest {
        (v - top, widest - top)
    } else {
        (bottom - v, bottom - widest)
    };
    // In steps of 1/FINE: t = FINE - sqrt(gone / height) * FINE.
    let t = FINE - isqrt(gone * FINE * FINE / height);
    let half = 18 * (FINE * FINE - t * t) / FINE;
    (mid - half, mid + half)
}

/// The integer square root.
fn isqrt(n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shapes_are_where_the_logo_has_them() {
        // The sheet's middle, below the paw; the folded corner is not.
        assert!(sheet(at(48), at(90)));
        assert!(!sheet(at(90), at(5)));
        assert!(!sheet(at(1), at(1)), "a round corner");
        // The dog-ear sits in the cut corner, apart from the sheet.
        assert!(dog_ear(at(80), at(20)));
        assert!(!dog_ear(at(93), at(5)), "above its slant");
        assert!(!sheet(at(80), at(20)));
        // The toes and the pad are cut out.
        assert!(paw(at(46), at(33), pad_span(at(33))));
        assert!(paw(at(46), at(67), pad_span(at(67))));
        assert!(!paw(at(10), at(67), pad_span(at(67))));
        assert_eq!(pad_span(at(67)), (at(28), at(64)), "36 across at most");
        assert_eq!(pad_span(at(20)), (0, 0));
    }

    #[test]
    fn drawn_band_by_band_it_is_the_same_logo() {
        use paperpaw_graphics::Framebuffer;
        use paperpaw_platform::PixelFormat;
        let area = Rect::new(4, 6, 90, 90);
        let whole = {
            let mut frame = Framebuffer::new(100, 100, PixelFormat::Gray8).unwrap();
            draw(&mut frame.canvas(), area, Color::BLACK);
            frame
        };
        let mut bands = Framebuffer::new(100, 100, PixelFormat::Gray8).unwrap();
        for top in (0..100).step_by(7) {
            let mut canvas = bands.canvas();
            let mut band = canvas.clipped(Rect::new(0, top, 100, 7));
            draw(&mut band, area, Color::BLACK);
        }
        let (a, b) = (whole.view(), bands.view());
        let mut inked = 0;
        for y in 0..100 {
            for x in 0..100 {
                assert_eq!(a.pixel(x, y), b.pixel(x, y), "at {x}, {y}");
                inked += usize::from(a.pixel(x, y) != Color::WHITE);
            }
        }
        assert!(inked > 2000, "a logo was drawn");
    }

    #[test]
    fn square_roots_are_whole() {
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(15), 3);
        assert_eq!(isqrt(16), 4);
        assert_eq!(isqrt(1 << 40), 1 << 20);
    }
}
