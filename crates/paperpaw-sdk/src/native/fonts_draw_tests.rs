#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::*;
use crate::native::{callbacks::DrawFrame, declare::records_wire as wire};
use alloc::string::String;
use core::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr,
};
use paperpaw_native::host_glue::{self as abi, PpHost};

#[derive(Debug, PartialEq)]
struct Drawn {
    font: u32,
    size: f32,
    x: f32,
    baseline: i32,
    color: u32,
    text: String,
}
#[derive(Default)]
struct Context {
    drawn: RefCell<Option<Drawn>>,
    status: Cell<i32>,
}
unsafe extern "C" fn acquire_draw_font(_: *mut c_void, _: PpBytes, out: *mut u32) -> i32 {
    // SAFETY: the wrapper supplies writable local output.
    unsafe {
        out.write(u32::MAX);
    }
    abi::OK
}
unsafe extern "C" fn release_draw_font(_: *mut c_void, _: u32) -> i32 {
    abi::OK
}
unsafe extern "C" fn font_drawing_slot(
    ctx: *mut c_void,
    font: u32,
    size: f32,
    x: f32,
    baseline: i32,
    color: u32,
    text: PpBytes,
    out: *mut f32,
) -> i32 {
    // SAFETY: each call retains the fixture and disjoint valid input/output.
    unsafe {
        let ctx = &*ctx.cast::<Context>();
        let text = core::str::from_utf8(core::slice::from_raw_parts(text.ptr, text.len)).unwrap();
        ctx.drawn.replace(Some(Drawn {
            font,
            size,
            x,
            baseline,
            color,
            text: String::from(text),
        }));
        out.write(-7.5);
        ctx.status.get()
    }
}
fn drawing_table() -> PpHost {
    let mut table = abi::host_table();
    table.font_acquire = acquire_draw_font;
    table.handle_release = release_draw_font;
    table.text_draw = font_drawing_slot;
    table
}
fn bound_frame<'a>(host: &'a Host<'a>, out: &'a mut [u8]) -> DrawFrame<'a> {
    let len = wire::records_encode(
        &[wire::Record::DrawFrame(wire::DrawFrame {
            purpose: 0,
            x: 0,
            y: 0,
            width: 100,
            height: 200,
            clip_top: 10,
            clip_bottom: 180,
            id: 0,
            payload: &[],
        })],
        out,
    )
    .unwrap();
    DrawFrame::decode(&out[..len]).unwrap().bind(host)
}
#[test]
fn text_draw_preserves_every_scalar_color_and_status() {
    let table = drawing_table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let font = Font::acquire_role(&host, FontRole::Ui).unwrap();
    let mut bytes = [0; 256];
    let mut frame = bound_frame(&host, &mut bytes);
    let canvas = frame.canvas().unwrap();
    let origin = TextOrigin::new(-12.25, i32::MIN);
    assert_eq!(
        font.render_text(
            &canvas,
            19.5,
            origin,
            Color::rgb(0x12, 0x34, 0x56),
            "Paw 🐾"
        ),
        Ok(-7.5)
    );
    assert_eq!(
        *ctx.drawn.borrow(),
        Some(Drawn {
            font: u32::MAX,
            size: 19.5,
            x: -12.25,
            baseline: i32::MIN,
            color: 0x0012_3456,
            text: String::from("Paw 🐾"),
        })
    );
    for code in [abi::ERR_STATE, abi::ERR_DENIED, -777] {
        ctx.status.set(code);
        assert_eq!(
            font.render_text(&canvas, 19.5, origin, Color::BLACK, ""),
            Err(Error::from_status(code))
        );
    }
}
#[test]
fn invalid_text_inputs_do_not_reach_the_host() {
    let table = drawing_table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let font = Font::acquire(&host, "role:ui").unwrap();
    let mut bytes = [0; 256];
    let mut frame = bound_frame(&host, &mut bytes);
    let canvas = frame.canvas().unwrap();
    for size in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert_eq!(
            font.render_text(&canvas, size, TextOrigin::new(0.0, 0), Color::BLACK, "x"),
            Err(Error::Invalid)
        );
    }
    assert_eq!(
        font.render_text(
            &canvas,
            12.0,
            TextOrigin::new(f32::NAN, 0),
            Color::BLACK,
            "x"
        ),
        Err(Error::Invalid)
    );
    let long = "é".repeat(TEXT_MAX_BYTES / 2 + 1);
    assert_eq!(
        font.render_text(&canvas, 12.0, TextOrigin::new(0.0, 0), Color::BLACK, &long),
        Err(Error::NoSpace)
    );
    assert!(ctx.drawn.borrow().is_none());
}
#[test]
fn font_cannot_be_drawn_through_another_host_binding() {
    let table = drawing_table();
    let mut first = Context::default();
    let mut second = Context::default();
    let owner = Host {
        table: &table,
        ctx: ptr::from_mut(&mut first).cast(),
    };
    let other = Host {
        table: &table,
        ctx: ptr::from_mut(&mut second).cast(),
    };
    let font = Font::acquire(&owner, "role:ui").unwrap();
    let mut bytes = [0; 256];
    let mut frame = bound_frame(&other, &mut bytes);
    let canvas = frame.canvas().unwrap();
    assert_eq!(
        font.render_text(&canvas, 12.0, TextOrigin::new(0.0, 0), Color::BLACK, "x"),
        Err(Error::Invalid)
    );
    assert!(first.drawn.borrow().is_none());
    assert!(second.drawn.borrow().is_none());
}
