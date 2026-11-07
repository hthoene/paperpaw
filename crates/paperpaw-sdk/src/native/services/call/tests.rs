#![allow(clippy::unwrap_used, reason = "fixture failures must fail tests")]
use super::*;
use crate::native::wire::services_wire::{ServiceOp, core_test_echo_v1 as echo};
use core::{cell::Cell, ffi::c_void, ptr};
use paperpaw_native::host_glue::{self as abi, PpHost};

#[derive(Default)]
struct State {
    opens: Cell<u32>,
    calls: Cell<u32>,
    releases: Cell<u32>,
    last_handle: Cell<u32>,
    result: Cell<i32>,
}

unsafe extern "C" fn open(ctx: *mut c_void, _: PpBytes, _: u16, _: u16, out: *mut u32) -> i32 {
    // SAFETY: fixtures provide State and exclusive aligned output storage.
    let state = unsafe { &*ctx.cast::<State>() };
    state.opens.set(state.opens.get() + 1);
    if state.result.get() < 0 {
        return state.result.get();
    }
    // SAFETY: fixtures provide aligned output storage for this call.
    unsafe { out.write(42) };
    abi::OK
}
unsafe extern "C" fn call(ctx: *mut c_void, handle: u32, _: u32, _: PpBytes, _: PpBuffer) -> i32 {
    // SAFETY: fixtures keep State alive through the callback.
    let state = unsafe { &*ctx.cast::<State>() };
    state.calls.set(state.calls.get() + 1);
    state.last_handle.set(handle);
    state.result.get()
}
unsafe extern "C" fn release(ctx: *mut c_void, handle: u32) -> i32 {
    // SAFETY: fixtures keep State alive through the callback.
    let state = unsafe { &*ctx.cast::<State>() };
    state.releases.set(state.releases.get() + 1);
    state.last_handle.set(handle);
    state.result.get()
}
fn table() -> PpHost {
    let mut table = abi::host_table();
    table.service_open = open;
    table.service_call = call;
    table.handle_release = release;
    table
}
fn host<'a>(table: &'a PpHost, state: &mut State) -> Host<'a> {
    Host {
        table,
        ctx: ptr::from_mut(state).cast(),
    }
}

#[test]
fn unknown_schema_major_and_minor_never_reach_host() {
    let table = table();
    let mut state = State::default();
    let host = host(&table, &mut state);
    for (key, major, minor) in [
        ("unknown.service", 1, 0),
        ("", 1, 0),
        (echo::KEY, 2, 0),
        (echo::KEY, echo::MAJOR, 1),
    ] {
        assert!(matches!(
            Service::open(&host, key, major, minor),
            Err(Error::Unsupported)
        ));
    }
    assert_eq!(state.opens.get(), 0);
}

#[test]
fn invalid_ops_and_sizes_never_reach_host() {
    let table = table();
    let mut state = State::default();
    let host = host(&table, &mut state);
    let service = Service::open(&host, echo::KEY, echo::MAJOR, 0).unwrap();
    assert_eq!(service.max_reply(echo::OP_ECHO), Some(512));
    assert_eq!(service.max_reply(999), None);
    assert_eq!(
        service.call(&host, 999, &[], &mut []),
        Err(Error::Unsupported)
    );
    assert_eq!(
        service.call(&host, echo::OP_ECHO, &[0; 513], &mut [0; 512]),
        Err(Error::NoSpace)
    );
    assert_eq!(
        service.call(&host, echo::OP_ECHO, &[], &mut [0; 511]),
        Err(Error::NoSpace)
    );
    assert_eq!(state.opens.get(), 1);
    assert_eq!(state.calls.get(), 0);
}

#[test]
fn future_op_is_not_available_at_opened_minor() {
    static SCHEMA: ServiceSchema = ServiceSchema {
        key: "fixture.versioned",
        major: 1,
        revision: 1,
        test_only: true,
        ops: &[ServiceOp {
            id: 1,
            name: "future",
            since: 1,
            mutates: false,
            feature: None,
            input: &[],
            reply: &[],
            max_input: 0,
            max_reply: 0,
            bound: crate::native::wire::services_wire::ServiceBound::None,
        }],
    };
    let table = table();
    let mut state = State::default();
    let host = host(&table, &mut state);
    let service = Service {
        handle: 42,
        schema: &SCHEMA,
        minor: 0,
    };
    assert_eq!(service.max_reply(1), None);
    assert_eq!(
        service.call(&host, 1, &[], &mut []),
        Err(Error::Unsupported)
    );
    assert_eq!(state.calls.get(), 0);
}

#[test]
fn handle_survives_callback_binding_and_releases_through_current_host() {
    let table = table();
    let mut first = State::default();
    let service = {
        let host = host(&table, &mut first);
        Service::open(&host, echo::KEY, echo::MAJOR, 0).unwrap()
    };
    let mut second = State::default();
    second.result.set(3);
    let current = host(&table, &mut second);
    assert_eq!(
        service.call(&current, echo::OP_ECHO, &[], &mut [0; 512]),
        Ok(3)
    );
    second.result.set(abi::OK);
    assert_eq!(service.release(&current), Ok(()));
    assert_eq!(
        (first.opens.get(), first.calls.get(), first.releases.get()),
        (1, 0, 0)
    );
    assert_eq!(
        (
            second.calls.get(),
            second.releases.get(),
            second.last_handle.get()
        ),
        (1, 1, 42)
    );
    assert!(!core::mem::needs_drop::<Service>());
}

#[test]
fn host_errors_and_impossible_returned_count_are_preserved() {
    let table = table();
    let mut state = State::default();
    state.result.set(abi::ERR_DENIED);
    let host = host(&table, &mut state);
    assert!(matches!(
        Service::open(&host, echo::KEY, echo::MAJOR, 0),
        Err(Error::Denied)
    ));
    state.result.set(abi::OK);
    let service = Service::open(&host, echo::KEY, echo::MAJOR, 0).unwrap();
    state.result.set(abi::ERR_BUSY);
    assert_eq!(
        service.call(&host, echo::OP_ECHO, &[], &mut [0; 512]),
        Err(Error::Busy)
    );
    state.result.set(-99);
    assert_eq!(
        service.call(&host, echo::OP_ECHO, &[], &mut [0; 512]),
        Err(Error::Other(-99))
    );
    state.result.set(513);
    assert_eq!(
        service.call(&host, echo::OP_ECHO, &[], &mut [0; 512]),
        Err(Error::Invalid)
    );
    state.result.set(abi::ERR_STATE);
    assert_eq!(service.release(&host), Err(Error::State));
    assert_eq!(state.releases.get(), 1);
}
