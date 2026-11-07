//! Declared tokens use the same owner admission as indexed registrations.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]

extern crate std;
use super::*;
use crate::abi7::core_frame::CoreFrame;
use paperpaw_native::host_glue::{ERR_INVALID, OK};
use paperpaw_platform::{Instant, PixelFormat};

std::thread_local! {
    static DECLARES: Cell<bool> = const { Cell::new(false) };
    static DESTROYS: Cell<u32> = const { Cell::new(0) };
    static RESULT: Cell<i32> = const { Cell::new(0) };
    static CALLS: Cell<u32> = const { Cell::new(0) };
    static SHORT: Cell<bool> = const { Cell::new(false) };
}

const TOKEN: u32 = 73;
struct Heap;
impl Services for Heap {
    fn alloc(&self, size: usize, align: usize) -> *mut u8 {
        core::alloc::Layout::from_size_align(size, align)
            // SAFETY: the layout is valid and nonzero, as the caller requires.
            .map_or(ptr::null_mut(), |layout| unsafe {
                std::alloc::alloc(layout)
            })
    }
    unsafe fn dealloc(&self, pointer: *mut u8, size: usize, align: usize) {
        // SAFETY: the service contract supplies this allocation's layout.
        unsafe {
            std::alloc::dealloc(
                pointer,
                core::alloc::Layout::from_size_align(size, align).unwrap(),
            );
        }
    }
    unsafe fn realloc(&self, pointer: *mut u8, old: usize, align: usize, new: usize) -> *mut u8 {
        // SAFETY: the service contract supplies this allocation's layout.
        unsafe {
            std::alloc::realloc(
                pointer,
                core::alloc::Layout::from_size_align(old, align).unwrap(),
                new,
            )
        }
    }
    fn micros(&self) -> u64 {
        0
    }
}

struct State {
    table: PpPlugin,
    context: *mut c_void,
}

unsafe extern "C" fn make(
    host: *const PpHost,
    context: *mut c_void,
    out: *mut *const PpPlugin,
) -> i32 {
    let size = if SHORT.with(Cell::get) {
        core::mem::offset_of!(PpPlugin, input)
    } else {
        size_of::<PpPlugin>()
    };
    if DECLARES.with(Cell::get) {
        let record = registrations::Record::ScreenSpec(registrations::ScreenSpec {
            name: "screen",
            title: "tokens.title",
            token: TOKEN,
            flags: 0,
        });
        let mut bytes = [0; 192];
        let length = registrations::records_encode(&[record], &mut bytes).unwrap();
        // SAFETY: the host supplies its stable table and context, and the
        // initialized encoded batch lives through this synchronous submit.
        let status = unsafe { ((*host).registrations_submit)(context, borrowed(&bytes[..length])) };
        if status != OK {
            return status;
        }
    }
    let state = Box::into_raw(Box::new(State {
        table: PpPlugin {
            size: u32::try_from(size).unwrap(),
            state: ptr::null_mut(),
            drop: Some(destroy),
            dispatch_event: Some(event),
            draw: Some(draw),
            input: Some(input),
        },
        context,
    }));
    // SAFETY: this fresh allocation and output are live and exclusive.
    unsafe {
        (*state).table.state = state.cast();
        out.write(&raw const (*state).table);
    }
    OK
}

unsafe extern "C" fn destroy(state: *mut c_void) {
    DESTROYS.with(|count| count.set(count.get() + 1));
    // SAFETY: this is exactly the State allocation published by make.
    unsafe { drop(Box::from_raw(state.cast::<State>())) };
}

fn invoked(state: *mut c_void, token: u32) -> bool {
    if token != TOKEN {
        return false;
    }
    // SAFETY: the admitted owner retains both the published state and context.
    let context = unsafe { &*(*state.cast::<State>()).context.cast::<Context<Heap>>() };
    if context.environment.with_frame(|_| ()).is_err() {
        return false;
    }
    CALLS.with(|count| count.set(count.get() + 1));
    true
}

unsafe extern "C" fn event(state: *mut c_void, token: u32, _: PpBytes, out: PpBuffer) -> i32 {
    if !invoked(state, token) || out.cap != 0 || out.ptr.is_null() {
        return ERR_INVALID;
    }
    RESULT.with(Cell::get)
}

unsafe extern "C" fn input(state: *mut c_void, token: u32, _: PpBytes) -> i32 {
    if !invoked(state, token) {
        return ERR_INVALID;
    }
    RESULT.with(Cell::get)
}

unsafe extern "C" fn draw(state: *mut c_void, token: u32, bytes: PpBytes) -> i32 {
    if !invoked(state, token) {
        return ERR_INVALID;
    }
    // SAFETY: the host lends these encoded bytes for this callback only.
    let bytes = unsafe { core::slice::from_raw_parts(bytes.ptr, bytes.len) };
    match registrations::records_decode_single(bytes) {
        Ok(registrations::Record::DrawFrame(frame))
            if frame.height == 80 && frame.clip_top == 5 && frame.clip_bottom == 9 =>
        {
            RESULT.with(Cell::get)
        }
        _ => ERR_INVALID,
    }
}

