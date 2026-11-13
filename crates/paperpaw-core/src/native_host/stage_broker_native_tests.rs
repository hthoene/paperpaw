//! The broker crosses the real retained native callback boundary.
// TODO(#1026, Plugin follow-up 38): add real Reader/Aids end-to-end coverage
// after the reader.filter schema uses the shared external generator. This
// widget fixture proves retained routing only; production activation is gated.
extern crate std;
use super::{Heap, NORMAL, reset};
use crate::native_host::{
    self, FlatHost,
    flat::FlatDepth,
    native_dispatch::NativeDispatch,
    native_instances::{NativeInstances, NativeLease},
    retained::Retained,
    shell,
    stage_broker::{Outcome, Run},
    stages_wire::core_widget_v1 as widget,
};
use alloc::{boxed::Box, rc::Rc, vec, vec::Vec};
use core::{cell::Cell, ffi::c_void, ptr};
use paperpaw_native::host_glue::{self as abi, PpBuffer, PpBytes, PpHost, PpPlugin};
use paperpaw_sdk::Id;

std::thread_local! {
    static CALLS: Cell<u32> = const { Cell::new(0) };
    static DESTROYS: Cell<u32> = const { Cell::new(0) };
    static REFUSE: Cell<bool> = const { Cell::new(false) };
}

struct State {
    table: PpPlugin,
    context: *mut c_void,
}

unsafe extern "C" fn make(
    _: *const PpHost,
    context: *mut c_void,
    out: *mut *const PpPlugin,
) -> i32 {
    let state = Box::into_raw(Box::new(State {
        table: PpPlugin {
            size: u32::try_from(size_of::<PpPlugin>()).unwrap(),
            state: ptr::null_mut(),
            drop: Some(destroy),
            dispatch_event: Some(event),
            draw: None,
            input: None,
        },
        context,
    }));
    // SAFETY: the host supplies exclusive output; this allocation lives until destroy.
    unsafe {
        (*state).table.state = state.cast();
        out.write(&raw const (*state).table);
    }
    abi::OK
}

unsafe extern "C" fn destroy(state: *mut c_void) {
    DESTROYS.with(|count| count.set(count.get() + 1));
    // SAFETY: the owner destroys this make allocation exactly once.
    unsafe { drop(Box::from_raw(state.cast::<State>())) };
}

unsafe extern "C" fn event(state: *mut c_void, token: u32, input: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: retained admission keeps the state and pinned host context alive.
    let context = unsafe {
        &*(*state.cast::<State>())
            .context
            .cast::<shell::Context<Heap>>()
    };
    assert!(context.environment.with_frame(|_| ()).is_ok());
    CALLS.with(|count| count.set(count.get() + 1));
    if REFUSE.with(Cell::get) {
        return abi::ERR_STATE;
    }
    // SAFETY: the callback receives live readable input and exclusive bounded output.
    let (input, output) = unsafe {
        (
            core::slice::from_raw_parts(input.ptr, input.len),
            core::slice::from_raw_parts_mut(out.ptr, out.cap),
        )
    };
    let Ok(native_host::registrations::Record::StageCall(call)) =
        native_host::registrations::records_decode_single(input)
    else {
        return abi::ERR_INVALID;
    };
    assert_eq!((call.stage, call.schema), ("core.widget", 1));
    assert!(widget::input_decode(call.payload).is_ok());
    widget::WidgetSize::new(token, 0)
        .encode(output)
        .map_or_else(|error| error, |len| i32::try_from(len).unwrap())
}

fn instance(kernel: &crate::Kernel, depth: &FlatDepth) -> NativeLease {
    // SAFETY: the fixture's linked entry and code live throughout the test.
    let host = unsafe {
        shell::Hosted::new(
            Heap,
            (),
            make,
            "native-widget",
            (ptr::null(), 0),
            |_, _, _, _| {},
            native_host::HostResources::try_new().unwrap(),
            shell::DeclaredServices::none(),
        )
    }
    .unwrap();
    let retained = Retained::make(host).unwrap();
    NativeLease::try_new(NativeDispatch::new(
        retained,
        depth.try_clone().unwrap(),
        FlatHost {
            log: Default::default(),
            meter: Rc::clone(&kernel.shared.meter),
            directory: None,
            stack_rows: Rc::clone(&kernel.shared.stack_rows),
            font_removals: Rc::clone(&kernel.shared.font_removals),
            registry_generation: Rc::clone(&kernel.shared.registry_generation),
        },
        |_, _, _, _| {},
        crate::native_host::native_instances::test_identity("probe"),
    ))
    .unwrap()
}

#[test]
fn retained_native_stage_replies_refuse_and_revoke_without_stale_calls() {
    reset(NORMAL);
    CALLS.with(|value| value.set(0));
    DESTROYS.with(|value| value.set(0));
    REFUSE.with(|value| value.set(false));
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let depth = FlatDepth::try_new().unwrap();
    let mut registry = NativeInstances::default();
    let plugin = registry
        .test_insert(
            Id::new_static("native-widget"),
            Some(instance(&kernel, &depth)),
            true,
        )
        .unwrap();
    let schema = native_host::stage_broker::tests::schema();
    let input = native_host::stage_broker::tests::input();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    let mut target = vec![77];
    for refuse in [false, true] {
        REFUSE.with(|value| value.set(refuse));
        let run = Run::prepare(
            &schema,
            depth.as_depth(),
            &input,
            &registry,
            [native_host::stage_broker::tests::row(plugin, 9, 0, 0)],
        )
        .unwrap();
        let result = run
            .apply(
                &mut target,
                &env,
                |target| Ok(Vec::from(target.as_slice())),
                |target, reply| {
                    target.push(widget::WidgetSize::decode(reply.bytes)?.height);
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(
            result,
            Outcome {
                applied: usize::from(!refuse),
                refused: usize::from(refuse)
            }
        );
        assert_eq!(target, [77, 9]);
        assert_eq!(depth.as_depth().0.current(), 0);
    }
    let run = Run::prepare(
        &schema,
        depth.as_depth(),
        &input,
        &registry,
        [native_host::stage_broker::tests::row(plugin, 9, 0, 0)],
    )
    .unwrap();
    drop(run);
    registry.stop(plugin).unwrap();
    let run = Run::prepare(
        &schema,
        depth.as_depth(),
        &input,
        &registry,
        [native_host::stage_broker::tests::row(plugin, 9, 0, 0)],
    )
    .unwrap();
    assert_eq!(
        run.apply(
            &mut target,
            &env,
            |target| Ok(Vec::from(target.as_slice())),
            |_, _| { panic!("revoked subscriber must not apply") }
        )
        .unwrap(),
        Outcome {
            applied: 0,
            refused: 1
        }
    );
    assert_eq!(CALLS.with(Cell::get), 2);
    assert_eq!(
        DESTROYS.with(Cell::get),
        0,
        "the pending row retains cleanup ownership"
    );
    assert!(registry.retire(plugin).unwrap().is_none());
    assert_eq!(DESTROYS.with(Cell::get), 1);
}
