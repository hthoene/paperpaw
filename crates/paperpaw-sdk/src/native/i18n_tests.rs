#![allow(clippy::unwrap_used, reason = "fixture failures must fail tests")]
use super::*;
use crate::native::wire::settings_wire::{self as wire, SETTINGS_HEADER_BYTES};
use core::{ffi::c_void, mem::MaybeUninit, ptr};
use paperpaw_native::host_glue::{self as abi, PpBuffer, PpBytes};

struct Context<'a> {
    value: wire::SettingValueRef<'a>,
    status: Option<i32>,
}
unsafe extern "C" fn read_slot(ctx: *mut c_void, key: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: each test retains its context and SDK lends live key/output buffers.
    let (ctx, key, out) = unsafe {
        (
            &*ctx.cast::<Context<'_>>(),
            core::slice::from_raw_parts(key.ptr, key.len),
            core::slice::from_raw_parts_mut(out.ptr.cast::<MaybeUninit<u8>>(), out.cap),
        )
    };
    assert_eq!(key, b"core.language");
    if let Some(status) = ctx.status {
        return status;
    }
    match wire::settings_encode(ctx.value, out) {
        Ok(n) => i32::try_from(n).unwrap(),
        Err(error) => error,
    }
}
fn read<'a>(
    value: wire::SettingValueRef<'_>,
    status: Option<i32>,
    scratch: &'a mut [u8],
) -> Result<&'a str, Error> {
    let mut table = abi::host_table();
    table.settings_read = read_slot;
    let mut ctx = Context { value, status };
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    language(&host, scratch)
}
#[test]
fn full_tags_borrow_scratch_and_preserve_case() {
    let mut scratch = [0; 128];
    for tag in ["de", "pt-BR", "zh-Hant-TW", "en-US-u-nu-latn"] {
        let expected = scratch.as_ptr().wrapping_add(SETTINGS_HEADER_BYTES);
        let got = read(wire::SettingValueRef::Text(tag), None, &mut scratch).unwrap();
        assert_eq!(got, tag);
        assert_eq!(got.as_ptr(), expected);
    }
}
#[test]
fn invalid_values_are_refused_without_a_default() {
    let mut scratch = [0; 128];
    for value in [
        wire::SettingValueRef::Absent,
        wire::SettingValueRef::Bool(true),
        wire::SettingValueRef::Int(1),
        wire::SettingValueRef::Text(""),
        wire::SettingValueRef::Text("pt_BR"),
        wire::SettingValueRef::Text("de--DE"),
        wire::SettingValueRef::Text("-de"),
        wire::SettingValueRef::Text("de-"),
        wire::SettingValueRef::Text("de\n"),
        wire::SettingValueRef::Text("dé"),
    ] {
        assert_eq!(read(value, None, &mut scratch), Err(Error::Invalid));
    }
}
#[test]
fn short_scratch_and_host_errors_are_preserved() {
    let value = wire::SettingValueRef::Text("pt-BR");
    assert_eq!(
        read(value, None, &mut [0; SETTINGS_HEADER_BYTES + 4]),
        Err(Error::NoSpace)
    );
    assert_eq!(
        read(value, None, &mut [0; SETTINGS_HEADER_BYTES + 5]),
        Ok("pt-BR")
    );
    assert_eq!(read(value, None, &mut []), Err(Error::NoSpace));
    assert_eq!(
        read(value, Some(abi::ERR_DENIED), &mut [0; 128]),
        Err(Error::Denied)
    );
    assert_eq!(
        read(value, Some(i32::MAX), &mut [0; 128]),
        Err(Error::NoSpace)
    );
}
