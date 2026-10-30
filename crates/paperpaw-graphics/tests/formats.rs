//! Framebuffers in every pixel format: grays come back as the nearest
//! level, ordered dithering keeps a gray's lightness, night mode is the
//! exact inverse of day, a screen drawn band by band is the screen drawn
//! at once, and the region reported changed is what changed.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{Op, config, draw, format, op, pixels};
use paperpaw_graphics::{Framebuffer, GraphicsError};
use paperpaw_platform::{Color, FrameView, PixelFormat, Point, Rect};
use proptest::prelude::*;

const W: u16 = 53;
const H: u16 = 29;

fn gray_formats() -> [PixelFormat; 4] {
    [
        PixelFormat::Mono1,
        PixelFormat::Gray2,
        PixelFormat::Gray4,
        PixelFormat::Gray8,
    ]
}

/// Draws `ops` on a whole screen of `format`.
fn whole(format: PixelFormat, inverted: bool, ops: &[Op]) -> Framebuffer {
    let mut fb = Framebuffer::new(W, H, format).unwrap();
    fb.set_inverted(inverted);
    {
        let mut canvas = fb.canvas();
        for op in ops {
            draw(&mut canvas, op);
        }
    }
    fb
}

proptest! {
    #![proptest_config(config(160))]

    /// A screen drawn band by band, each band cleared and drawn whole,
    /// is the screen drawn at once.
    #[test]
    fn bands_put_together_are_the_whole_screen(
        format in format(),
        rows in 1u16..40,
        ops in proptest::collection::vec(op(true), 1..6),
    ) {
        let expected = whole(format, false, &ops);
        let mut screen = Framebuffer::new(W, H, format).unwrap();
        let mut band = Framebuffer::banded(W, H, rows, format).unwrap();
        prop_assert_eq!(band.rows(), rows.min(H));
        let mut top = 0;
        while top < H {
            band.set_top(top);
            let held = band.band();
            {
                let mut canvas = band.canvas();
                prop_assert_eq!(canvas.clip(), held);
                canvas.clear(Color::WHITE);
                for op in &ops {
                    draw(&mut canvas, op);
                }
            }
            screen.write_rows(top, band.view());
            top += band.rows();
        }
        prop_assert_eq!(pixels(&screen), pixels(&expected));
    }

    /// In night mode every pixel is stored as the inverse of the day's,
    /// whatever is drawn on the paper (grays: the levels are symmetric).
    #[test]
    fn night_is_the_inverse_of_day(
        format in (0usize..4).prop_map(|i| gray_formats()[i]),
        ops in proptest::collection::vec(op(false), 1..6),
    ) {
        // The paper first: inverting leaves what is stored as it is.
        let ops: Vec<Op> = std::iter::once(Op::Clear(Color::WHITE)).chain(ops).collect();
        let day = pixels(&whole(format, false, &ops));
        let night = pixels(&whole(format, true, &ops));
        let inverted: Vec<Color> = day.iter().map(|c| c.inverted()).collect();
        prop_assert_eq!(night, inverted);
    }

    /// A gray fills as the nearest level the format has.
    #[test]
    fn grays_come_back_as_the_nearest_level(format in format(), gray in any::<u8>()) {
        let mut fb = Framebuffer::new(3, 2, format).unwrap();
        fb.canvas().clear(Color::gray(gray));
        let got = fb.view().pixel(2, 1);
        // RGB565 has a bit more for green: its grays are near gray.
        let levels = u32::from(format.gray_levels().unwrap_or(32));
        let step = 255.0 / f64::from(levels - 1);
        for channel in [got.r, got.g, got.b] {
            prop_assert!((f64::from(channel) - f64::from(gray)).abs() <= step / 2.0 + 0.5, "{:?} for {}", got, gray);
        }
        if format.gray_levels().is_some() {
            prop_assert!(got.r == got.g && got.g == got.b);
        }
    }

    /// Ordered dithering: the mean of each 4 × 4 tile of a flat gray is
    /// that gray (to within a level's sixteenth and rounding); darker
    /// grays never give fewer dark pixels.
    #[test]
    fn dithered_grays_keep_their_lightness(
        format in (0usize..3).prop_map(|i| gray_formats()[i]),
        gray in any::<u8>(),
    ) {
        let mean = |gray: u8| {
            let mut fb = Framebuffer::new(8, 8, format).unwrap();
            {
                let mut canvas = fb.canvas();
                for y in 0..8 {
                    canvas.draw_gray_row(0, y, &[gray; 8]);
                }
            }
            let tile: u32 = (0..4u16)
                .flat_map(|y| (0..4u16).map(move |x| (x, y)))
                .map(|(x, y)| u32::from(fb.view().pixel(x, y).r))
                .sum();
            f64::from(tile) / 16.0
        };
        let levels = f64::from(format.gray_levels().unwrap() - 1);
        let got = mean(gray);
        prop_assert!((got - f64::from(gray)).abs() <= 255.0 / levels / 16.0 + 8.0, "{} for {}", got, gray);
        if gray > 0 {
            prop_assert!(mean(gray - 1) <= got);
        }
    }

    /// In black and white, an edge pixel takes the colour from the
    /// coverage set on.
    #[test]
    fn black_and_white_edges_follow_the_coverage_set(coverage in any::<u8>(), alpha in any::<u8>()) {
        let mut fb = Framebuffer::new(2, 1, PixelFormat::Mono1).unwrap();
        fb.set_mono_coverage(coverage);
        prop_assert_eq!(fb.mono_coverage(), coverage.max(1));
        fb.canvas().draw_mask(0, 0, 1, 1, Color::BLACK, |_, _| alpha);
        let black = fb.view().pixel(0, 0) == Color::BLACK;
        prop_assert_eq!(black, alpha > 0 && alpha >= coverage.max(1));
    }
}

