#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::super::declare::settings_value::settings_encode;
use super::*;
use alloc::{string::String, vec::Vec};
use core::{
    cell::{Cell, RefCell},
    ffi::c_void,
    mem::MaybeUninit,
    ptr,
};
use paperpaw_native::host_glue::{self as abi, PpHost};

#[derive(Default)]
struct Context {
    keys: RefCell<Vec<String>>,
    value: Cell<Option<bool>>,
    status: Cell<i32>,
}
unsafe extern "C" fn read_slot(ctx: *mut c_void, key: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: tests provide a live context, readable key and writable output.
    unsafe {
        let ctx = &*ctx.cast::<Context>();
        let key = core::str::from_utf8(core::slice::from_raw_parts(key.ptr, key.len)).unwrap();
        ctx.keys.borrow_mut().push(key.into());
        if ctx.status.get() != 0 {
            return ctx.status.get();
        }
        let value = ctx
            .value
            .get()
            .map_or(SettingValueRef::Text("x"), SettingValueRef::Bool);
        let out = core::slice::from_raw_parts_mut(out.ptr.cast::<MaybeUninit<u8>>(), out.cap);
        settings_encode(value, out).unwrap() as i32
    }
}
fn table() -> PpHost {
    let mut table = abi::host_table();
    table.settings_read = read_slot;
    table
}

#[test]
fn reads_own_grant_as_yes_or_no_and_preserves_permission_refusals() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    ctx.value.set(Some(true));
    assert_eq!(read(&host, "core.grant.style.example.reader"), Ok(true));
    ctx.value.set(Some(false));
    assert_eq!(read(&host, "core.grant.install.example.reader"), Ok(false));
    ctx.status.set(abi::ERR_DENIED);
    assert_eq!(
        read(&host, "core.grant.style.other.plugin"),
        Err(Error::Denied)
    );
    assert_eq!(
        *ctx.keys.borrow(),
        [
            "core.grant.style.example.reader",
            "core.grant.install.example.reader",
            "core.grant.style.other.plugin"
        ]
    );
    ctx.status.set(-125);
    assert_eq!(
        read(&host, "core.grant.future.example.reader"),
        Err(Error::Other(-125))
    );
}

#[test]
fn malformed_keys_and_non_boolean_or_truncated_replies_are_refused() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    for key in [
        "",
        "other.key",
        "core.grant.",
        "core.grant.style",
        "core.grant..owner",
        "core.grant.style.",
    ] {
        assert_eq!(read(&host, key), Err(Error::Invalid));
    }
    assert!(ctx.keys.borrow().is_empty());
    assert_eq!(read(&host, "core.grant.style.owner"), Err(Error::Invalid));
    ctx.status.set((SETTINGS_HEADER_BYTES + 2) as i32);
    assert_eq!(read(&host, "core.grant.style.owner"), Err(Error::Invalid));
    ctx.status.set(1);
    assert_eq!(read(&host, "core.grant.style.owner"), Err(Error::Invalid));
    ctx.status.set(abi::ERR_NOT_FOUND);
    assert_eq!(read(&host, "core.grant.style.owner"), Err(Error::NotFound));
}
