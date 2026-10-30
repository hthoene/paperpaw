//! The platform's small types: monotonic [`Instant`]s at their limits,
//! [`WallClock`] times before and after it was set, colour panels'
//! boost, reading pixels of every format from a [`FrameView`] (also out of
//! bounds and from short buffers), pixel formats' strides, colours, and
//! rectangles' geometry (properties of union, intersection, contains,
//! inset and grow).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]
// Tests panic on purpose when something is wrong.

use paperpaw_platform::{
    Color, ColorPanel, FrameView, Instant, PixelFormat, Point, Rect, WallClock,
};
use proptest::prelude::*;

#[test]
fn instants_saturate_at_their_limits() {
    assert_eq!(Instant(u64::MAX).add_millis(1), Instant(u64::MAX));
    assert_eq!(Instant(5).add_millis(10).millis(), 15);
    assert_eq!(Instant(10).since(Instant(4)), 6);
    assert_eq!(Instant(4).since(Instant(10)), 0, "an earlier now is 0");
    assert_eq!(Instant(u64::MAX).since(Instant(0)), u64::MAX);
    assert!(Instant(1) < Instant(2));
    assert_eq!(Instant::default().millis(), 0);
}

#[test]
fn wall_clocks_follow_the_monotonic_clock_both_ways() {
    let clock = WallClock::new(Instant(10_000), 1_791_967_260_000, Some(120));
    assert_eq!(clock.unix_millis_at(Instant(10_000)), 1_791_967_260_000);
    assert_eq!(clock.unix_millis_at(Instant(70_000)), 1_791_967_320_000);
    assert_eq!(
        clock.unix_millis_at(Instant(4_000)),
        1_791_967_254_000,
        "before it was set"
    );
}

#[test]
fn wall_clocks_before_1970_stay_negative() {
    let clock = WallClock::new(Instant(0), -86_400_000, None);
    assert_eq!(clock.unix_millis_at(Instant(1_000)), -86_399_000);
    let epoch = WallClock::new(Instant(500), 0, Some(-300));
    assert_eq!(epoch.unix_millis_at(Instant(0)), -500);
}

#[test]
fn colour_boost_stays_between_once_and_twice() {
    let boost = |saturation_percent| ColorPanel::new(2, saturation_percent).boost_percent();
    assert_eq!(boost(0), 200, "no saturation does not divide by zero");
    assert_eq!(boost(1), 200);
    assert_eq!(boost(50), 200);
    assert_eq!(boost(80), 125);
    assert_eq!(boost(100), 100);
    assert_eq!(boost(255), 100, "more than sRGB is never toned down");
    for s in 0..=255u8 {
        assert!((100..=200).contains(&boost(s)));
    }
}

fn view(format: PixelFormat, width: u16, height: u16, data: &[u8]) -> FrameView<'_> {
    FrameView::new(width, height, format, format.stride(width), data)
}

#[test]
fn mono_pixels_are_read_most_significant_bit_first() {
    let data = [0b1000_0001, 0b0100_0000];
    let frame = view(PixelFormat::Mono1, 10, 1, &data);
    let levels: Vec<u8> = (0..10).map(|x| frame.pixel(x, 0).r).collect();
    assert_eq!(levels, [255, 0, 0, 0, 0, 0, 0, 255, 0, 255]);
}

#[test]
fn gray_pixels_spread_over_the_full_range() {
    let gray2 = [0b00_01_10_11];
    let frame = view(PixelFormat::Gray2, 4, 1, &gray2);
    let levels: Vec<u8> = (0..4).map(|x| frame.pixel(x, 0).r).collect();
    assert_eq!(levels, [0, 85, 170, 255]);
    let gray4 = [0x0f, 0x80];
    let frame = view(PixelFormat::Gray4, 4, 1, &gray4);
    let levels: Vec<u8> = (0..4).map(|x| frame.pixel(x, 0).r).collect();
    assert_eq!(levels, [0, 255, 136, 0]);
    let gray8 = [0, 1, 254, 255];
    let frame = view(PixelFormat::Gray8, 2, 2, &gray8);
    assert_eq!(frame.pixel(1, 0), Color::gray(1));
    assert_eq!(frame.pixel(0, 1), Color::gray(254));
}

#[test]
fn colour_pixels_expand_to_eight_bits() {
    let rgb565 = [0xff, 0xff, 0x00, 0xf8, 0xe0, 0x07, 0x1f, 0x00];
    let frame = view(PixelFormat::Rgb565, 4, 1, &rgb565);
    assert_eq!(frame.pixel(0, 0), Color::WHITE);
    assert_eq!(frame.pixel(1, 0), Color::rgb(255, 0, 0));
    assert_eq!(frame.pixel(2, 0), Color::rgb(0, 255, 0));
    assert_eq!(frame.pixel(3, 0), Color::rgb(0, 0, 255));
    let rgb888 = [1, 2, 3, 4, 5, 6];
    let frame = view(PixelFormat::Rgb888, 2, 1, &rgb888);
    assert_eq!(frame.pixel(1, 0), Color::rgb(4, 5, 6));
}

