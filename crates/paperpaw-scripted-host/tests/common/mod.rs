//! A service proxy plugin: each event is "call op N of service K with this
//! payload"; the reply (or the status) comes back to the test. The handle of
//! a service opens at its first call and stays open until the `drop` hook.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture failures fail the test; not every test uses every helper"
)]
use core::ffi::c_void;
use paperpaw_native::host_glue::{OK, PpBuffer, PpBytes, PpEntry, PpHost, PpPlugin};
use std::cell::RefCell;

struct Proxy {
    host: *const PpHost,
    ctx: *mut c_void,
    open: RefCell<Vec<(String, u32)>>,
}

unsafe extern "C" fn event(state: *mut c_void, _token: u32, input: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: `state` is the leaked `Proxy`; `input` is readable.
    let (proxy, input) = unsafe {
        (
            &*state.cast::<Proxy>(),
            core::slice::from_raw_parts(input.ptr, input.len),
        )
    };
    // [key length][key][op][payload...]
    let key_len = input[0] as usize;
    let key = std::str::from_utf8(&input[1..=key_len]).unwrap().to_owned();
    let op = u32::from(input[key_len + 1]);
    let payload = &input[key_len + 2..];
    // SAFETY: the scripted table outlives the plugin.
    let host = unsafe { &*proxy.host };
    let known = proxy
        .open
        .borrow()
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, h)| *h);
    let handle = if let Some(handle) = known {
        handle
    } else {
        let mut handle = 0_u32;
        // SAFETY: scripted slot; pointers live for the call.
        let status = unsafe {
            (host.service_open)(
                proxy.ctx,
                PpBytes {
                    ptr: key.as_ptr(),
                    len: key.len(),
                },
                1,
                0,
                &raw mut handle,
            )
        };
        if status != OK {
            return status;
        }
        proxy.open.borrow_mut().push((key, handle));
        handle
    };
    // SAFETY: scripted slot; pointers live for the call.
    unsafe {
        (host.service_call)(
            proxy.ctx,
            handle,
            op,
            PpBytes {
                ptr: payload.as_ptr(),
                len: payload.len(),
            },
            out,
        )
    }
}

unsafe extern "C" fn plugin_drop(state: *mut c_void) {
    // SAFETY: the box leaked by `make`, dropped once.
    let proxy = unsafe { Box::from_raw(state.cast::<Proxy>()) };
    // SAFETY: the scripted table outlives the plugin.
    let host = unsafe { &*proxy.host };
    for (_, handle) in proxy.open.borrow().iter() {
        // SAFETY: scripted slot.
        unsafe { (host.handle_release)(proxy.ctx, *handle) };
    }
}

unsafe extern "C" fn make(host: *const PpHost, ctx: *mut c_void, out: *mut *const PpPlugin) -> i32 {
    let state = Box::into_raw(Box::new(Proxy {
        host,
        ctx,
        open: RefCell::new(Vec::new()),
    }));
    let table = Box::new(PpPlugin {
        size: core::mem::size_of::<PpPlugin>() as u32,
        state: state.cast(),
        drop: Some(plugin_drop),
        dispatch_event: Some(event),
        draw: None,
        input: None,
    });
    // SAFETY: `out` is the host's writable slot; the table is leaked for the
    // test process.
    unsafe { *out = Box::into_raw(table) };
    OK
}

pub fn entry() -> PpEntry {
    PpEntry {
        size: core::mem::size_of::<PpEntry>() as u32,
        abi_major: 7,
        needs_minor: 1,
        metadata: PpBytes {
            ptr: core::ptr::null(),
            len: 0,
        },
        make: Some(make),
    }
}

/// The proxy input for `op` of `key` with `payload`.
pub fn call(key: &str, op: u8, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![u8::try_from(key.len()).unwrap()];
    bytes.extend_from_slice(key.as_bytes());
    bytes.push(op);
    bytes.extend_from_slice(payload);
    bytes
}
