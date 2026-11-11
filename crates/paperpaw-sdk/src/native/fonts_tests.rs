#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::*;
use alloc::{string::String, vec::Vec};
use core::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr,
};
use paperpaw_native::host_glue::{self as abi, PpHost};

#[derive(Default)]
struct Context {
    key: RefCell<String>,
    text: RefCell<String>,
    acquired: Cell<usize>,
    measured: Cell<usize>,
    handle: Cell<u32>,
    measured_handle: Cell<u32>,
    measured_size: Cell<f32>,
    advance: Cell<f32>,
    acquire_status: Cell<i32>,
    measure_status: Cell<i32>,
    release_status: Cell<i32>,
    released: RefCell<Vec<u32>>,
}

unsafe fn text_in<'a>(bytes: PpBytes) -> &'a str {
    // SAFETY: all test calls come from wrappers retaining readable UTF-8 text.
    unsafe { core::str::from_utf8(core::slice::from_raw_parts(bytes.ptr, bytes.len)).unwrap() }
}

unsafe extern "C" fn font_acquire_slot(ctx: *mut c_void, key: PpBytes, out: *mut u32) -> i32 {
    // SAFETY: test host/context/output live throughout each wrapper call.
    unsafe {
        let ctx = &*ctx.cast::<Context>();
        ctx.key.replace(String::from(text_in(key)));
        ctx.acquired.set(ctx.acquired.get() + 1);
        out.write(ctx.handle.get());
        ctx.acquire_status.get()
    }
}

unsafe extern "C" fn font_measure_slot(
    ctx: *mut c_void,
    font: u32,
    size: f32,
    text: PpBytes,
    out: *mut f32,
) -> i32 {
    // SAFETY: test host/context/output live throughout each wrapper call.
    unsafe {
        let ctx = &*ctx.cast::<Context>();
        ctx.text.replace(String::from(text_in(text)));
        ctx.measured.set(ctx.measured.get() + 1);
        ctx.measured_handle.set(font);
        ctx.measured_size.set(size);
        out.write(ctx.advance.get());
        ctx.measure_status.get()
    }
}

unsafe extern "C" fn font_release_slot(ctx: *mut c_void, handle: u32) -> i32 {
    // SAFETY: test font retains this context until it is released or dropped.
    let ctx = unsafe { &*ctx.cast::<Context>() };
    ctx.released.borrow_mut().push(handle);
    ctx.release_status.get()
}

fn table() -> PpHost {
    let mut table = abi::host_table();
    table.font_acquire = font_acquire_slot;
    table.text_measure = font_measure_slot;
    table.handle_release = font_release_slot;
    table
}

#[test]
fn opaque_handles_including_zero_are_measured_and_released_once() {
    let table = table();
    let mut ctx = Context::default();
    ctx.advance.set(123.5);
    let ctx_ptr = ptr::from_mut(&mut ctx).cast();
    let host = Host {
        table: &table,
        ctx: ctx_ptr,
    };
    for handle in [0, 1, u32::MAX] {
        ctx.handle.set(handle);
        let font = Font::acquire(&host, "fonts-default.ui").unwrap();
        assert_eq!(font.measure_text(17.25, "Grüße"), Ok(123.5));
        assert_eq!(ctx.key.borrow().as_str(), "fonts-default.ui");
        assert_eq!(ctx.text.borrow().as_str(), "Grüße");
        assert_eq!(ctx.measured_handle.get(), handle);
        assert_eq!(ctx.measured_size.get(), 17.25);
        font.release().unwrap();
    }
    assert_eq!(ctx.released.borrow().as_slice(), &[0, 1, u32::MAX]);
    assert_eq!((ctx.acquired.get(), ctx.measured.get()), (3, 3));
}

#[test]
fn role_keys_are_resolved_by_the_host_and_drop_releases() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    for (role, key) in [
        (FontRole::Ui, "role:ui"),
        (FontRole::UiBold, "role:ui-bold"),
        (FontRole::Reading, "role:reading"),
        (FontRole::ReadingBold, "role:reading-bold"),
        (FontRole::ReadingItalic, "role:reading-italic"),
        (FontRole::ReadingBoldItalic, "role:reading-bold-italic"),
        (FontRole::Status, "role:status"),
        (FontRole::Mono, "role:mono"),
    ] {
        let font = Font::acquire_role(&host, role).unwrap();
        assert_eq!(ctx.key.borrow().as_str(), key);
        drop(font);
    }
    assert_eq!(ctx.released.borrow().len(), 8);
}

