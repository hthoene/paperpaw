//! Borrowed graphics backends before the ABI dispatcher is wired.

#![allow(clippy::unwrap_used, reason = "tests fail loudly on unexpected errors")]

#[path = "../src/abi7/graphics.rs"]
mod graphics;

use graphics::InvalidInput;
use paperpaw_graphics::{Canvas, Described, Framebuffer, Role};
use paperpaw_platform::{Color, PixelFormat, Point, Rect};
use paperpaw_sdk::{Font, FontSpec, Fonts, Id};

fn frame(format: PixelFormat) -> Framebuffer {
    let mut frame = Framebuffer::new(40, 32, format).unwrap();
    frame.canvas().fill(Rect::from_size(40, 32), Color::WHITE);
    frame
}

fn fonts() -> Fonts {
    let mut fonts = Fonts::new(4096);
    let font = Font::from_data(
        &include_bytes!("../../paperpaw-text/testdata/pack.ppfp.v1")[..],
        &[],
    )
    .unwrap();
    fonts
        .add(FontSpec::new(Id::new_static("test.font"), font))
        .unwrap();
    fonts
}

fn shapes(canvas: &mut Canvas<'_>, backend: bool) {
    if backend {
        graphics::fill_rect(canvas, -6, 2, 18, 9, Color::BLACK).unwrap();
        graphics::draw_line(canvas, -9, 21, 31, -4, 3, Color::gray(90)).unwrap();
        graphics::fill_round(canvas, 14, 3, 17, 19, 6, Color::gray(140)).unwrap();
        graphics::stroke_round(canvas, -3, 12, 23, 16, 2, 5, Color::BLACK).unwrap();
    } else {
        canvas.fill_rect(-6, 2, 18, 9, Color::BLACK);
        canvas.draw_line(Point::new(-9, 21), Point::new(31, -4), 3, Color::gray(90));
        canvas.fill_round_rect(14, 3, 17, 19, 6, Color::gray(140));
        canvas.stroke_round_rect(-3, 12, 23, 16, 2, 5, Color::BLACK);
    }
}

fn described(canvas: &mut Canvas<'_>) {
    canvas
        .describe(Role::Button, "Existing label", Rect::new(2, 3, 7, 5))
        .focused(true)
        .on(Some(false));
}

#[test]
fn shapes_match_direct_calls_with_offset_clipping_and_semantics() {
    for format in [PixelFormat::Mono1, PixelFormat::Gray8] {
        for offset in [Point::new(5, 4), Point::new(-3, -2)] {
            let (mut direct, mut adapted) = (frame(format), frame(format));
            let (mut expected, mut actual) = (Vec::new(), Vec::new());
            for (frame, sink, backend) in [
                (&mut direct, &mut expected, false),
                (&mut adapted, &mut actual, true),
            ] {
                let mut canvas = frame.canvas().describing(sink).offset(offset.x, offset.y);
                let mut canvas = canvas.clipped(Rect::new(1, 1, 23, 22));
                described(&mut canvas);
                shapes(&mut canvas, backend);
                canvas.describe(Role::Text, "After drawing", Rect::new(4, 5, 9, 3));
            }
            assert_eq!(adapted.view().data, direct.view().data);
            assert_eq!(actual, expected);
            assert_eq!(actual.len(), 2);
            assert!(adapted.view().data.iter().any(|byte| *byte != 255));
        }
    }
}

#[test]
fn integer_errors_leave_pixels_and_semantics_untouched() {
    let mut frame = frame(PixelFormat::Gray8);
    let before = frame.view().data.to_vec();
    let mut records = Vec::<Described>::new();
    let mut canvas = frame.canvas().describing(&mut records);
    let too_large = u64::from(u32::MAX) + 1;
    let too_far = i64::from(i32::MAX) + 1;
    assert_eq!(
        graphics::fill_rect(&mut canvas, too_far, 0, 3, 3, Color::BLACK),
        Err(InvalidInput)
    );
    assert_eq!(
        graphics::fill_rect(&mut canvas, 0, 0, too_large, 3, Color::BLACK),
        Err(InvalidInput)
    );
    assert_eq!(
        graphics::draw_line(&mut canvas, 0, 0, -too_far - 1, 3, 1, Color::BLACK),
        Err(InvalidInput)
    );
    assert_eq!(
        graphics::draw_line(&mut canvas, 0, 0, 3, 3, too_large, Color::BLACK),
        Err(InvalidInput)
    );
    assert_eq!(
        graphics::fill_round(&mut canvas, 0, 0, 3, 3, too_large, Color::BLACK),
        Err(InvalidInput)
    );
    assert_eq!(
        graphics::stroke_round(&mut canvas, 0, 0, 3, 3, 1, too_large, Color::BLACK),
        Err(InvalidInput)
    );
    assert_eq!(
        graphics::stroke_round(
            &mut canvas,
            0,
            0,
            u64::from(u32::MAX),
            u64::from(u32::MAX),
            1 << 31,
            1,
            Color::BLACK
        ),
        Err(InvalidInput)
    );
    assert_eq!(frame.view().data, before);
    assert!(records.is_empty());
}

