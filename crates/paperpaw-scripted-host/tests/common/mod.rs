//! A service proxy plugin: each event is "call op N of service K with this
//! payload" into a reply buffer of the size the test chooses; the call's
//! status (the reply length, or an error) and the reply bytes come back to the
//! test through [`run`], since an event returns a status only. The handle of
//! a service opens at its first call and stays open until the `drop` hook.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture failures fail the test; not every test uses every helper"
)]
use core::ffi::c_void;
use paperpaw_native::host_glue::{OK, PpBuffer, PpBytes, PpEntry, PpHost, PpPlugin};
use paperpaw_scripted_host::Plugin;
use std::cell::RefCell;

thread_local! {
    static SEEN: RefCell<(i32, Vec<u8>)> = const { RefCell::new((0, Vec::new())) };
}

/// What one proxied call answered.
pub struct Done {
    /// The service call's status: the reply length or an error.
    pub status: i32,
    /// The reply bytes the plugin's buffer received.
    pub out: Vec<u8>,
    /// The command batches the callback submitted.
    pub batches: Vec<Vec<u8>>,
}

impl Done {
    /// Whether any submitted batch holds `needle`.
    pub fn batches_contain(&self, needle: &[u8]) -> bool {
        !needle.is_empty()
            && self
                .batches
                .iter()
                .any(|batch| batch.windows(needle.len()).any(|window| window == needle))
    }
}

/// Sends `input` (see [`call`]) to the proxy with a reply buffer of `cap`
/// bytes and returns what the service answered.
pub fn run(plugin: &Plugin<'_>, input: &[u8], cap: usize) -> Done {
    let mut framed = u16::try_from(cap).unwrap().to_le_bytes().to_vec();
    framed.extend_from_slice(input);
    SEEN.with(|seen| *seen.borrow_mut() = (0, Vec::new()));
    let done = plugin.event(0, &framed).unwrap();
    assert_eq!(done.status, OK, "the proxy itself never fails");
    let (status, out) = SEEN.with(|seen| seen.take());
    Done {
        status,
        out,
        batches: done.batches,
    }
}

struct Proxy {
    host: *const PpHost,
    ctx: *mut c_void,
    open: RefCell<Vec<(String, u32)>>,
}

unsafe extern "C" fn event(state: *mut c_void, _token: u32, input: PpBytes, _out: PpBuffer) -> i32 {
    // SAFETY: `state` is the leaked `Proxy`; `input` is readable.
    let (proxy, input) = unsafe {
        (
            &*state.cast::<Proxy>(),
            core::slice::from_raw_parts(input.ptr, input.len),
        )
    };
    // [reply capacity u16 LE][key length][key][op][payload...]
    let cap = usize::from(u16::from_le_bytes([input[0], input[1]]));
    let input = &input[2..];
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
            SEEN.with(|seen| *seen.borrow_mut() = (status, Vec::new()));
            return OK;
        }
        proxy.open.borrow_mut().push((key, handle));
        handle
    };
    let mut reply = vec![0_u8; cap];
    // SAFETY: scripted slot; pointers live for the call.
    let status = unsafe {
        (host.service_call)(
            proxy.ctx,
            handle,
            op,
            PpBytes {
                ptr: payload.as_ptr(),
                len: payload.len(),
            },
            PpBuffer {
                ptr: reply.as_mut_ptr(),
                cap,
            },
        )
    };
    let written = usize::try_from(status).unwrap_or(0).min(cap);
    reply.truncate(written);
    SEEN.with(|seen| *seen.borrow_mut() = (status, reply));
    OK
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
