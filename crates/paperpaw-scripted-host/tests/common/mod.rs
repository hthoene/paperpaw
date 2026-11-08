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
    static SEEN: RefCell<Vec<(i32, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
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
    let (status, mut done) = run_calls(plugin, &[(input.to_vec(), cap)], OK);
    assert_eq!(status, OK, "the proxy itself never fails");
    done.pop().unwrap()
}

/// One callback that makes each call of `calls` (`(input, reply capacity)`,
/// in order) and then returns `end_status`: the callback's own status and
/// what each service call answered.
pub fn run_calls(
    plugin: &Plugin<'_>,
    calls: &[(Vec<u8>, usize)],
    end_status: i32,
) -> (i32, Vec<Done>) {
    // [end status i32 LE][calls u8] then per call [cap u16 LE][len u16 LE][input]
    let mut framed = end_status.to_le_bytes().to_vec();
    framed.push(u8::try_from(calls.len()).unwrap());
    for (input, cap) in calls {
        framed.extend_from_slice(&u16::try_from(*cap).unwrap().to_le_bytes());
        framed.extend_from_slice(&u16::try_from(input.len()).unwrap().to_le_bytes());
        framed.extend_from_slice(input);
    }
    SEEN.with(|seen| seen.borrow_mut().clear());
    let done = plugin.event(0, &framed).unwrap();
    let seen = SEEN.with(|seen| seen.take());
    (
        done.status,
        seen.into_iter()
            .map(|(status, out)| Done {
                status,
                out,
                batches: done.batches.clone(),
            })
            .collect(),
    )
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
    let end_status = i32::from_le_bytes([input[0], input[1], input[2], input[3]]);
    let mut at = 5;
    for _ in 0..input[4] {
        let cap = usize::from(u16::from_le_bytes([input[at], input[at + 1]]));
        let len = usize::from(u16::from_le_bytes([input[at + 2], input[at + 3]]));
        at += 4;
        let seen = one_call(proxy, cap, &input[at..at + len]);
        at += len;
        SEEN.with(|all| all.borrow_mut().push(seen));
    }
    end_status
}

/// [key length][key][op][payload...] into a reply buffer of `cap` bytes.
fn one_call(proxy: &Proxy, cap: usize, input: &[u8]) -> (i32, Vec<u8>) {
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
            return (status, Vec::new());
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
    reply.truncate(usize::try_from(status).unwrap_or(0).min(cap));
    (status, reply)
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
