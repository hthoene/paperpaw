//! Real pixel and borrow-lifetime checks for the public host drawing scope.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail assertions")]
use paperpaw_core::native_host::{Error, canvas::CanvasScope};
use paperpaw_graphics::{Framebuffer, Role};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_STATE, OK};
use paperpaw_platform::{Color, PixelFormat, Rect};
use std::pin::pin;

fn frame(format: PixelFormat) -> Framebuffer {
    let mut frame = Framebuffer::new(40, 32, format).unwrap();
    frame.canvas().fill(Rect::from_size(40, 32), Color::WHITE);
    frame
}
#[test]
fn borrowed_fill_matches_pixels_origin_clip_and_existing_semantics() {
    for format in [PixelFormat::Mono1, PixelFormat::Gray8] {
        let (mut expected, mut actual) = (frame(format), frame(format));
        let (mut expected_sink, mut actual_sink) = (Vec::new(), Vec::new());
        let scope = pin!(CanvasScope::new());
        for (frame, sink, adapted) in [
            (&mut expected, &mut expected_sink, false),
            (&mut actual, &mut actual_sink, true),
        ] {
            let mut canvas = frame.canvas().describing(sink).offset(5, -2);
            let mut canvas = canvas.clipped(Rect::new(1, 3, 22, 20));
            canvas.describe(Role::Button, "Before", Rect::new(3, 3, 5, 5));
            if adapted {
                scope
                    .as_ref()
                    .scope(&mut canvas, || {
                        assert_eq!(scope.fill_rect(-4, 5, 18, 9, Color::rgb(40, 70, 110)), OK);
                        assert_eq!(scope.fill_rect(10, 8, 15, 14, Color::BLACK), OK);
                    })
                    .unwrap();
            } else {
                canvas.fill_rect(-4, 5, 18, 9, Color::rgb(40, 70, 110));
                canvas.fill_rect(10, 8, 15, 14, Color::BLACK);
            }
            canvas.describe(Role::Text, "After", Rect::new(5, 5, 6, 4));
        }
        assert_eq!(actual.view().data, expected.view().data);
        assert_eq!(actual_sink, expected_sink);
        assert!(actual.view().data.iter().any(|byte| *byte != 255));
        assert_eq!(scope.fill_rect(0, 0, 1, 1, Color::BLACK), ERR_STATE);
    }
}
#[test]
fn rejected_calls_leave_pixels_untouched() {
    let mut frame = frame(PixelFormat::Gray8);
    let before = frame.view().data.to_vec();
    let scope = pin!(CanvasScope::new());
    assert_eq!(scope.fill_rect(0, 0, 1, 1, Color::BLACK), ERR_STATE);
    scope
        .as_ref()
        .scope(&mut frame.canvas(), || {
            for (x, y, w, h) in [
                (i64::MAX, 0, 1, 1),
                (0, i64::MIN, 1, 1),
                (0, 0, u64::MAX, 1),
                (0, 0, 1, u64::MAX),
            ] {
                assert_eq!(scope.fill_rect(x, y, w, h, Color::BLACK), ERR_INVALID);
            }
            assert_eq!(scope.fill_rect(0, 0, 0, 10, Color::BLACK), OK);
            assert_eq!(
                scope.fill_rect(i64::from(i32::MAX), 0, u64::from(u32::MAX), 1, Color::BLACK),
                OK
            );
        })
        .unwrap();
    assert_eq!(frame.view().data, before);
}
#[test]
fn nested_error_and_unwind_paths_clear_or_preserve_the_correct_canvas() {
    let mut first = frame(PixelFormat::Gray8);
    let mut second = frame(PixelFormat::Gray8);
    let untouched = second.view().data.to_vec();
    let scope = pin!(CanvasScope::new());
    let result = scope
        .as_ref()
        .scope(&mut first.canvas(), || {
            assert!(matches!(
                scope.as_ref().scope(&mut second.canvas(), || ()),
                Err(Error::Busy)
            ));
            assert_eq!(scope.fill_rect(0, 0, 1, 1, Color::BLACK), OK);
            Err::<(), _>(17)
        })
        .unwrap();
    assert_eq!(result, Err(17));
    assert_eq!(second.view().data, untouched);
    assert_eq!(scope.fill_rect(0, 0, 1, 1, Color::BLACK), ERR_STATE);
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        scope
            .as_ref()
            .scope(&mut first.canvas(), || panic!("fixture unwind"))
            .unwrap();
    }));
    assert!(unwind.is_err());
    assert_eq!(scope.fill_rect(0, 0, 1, 1, Color::BLACK), ERR_STATE);
    let before = first.view().data.to_vec();
    scope
        .as_ref()
        .scope(&mut second.canvas(), || {
            assert_eq!(scope.fill_rect(2, 2, 3, 3, Color::BLACK), OK);
        })
        .unwrap();
    assert_eq!(first.view().data, before);
    assert_ne!(second.view().data, untouched);
}