#[test]
fn zero_width_lines_and_zero_extent_strokes_keep_existing_semantics() {
    let (mut direct, mut adapted) = (frame(PixelFormat::Gray8), frame(PixelFormat::Gray8));
    direct
        .canvas()
        .draw_line(Point::new(2, 2), Point::new(10, 10), 0, Color::BLACK);
    graphics::draw_line(&mut adapted.canvas(), 2, 2, 10, 10, 0, Color::BLACK).unwrap();
    direct
        .canvas()
        .stroke_round_rect(15, 5, 0, 9, 2, 4, Color::BLACK);
    graphics::stroke_round(&mut adapted.canvas(), 15, 5, 0, 9, 2, 4, Color::BLACK).unwrap();
    assert_eq!(adapted.view().data, direct.view().data);
}

#[test]
fn full_integer_ranges_are_clipped_without_narrowing() {
    let mut filled = frame(PixelFormat::Gray8);
    graphics::fill_rect(
        &mut filled.canvas(),
        i64::from(i32::MIN),
        i64::from(i32::MIN),
        u64::from(u32::MAX),
        u64::from(u32::MAX),
        Color::BLACK,
    )
    .unwrap();
    assert!(filled.view().data.iter().all(|byte| *byte == 0));

    let mut line = frame(PixelFormat::Gray8);
    graphics::draw_line(
        &mut line.canvas(),
        i64::from(i32::MIN),
        0,
        i64::from(i32::MAX),
        0,
        1,
        Color::BLACK,
    )
    .unwrap();
    assert!(line.view().data[..40].iter().all(|byte| *byte == 0));
    assert!(line.view().data[40..].iter().all(|byte| *byte == 255));

    let (mut rounded, mut direct) = (frame(PixelFormat::Gray8), frame(PixelFormat::Gray8));
    graphics::fill_round(
        &mut rounded.canvas(),
        0,
        0,
        u64::from(u32::MAX),
        u64::from(u32::MAX),
        1,
        Color::BLACK,
    )
    .unwrap();
    direct
        .canvas()
        .fill_round_rect(0, 0, u32::MAX, u32::MAX, 1, Color::BLACK);
    assert_eq!(rounded.view().data, direct.view().data);
    assert!(rounded.view().data.iter().any(|byte| *byte != 255));
}

#[test]
fn borrowed_text_matches_direct_font_calls_and_preserves_semantics() {
    let fonts = fonts();
    let font = fonts.get("test.font").unwrap();
    for text in ["", "a", "abc", "אב a", "a\u{0301}"] {
        for size in [0.0, 12.0, 18.0] {
            assert_eq!(
                graphics::text_measure(&font, size, text).unwrap(),
                font.measure(size, text)
            );
            let (mut direct, mut adapted) = (frame(PixelFormat::Gray8), frame(PixelFormat::Gray8));
            let (mut expected, mut actual) = (Vec::new(), Vec::new());
            let mut widths = Vec::new();
            for (frame, sink, backend) in [
                (&mut direct, &mut expected, false),
                (&mut adapted, &mut actual, true),
            ] {
                let mut canvas = frame.canvas().describing(sink).offset(5, 4);
                let mut canvas = canvas.clipped(Rect::new(1, 1, 25, 22));
                described(&mut canvas);
                widths.push(if backend {
                    graphics::text_draw(&mut canvas, &font, size, -1.5, 16, text, Color::BLACK)
                        .unwrap()
                } else {
                    font.draw(&mut canvas, size, -1.5, 16, text, Color::BLACK)
                });
                canvas.describe(Role::Text, text, Rect::new(1, 2, 10, 4));
            }
            assert_eq!(widths[0], widths[1]);
            assert_eq!(adapted.view().data, direct.view().data);
            assert_eq!(actual, expected);
            assert_eq!(actual.len(), 2);
        }
    }
}

#[test]
fn invalid_text_arguments_fail_before_drawing() {
    let fonts = fonts();
    let font = fonts.get("test.font").unwrap();
    let mut frame = frame(PixelFormat::Gray8);
    let before = frame.view().data.to_vec();
    for size in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0] {
        assert_eq!(graphics::text_measure(&font, size, "a"), Err(InvalidInput));
        assert_eq!(
            graphics::text_draw(&mut frame.canvas(), &font, size, 0.0, 16, "a", Color::BLACK),
            Err(InvalidInput)
        );
    }
    for x in [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        2_147_483_648.0,
        -4_294_967_296.0,
    ] {
        assert_eq!(
            graphics::text_draw(&mut frame.canvas(), &font, 12.0, x, 16, "a", Color::BLACK),
            Err(InvalidInput)
        );
    }
    assert_eq!(
        graphics::text_draw(
            &mut frame.canvas(),
            &font,
            12.0,
            0.0,
            i64::from(i32::MAX) + 1,
            "a",
            Color::BLACK
        ),
        Err(InvalidInput)
    );
    assert_eq!(frame.view().data, before);
}
