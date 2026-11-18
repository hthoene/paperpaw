//! Slot 42 tells parts only to a describing canvas, as `Canvas::describe` does.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use alloc::vec::Vec;
use paperpaw_graphics::{Described, Framebuffer};
use paperpaw_native::host_glue::ERR_STATE;
use paperpaw_platform::PixelFormat;

fn frame() -> Framebuffer {
    Framebuffer::new(64, 64, PixelFormat::Gray8).unwrap()
}

#[test]
fn a_part_is_told_with_its_role_flags_and_area_where_somebody_asks() {
    let mut frame = frame();
    let mut sink: Vec<Described> = Vec::new();
    let scope = core::pin::pin!(CanvasScope::new());
    {
        let base = frame.canvas().describing(&mut sink);
        let mut canvas = base.offset(0, 10);
        scope
            .as_ref()
            .scope(&mut canvas, || {
                assert_eq!(scope.describe(2, FOCUSED, "", 0, 0, 64, 20), 1);
                assert_eq!(scope.describe(4, HAS_ON | ON, "", 40, 2, 16, 16), 1);
                assert_eq!(scope.describe(1, 0, "Title", -8, -20, 30, 10), 1);
                assert_eq!(scope.describe(99, DISABLED, "Later", 0, 30, 10, 10), 1);
            })
            .unwrap();
    }
    let told: Vec<_> = sink
        .iter()
        .map(|d| {
            (
                d.role,
                d.label.as_str(),
                d.area,
                d.focused,
                d.on,
                d.disabled,
            )
        })
        .collect();
    assert_eq!(
        told,
        [
            (Role::Row, "", Rect::new(0, 10, 64, 20), true, None, false),
            (
                Role::Switch,
                "",
                Rect::new(40, 12, 16, 16),
                false,
                Some(true),
                false
            ),
            // Cut at the canvas's (0, 0): what lies above it is not moved back.
            (
                Role::Heading,
                "Title",
                Rect::new(0, 10, 22, 0),
                false,
                None,
                false
            ),
            // A role this build does not know is told as text.
            (
                Role::Text,
                "Later",
                Rect::new(0, 40, 10, 10),
                false,
                None,
                true
            ),
        ]
    );
}

#[test]
fn nobody_asking_tells_nothing_and_refusals_come_first() {
    let mut frame = frame();
    let scope = core::pin::pin!(CanvasScope::new());
    assert_eq!(scope.describe(2, 0, "", 0, 0, 1, 1), ERR_STATE);
    let mut canvas = frame.canvas();
    scope
        .as_ref()
        .scope(&mut canvas, || {
            assert_eq!(scope.describe(2, 0, "Row", 0, 0, 10, 10), 0);
            assert_eq!(scope.describe(0, 0, "", 0, 0, 10, 10), ERR_INVALID);
            assert_eq!(scope.describe(2, 1 << 4, "", 0, 0, 10, 10), ERR_INVALID);
            // A part to touch needs an area; a heading does not.
            assert_eq!(scope.describe(3, 0, "OK", 5, 5, 0, 10), ERR_INVALID);
            assert_eq!(scope.describe(1, 0, "Title", 5, 5, 0, 0), 0);
        })
        .unwrap();
}

#[test]
fn an_area_is_cut_to_what_a_rect_holds() {
    assert_eq!(area(-5, -5, 10, 10), Rect::new(0, 0, 5, 5));
    assert_eq!(area(70_000, 0, 10, 10), Rect::new(65_535, 0, 0, 10));
    assert_eq!(area(0, 0, u64::MAX, 3), Rect::new(0, 0, 65_535, 3));
}