#[test]
fn black_and_white_round_trip_in_every_format() {
    for format in common::formats() {
        let mut fb = Framebuffer::new(9, 3, format).unwrap();
        assert_eq!(fb.format(), format);
        assert_eq!(fb.byte_len(), format.stride(9) * 3);
        assert!(pixels(&fb).iter().all(|&c| c == Color::WHITE));
        fb.canvas().fill_rect(1, 1, 7, 1, Color::BLACK);
        let view = fb.view();
        for x in 0..9 {
            let expected = if (1..8).contains(&x) {
                Color::BLACK
            } else {
                Color::WHITE
            };
            assert_eq!(view.pixel(x, 1), expected, "{format:?}");
            assert_eq!(view.pixel(x, 0), Color::WHITE, "{format:?}");
        }
    }
}

#[cfg(feature = "color")]
#[test]
fn colours_round_trip_in_colour_formats() {
    let mut fb = Framebuffer::new(2, 1, PixelFormat::Rgb888).unwrap();
    fb.canvas()
        .set_pixel(Point::new(0, 0), Color::rgb(12, 200, 99));
    assert_eq!(fb.view().pixel(0, 0), Color::rgb(12, 200, 99));
    let mut fb = Framebuffer::new(2, 1, PixelFormat::Rgb565).unwrap();
    fb.canvas()
        .set_pixel(Point::new(0, 0), Color::rgb(255, 0, 0));
    assert_eq!(fb.view().pixel(0, 0), Color::rgb(255, 0, 0));
    // Pictures' colours made stronger, grays left.
    let mut fb = Framebuffer::new(2, 1, PixelFormat::Rgb888).unwrap();
    fb.set_saturation(150);
    fb.canvas().draw_rgb_row(0, 0, &[200, 100, 100, 90, 90, 90]);
    let strong = fb.view().pixel(0, 0);
    assert!(strong.r > 200 && strong.g < 100);
    assert_eq!(fb.view().pixel(1, 0), Color::gray(90));
}

#[cfg(not(feature = "color"))]
#[test]
fn colour_formats_need_the_feature() {
    for format in [PixelFormat::Rgb565, PixelFormat::Rgb888] {
        assert_eq!(
            Framebuffer::new(4, 4, format).unwrap_err(),
            GraphicsError::UnsupportedFormat(format)
        );
        assert!(format!("{}", GraphicsError::UnsupportedFormat(format)).contains("not supported"));
    }
}

#[test]
fn colour_rows_are_their_gray_on_gray_panels() {
    for format in gray_formats() {
        let mut a = Framebuffer::new(4, 1, format).unwrap();
        let mut b = Framebuffer::new(4, 1, format).unwrap();
        let rgb = [255, 0, 0, 0, 255, 0, 0, 0, 255, 30, 60, 90];
        a.canvas().draw_rgb_row(0, 0, &rgb);
        let grays: Vec<u8> = rgb
            .chunks(3)
            .map(|p| Color::rgb(p[0], p[1], p[2]).luma())
            .collect();
        b.canvas().draw_gray_row(0, 0, &grays);
        assert_eq!(pixels(&a), pixels(&b), "{format:?}");
    }
}

