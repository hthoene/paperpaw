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
    Covers(u32, String),
}
#[derive(Default)]
struct Context {
    calls: RefCell<Vec<Call>>,
    metrics: Cell<[f32; 3]>,
    ink: Cell<[i32; 4]>,
    covers: Cell<i32>,
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
unsafe extern "C" fn covers_slot(ctx: *mut c_void, font: u32, text: PpBytes) -> i32 {
    // SAFETY: the wrapper supplies a live context and readable text.
    unsafe {
        let ctx = &*ctx.cast::<Context>();
        let text = core::str::from_utf8(core::slice::from_raw_parts(text.ptr, text.len)).unwrap();
        ctx.calls.borrow_mut().push(Call::Covers(font, text.into()));
        ctx.covers.get()
    }
}
fn table() -> PpHost {
    let mut table = abi::host_table();
    table.text_metrics = metrics_slot;
    table.text_ink = ink_slot;
    table.text_covers = covers_slot;
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
        font.ink(12.0, "x".repeat(super::super::TEXT_INK_MAX_BYTES + 1)),
        Err(Error::Invalid)
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

#[test]
fn covers_answers_the_length_the_font_draws_at_a_character_boundary() {
    let table = table();
    let mut ctx = Context::default();
    let host = super::super::Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let font = Font {
        host: &host,
        handle: Some(5),
    };
    // Every character drawn, the first undrawn one, and none at all.
    for (text, length) in [("abc", 3), ("añb", 1), ("añb", 3), ("añb", 0), ("", 0)] {
        ctx.covers.set(i32::try_from(length).unwrap());
        assert_eq!(font.covers(text), Ok(length), "{text:?}");
    }
    assert_eq!(
        ctx.calls.borrow().first(),
        Some(&Call::Covers(5, "abc".into()))
    );
}

#[test]
fn covers_refuses_what_the_contract_does_not_allow() {
    let table = table();
    let mut ctx = Context::default();
    let host = super::super::Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let font = Font {
        host: &host,
        handle: Some(5),
    };
    // Over 1,024 bytes nothing is asked; exactly 1,024 is.
    assert_eq!(font.covers("x".repeat(1025)), Err(Error::Invalid));
    assert!(ctx.calls.borrow().is_empty());
    ctx.covers.set(1024);
    assert_eq!(font.covers("x".repeat(1024)), Ok(1024));
    // A length past the text, or inside a character, is no answer.
    ctx.covers.set(4);
    assert_eq!(font.covers("abc"), Err(Error::Invalid));
    ctx.covers.set(2);
    assert_eq!(font.covers("añb"), Err(Error::Invalid));
    // Refusals keep their status; a font without a handle is stale.
    for (code, expected) in [
        (abi::ERR_STATE, Error::State),
        (abi::ERR_UNSUPPORTED, Error::Unsupported),
        (abi::ERR_UTF8, Error::Utf8),
    ] {
        ctx.covers.set(code);
        assert_eq!(font.covers("abc"), Err(expected));
    }
    let stale = Font {
        host: &host,
        handle: None,
    };
    assert_eq!(stale.covers("abc"), Err(Error::State));
}
