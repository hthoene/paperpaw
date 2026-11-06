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

struct Context<'a> {
    owner: &'a str,
    keys: RefCell<Vec<String>>,
    value: Cell<Option<bool>>,
    status: Cell<i32>,
}
impl<'a> Context<'a> {
    fn new(owner: &'a str) -> Self {
        Self {
            owner,
            keys: RefCell::default(),
            value: Cell::new(None),
            status: Cell::new(0),
        }
    }
}
unsafe extern "C" fn read_slot(ctx: *mut c_void, key: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: tests provide a live context, readable key and writable output.
    unsafe {
        let ctx = &*ctx.cast::<Context<'_>>();
        let key = core::str::from_utf8(core::slice::from_raw_parts(key.ptr, key.len)).unwrap();
        ctx.keys.borrow_mut().push(key.into());
        let (_, owner) = key.strip_prefix(PREFIX).unwrap().split_once('.').unwrap();
        if owner != ctx.owner {
            return abi::ERR_DENIED;
        }
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
fn reads_own_grant_as_yes_or_no_and_host_denies_foreign_owner() {
    let table = table();
    let mut ctx = Context::new("reader");
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    ctx.value.set(Some(true));
    assert_eq!(granted(&host, GrantKind::Style, "reader"), Ok(true));
    ctx.value.set(Some(false));
    assert_eq!(granted(&host, GrantKind::Install, "reader"), Ok(false));
    assert_eq!(
        granted(&host, GrantKind::Style, "other-plugin"),
        Err(Error::Denied)
    );
    assert_eq!(
        *ctx.keys.borrow(),
        [
            "core.grant.style.reader",
            "core.grant.install.reader",
            "core.grant.style.other-plugin"
        ]
    );
    ctx.status.set(-125);
    assert_eq!(
        granted(&host, GrantKind::Style, "reader"),
        Err(Error::Other(-125))
    );
}

#[test]
fn invalid_ids_and_non_boolean_or_truncated_replies_are_refused() {
    let table = table();
    let mut ctx = Context::new("reader");
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    for plugin in [
        "",
        "A",
        "-reader",
        "reader.id",
        "reader/other",
        "reader other",
        &"x".repeat(MAX_PLUGIN_BYTES + 1),
    ] {
        assert_eq!(
            granted(&host, GrantKind::Style, plugin),
            Err(Error::Invalid)
        );
    }
    assert!(ctx.keys.borrow().is_empty());
    assert_eq!(
        granted(&host, GrantKind::Style, "reader"),
        Err(Error::Invalid)
    );
    ctx.status.set((SETTINGS_HEADER_BYTES + 2) as i32);
    assert_eq!(
        granted(&host, GrantKind::Style, "reader"),
        Err(Error::Invalid)
    );
    ctx.status.set(1);
    assert_eq!(
        granted(&host, GrantKind::Style, "reader"),
        Err(Error::Invalid)
    );
    ctx.status.set(abi::ERR_NOT_FOUND);
    assert_eq!(
        granted(&host, GrantKind::Style, "reader"),
        Err(Error::NotFound)
    );
}

#[test]
fn longest_plugin_id_and_kind_fit_the_bounded_key() {
    let table = table();
    let plugin = "x".repeat(MAX_PLUGIN_BYTES);
    let mut ctx = Context::new(&plugin);
    ctx.value.set(Some(true));
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    assert_eq!(granted(&host, GrantKind::Install, &plugin), Ok(true));
    assert_eq!(
        ctx.keys.borrow()[0],
        alloc::format!("core.grant.install.{plugin}")
    );
}
