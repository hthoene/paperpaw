#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::*;
use alloc::{string::String, vec, vec::Vec};
use core::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr,
};
use paperpaw_native::host_glue::{self as abi, PpBytes, PpHost};

#[derive(Debug, PartialEq)]
enum Call {
    Metrics(u32, f32),
    Ink(u32, f32, String),
}
#[derive(Default)]
struct Context {
    calls: RefCell<Vec<Call>>,
    metrics: Cell<[f32; 3]>,
    ink: Cell<[i32; 4]>,
    result: Cell<i32>,
}
unsafe extern "C" fn metrics_slot(
    ctx: *mut c_void,
    font: u32,
    size: f32,
    ascent: *mut f32,
    descent: *mut f32,
    line_height: *mut f32,
) -> i32 {
    // SAFETY: the wrapper supplies a live context and three writable scalar outputs.
    unsafe {
        let ctx = &*ctx.cast::<Context>();
        ctx.calls.borrow_mut().push(Call::Metrics(font, size));
        if ctx.result.get() == abi::OK {
            let values = ctx.metrics.get();
            ascent.write(values[0]);
            descent.write(values[1]);
            line_height.write(values[2]);
        }
        ctx.result.get()
    }
}
unsafe extern "C" fn ink_slot(
    ctx: *mut c_void,
    font: u32,
    size: f32,
    text: PpBytes,
    top: *mut i32,
    bottom: *mut i32,
    left: *mut i32,
    right: *mut i32,
) -> i32 {
    // SAFETY: the wrapper supplies readable text and four writable scalar outputs.
    unsafe {
        let ctx = &*ctx.cast::<Context>();
        let text = core::str::from_utf8(core::slice::from_raw_parts(text.ptr, text.len)).unwrap();
        ctx.calls
            .borrow_mut()
            .push(Call::Ink(font, size, text.into()));
        if ctx.result.get() == abi::OK {
            let values = ctx.ink.get();
            top.write(values[0]);
            bottom.write(values[1]);
            left.write(values[2]);
            right.write(values[3]);
        }
        ctx.result.get()
    }
}
fn table() -> PpHost {
    let mut table = abi::host_table();
    table.text_metrics = metrics_slot;
    table.text_ink = ink_slot;
    table
}

#[test]
fn metrics_and_ink_forward_distinct_scalars_without_clamping() {
    let table = table();
    let mut ctx = Context::default();
    ctx.metrics.set([11.25, 3.75, 20.5]);
    ctx.ink.set([i32::MIN, i32::MAX, -17, 23]);
    let host = super::super::Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    for handle in [0, u32::MAX] {
        let font = Font {
            host: &host,
            handle: Some(handle),
        };
        assert_eq!(
            font.read_line_metrics(17.25),
            Ok(LineMetrics::new(11.25, 3.75, 20.5))
        );
        assert_eq!(
            font.ink(19.5, "Grüße"),
            Ok(Some(InkBounds::new(i32::MIN, i32::MAX, -17, 23)))
        );
    }
    assert_eq!(
        *ctx.calls.borrow(),
        vec![
            Call::Metrics(0, 17.25),
            Call::Ink(0, 19.5, "Grüße".into()),
            Call::Metrics(u32::MAX, 17.25),
            Call::Ink(u32::MAX, 19.5, "Grüße".into())
        ]
    );
}

#[test]
fn no_ink_is_none_and_other_statuses_are_preserved() {
    let table = table();
    let mut ctx = Context::default();
    let host = super::super::Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let font = Font {
        host: &host,
        handle: Some(42),
    };
    ctx.result.set(abi::ERR_NOT_FOUND);
    assert_eq!(font.ink(12.0, ""), Ok(None));
    assert_eq!(font.ink(12.0, "   "), Ok(None));
    assert_eq!(font.read_line_metrics(12.0), Err(Error::NotFound));
    for (code, expected) in [
        (abi::ERR_STATE, Error::State),
        (abi::ERR_DENIED, Error::Denied),
        (-125, Error::Other(-125)),
    ] {
        ctx.result.set(code);
        assert_eq!(font.read_line_metrics(12.0), Err(expected));
        assert_eq!(font.ink(12.0, "ink"), Err(expected));
    }
}

#[test]
fn invalid_sizes_overlong_text_and_released_handles_never_reach_host() {
    let table = table();
    let mut ctx = Context::default();
    let host = super::super::Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut font = Font {
        host: &host,
        handle: Some(7),
    };
    for size in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(font.read_line_metrics(size), Err(Error::Invalid));
        assert_eq!(font.ink(size, "text"), Err(Error::Invalid));
    }
    assert_eq!(
        font.ink(12.0, "x".repeat(super::super::TEXT_MAX_BYTES + 1)),
        Err(Error::NoSpace)
    );
    font.handle = None;
    assert_eq!(font.read_line_metrics(12.0), Err(Error::State));
    assert_eq!(font.ink(12.0, "text"), Err(Error::State));
    assert!(ctx.calls.borrow().is_empty());
}

#[test]
fn default_metrics_and_ink_slots_are_unsupported() {
    let table = abi::host_table();
    let host = super::super::Host {
        table: &table,
        ctx: ptr::null_mut(),
    };
    let font = Font {
        host: &host,
        handle: Some(1),
    };
    assert_eq!(font.read_line_metrics(12.0), Err(Error::Unsupported));
    assert_eq!(font.ink(12.0, "text"), Err(Error::Unsupported));
}