fn hosted(short: bool) -> Hosted<Heap, ()> {
    DECLARES.with(|value| value.set(false));
    SHORT.with(|value| value.set(short));
    CALLS.with(|count| count.set(0));
    RESULT.with(|result| result.set(0));
    // SAFETY: this fixture's linked make and its code live for the whole test.
    unsafe {
        Hosted::new(
            Heap,
            (),
            make,
            "tokens",
            (0, 0),
            |_, _, _, _| {},
            super::super::HostResources::try_new().unwrap(),
            DeclaredServices::none(),
        )
    }
    .unwrap()
}

fn scoped(kernel: &crate::Kernel, call: impl FnOnce(&CoreFrame<'_>)) {
    let env = kernel.shared.env(Instant(12));
    call(&CoreFrame {
        env: &env,
        meter: &kernel.shared.meter,
        font_removals: &kernel.shared.font_removals,
        usage: &kernel.shared.usage,
        plugins: &kernel.shared.plugins,
        work: &kernel.shared.work_states,
    });
}

#[test]
fn declared_input_accepts_only_zero_one_or_negative_and_clears_frame() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let hosted = hosted(false);
    let batch = hosted
        .construct_registered(hosted.begin().unwrap())
        .unwrap();
    assert!(
        batch.is_empty(),
        "declared tokens do not need fake registrations"
    );
    let depth = DispatchDepth::new();
    scoped(&kernel, |frame| {
        for (status, expected) in [
            (0, 0),
            (1, 1),
            (-3, -3),
            (2, ERR_INVALID),
            (i32::MAX, ERR_INVALID),
        ] {
            RESULT.with(|value| value.set(status));
            assert_eq!(
                dispatch_token(&batch, TOKEN, &depth, frame, TokenInvocation::Input(&[])),
                Ok(expected)
            );
        }
        RESULT.with(|value| value.set(1));
        assert_eq!(
            dispatch_token(&batch, TOKEN, &depth, frame, TokenInvocation::Event(&[])),
            Ok(ERR_INVALID)
        );
    });
    assert_eq!(CALLS.with(Cell::get), 6);
    assert_eq!(
        hosted.owner().context().environment.with_frame(|_| ()),
        Err(host_glue::ERR_STATE)
    );
}

#[test]
fn short_output_tail_has_no_input_callback() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let hosted = hosted(true);
    let batch = hosted
        .construct_registered(hosted.begin().unwrap())
        .unwrap();
    scoped(&kernel, |frame| {
        assert_eq!(
            dispatch_token(
                &batch,
                TOKEN,
                &DispatchDepth::new(),
                frame,
                TokenInvocation::Input(&[])
            ),
            Ok(0)
        )
    });
    assert_eq!(CALLS.with(Cell::get), 0);
}

#[test]
fn declared_tokens_refuse_reentry_and_revocation_before_plugin_code() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let hosted = hosted(false);
    let batch = hosted
        .construct_registered(hosted.begin().unwrap())
        .unwrap();
    let depth = DispatchDepth::new();
    scoped(&kernel, |frame| {
        let result = batch.dispatch_owner(&depth, |_| {
            dispatch_token(&batch, TOKEN, &depth, frame, TokenInvocation::Input(&[]))
        });
        assert_eq!(result, Ok(Err(Error::Lifecycle(super::super::Error::Busy))));
        hosted.owner().revoke();
        assert_eq!(
            dispatch_token(&batch, TOKEN, &depth, frame, TokenInvocation::Input(&[])),
            Err(Error::Lifecycle(super::super::Error::State))
        );
    });
    assert_eq!(CALLS.with(Cell::get), 0);
}

#[test]
fn declared_draw_keeps_full_pane_separate_from_clip_and_rejects_positive_status() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let hosted = hosted(false);
    let batch = hosted
        .construct_registered(hosted.begin().unwrap())
        .unwrap();
    let mut framebuffer = paperpaw_graphics::Framebuffer::new(20, 80, PixelFormat::Gray8).unwrap();
    let mut full_canvas = framebuffer.canvas();
    let mut canvas = full_canvas.clipped(Rect::new(0, 5, 20, 4));
    let draw_frame = super::super::frames::screen(0, 0, 20, 80, (5, 9));
    scoped(&kernel, |frame| {
        assert_eq!(
            dispatch_token_draw(
                &batch,
                TOKEN,
                &DispatchDepth::new(),
                frame,
                &draw_frame,
                &mut canvas
            ),
            Ok(0)
        );
        RESULT.with(|value| value.set(1));
        assert_eq!(
            dispatch_token_draw(
                &batch,
                TOKEN,
                &DispatchDepth::new(),
                frame,
                &draw_frame,
                &mut canvas
            ),
            Ok(ERR_INVALID)
        );
    });
    assert_eq!(CALLS.with(Cell::get), 2);
}

