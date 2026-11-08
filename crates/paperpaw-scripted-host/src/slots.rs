//! The host table's slots as the scripted host answers them: each reads the
//! shared [`State`] through `ctx`, counts the call, applies a refusal rule and
//! otherwise does what the real shell does (statuses and lengths included).
use crate::{Slot, State};
use core::ffi::c_void;
use paperpaw_native::host_glue::{
    ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED, OK,
    PpBuffer, PpBytes, PpHost, host_table,
};
use std::alloc::Layout;

/// The state behind `ctx`, or `None` for a null context.
fn state<'a>(ctx: *mut c_void) -> Option<&'a State> {
    // SAFETY: `ctx` is the address of the boxed `State` the host handed to
    // `make`; it lives until the `ScriptedHost` drops, after the plugin.
    unsafe { ctx.cast::<State>().as_ref() }
}

/// Readable bytes of an input pair (empty for a null pointer).
fn bytes<'a>(input: PpBytes) -> Option<&'a [u8]> {
    if input.ptr.is_null() {
        return (input.len == 0).then_some(&[]);
    }
    // SAFETY: the slot contract supplies `len` readable bytes for the call.
    Some(unsafe { core::slice::from_raw_parts(input.ptr, input.len) })
}

/// Copies `reply` into `out`: the complete length is the result, the fitting
/// prefix is written (the shell's `settings.read` and `env.snapshot` shape).
fn copy_prefix(reply: &[u8], out: PpBuffer) -> i32 {
    let n = reply.len().min(out.cap);
    if n > 0 {
        // SAFETY: the slot contract supplies `cap` writable bytes, disjoint
        // from the input.
        unsafe { core::ptr::copy_nonoverlapping(reply.as_ptr(), out.ptr, n) };
    }
    i32::try_from(reply.len()).unwrap_or(ERR_NO_SPACE)
}

unsafe extern "C" fn heap_alloc(
    ctx: *mut c_void,
    size: usize,
    align: usize,
    out: *mut *mut c_void,
) -> i32 {
    let Some(state) = state(ctx) else {
        return ERR_INVALID;
    };
    if let Some(status) = state.refusals.call(Slot::Heap) {
        return status;
    }
    let Ok(layout) = Layout::from_size_align(size, align) else {
        return ERR_INVALID;
    };
    if size == 0 || out.is_null() {
        return ERR_INVALID;
    }
    // SAFETY: nonzero, valid layout.
    let block = unsafe { std::alloc::alloc(layout) };
    if block.is_null() {
        return ERR_NO_MEMORY;
    }
    state.live.set(state.live.get() + 1);
    // SAFETY: `out` is the plugin's writable pointer slot.
    unsafe { *out = block.cast() };
    OK
}

unsafe extern "C" fn heap_dealloc(
    ctx: *mut c_void,
    block: *mut c_void,
    size: usize,
    align: usize,
) -> i32 {
    let Some(state) = state(ctx) else {
        return ERR_INVALID;
    };
    let Ok(layout) = Layout::from_size_align(size, align) else {
        return ERR_INVALID;
    };
    if block.is_null() {
        return ERR_INVALID;
    }
    // SAFETY: the block came from `heap_alloc`/`heap_realloc` with this layout.
    unsafe { std::alloc::dealloc(block.cast(), layout) };
    state.live.set(state.live.get().saturating_sub(1));
    OK
}

unsafe extern "C" fn heap_realloc(
    ctx: *mut c_void,
    block: *mut c_void,
    old: usize,
    align: usize,
    new: usize,
    out: *mut *mut c_void,
) -> i32 {
    let Some(state) = state(ctx) else {
        return ERR_INVALID;
    };
    if let Some(status) = state.refusals.call(Slot::Heap) {
        return status;
    }
    let Ok(layout) = Layout::from_size_align(old, align) else {
        return ERR_INVALID;
    };
    if block.is_null() || new == 0 || out.is_null() {
        return ERR_INVALID;
    }
    // SAFETY: the block came from this heap with `layout`; `new` is nonzero.
    let moved = unsafe { std::alloc::realloc(block.cast(), layout, new) };
    if moved.is_null() {
        return ERR_NO_MEMORY;
    }
    // SAFETY: `out` is the plugin's writable pointer slot.
    unsafe { *out = moved.cast() };
    OK
}

unsafe extern "C" fn clock_micros(ctx: *mut c_void, out: *mut u64) -> i32 {
    let Some(state) = state(ctx) else {
        return ERR_INVALID;
    };
    // SAFETY: `out` is the plugin's writable scalar.
    unsafe { *out = state.clock.get() };
    OK
}

unsafe extern "C" fn env_snapshot(ctx: *mut c_void, out: PpBuffer) -> i32 {
    let Some(state) = state(ctx) else {
        return ERR_INVALID;
    };
    if let Some(status) = state.refusals.call(Slot::EnvSnapshot) {
        return status;
    }
    let snapshot = state.env.borrow();
    if snapshot.len() > out.cap {
        return ERR_NO_SPACE;
    }
    copy_prefix(&snapshot, out);
    OK
}

