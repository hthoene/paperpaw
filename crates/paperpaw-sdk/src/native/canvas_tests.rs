#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::*;
use alloc::vec::Vec;
use core::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr,
};
use paperpaw_native::host_glue::{self as abi, PpHost};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Call {
    Rect(i32, i32, u32, u32, u32),
    Line(i32, i32, i32, i32, u32, u32),
    Fill(i32, i32, u32, u32, u32, u32),
    Stroke(i32, i32, u32, u32, u32, u32, u32),
}

#[derive(Default)]
struct Context {
    calls: RefCell<Vec<Call>>,
    status: Cell<i32>,
    drawing: Cell<bool>,
}

unsafe fn record_canvas_call(ctx: *mut c_void, call: Call) -> i32 {
    // SAFETY: all test canvas calls retain this context through their host.
    let ctx = unsafe { &*ctx.cast::<Context>() };
    if !ctx.drawing.get() {
        return abi::ERR_STATE;
    }
    ctx.calls.borrow_mut().push(call);
    ctx.status.get()
}

unsafe extern "C" fn canvas_rect_slot(
    ctx: *mut c_void,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    color: u32,
) -> i32 {
    // SAFETY: context is the one retained by this test's callback host.
    unsafe { record_canvas_call(ctx, Call::Rect(x, y, w, h, color)) }
}
unsafe extern "C" fn canvas_line_slot(
    ctx: *mut c_void,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    width: u32,
    color: u32,
) -> i32 {
    // SAFETY: context is the one retained by this test's callback host.
    unsafe { record_canvas_call(ctx, Call::Line(x0, y0, x1, y1, width, color)) }
}
unsafe extern "C" fn canvas_round_fill_slot(
    ctx: *mut c_void,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    radius: u32,
    color: u32,
) -> i32 {
    // SAFETY: context is the one retained by this test's callback host.
    unsafe { record_canvas_call(ctx, Call::Fill(x, y, w, h, radius, color)) }
}
unsafe extern "C" fn canvas_round_stroke_slot(
    ctx: *mut c_void,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    radius: u32,
    thickness: u32,
    color: u32,
) -> i32 {
    // SAFETY: context is the one retained by this test's callback host.
    unsafe { record_canvas_call(ctx, Call::Stroke(x, y, w, h, radius, thickness, color)) }
}

fn table() -> PpHost {
    let mut table = abi::host_table();
    table.canvas_fill_rect = canvas_rect_slot;
    table.canvas_draw_line = canvas_line_slot;
    table.canvas_fill_round = canvas_round_fill_slot;
    table.canvas_stroke_round = canvas_round_stroke_slot;
    table
}

#[test]
fn canvas_preserves_full_width_scalars_and_rgb_in_every_slot() {
    let table = table();
    let mut ctx = Context::default();
    ctx.drawing.set(true);
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let frame = FrameRect::new(-7, 13, 70_000, 90_000);
    let canvas = Canvas::from(&host, frame, frame);
    assert_eq!(canvas.frame_rect(), frame);
    let bounds = FrameRect::new(i32::MIN, i32::MAX, u32::MAX, 70_001);
    let color = Color::rgb(0x12, 0x34, 0x56);
    canvas.fill_rectangle(bounds, color).unwrap();
    canvas
        .draw_segment(
            Point::new(-80_000, i32::MAX),
            Point::new(i32::MIN, 90_000),
            u32::MAX,
            color,
        )
        .unwrap();
    canvas
        .fill_rounded_rectangle(bounds, 80_001, color)
        .unwrap();
    canvas
        .stroke_rounded_rectangle(bounds, 90_002, 100_003, color)
        .unwrap();
    assert_eq!(
        ctx.calls.borrow().as_slice(),
        &[
            Call::Rect(i32::MIN, i32::MAX, u32::MAX, 70_001, 0x0012_3456),
            Call::Line(-80_000, i32::MAX, i32::MIN, 90_000, u32::MAX, 0x0012_3456),
            Call::Fill(i32::MIN, i32::MAX, u32::MAX, 70_001, 80_001, 0x0012_3456),
            Call::Stroke(
                i32::MIN,
                i32::MAX,
                u32::MAX,
                70_001,
                90_002,
                100_003,
                0x0012_3456
            ),
        ]
    );
}

fn assert_operations(canvas: &Canvas<'_>, expected: Result<(), Error>) {
    let bounds = FrameRect::new(-1, -2, 0, 0);
    let color = Color::rgb(255, 0, 255);
    assert_eq!(canvas.fill_rectangle(bounds, color), expected);
    assert_eq!(
        canvas.draw_segment(Point::new(-3, -4), Point::new(5, 6), 0, color),
        expected
    );
    assert_eq!(canvas.fill_rounded_rectangle(bounds, 7, color), expected);
    assert_eq!(
        canvas.stroke_rounded_rectangle(bounds, 8, 9, color),
        expected
    );
}

