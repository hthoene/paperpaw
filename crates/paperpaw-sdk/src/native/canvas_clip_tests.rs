#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::super::super::Host;
use super::*;
use alloc::vec::Vec;
use core::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr,
};
use paperpaw_native::host_glue::{self as abi, PpHost};

#[derive(Default)]
struct Context {
    calls: RefCell<Vec<FrameRect>>,
    clip: Cell<Option<FrameRect>>,
    status: Cell<i32>,
    drawing: Cell<bool>,
}

unsafe extern "C" fn canvas_clip_slot(
    ctx: *mut c_void,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> i32 {
    // SAFETY: the test retains Context for the callback's whole host lease.
    let ctx = unsafe { &*ctx.cast::<Context>() };
    let rect = FrameRect::new(x, y, width, height);
    ctx.calls.borrow_mut().push(rect);
    if !ctx.drawing.get() {
        return abi::ERR_STATE;
    }
    if ctx.status.get() == abi::OK {
        ctx.clip.set(Some(rect));
    }
    ctx.status.get()
}

fn table() -> PpHost {
    let mut table = abi::host_table();
    table.canvas_clip = canvas_clip_slot;
    table
}

fn frame() -> FrameRect {
    FrameRect::new(-10, -20, 200, 300)
}

#[test]
fn nested_guards_restore_parent_clips_in_reverse_order() {
    let table = table();
    let mut ctx = Context::default();
    ctx.drawing.set(true);
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut canvas = Canvas::from(&host, frame(), frame());
    let parent = FrameRect::new(0, 0, 90, 80);
    let child = FrameRect::new(10, 20, 80, 60);
    {
        let mut outer = canvas.clip_region(parent).unwrap();
        assert_eq!(outer._clip, parent);
        {
            let inner = outer.clip_region(FrameRect::new(10, 20, 200, 200)).unwrap();
            assert_eq!(inner._clip, child);
        }
        assert_eq!(outer._clip, parent);
    }
    assert_eq!(canvas._clip, frame());
    assert_eq!(ctx.clip.get(), Some(frame()));
    assert_eq!(
        ctx.calls.borrow().as_slice(),
        &[parent, child, parent, frame()]
    );
}

#[test]
fn explicit_restore_is_reported_once_and_empty_clips_are_supported() {
    let table = table();
    let mut ctx = Context::default();
    ctx.drawing.set(true);
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut canvas = Canvas::from(&host, frame(), frame());
    let empty = FrameRect::new(500, 500, 0, 0);
    let guard = canvas
        .clip_region(FrameRect::new(500, 500, 90, 80))
        .unwrap();
    assert_eq!(guard._clip, empty);
    guard.restore().unwrap();
    assert_eq!(canvas._clip, frame());
    assert_eq!(ctx.calls.borrow().as_slice(), &[empty, frame()]);
}

#[test]
fn host_errors_leave_local_clip_at_the_last_accepted_rectangle() {
    let table = table();
    let mut ctx = Context::default();
    ctx.drawing.set(true);
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut canvas = Canvas::from(&host, frame(), frame());
    let child = FrameRect::new(0, 0, 90, 80);
    for code in [abi::ERR_INVALID, abi::ERR_BUSY, abi::ERR_UNSUPPORTED, -900] {
        ctx.status.set(code);
        assert_eq!(
            canvas.clip_region(child).unwrap_err(),
            Error::from_status(code)
        );
        assert_eq!(canvas._clip, frame());
    }
    ctx.status.set(abi::OK);
    let guard = canvas.clip_region(child).unwrap();
    ctx.status.set(abi::ERR_IO);
    assert_eq!(guard.restore(), Err(Error::Io));
    assert_eq!(canvas._clip, child);
    assert_eq!(ctx.clip.get(), Some(child));
    // Four refused entries, one accepted entry and one failed explicit restore.
    assert_eq!(ctx.calls.borrow().len(), 6);
    ctx.status.set(abi::OK);
    let nested = canvas.clip_region(FrameRect::new(10, 20, 20, 30)).unwrap();
    ctx.status.set(abi::ERR_STATE);
    drop(nested);
    assert_eq!(canvas._clip, FrameRect::new(10, 20, 20, 30));
    assert_eq!(ctx.calls.borrow().len(), 8);
}

#[test]
fn absent_draw_context_and_default_table_refuse_clipping() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut canvas = Canvas::from(&host, frame(), frame());
    assert_eq!(canvas.clip_region(frame()).unwrap_err(), Error::State);
    assert_eq!(canvas._clip, frame());
    let default = abi::host_table();
    let host = Host {
        table: &default,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut canvas = Canvas::from(&host, frame(), frame());
    assert_eq!(canvas.clip_region(frame()).unwrap_err(), Error::Unsupported);
}

#[test]
fn checked_intersection_preserves_extreme_coordinates_and_empty_parent() {
    let parent = FrameRect::new(i32::MIN, i32::MIN, u32::MAX, u32::MAX);
    let child = FrameRect::new(i32::MAX - 1, -3, u32::MAX, 10);
    assert_eq!(
        intersect_frame_clips(parent, child),
        Ok(FrameRect::new(i32::MAX - 1, -3, 1, 10))
    );
    let empty = FrameRect::new(7, 9, 0, 0);
    assert_eq!(intersect_frame_clips(empty, parent), Ok(empty));
}

#[test]
fn unwind_restores_every_live_guard_in_reverse_order() {
    extern crate std;
    let table = table();
    let mut ctx = Context::default();
    ctx.drawing.set(true);
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut canvas = Canvas::from(&host, frame(), frame());
    let parent = FrameRect::new(0, 0, 90, 80);
    let child = FrameRect::new(10, 20, 20, 30);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut outer = canvas.clip_region(parent).unwrap();
        let _inner = outer.clip_region(child).unwrap();
        panic!("exercise clip restoration during unwinding");
    }));
    assert!(result.is_err());
    assert_eq!(canvas._clip, frame());
    assert_eq!(ctx.clip.get(), Some(frame()));
    assert_eq!(
        ctx.calls.borrow().as_slice(),
        &[parent, child, parent, frame()]
    );
}
