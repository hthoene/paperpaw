//! Read-back of the band a canvas draws into, and fills in a gray between
//! the frame's levels.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_graphics::{Dither, Framebuffer};
use paperpaw_platform::{Color, PixelFormat, Point, Rect};

fn at(x: i32, y: i32) -> Point {
    Point::new(x, y)
}

#[test]
fn a_canvas_reads_back_what_it_drew_in_the_band() {
    let mut frame = Framebuffer::new(40, 40, PixelFormat::Gray8).unwrap();
    let mut canvas = frame.canvas();
    canvas.clear(Color::WHITE);
    canvas.fill(Rect::new(4, 4, 8, 8), Color::gray(60));
    assert_eq!(canvas.pixel(at(5, 5)), Some(Color::gray(60)));
    assert_eq!(canvas.gray_at(at(5, 5)), Some(60));
    assert_eq!(canvas.gray_at(at(20, 20)), Some(255));
    // Outside the frame: nothing.
    assert_eq!(canvas.pixel(at(-1, 5)), None);
    assert_eq!(canvas.pixel(at(40, 5)), None);
    assert_eq!(canvas.pixel(at(5, 400)), None);
}

#[test]
fn reading_stops_at_the_clip_and_follows_the_origin() {
    let mut frame = Framebuffer::new(40, 40, PixelFormat::Gray8).unwrap();
    let mut canvas = frame.canvas();
    canvas.clear(Color::gray(200));
    canvas.fill(Rect::new(10, 10, 4, 4), Color::BLACK);
    let clipped = canvas.clipped(Rect::new(8, 8, 8, 8));
    assert_eq!(clipped.pixel(at(11, 11)), Some(Color::BLACK));
    assert_eq!(clipped.pixel(at(2, 2)), None);
    assert_eq!(clipped.gray_at(at(8, 8)), Some(200));
}

#[test]
fn another_band_is_not_readable() {
    let mut frame = Framebuffer::banded(40, 100, 20, PixelFormat::Gray8).unwrap();
    frame.set_top(40);
    let mut canvas = frame.canvas();
    canvas.clear(Color::gray(90));
    assert_eq!(canvas.gray_at(at(3, 45)), Some(90));
    assert_eq!(canvas.gray_at(at(3, 10)), None);
    assert_eq!(canvas.gray_at(at(3, 70)), None);
}

#[test]
fn a_blind_or_describing_canvas_reads_nothing() {
    let mut blind = Framebuffer::blind(40, 40);
    assert_eq!(blind.canvas().pixel(at(1, 1)), None);
    let mut frame = Framebuffer::new(40, 40, PixelFormat::Gray8).unwrap();
    let mut sink = Vec::new();
    let described = frame.canvas().describing(&mut sink);
    assert_eq!(described.pixel(at(1, 1)), None);
}

#[test]
fn night_mode_reads_the_colour_as_drawn() {
    let mut frame = Framebuffer::new(40, 40, PixelFormat::Gray8).unwrap();
    frame.set_inverted(true);
    let mut canvas = frame.canvas();
    canvas.fill(Rect::new(0, 0, 4, 4), Color::gray(30));
    assert_eq!(canvas.gray_at(at(1, 1)), Some(30));
}

#[test]
fn a_gray_between_levels_is_a_pattern_or_the_nearest_level() {
    let area = Rect::new(0, 0, 8, 8);
    let count_white = |dither: Dither, format: PixelFormat| {
        let mut frame = Framebuffer::new(8, 8, format).unwrap();
        let mut canvas = frame.canvas();
        canvas.clear(Color::BLACK);
        canvas.fill_dithered(area, 128, dither);
        let view = frame.view();
        (0..8u16)
            .flat_map(|y| (0..8u16).map(move |x| (x, y)))
            .filter(|(x, y)| view.pixel(*x, *y) == Color::WHITE)
            .count()
    };
    // One bit: ordered is half black and half white, none is all one level.
    assert_eq!(count_white(Dither::ORDERED, PixelFormat::Mono1), 32);
    assert!(matches!(
        count_white(Dither::NONE, PixelFormat::Mono1),
        0 | 64
    ));
    // A dither nobody made yet draws as ordered.
    assert_eq!(count_white(Dither::from_value(77), PixelFormat::Mono1), 32);
    // Many grays: the gray itself.
    let mut frame = Framebuffer::new(8, 8, PixelFormat::Gray8).unwrap();
    frame.canvas().fill_dithered(area, 128, Dither::ORDERED);
    assert_eq!(frame.view().pixel(3, 3), Color::gray(128));
    assert_eq!((Dither::ORDERED.value(), Dither::NONE.value()), (0, 1));
}