#[test]
fn host_drawing_admission_and_errors_are_preserved() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let canvas = Canvas::from(
        &host,
        FrameRect::new(0, 0, 600, 800),
        FrameRect::new(0, 0, 600, 800),
    );
    assert_operations(&canvas, Err(Error::State));
    assert!(ctx.calls.borrow().is_empty());
    ctx.drawing.set(true);
    for error in [
        abi::ERR_INVALID,
        abi::ERR_BUSY,
        abi::ERR_DENIED,
        abi::ERR_UNSUPPORTED,
        -900,
    ] {
        ctx.status.set(error);
        assert_operations(&canvas, Err(Error::from_status(error)));
    }
    assert_eq!(ctx.calls.borrow().len(), 20);
}

#[test]
fn default_host_table_refuses_every_canvas_operation() {
    let table = abi::host_table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let canvas = Canvas::from(
        &host,
        FrameRect::new(0, 0, 600, 800),
        FrameRect::new(0, 0, 600, 800),
    );
    assert_operations(&canvas, Err(Error::Unsupported));
}

fn draw_bytes() -> Vec<u8> {
    use super::super::declare::records_wire::{self as wire, Record};
    let frame = wire::DrawFrame {
        purpose: 0,
        x: -17,
        y: -20,
        width: 70_000,
        height: 80_000,
        clip_top: -10,
        clip_bottom: 123,
        id: 73,
        payload: b"lease",
    };
    let mut bytes = [0; 128];
    let len = wire::records_encode(&[Record::DrawFrame(frame)], &mut bytes).unwrap();
    bytes[..len].to_vec()
}

#[test]
fn bound_draw_frame_exposes_exact_callback_canvas_and_clip() {
    use super::super::callbacks::DrawFrame;
    let table = table();
    let mut ctx = Context::default();
    ctx.drawing.set(true);
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let bytes = draw_bytes();
    let decoded = DrawFrame::decode(&bytes).unwrap();
    let mut bound = decoded.bind(&host);
    assert_eq!(DrawFrame::decode(&bytes).unwrap(), bound);
    let canvas = bound.draw_canvas().unwrap();
    assert_eq!(
        canvas.frame_rect(),
        FrameRect::new(-17, -20, 70_000, 80_000)
    );
    assert_eq!(canvas._clip, FrameRect::new(-17, -10, 70_000, 133));
    assert!(core::ptr::eq(canvas.host(), &host));
    canvas
        .fill_rectangle(canvas.frame_rect(), Color::WHITE)
        .unwrap();
    assert_eq!(
        ctx.calls.borrow().as_slice(),
        &[Call::Rect(-17, -20, 70_000, 80_000, 0x00ff_ffff)]
    );
}

#[test]
fn callback_canvas_refuses_mutated_invalid_frames_before_drawing() {
    use super::super::callbacks::DrawFrame;
    let table = table();
    let mut ctx = Context::default();
    ctx.drawing.set(true);
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let bytes = draw_bytes();
    let original = || DrawFrame::decode(&bytes).unwrap().bind(&host);
    for (top, bottom) in [(-21, 0), (0, 80_000), (9, 9), (10, 9)] {
        let mut frame = original();
        frame.clip_top = top;
        frame.clip_bottom = bottom;
        assert_eq!(frame.draw_canvas().unwrap_err(), Error::Invalid);
    }
    let mut empty_width = original();
    empty_width.rect.width = 0;
    assert_eq!(empty_width.draw_canvas().unwrap_err(), Error::Invalid);
    let mut empty_height = original();
    empty_height.rect.height = 0;
    assert_eq!(empty_height.draw_canvas().unwrap_err(), Error::Invalid);
    let mut frame = original();
    frame.rect.y = i32::MIN;
    frame.rect.height = u32::MAX;
    frame.clip_top = i32::MIN;
    frame.clip_bottom = i32::MAX;
    assert_eq!(
        frame.draw_canvas().unwrap()._clip,
        FrameRect::new(-17, i32::MIN, 70_000, u32::MAX)
    );
    assert!(ctx.calls.borrow().is_empty());
}

#[test]
fn dropping_canvas_allows_another_borrow_of_the_same_frame() {
    use super::super::callbacks::DrawFrame;
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let bytes = draw_bytes();
    let mut frame = DrawFrame::decode(&bytes).unwrap().bind(&host);
    let rect = frame.rect;
    let first = frame.draw_canvas().unwrap();
    assert_eq!(first.frame_rect(), rect);
    drop(first);
    let second = frame.draw_canvas().unwrap();
    assert_eq!(second.frame_rect(), rect);
    drop(second);
    assert!(frame.draw_canvas().is_ok());
}
