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