#[test]
fn retained_instance_keeps_callbacks_live_and_destroys_state_once() {
    use super::super::retained::Retained;
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    DESTROYS.with(|count| count.set(0));
    let instance = Retained::make(hosted(false)).unwrap();
    let depth = DispatchDepth::new();
    let mut records = 0;
    instance
        .registrations(&depth, &mut |_, _| records += 1)
        .unwrap();
    assert_eq!(records, 0);
    scoped(&kernel, |frame| {
        assert_eq!(
            dispatch_token(
                instance.batch().unwrap(),
                TOKEN,
                &depth,
                frame,
                TokenInvocation::Input(&[])
            ),
            Ok(0)
        );
    });
    assert_eq!(CALLS.with(Cell::get), 1);
    assert_eq!(DESTROYS.with(Cell::get), 0);
    assert_eq!(
        instance
            .host()
            .owner()
            .context()
            .environment
            .with_frame(|_| ()),
        Err(host_glue::ERR_STATE)
    );
    drop(instance);
    assert_eq!(DESTROYS.with(Cell::get), 1);
}

#[test]
fn native_mapping_refusal_destroys_output_before_it_can_dispatch() {
    use super::super::retained::Retained;
    for status in [host_glue::ERR_UNSUPPORTED, host_glue::ERR_NO_MEMORY] {
        let host = hosted(false);
        DECLARES.with(|value| value.set(true));
        DESTROYS.with(|count| count.set(0));
        let result = Retained::make_native(host, |view| -> Result<(), i32> {
            let mut count = 0;
            view.visit(&mut |record| {
                assert!(matches!(record, registrations::Record::ScreenSpec(_)));
                count += 1;
            })?;
            assert_eq!(count, 1);
            Err(status)
        });
        assert_eq!(result.err(), Some(Error::Status(status)));
        assert_eq!(CALLS.with(Cell::get), 0);
        assert_eq!(DESTROYS.with(Cell::get), 1);
    }
}

#[test]
fn native_mapping_finishes_before_callbacks_activate() {
    use super::super::retained::Retained;
    let host = hosted(false);
    DECLARES.with(|value| value.set(true));
    DESTROYS.with(|count| count.set(0));
    let (instance, count) = Retained::make_native(host, |view| {
        let mut count = 0;
        view.visit(&mut |record| {
            assert!(matches!(record, registrations::Record::ScreenSpec(_)));
            count += 1;
        })?;
        assert_eq!(CALLS.with(Cell::get), 0);
        assert_eq!(DESTROYS.with(Cell::get), 0);
        Ok(count)
    })
    .unwrap();
    assert_eq!(count, 1);
    let batch = instance.batch().unwrap();
    assert!(batch.raw.batches.is_empty());
    assert_eq!(batch.raw.batches.capacity(), 0);
    assert_eq!(batch.raw.index.capacity(), 0);
    assert!(batch.declarations.is_empty());
    drop(instance);
    assert_eq!(DESTROYS.with(Cell::get), 1);
}

#[test]
fn shared_native_dispatch_scopes_all_screen_callbacks_and_retains_owner() {
    use super::super::{
        native_dispatch::NativeDispatch, native_screen::Dispatch, retained::Retained,
        shared::Shared,
    };
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    DESTROYS.with(|count| count.set(0));
    let instance = Retained::make(hosted(false)).unwrap();
    let dispatch = Shared::try_new(NativeDispatch::new(
        instance,
        super::super::flat::FlatDepth::try_new().unwrap(),
        super::super::FlatHost {
            meter: alloc::rc::Rc::clone(&kernel.shared.meter),
            font_removals: alloc::rc::Rc::clone(&kernel.shared.font_removals),
        },
        |_, _, _| {},
    ))
    .unwrap();
    let retained = dispatch.try_clone().unwrap();
    let env = kernel.shared.env(Instant(12));
    assert_eq!(dispatch.event(TOKEN, &[], &env), Ok(0));
    assert_eq!(dispatch.input(TOKEN, &[], &env), Ok(0));
    RESULT.with(|result| result.set(1));
    assert_eq!(dispatch.input(TOKEN, &[], &env), Ok(1));
    assert_eq!(dispatch.event(TOKEN, &[], &env), Ok(ERR_INVALID));
    RESULT.with(|result| result.set(0));
    let mut framebuffer = paperpaw_graphics::Framebuffer::new(20, 80, PixelFormat::Gray8).unwrap();
    let mut canvas = framebuffer.canvas();
    let mut clip = canvas.clipped(Rect::new(0, 5, 20, 4));
    let frame = super::super::frames::screen(0, 0, 20, 80, (5, 9));
    assert_eq!(dispatch.draw(TOKEN, &frame, &env, &mut clip), Ok(0));
    assert_eq!(CALLS.with(Cell::get), 5);
    drop(dispatch);
    assert_eq!(DESTROYS.with(Cell::get), 0);
    assert_eq!(retained.input(TOKEN, &[], &env), Ok(0));
    drop(retained);
    assert_eq!(DESTROYS.with(Cell::get), 1);
}