#[test]
fn drawn_text_is_told_as_text_only_where_somebody_asks() {
    let mut frame = frame();
    let mut sink: Vec<Described> = Vec::new();
    {
        let mut canvas = frame.canvas().describing(&mut sink);
        told_text(&mut canvas, "Name", 2.4, 3.6, 20.2, 12.0);
    }
    assert_eq!(sink.len(), 1);
    assert_eq!(sink[0].role, Role::Text);
    assert_eq!(sink[0].label, "Name");
    assert_eq!(sink[0].area, Rect::new(2, 3, 21, 12));
    let mut canvas = frame.canvas();
    // Not describing: nothing is made (and nothing allocated).
    told_text(&mut canvas, "Name", 0.0, 0.0, 10.0, 10.0);
    assert!(!canvas.is_describing());
}

/// Text positions round outward to whole pixels without `libm`.
#[test]
fn floats_round_outward_to_whole_pixels() {
    assert_eq!(
        (floor(1.5), floor(-1.5), floor(2.0), floor(f32::NAN)),
        (1, -2, 2, 0)
    );
    assert_eq!(
        (ceil(1.5), ceil(-1.5), ceil(2.0), ceil(f32::NAN)),
        (2, -1, 2, 0)
    );
    assert_eq!(floor(f32::NEG_INFINITY), i64::MIN);
}

/// A screen describing (or drawing text) in a loop stops at the pass's
/// bounds: slot 42 then answers 0, as where nobody asks (Senior R1, #1166).
#[test]
fn one_pass_keeps_at_most_its_parts_and_label_bytes() {
    let mut frame = frame();
    let mut sink: Vec<Described> = Vec::new();
    let scope = core::pin::pin!(CanvasScope::new());
    {
        let mut canvas = frame.canvas().describing(&mut sink);
        scope
            .as_ref()
            .scope(&mut canvas, || {
                let label = "x".repeat(256);
                let fitting = MAX_LABELS / label.len();
                for _ in 0..fitting {
                    assert_eq!(scope.describe(6, 0, &label, 0, 0, 4, 4), 1);
                }
                assert_eq!(scope.describe(6, 0, &label, 0, 0, 4, 4), 0);
                let mut told = 0;
                for _ in 0..2 * MAX_PARTS {
                    told += scope.describe(2, 0, "", 0, 0, 4, 4);
                }
                assert_eq!(usize::try_from(told).unwrap(), MAX_PARTS - fitting);
            })
            .unwrap();
        told_text(&mut canvas, "late", 0.0, 0.0, 4.0, 4.0);
    }
    assert_eq!(sink.len(), MAX_PARTS);
    assert_eq!(
        sink.iter().map(|d| d.label.len()).sum::<usize>(),
        MAX_LABELS
    );
}

/// End to end through the merge rules (`paperpaw_sdk::semantics::parts`): a
/// row told without a label takes the text drawn inside it as its one label;
/// a row with a label of its own keeps it (Code Reviewer, #1166).
#[test]
fn drawn_text_labels_an_empty_row_and_leaves_a_labelled_row_alone() {
    let mut frame = frame();
    let mut sink: Vec<Described> = Vec::new();
    let scope = core::pin::pin!(CanvasScope::new());
    {
        let mut canvas = frame.canvas().describing(&mut sink);
        scope
            .as_ref()
            .scope(&mut canvas, || {
                assert_eq!(scope.describe(2, FOCUSED, "", 0, 0, 64, 20), 1);
            })
            .unwrap();
        told_text(&mut canvas, "Name", 4.0, 4.0, 30.0, 12.0);
        scope
            .as_ref()
            .scope(&mut canvas, || {
                assert_eq!(scope.describe(2, 0, "Own", 0, 30, 64, 20), 1);
            })
            .unwrap();
        told_text(&mut canvas, "Other", 4.0, 34.0, 30.0, 12.0);
    }
    let parts = paperpaw_sdk::semantics::parts(sink);
    let told: Vec<_> = parts
        .iter()
        .map(|p| (p.role, p.label.as_str(), p.focused))
        .collect();
    assert_eq!(told, [(Role::Row, "Name", true), (Role::Row, "Own", false)]);
}