#[test]
fn host_errors_are_preserved_and_failed_release_is_not_retried() {
    let table = table();
    let mut ctx = Context::default();
    ctx.handle.set(u32::MAX);
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    for code in [abi::ERR_NOT_FOUND, abi::ERR_FULL, abi::ERR_DENIED, -900] {
        ctx.acquire_status.set(code);
        assert_eq!(
            Font::acquire(&host, "missing").unwrap_err(),
            Error::from_status(code)
        );
    }
    assert!(ctx.released.borrow().is_empty());
    ctx.acquire_status.set(abi::OK);
    let font = Font::acquire(&host, "role:ui").unwrap();
    for code in [abi::ERR_STATE, abi::ERR_UNSUPPORTED, -901] {
        ctx.measure_status.set(code);
        assert_eq!(font.measure_text(18.0, "A"), Err(Error::from_status(code)));
    }
    ctx.release_status.set(abi::ERR_STATE);
    assert_eq!(font.release(), Err(Error::State));
    assert_eq!(ctx.released.borrow().as_slice(), &[u32::MAX]);
}

#[test]
fn invalid_sizes_and_oversized_utf8_are_refused_before_host_calls() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    assert_eq!(
        Font::acquire(&host, "é".repeat(65)).unwrap_err(),
        Error::Invalid
    );
    assert_eq!(ctx.acquired.get(), 0);
    let font = Font::acquire(&host, "a".repeat(128)).unwrap();
    for size in [0.0, -0.0, -1.0, f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
        assert_eq!(font.measure_text(size, "A"), Err(Error::Invalid));
    }
    assert_eq!(
        font.measure_text(18.0, "é".repeat(2049)),
        Err(Error::Invalid)
    );
    assert_eq!(ctx.measured.get(), 0);
    assert_eq!(font.measure_text(18.0, "é".repeat(2048)), Ok(0.0));
    assert_eq!(ctx.measured.get(), 1);
}

#[test]
fn default_host_table_reports_unsupported() {
    let table = abi::host_table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    assert_eq!(
        Font::acquire_role(&host, FontRole::Ui).unwrap_err(),
        Error::Unsupported
    );
}

#[test]
fn host_font_limit_is_forwarded_and_recovers_after_release() {
    // This fake host chooses a small limit; the SDK forwards its refusal.
    #[derive(Default)]
    struct Capped {
        next: Cell<u32>,
        live: RefCell<Vec<u32>>,
    }
    unsafe extern "C" fn capped_acquire(ctx: *mut c_void, _: PpBytes, out: *mut u32) -> i32 {
        // SAFETY: this test retains Capped and supplies a writable output.
        let ctx = unsafe { &*ctx.cast::<Capped>() };
        let mut live = ctx.live.borrow_mut();
        if live.len() == 4 {
            return abi::ERR_FULL;
        }
        let handle = ctx.next.get();
        ctx.next.set(handle + 1);
        live.push(handle);
        // SAFETY: font acquisition supplies disjoint scalar output storage.
        unsafe { out.write(handle) };
        abi::OK
    }
    unsafe extern "C" fn capped_release(ctx: *mut c_void, handle: u32) -> i32 {
        // SAFETY: every font retains this test's Capped context until drop.
        let ctx = unsafe { &*ctx.cast::<Capped>() };
        let mut live = ctx.live.borrow_mut();
        let Some(index) = live.iter().position(|item| *item == handle) else {
            return abi::ERR_STATE;
        };
        live.swap_remove(index);
        abi::OK
    }
    let mut table = abi::host_table();
    table.font_acquire = capped_acquire;
    table.handle_release = capped_release;
    let mut ctx = Capped::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut fonts = Vec::new();
    for _ in 0..4 {
        fonts.push(Font::acquire_role(&host, FontRole::Ui).unwrap());
    }
    assert_eq!(ctx.live.borrow().len(), 4);
    assert_eq!(
        Font::acquire_role(&host, FontRole::Reading).unwrap_err(),
        Error::Full
    );
    fonts.remove(1).release().unwrap();
    assert_eq!(ctx.live.borrow().len(), 3);
    let replacement = Font::acquire_role(&host, FontRole::Reading).unwrap();
    assert_eq!(ctx.live.borrow().len(), 4);
    drop(fonts);
    drop(replacement);
    assert!(ctx.live.borrow().is_empty());
}
