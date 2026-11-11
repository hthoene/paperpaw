//! Declared tokens use the same owner admission as indexed registrations.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]

extern crate std;
use super::*;
use crate::abi7::core_frame::CoreFrame;
use core::any::TypeId;
use paperpaw_native::host_glue::{ERR_INVALID, OK};
use paperpaw_platform::{Instant, PixelFormat};

std::thread_local! {
    static DECLARES: Cell<bool> = const { Cell::new(false) };
    static DESTROYS: Cell<u32> = const { Cell::new(0) };
    static RESULT: Cell<i32> = const { Cell::new(0) };
    static CALLS: Cell<u32> = const { Cell::new(0) };
    static REPLY: Cell<bool> = const { Cell::new(false) };
    static NO_EVENT: Cell<bool> = const { Cell::new(false) };
    static CAPACITY: Cell<usize> = const { Cell::new(0) };
    static SHORT: Cell<bool> = const { Cell::new(false) };
    static PROBE: core::cell::RefCell<Option<(alloc::rc::Rc<crate::native_host::stack_rows::StackRows>, u64)>> =
        const { core::cell::RefCell::new(None) };
    static SEEN: Cell<Option<u64>> = const { Cell::new(None) };
    static METER: core::cell::RefCell<Option<alloc::rc::Rc<crate::meter::Meter>>> =
        const { core::cell::RefCell::new(None) };
    static RUNNING: Cell<Option<bool>> = const { Cell::new(None) };
    /// How many token callbacks ran as the frame owner, and how many did not.
    static OURS: Cell<u32> = const { Cell::new(0) };
    static NOT_OURS: Cell<u32> = const { Cell::new(0) };
    static SUBSCRIBE: Cell<u32> = const { Cell::new(0) };
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
    let tag = SUBSCRIBE.with(Cell::get);
    if tag != 0 {
        let record = registrations::Registration::Event {
            event_tag: tag,
            token: TOKEN,
        };
        let mut bytes = [0; 64];
        let length = registrations::registrations_encode(&[record], &mut bytes).unwrap();
        // SAFETY: as for the screen declaration above.
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
            dispatch_event: if NO_EVENT.with(Cell::get) {
                None
            } else {
                Some(event)
            },
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
    let ours = super::super::frame_owner::owner() == Some(context.owner_key());
    let seen = if ours { &OURS } else { &NOT_OURS };
    seen.with(|count| count.set(count.get() + 1));
    METER.with(|meter| {
        if let Some(meter) = &*meter.borrow() {
            use paperpaw_sdk::secrets::Running;
            RUNNING.with(|seen| seen.set(Some(meter.runs("tokens"))));
        }
    });
    PROBE.with(|probe| {
        if let Some((rows, owner)) = &*probe.borrow() {
            SEEN.with(|seen| seen.set(rows.showing_screen(*owner)));
        }
    });
    true
}

unsafe extern "C" fn event(state: *mut c_void, token: u32, input: PpBytes, out: PpBuffer) -> i32 {
    if !invoked(state, token) || out.ptr.is_null() {
        return ERR_INVALID;
    }
    if REPLY.with(Cell::get) {
        CAPACITY.with(|value| value.set(out.cap));
        // SAFETY: the host lends the initialized request for this callback.
        let bytes = unsafe { core::slice::from_raw_parts(input.ptr, input.len) };
        if bytes != b"ask" {
            return ERR_INVALID;
        }
        // SAFETY: the host lends exclusive output storage of this capacity.
        unsafe {
            ptr::copy_nonoverlapping(b"rpc".as_ptr(), out.ptr, out.cap.min(3));
        }
    } else if out.cap != 0 {
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
    REPLY.with(|value| value.set(false));
    NO_EVENT.with(|value| value.set(false));
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
            (core::ptr::null(), 0),
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
        callback: None,
        instance: None,
        env: &env,
        meter: &kernel.shared.meter,
        depth: None,
        directory: None,
        stack_generation: kernel.shared.stack_rows.generation(),
        stack_rows: &kernel.shared.stack_rows,
        font_removals: &kernel.shared.font_removals,
        registry_generation: &kernel.shared.registry_generation,
        usage: &kernel.shared.usage,
        plugins: &kernel.shared.plugins,
        work: &kernel.shared.work_states,
        log: Default::default(),
        ui: None,
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
fn retained_by_value_early_return_destroys_once() {
    use super::super::retained::Retained;
    fn consume(instance: Retained<Heap, ()>) -> Result<(), ()> {
        if instance.batch().is_ok() {
            return Err(());
        }
        Ok(())
    }
    DESTROYS.with(|count| count.set(0));
    assert_eq!(consume(Retained::make(hosted(false)).unwrap()), Err(()));
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
    // The retained batch is the records alone: no room for declarations.
    let batch: &Published<'_, _, _, Registration> = instance.batch().unwrap();
    assert!(batch.is_empty());
    drop(instance);
    assert_eq!(DESTROYS.with(Cell::get), 1);
}

#[test]
fn registry_dispatches_idle_retained_owner_and_releases_it_after_last_frozen_lease() {
    use super::super::{
        native_dispatch::NativeDispatch,
        native_instances::{InstanceState, NativeInstances, NativeLease},
        native_screen::Dispatch,
        retained::Retained,
    };
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let lease = NativeLease::try_new(NativeDispatch::new(
        Retained::make(hosted(false)).unwrap(),
        super::super::flat::FlatDepth::try_new().unwrap(),
        super::super::FlatHost {
            log: Default::default(),
            meter: alloc::rc::Rc::clone(&kernel.shared.meter),
            directory: None,
            stack_rows: alloc::rc::Rc::clone(&kernel.shared.stack_rows),
            font_removals: alloc::rc::Rc::clone(&kernel.shared.font_removals),
            registry_generation: alloc::rc::Rc::clone(&kernel.shared.registry_generation),
        },
        |_, _, _| {},
        crate::native_host::native_instances::test_identity("tokens"),
    ))
    .unwrap();
    let mut registry = NativeInstances::default();
    let key = registry
        .test_insert(paperpaw_sdk::Id::new_static("tokens"), Some(lease), true)
        .unwrap();
    let env = kernel.shared.env(Instant(12));
    for expected in 1..=2 {
        let InstanceState::Running(lease) = registry.state(key).unwrap() else {
            panic!("idle retained owner must remain running");
        };
        assert_eq!(lease.event(TOKEN, &[], &env), Ok(0));
        assert_eq!(CALLS.with(Cell::get), expected);
    }
    let InstanceState::Running(frozen) = registry.state(key).unwrap() else {
        panic!("running between calls");
    };
    registry.stop(key).unwrap();
    assert!(matches!(registry.state(key), Ok(InstanceState::Stopped)));
    assert_eq!(
        frozen.event(TOKEN, &[], &env),
        Err(host_glue::ERR_CANCELLED)
    );
    assert_eq!(CALLS.with(Cell::get), 2);
    assert_eq!(DESTROYS.with(Cell::get), 0);
    drop(frozen);
    assert_eq!(
        DESTROYS.with(Cell::get),
        0,
        "cleanup ownership stays in the pending row"
    );
    assert!(registry.retire(key).unwrap().is_none());
    assert_eq!(DESTROYS.with(Cell::get), 1);
    assert!(matches!(registry.state(key), Err(host_glue::ERR_STATE)));
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
            log: Default::default(),
            meter: alloc::rc::Rc::clone(&kernel.shared.meter),
            directory: None,
            stack_rows: alloc::rc::Rc::clone(&kernel.shared.stack_rows),
            font_removals: alloc::rc::Rc::clone(&kernel.shared.font_removals),
            registry_generation: alloc::rc::Rc::clone(&kernel.shared.registry_generation),
        },
        |_, _, _| {},
        crate::native_host::native_instances::test_identity("tokens"),
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

#[test]
fn stage_reply_preserves_count_or_negative_status_and_scopes_core_views() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let hosted = hosted(false);
    let batch = hosted
        .construct_registered(hosted.begin().unwrap())
        .unwrap();
    REPLY.with(|value| value.set(true));
    let depth = DispatchDepth::new();
    scoped(&kernel, |frame| {
        for (result, expected) in [
            (3, 3),
            (8, 8),
            (9, ERR_INVALID),
            (i32::MAX, ERR_INVALID),
            (-42, -42),
            (0, 0),
        ] {
            RESULT.with(|value| value.set(result));
            let mut output = [0xa5; 8];
            assert_eq!(
                super::dispatch_stage(&batch, TOKEN, &depth, frame, b"ask", &mut output),
                Ok(expected)
            );
            assert_eq!(&output[..3], b"rpc");
            assert_eq!(&output[3..], &[0xa5; 5]);
            assert_eq!(CAPACITY.with(Cell::get), 8);
            assert_eq!(
                hosted.owner().context().environment.with_frame(|_| ()),
                Err(host_glue::ERR_STATE)
            );
        }
    });
    assert_eq!(CALLS.with(Cell::get), 6);
}

#[test]
fn stage_reply_obeys_owner_admission_and_missing_callback_without_running_plugin() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let hosted = hosted(false);
    let batch = hosted
        .construct_registered(hosted.begin().unwrap())
        .unwrap();
    REPLY.with(|value| value.set(true));
    let depth = DispatchDepth::new();
    scoped(&kernel, |frame| {
        let mut output = [0xa5; 8];
        let nested = batch
            .dispatch_owner(&depth, |_| {
                super::dispatch_stage(&batch, TOKEN, &depth, frame, b"ask", &mut output)
            })
            .unwrap();
        assert!(matches!(
            nested,
            Err(Error::Lifecycle(super::super::Error::Busy))
        ));
        assert_eq!(CALLS.with(Cell::get), 0);
        assert_eq!(output, [0xa5; 8]);
        hosted.stop();
        let refused = super::dispatch_stage(&batch, TOKEN, &depth, frame, b"ask", &mut output);
        assert!(matches!(
            refused,
            Err(Error::Lifecycle(super::super::Error::State))
        ));
        assert_eq!(output, [0xa5; 8]);
    });
    drop(batch);
    drop(hosted);
    let hosted = self::hosted(false);
    NO_EVENT.with(|value| value.set(true));
    let batch = hosted
        .construct_registered(hosted.begin().unwrap())
        .unwrap();
    scoped(&kernel, |frame| {
        let mut output = [0xa5; 8];
        assert_eq!(
            super::dispatch_stage(&batch, TOKEN, &depth, frame, b"ask", &mut output),
            Err(Error::Status(host_glue::ERR_UNSUPPORTED))
        );
        assert_eq!(output, [0xa5; 8]);
        assert_eq!(CALLS.with(Cell::get), 0);
    });
}

#[test]
fn a_screen_callback_is_a_showing_screen_only_while_it_runs_on_the_stack() {
    use super::super::{
        native_dispatch::NativeDispatch, native_screen::Dispatch, retained::Retained,
        stack_rows::StackRow,
    };
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let identity = crate::native_host::native_instances::test_identity("tokens");
    let owner = identity.owner();
    let rows = alloc::rc::Rc::clone(&kernel.shared.stack_rows);
    rows.update([StackRow {
        instance: 5,
        owner,
        flags: 0,
    }]);
    let dispatch = NativeDispatch::new(
        Retained::make(hosted(false)).unwrap(),
        super::super::flat::FlatDepth::try_new().unwrap(),
        super::super::FlatHost {
            log: Default::default(),
            meter: alloc::rc::Rc::clone(&kernel.shared.meter),
            directory: None,
            stack_rows: alloc::rc::Rc::clone(&rows),
            font_removals: alloc::rc::Rc::clone(&kernel.shared.font_removals),
            registry_generation: alloc::rc::Rc::clone(&kernel.shared.registry_generation),
        },
        |_, _, _| {},
        identity,
    );
    PROBE.with(|probe| *probe.borrow_mut() = Some((alloc::rc::Rc::clone(&rows), owner)));
    let env = kernel.shared.env(Instant(12));
    let key = paperpaw_sdk::ScreenKey::new(5);
    SEEN.with(|seen| seen.set(Some(0)));
    assert_eq!(
        dispatch.screen_callback(Some(key), TOKEN, &[], &env, false),
        Ok(0)
    );
    assert_eq!(
        SEEN.with(Cell::get),
        Some(5),
        "the running screen is showing"
    );
    SEEN.with(|seen| seen.set(Some(0)));
    assert_eq!(dispatch.event(TOKEN, &[], &env), Ok(0));
    assert_eq!(
        SEEN.with(Cell::get),
        None,
        "a plain event is no screen callback"
    );
    assert_eq!(rows.showing_screen(owner), None, "nothing runs afterwards");
    PROBE.with(|probe| *probe.borrow_mut() = None);
}

#[test]
fn a_native_owner_is_subscribed_to_the_events_it_registered_only() {
    use super::super::{
        native_dispatch::NativeDispatch, native_instances::NativeInstance, retained::Retained,
    };
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let make = |tag: u32| {
        let hosted = hosted(false);
        SUBSCRIBE.with(|value| value.set(tag));
        NativeDispatch::new(
            Retained::make(hosted).unwrap(),
            super::super::flat::FlatDepth::try_new().unwrap(),
            super::super::FlatHost {
                log: Default::default(),
                meter: alloc::rc::Rc::clone(&kernel.shared.meter),
                directory: None,
                stack_rows: alloc::rc::Rc::clone(&kernel.shared.stack_rows),
                font_removals: alloc::rc::Rc::clone(&kernel.shared.font_removals),
                registry_generation: alloc::rc::Rc::clone(&kernel.shared.registry_generation),
            },
            |_, _, _| {},
            crate::native_host::native_instances::test_identity("tokens"),
        )
    };
    // Only events with a host encoder can be registered today.
    let started = make(registrations::EVENT_STARTED_TAG);
    SUBSCRIBE.with(|value| value.set(0));
    assert!(started.subscribed(registrations::EVENT_STARTED_TAG));
    assert!(!started.subscribed(registrations::EVENT_PAGE_TURNED_TAG));
    assert!(!started.subscribed(registrations::EVENT_HTTP_DONE_TAG));
    assert!(!make(0).subscribed(registrations::EVENT_STARTED_TAG));
}

/// A stage call reached from inside another plugin's callback runs the plugin
/// called on the usage meter, and gives the tracker back after: the secrets and
/// Bluetooth views and the meter's charges name the plugin that runs.
#[test]
fn a_nested_stage_call_is_charged_to_the_plugin_called() {
    use paperpaw_sdk::secrets::Running;
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let hosted = hosted(false);
    let batch = hosted
        .construct_registered(hosted.begin().unwrap())
        .unwrap();
    let depth = DispatchDepth::new();
    REPLY.with(|value| value.set(true));
    RESULT.with(|value| value.set(0));
    METER.with(|meter| *meter.borrow_mut() = Some(kernel.shared.meter.clone()));
    RUNNING.with(|seen| seen.set(None));
    // The Bluetooth manager's callback is the running one.
    let outer = kernel.shared.meter.enter("ble");
    assert!(kernel.shared.meter.runs("ble"));
    scoped(&kernel, |frame| {
        let mut output = [0; 8];
        assert_eq!(
            super::dispatch_stage(&batch, TOKEN, &depth, frame, b"ask", &mut output),
            Ok(0)
        );
    });
    assert_eq!(
        RUNNING.with(Cell::get),
        Some(true),
        "the callee ran as itself"
    );
    assert!(kernel.shared.meter.runs("ble"), "and the caller runs again");
    drop(outer);
    METER.with(|meter| *meter.borrow_mut() = None);
}

/// A draw reached from inside another plugin's callback (a compose child) runs
/// the plugin called on the usage meter and gives the tracker back; a plugin
/// that runs already is not entered twice.
#[test]
fn a_nested_draw_is_charged_to_the_plugin_called() {
    use paperpaw_sdk::secrets::Running;
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
    METER.with(|meter| *meter.borrow_mut() = Some(kernel.shared.meter.clone()));
    RESULT.with(|value| value.set(0));
    // The last case has the plugin drawn lower on the stack, under another.
    for stack in [&["composer"][..], &["tokens"], &["tokens", "composer"]] {
        let outer = *stack.last().unwrap();
        RUNNING.with(|seen| seen.set(None));
        let running: std::vec::Vec<_> = stack
            .iter()
            .map(|plugin| kernel.shared.meter.enter(plugin))
            .collect();
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
        });
        // The plugin drawn runs as itself, inside the composer's call and where
        // it was running already; whoever ran before runs again after.
        assert_eq!(RUNNING.with(Cell::get), Some(true), "{stack:?}");
        assert!(kernel.shared.meter.runs(outer), "{outer} runs again after");
        drop(running);
    }
    METER.with(|meter| *meter.borrow_mut() = None);
}