#[test]
fn pixels_outside_the_frame_or_its_data_are_white() {
    for format in [
        PixelFormat::Mono1,
        PixelFormat::Gray2,
        PixelFormat::Gray4,
        PixelFormat::Gray8,
        PixelFormat::Rgb565,
        PixelFormat::Rgb888,
    ] {
        let zeros = vec![0u8; format.stride(8) * 2];
        let frame = view(format, 8, 2, &zeros);
        assert_eq!(frame.pixel(0, 0), Color::BLACK, "{format:?}");
        assert_eq!(frame.pixel(8, 0), Color::WHITE, "{format:?} right of it");
        assert_eq!(frame.pixel(0, 2), Color::WHITE, "{format:?} below it");
        assert_eq!(frame.pixel(u16::MAX, u16::MAX), Color::WHITE);
        // A buffer shorter than the frame says: what is missing is white.
        let short = view(format, 8, 2, &zeros[..format.stride(8)]);
        assert_eq!(short.pixel(7, 1), Color::WHITE, "{format:?} short");
        let empty = view(format, 8, 2, &[]);
        assert_eq!(empty.pixel(0, 0), Color::WHITE, "{format:?} empty");
    }
}

#[test]
fn formats_describe_themselves() {
    for (format, bits, color, levels) in [
        (PixelFormat::Mono1, 1, false, Some(2)),
        (PixelFormat::Gray2, 2, false, Some(4)),
        (PixelFormat::Gray4, 4, false, Some(16)),
        (PixelFormat::Gray8, 8, false, Some(256)),
        (PixelFormat::Rgb565, 16, true, None),
        (PixelFormat::Rgb888, 24, true, None),
    ] {
        assert_eq!(format.bits_per_pixel(), bits);
        assert_eq!(format.is_color(), color);
        assert_eq!(format.gray_levels(), levels);
        assert_eq!(format.stride(0), 0);
        assert_eq!(
            format.stride(u16::MAX),
            (usize::from(u16::MAX) * usize::from(bits)).div_ceil(8)
        );
    }
}

#[test]
fn colours_invert_and_weigh_green_most() {
    assert_eq!(Color::BLACK.inverted(), Color::WHITE);
    assert_eq!(Color::rgb(1, 2, 3).inverted(), Color::rgb(254, 253, 252));
    assert!(Color::rgb(0, 255, 0).luma() > Color::rgb(255, 0, 0).luma());
    assert!(Color::rgb(255, 0, 0).luma() > Color::rgb(0, 0, 255).luma());
    for v in [0u8, 1, 127, 200, 255] {
        assert_eq!(Color::gray(v).luma(), v, "gray keeps its level");
    }
}

#[test]
fn rect_helpers_at_the_edges_of_u16() {
    let r = Rect::new(u16::MAX, u16::MAX, u16::MAX, u16::MAX);
    assert_eq!(r.right(), 2 * u32::from(u16::MAX));
    assert_eq!(r.area(), u32::from(u16::MAX) * u32::from(u16::MAX));
    assert!(r.contains(Point::new(i32::from(u16::MAX), i32::from(u16::MAX))));
    assert!(!r.contains(Point::new(i32::MIN, i32::MAX)));
    assert!(
        !Rect::default().contains(Point::new(0, 0)),
        "empty holds nothing"
    );
    assert_eq!(Rect::new(1, 1, 3, 3).inset(2, 2), Rect::new(3, 3, 0, 0));
    assert!(Rect::new(1, 1, 3, 3).inset(2, 2).is_empty());
    assert_eq!(Rect::new(1, 1, 2, 2).grow(5), Rect::new(0, 0, 12, 12));
    assert_eq!(
        Rect::from_size(10, 10).grow(u16::MAX),
        Rect::new(0, 0, u16::MAX, u16::MAX)
    );
}

/// Rects reaching past 65535 must not wrap to a tiny one.
#[test]
fn the_union_of_far_apart_rects_never_shrinks() {
    let a = Rect::new(0, 0, 10, 10);
    let b = Rect::new(65_530, 0, 10, 10);
    let joined = a.union(&b);
    assert!(
        joined.width >= a.width && joined.width >= b.width,
        "{joined:?}"
    );
}

/// A rectangle that stays within the u16 coordinate space.
fn rect() -> impl Strategy<Value = Rect> {
    (0u16..2000, 0u16..2000, 0u16..2000, 0u16..2000)
        .prop_map(|(x, y, width, height)| Rect::new(x, y, width, height))
}

fn point() -> impl Strategy<Value = Point> {
    (-100i32..4200, -100i32..4200).prop_map(|(x, y)| Point::new(x, y))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    #[test]
    fn a_union_covers_both(a in rect(), b in rect(), p in point()) {
        let u = a.union(&b);
        if a.contains(p) || b.contains(p) {
            prop_assert!(u.contains(p), "{u:?} misses {p:?}");
        }
        prop_assert!(u.area() >= a.area().max(b.area()));
        if !a.is_empty() && !b.is_empty() {
            prop_assert_eq!(u, b.union(&a));
        }
    }

    #[test]
    fn an_intersection_is_what_both_contain(a in rect(), b in rect(), p in point()) {
        let both = a.contains(p) && b.contains(p);
        match a.intersection(&b) {
            Some(i) => {
                prop_assert!(!i.is_empty());
                prop_assert_eq!(i.contains(p), both);
                prop_assert_eq!(Some(i), b.intersection(&a));
                prop_assert_eq!(a.intersection(&i), Some(i));
            }
            None => prop_assert!(!both),
        }
    }

    #[test]
    fn inset_and_grow_stay_inside_and_outside(r in rect(), by in 0u16..100, p in point()) {
        let inner = r.inset(by, by);
        let outer = r.grow(by);
        if inner.contains(p) {
            prop_assert!(r.contains(p));
        }
        if r.contains(p) {
            prop_assert!(outer.contains(p));
        }
    }
}
