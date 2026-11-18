//! A handover encodes its params straight into the caller's buffer: the
//! SDK allocates nothing for it (the old record list took 3,328 bytes, 81 %
//! of the X4's 4 KiB gate).
#![allow(clippy::unwrap_used, reason = "fixture failures must fail tests")]
use super::{Error, Host, Param, Ui, wire};
use crate::native::commands::menu_alloc_tests::track;
use core::{cell::Cell, ffi::c_void};
use paperpaw_native::host_glue::{self as abi, PpBuffer, PpBytes, PpHost};

/// The bytes the host replies with, held without allocating.
struct Reply {
    bytes: [u8; 32],
    len: usize,
    calls: Cell<u32>,
}

unsafe extern "C" fn open(_: *mut c_void, _: PpBytes, _: u16, _: u16, out: *mut u32) -> i32 {
    // SAFETY: the client supplies aligned exclusive output storage.
    unsafe { out.write(9) };
    abi::OK
}

unsafe extern "C" fn call(ctx: *mut c_void, _: u32, _: u32, _: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: the fixture keeps its reply alive and the output is exclusive.
    let reply = unsafe { &*ctx.cast::<Reply>() };
    reply.calls.set(reply.calls.get() + 1);
    assert!(reply.len <= out.cap);
    // SAFETY: disjoint fixture reply and client output buffers.
    unsafe { core::ptr::copy_nonoverlapping(reply.bytes.as_ptr(), out.ptr, reply.len) };
    i32::try_from(reply.len).unwrap()
}

#[test]
fn a_full_handover_allocates_nothing() {
    let mut table: PpHost = abi::host_table();
    table.service_open = open;
    table.service_call = call;
    let mut reply = Reply {
        bytes: [0; 32],
        len: 0,
        calls: Cell::new(0),
    };
    reply.len = wire::record_encode_one(
        &wire::Record::ServiceUiHandle(wire::ServiceUiHandle { handle: 5 }),
        &mut reply.bytes,
    )
    .unwrap();
    let host = Host {
        table: &table,
        ctx: core::ptr::from_mut(&mut reply).cast(),
    };
    let ui = Ui::open(&host).unwrap();
    let keys: [[u8; 3]; 32] = core::array::from_fn(|i| {
        let i = u8::try_from(i).unwrap();
        [b'p', b'a' + i % 26, b'a' + i / 26]
    });
    let params: [Param<'_>; 32] =
        core::array::from_fn(|i| Param::new(core::str::from_utf8(&keys[i]).unwrap(), "v").unwrap());
    let mut input = [0; 4096];
    let mut out = [0; 4096];
    let (result, counts) = track(false, || ui.handover(&host, &params, &mut input, &mut out));
    assert_eq!(result.unwrap().get(), 5);
    assert_eq!(counts.peak, 0, "32 params, whole counted batch, no heap");
    assert_eq!(counts.live, 0);

    let (result, counts) = track(true, || ui.handover(&host, &params, &mut input, &mut out));
    assert!(result.is_ok(), "an allocator that refuses is never asked");
    assert!(!counts.refused);
    let calls = reply.calls.get();
    let duplicate = [params[0], params[0]];
    let (result, counts) = track(false, || {
        ui.handover(&host, &duplicate, &mut input, &mut out)
    });
    assert_eq!(result.unwrap_err(), Error::Invalid);
    assert_eq!(counts.peak, 0);
    assert_eq!(reply.calls.get(), calls);
}