unsafe extern "C" fn settings_read(ctx: *mut c_void, key: PpBytes, out: PpBuffer) -> i32 {
    let Some(state) = state(ctx) else {
        return ERR_INVALID;
    };
    if let Some(status) = state.refusals.call(Slot::SettingsRead) {
        return status;
    }
    let Some(key) = bytes(key).and_then(|key| core::str::from_utf8(key).ok()) else {
        return ERR_INVALID;
    };
    match state.values.borrow().get(key) {
        Some(value) => copy_prefix(value, out),
        None => ERR_NOT_FOUND,
    }
}

unsafe extern "C" fn commands_submit(ctx: *mut c_void, batch: PpBytes) -> i32 {
    let Some(state) = state(ctx) else {
        return ERR_INVALID;
    };
    if let Some(status) = state.refusals.call(Slot::Commands) {
        return status;
    }
    // SAFETY: the stage scope is pinned inside the boxed state; the slot
    // contract supplies readable input for this call. Outside a callback (or
    // in a draw) no stage is lent and the scope answers `ERR_STATE`.
    unsafe { state.scope.submit(batch.ptr, batch.len) }
}

unsafe extern "C" fn registrations_submit(ctx: *mut c_void, batch: PpBytes) -> i32 {
    let Some(state) = state(ctx) else {
        return ERR_INVALID;
    };
    if let Some(status) = state.refusals.call(Slot::Registrations) {
        return status;
    }
    let Some(batch) = bytes(batch) else {
        return ERR_INVALID;
    };
    state.registrations.borrow_mut().push(batch.to_vec());
    OK
}

unsafe extern "C" fn service_open(
    ctx: *mut c_void,
    key: PpBytes,
    major: u16,
    minor: u16,
    out: *mut u32,
) -> i32 {
    let Some(state) = state(ctx) else {
        return ERR_INVALID;
    };
    if let Some(status) = state.refusals.call(Slot::ServiceOpen) {
        return status;
    }
    let Some(key) = bytes(key).and_then(|key| core::str::from_utf8(key).ok()) else {
        return ERR_INVALID;
    };
    let services = state.services.borrow();
    let Some(index) = services.iter().position(|service| service.key == key) else {
        return ERR_UNSUPPORTED;
    };
    let service = &services[index];
    if service.major != major || minor > service.revision {
        return ERR_UNSUPPORTED;
    }
    let handle = u32::try_from(index + 1).unwrap_or(u32::MAX);
    state.open.borrow_mut().push(handle);
    // SAFETY: `out` is the plugin's writable scalar.
    unsafe { *out = handle };
    OK
}

unsafe extern "C" fn service_call(
    ctx: *mut c_void,
    handle: u32,
    op: u32,
    input: PpBytes,
    out: PpBuffer,
) -> i32 {
    let Some(state) = state(ctx) else {
        return ERR_INVALID;
    };
    if let Some(status) = state.refusals.call(Slot::ServiceCall) {
        return status;
    }
    if !state.open.borrow().contains(&handle) {
        return ERR_STATE;
    }
    let Some(input) = bytes(input) else {
        return ERR_INVALID;
    };
    let mut services = state.services.borrow_mut();
    let Some(service) = handle
        .checked_sub(1)
        .and_then(|index| services.get_mut(index as usize))
    else {
        return ERR_STATE;
    };
    let reply = match (service.handler)(op, input) {
        Ok(reply) => reply,
        Err(status) => return status,
    };
    // A reply that does not fit the caller's buffer is `ERR_NO_SPACE` and
    // writes nothing (the shell checks the op's reply bound before it calls).
    if reply.len() > out.cap {
        return ERR_NO_SPACE;
    }
    copy_prefix(&reply, out)
}

unsafe extern "C" fn handle_release(ctx: *mut c_void, handle: u32) -> i32 {
    let Some(state) = state(ctx) else {
        return ERR_INVALID;
    };
    let mut open = state.open.borrow_mut();
    match open.iter().position(|open| *open == handle) {
        Some(at) => {
            open.remove(at);
            OK
        }
        None => ERR_INVALID,
    }
}

/// The host table the plugin is made with: the real stub table (every slot
/// not named here answers `ERR_UNSUPPORTED`) with the scripted slots in it.
pub(crate) fn table() -> PpHost {
    let mut table = host_table();
    table.heap_alloc = heap_alloc;
    table.heap_dealloc = heap_dealloc;
    table.heap_realloc = heap_realloc;
    table.clock_micros = clock_micros;
    table.env_snapshot = env_snapshot;
    table.settings_read = settings_read;
    table.commands_submit = commands_submit;
    table.registrations_submit = registrations_submit;
    table.service_open = service_open;
    table.service_call = service_call;
    table.handle_release = handle_release;
    table
}
