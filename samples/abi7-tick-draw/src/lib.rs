//! Inactive plugin-owned ABI 7 tick/draw sample, without an ergonomic facade.
#![no_std]

#[cfg(not(test))]
#[rustfmt::skip]
#[path = "../../../abi/generated/paperpaw_abi.rs"]
#[allow(
    dead_code,
    reason = "sample consumes only the published tick/draw subset"
)]
mod abi;
#[cfg(test)]
mod abi {
    pub use paperpaw_native::host_glue::*;
}
#[rustfmt::skip]
#[path = "../../../abi/generated/records_wire.rs"]
#[allow(
    dead_code,
    reason = "sample encodes registrations and decodes tick events"
)]
mod wire;

use abi::*;
use core::{ffi::c_void, mem, ptr};
use wire::{EVENT_TICK_TAG, Registration, registrations_encode, tick_decode};

include!(concat!(env!("OUT_DIR"), "/metadata.rs"));
const EVENT_TOKEN: u32 = 1;
const DRAW_TOKEN: u32 = 2;

/// Immutable export wrapping a raw-pointer-containing generated record.
#[repr(transparent)]
pub struct Entry(PpEntry);
// SAFETY: the export points only to immutable metadata and a function.
unsafe impl Sync for Entry {}

#[unsafe(no_mangle)]
pub static PAPERPAW_ENTRY: Entry = Entry(PpEntry {
    size: mem::size_of::<PpEntry>() as u32,
    abi_major: MAJOR,
    needs_minor: MINOR,
    metadata: PpBytes {
        ptr: PAPERPAW_METADATA.as_ptr(),
        len: PAPERPAW_METADATA.len(),
    },
    make: Some(make),
});

struct State {
    host: *const PpHost,
    ctx: *mut c_void,
    plugin: PpPlugin,
    last_tick_ms: Option<u64>,
    ticks: u64,
    draws: u64,
    draw_clock_us: u64,
}

/// The host guarantees valid, disjoint table/context/output storage and lifetime.
unsafe extern "C" fn make(host: *const PpHost, ctx: *mut c_void, out: *mut *const PpPlugin) -> i32 {
    if out.is_null() {
        return ERR_INVALID;
    }
    // SAFETY: output storage is caller-provided and writable for this call.
    unsafe {
        out.write(ptr::null());
    }
    if host.is_null() || ctx.is_null() {
        return ERR_INVALID;
    }
    // SAFETY: the caller exposes a live table header; check its size before fields.
    let size = unsafe { ptr::read_unaligned(host.cast::<u32>()) } as usize;
    if size < mem::size_of::<PpHost>() {
        return ERR_UNSUPPORTED;
    }
    // SAFETY: a compatible complete host table is live until plugin destruction.
    let h = unsafe { &*host };
    if h.major != MAJOR || h.minor < MINOR {
        return ERR_UNSUPPORTED;
    }
    let mut batch = [0u8; 64];
    let n = match registrations_encode(
        &[
            Registration::Event {
                event_tag: EVENT_TICK_TAG,
                token: EVENT_TOKEN,
            },
            Registration::Draw { token: DRAW_TOKEN },
        ],
        &mut batch,
    ) {
        Ok(n) => n,
        Err(e) => return e,
    };
    // SAFETY: generated bounded bytes remain live only for synchronous submission.
    let status = unsafe {
        (h.registrations_submit)(
            ctx,
            PpBytes {
                ptr: batch.as_ptr(),
                len: n,
            },
        )
    };
    if status != OK {
        return status;
    }
    let mut block = ptr::null_mut();
    // SAFETY: matching host allocator owns the instance until destroy.
    let status = unsafe {
        (h.heap_alloc)(
            ctx,
            mem::size_of::<State>(),
            mem::align_of::<State>(),
            &mut block,
        )
    };
    if status != OK {
        return status;
    }
    if block.is_null() {
        return ERR_NO_MEMORY;
    }
    let state = block.cast::<State>();
    // SAFETY: the allocator supplies sufficient aligned writable storage.
    unsafe {
        state.write(State {
            host,
            ctx,
            plugin: PpPlugin {
                size: mem::size_of::<PpPlugin>() as u32,
                state: block,
                drop: Some(destroy),
                dispatch_event: Some(event),
                draw: Some(draw),
            },
            last_tick_ms: None,
            ticks: 0,
            draws: 0,
            draw_clock_us: 0,
        });
        out.write(&raw const (*state).plugin);
    }
    OK
}

/// State is plugin-owned and exclusively admitted by the host for this callback.
unsafe extern "C" fn event(state: *mut c_void, token: u32, input: PpBytes) -> i32 {
    if state.is_null()
        || token != EVENT_TOKEN
        || input.len > wire::REGISTRATION_MAX_BYTES
        || (input.len != 0 && input.ptr.is_null())
    {
        return ERR_INVALID;
    }
    let bytes = if input.len == 0 {
        &[]
    } else {
        // SAFETY: the host supplies a readable borrowed input for this callback.
        unsafe { core::slice::from_raw_parts(input.ptr, input.len) }
    };
    let now = match tick_decode(bytes) {
        Ok(now) => now,
        Err(e) => return e,
    };
    // SAFETY: admitted state remains live and unaliased through the call.
    let s = unsafe { &mut *state.cast::<State>() };
    s.last_tick_ms = Some(now);
    s.ticks = s.ticks.saturating_add(1);
    OK
}

/// This callback records draw admission. No Canvas/Env service is wired yet.
unsafe extern "C" fn draw(state: *mut c_void, token: u32) -> i32 {
    if state.is_null() || token != DRAW_TOKEN {
        return ERR_INVALID;
    }
    // SAFETY: the host admits exclusive access and retains table/context leases.
    let s = unsafe { &mut *state.cast::<State>() };
    let mut now = 0;
    // SAFETY: retained host/context and the local output remain valid for the call.
    let status = unsafe { ((*s.host).clock_micros)(s.ctx, &mut now) };
    if status != OK {
        return status;
    }
    s.draw_clock_us = now;
    s.draws = s.draws.saturating_add(1);
    OK
}

/// The host calls this once, after revocation and callback leases have drained.
unsafe extern "C" fn destroy(state: *mut c_void) {
    // SAFETY: this is the one live instance allocated by make.
    let s = unsafe { &*state.cast::<State>() };
    let (host, ctx) = (s.host, s.ctx);
    // SAFETY: no field owns allocations; destroy before returning matching storage.
    unsafe {
        ptr::drop_in_place(state.cast::<State>());
    }
    // SAFETY: the matching allocator and context outlive plugin destruction.
    unsafe {
        ((*host).heap_dealloc)(
            ctx,
            state,
            mem::size_of::<State>(),
            mem::align_of::<State>(),
        );
    }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;