#[test]
fn empty_framebuffers_draw_nothing() {
    for (w, h) in [(0, 0), (0, 5), (5, 0)] {
        for format in common::formats() {
            let mut fb = Framebuffer::new(w, h, format).unwrap();
            let mut canvas = fb.canvas();
            assert!(canvas.clip().is_empty());
            canvas.clear(Color::BLACK);
            canvas.fill_circle(Point::new(0, 0), 5, Color::BLACK);
            canvas.draw_gray_row(0, 0, &[0; 4]);
            assert_eq!(fb.take_dirty(), None);
        }
    }
}

#[test]
fn bands_never_reach_past_the_screen() {
    let mut fb = Framebuffer::banded(10, 25, 10, PixelFormat::Gray4).unwrap();
    assert_eq!(fb.band(), Rect::new(0, 0, 10, 10));
    fb.set_top(20);
    assert_eq!(fb.band(), Rect::new(0, 20, 10, 5));
    fb.set_top(1000);
    assert_eq!(fb.band(), Rect::new(0, 24, 10, 1));
    // More rows than the screen has: the screen.
    let fb = Framebuffer::banded(10, 25, 100, PixelFormat::Gray4).unwrap();
    assert_eq!(fb.rows(), 25);
    let fb = Framebuffer::banded(10, 25, 0, PixelFormat::Gray4).unwrap();
    assert_eq!(fb.rows(), 1);
}

#[test]
fn views_of_rows_are_clamped_to_the_band() {
    let mut fb = Framebuffer::banded(8, 30, 10, PixelFormat::Gray8).unwrap();
    fb.set_top(10);
    assert_eq!(fb.view_rows(Rect::new(0, 0, 8, 5)).height, 0);
    assert_eq!(fb.view_rows(Rect::new(0, 5, 8, 10)).height, 5);
    assert_eq!(fb.view_rows(Rect::new(0, 12, 8, 100)).height, 8);
    assert_eq!(fb.view_rows(Rect::new(0, 25, 8, 3)).height, 0);
    assert_eq!(fb.view_rows(Rect::new(0, u16::MAX, 8, u16::MAX)).height, 0);
    assert_eq!(fb.view_rows(Rect::new(0, 15, 8, 2)).data.len(), 16);
}

#[test]
fn rows_of_another_format_or_width_are_not_written() {
    let mut fb = Framebuffer::new(8, 4, PixelFormat::Gray8).unwrap();
    let black = [0u8; 64];
    for (width, format) in [(8, PixelFormat::Gray4), (7, PixelFormat::Gray8)] {
        fb.write_rows(0, FrameView::new(width, 4, format, 8, &black));
        assert!(pixels(&fb).iter().all(|&c| c == Color::WHITE));
    }
    // Rows given short: as far as they go.
    fb.write_rows(2, FrameView::new(8, 4, PixelFormat::Gray8, 8, &black[..12]));
    assert_eq!(fb.view().pixel(0, 2), Color::BLACK);
    assert_eq!(fb.view().pixel(0, 3), Color::WHITE);
    // Rows past the screen's last: skipped, as are those beyond u16.
    fb.write_rows(
        u16::MAX - 1,
        FrameView::new(8, 4, PixelFormat::Gray8, 8, &black[..32]),
    );
    assert_eq!(fb.view().pixel(0, 0), Color::WHITE);
}

#[test]
fn the_changed_region_grows_with_each_drawing_and_is_taken_once() {
    let mut fb = Framebuffer::new(W, H, PixelFormat::Gray4).unwrap();
    {
        let mut canvas = fb.canvas();
        canvas.fill_rect(2, 3, 4, 5, Color::BLACK);
        canvas.draw_gray_row(40, 20, &[0; 3]);
    }
    assert_eq!(fb.take_dirty(), Some(Rect::new(2, 3, 41, 18)));
    assert_eq!(fb.take_dirty(), None);
    // Nothing drawn (a rectangle of no size): nothing changed.
    fb.canvas().fill_rect(5, 5, 0, 9, Color::BLACK);
    assert_eq!(fb.take_dirty(), None);
}

#[test]
fn shading_on_few_grays_is_reported_dirty() {
    // `Framebuffer::shade_clipped` stores the dithered pixels without
    // `mark_dirty`, where the format has 2 to 16 grays: a shade behind
    // marked text is not in the region the kernel refreshes.
    for format in [PixelFormat::Mono1, PixelFormat::Gray2, PixelFormat::Gray4] {
        for round in [false, true] {
            let mut fb = Framebuffer::new(W, H, format).unwrap();
            let area = Rect::new(4, 4, 20, 10);
            if round {
                fb.canvas().fill_round_shaded(area, 3, Color::gray(60));
            } else {
                fb.canvas().fill_shaded(area, Color::gray(60));
            }
            assert_ne!(
                pixels(&fb),
                pixels(&Framebuffer::new(W, H, format).unwrap())
            );
            assert_eq!(fb.take_dirty(), Some(area), "{format:?}");
        }
    }
}