/// Every kind of declared entry (input, event, draw, stage reply) runs the
/// plugin as the frame owner, and the previous owner is back after it, also
/// when the callback answers an error and when the call is nested in another
/// plugin's.
#[test]
fn every_token_entry_runs_as_the_frame_owner_and_gives_it_back() {
    use super::super::frame_owner;
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
    let depth = DispatchDepth::new();
    OURS.with(|count| count.set(0));
    NOT_OURS.with(|count| count.set(0));
    REPLY.with(|value| value.set(true));
    let run = |frame: &CoreFrame<'_>, canvas: &mut paperpaw_graphics::Canvas<'_>| {
        let _ = dispatch_token(&batch, TOKEN, &depth, frame, TokenInvocation::Input(&[]));
        let _ = dispatch_token(&batch, TOKEN, &depth, frame, TokenInvocation::Event(&[]));
        let _ = dispatch_token_draw(&batch, TOKEN, &depth, frame, &draw_frame, canvas);
        let mut output = [0; 8];
        let _ = super::dispatch_stage(&batch, TOKEN, &depth, frame, b"ask", &mut output);
    };
    scoped(&kernel, |frame| {
        // An answer and an error return: four entries each.
        for status in [0, -3] {
            RESULT.with(|value| value.set(status));
            run(frame, &mut canvas);
            assert_eq!(frame_owner::owner(), None, "status {status}");
        }
        // Nested in another plugin's frame.
        frame_owner::within(99, ptr::null(), TypeId::of::<()>(), || {
            run(frame, &mut canvas);
            assert_eq!(frame_owner::owner(), Some(99));
        });
    });
    assert_eq!(OURS.with(Cell::get), 12);
    assert_eq!(NOT_OURS.with(Cell::get), 0);
}