#[test]
fn pictures_are_counted_unless_small() {
    let mut fb = Framebuffer::new(W, H, PixelFormat::Gray4).unwrap();
    {
        let mut canvas = fb.canvas();
        canvas.draw_gray_row(-5, 0, &[0; 20]);
        canvas.draw_rgb_row(50, 1, &[0; 30]);
        canvas.small_pictures(|c| c.draw_gray_row(0, 2, &[0; 9]));
        canvas.fill_rect(0, 3, 10, 1, Color::BLACK);
    }
    // 15 and 3 pixels inside the screen.
    assert_eq!(fb.take_pictured(), 18);
    assert_eq!(fb.take_pictured(), 0);
}

#[test]
fn pictures_kept_in_night_mode_stay_as_they_are() {
    for format in gray_formats() {
        let ops = [
            Op::GrayRow(0, 0, (0..W).map(|x| (x * 5) as u8).collect()),
            Op::RgbRow(0, 1, (0..3 * W).map(|x| (x * 7) as u8).collect()),
        ];
        let day = whole(format, false, &ops);
        let mut night = Framebuffer::new(W, H, format).unwrap();
        night.set_inverted(true);
        night.set_pictures_kept(true);
        assert!(night.pictures_kept() && night.is_inverted());
        {
            let mut canvas = night.canvas();
            for op in &ops {
                draw(&mut canvas, op);
            }
        }
        let rows = |fb: &Framebuffer| pixels(fb)[..2 * usize::from(W)].to_vec();
        assert_eq!(rows(&night), rows(&day), "{format:?}");
    }
}

#[test]
fn a_band_of_the_largest_screen_takes_its_rows_only() {
    let fb = Framebuffer::banded(u16::MAX, u16::MAX, 1, PixelFormat::Gray8).unwrap();
    assert_eq!(fb.byte_len(), usize::from(u16::MAX));
    let fb = Framebuffer::banded(u16::MAX, u16::MAX, 3, PixelFormat::Mono1).unwrap();
    assert_eq!(fb.byte_len(), 3 * 8192);
    let error = GraphicsError::OutOfMemory { bytes: 12 };
    assert!(format!("{error}").contains("12 bytes"));
}

#[test]
fn set_pixel_draws_one_pixel_exactly() {
    for format in common::formats() {
        let mut fb = Framebuffer::new(W, H, format).unwrap();
        fb.canvas().set_pixel(Point::new(17, 9), Color::BLACK);
        let changed: Vec<usize> = pixels(&fb)
            .iter()
            .enumerate()
            .filter(|(_, c)| **c != Color::WHITE)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(changed, [9 * usize::from(W) + 17], "{format:?}");
    }
}

#[test]
fn canvases_describe_what_they_draw_to_those_who_ask() {
    use paperpaw_graphics::{Described, Role};
    let mut fb = Framebuffer::new(W, H, PixelFormat::Gray4).unwrap();
    // Nobody asks: nothing kept.
    {
        let mut canvas = fb.canvas();
        assert!(!canvas.is_describing());
        canvas
            .describe(Role::Button, "OK", Rect::new(1, 1, 5, 5))
            .focused(true);
    }
    let mut described = Vec::new();
    {
        let mut canvas = fb.canvas().describing(&mut described);
        assert!(canvas.is_describing());
        canvas.describe(Role::Heading, "Title", Rect::new(0, 0, 50, 8));
        // A clipped canvas describes into the same list.
        let mut inner = canvas.clipped(Rect::new(0, 10, 20, 10));
        assert!(inner.is_describing());
        inner
            .describe(Role::Switch, "Wi-Fi", Rect::new(0, 10, 20, 10))
            .focused(true)
            .on(Some(true));
    }
    assert_eq!(
        described,
        [
            Described::new(Role::Heading, "Title", Rect::new(0, 0, 50, 8))
                .clip(Rect::new(0, 0, W, H)),
            Described::new(Role::Switch, "Wi-Fi", Rect::new(0, 10, 20, 10))
                .focused(true)
                .on(Some(true))
                .clip(Rect::new(0, 10, 20, 10)),
        ]
    );
}
