#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::super::declare::settings_value::{SETTINGS_HEADER_BYTES, settings_encode};
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
    reply: RefCell<Vec<u8>>,
    result: Cell<Option<i32>>,
}
impl Context {
    fn set(&self, value: Value<'_>) {
        let mut scratch = [MaybeUninit::uninit(); 128];
        let length = settings_encode(value, &mut scratch).unwrap();
        *self.reply.borrow_mut() = scratch[..length]
            .iter()
            .map(|byte| {
                // SAFETY: the generated encoder initialized every byte of this prefix.
                unsafe { byte.assume_init() }
            })
            .collect();
    }
}
unsafe extern "C" fn read_slot(ctx: *mut c_void, key: PpBytes, out: PpBuffer) -> i32 {
    // SAFETY: tests provide a live context, readable key and writable output.
    unsafe {
        let ctx = &*ctx.cast::<Context>();
        let key = core::str::from_utf8(core::slice::from_raw_parts(key.ptr, key.len)).unwrap();
        ctx.keys.borrow_mut().push(key.into());
        if let Some(result) = ctx.result.get() {
            return result;
        }
        let reply = ctx.reply.borrow();
        ptr::copy_nonoverlapping(reply.as_ptr(), out.ptr, reply.len().min(out.cap));
        reply.len() as i32
    }
}
fn table() -> PpHost {
    let mut table = abi::host_table();
    table.settings_read = read_slot;
    table
}

#[test]
fn values_round_trip_and_text_borrows_the_callers_scratch() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 128];
    for value in [
        Value::Absent,
        Value::Bool(false),
        Value::Bool(true),
        Value::Int(i64::MIN),
        Value::Int(i64::MAX),
        Value::Text("Grüße"),
    ] {
        ctx.set(value);
        let start = scratch.as_ptr();
        let got = read(&host, "battery.show-percent", &mut scratch).unwrap();
        assert_eq!(got, value);
        if let Value::Text(text) = got {
            assert_eq!(text.as_ptr(), start.wrapping_add(SETTINGS_HEADER_BYTES));
        }
    }
    assert!(
        ctx.keys
            .borrow()
            .iter()
            .all(|key| key == "battery.show-percent")
    );
}

#[test]
fn truncated_values_are_not_decoded_and_exact_capacity_succeeds() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    ctx.set(Value::Bool(true));
    assert_eq!(
        read(&host, "battery.show-percent", &mut []),
        Err(Error::NoSpace)
    );
    assert_eq!(
        read(
            &host,
            "battery.show-percent",
            &mut [0; SETTINGS_HEADER_BYTES]
        ),
        Err(Error::NoSpace)
    );
    assert_eq!(
        read(
            &host,
            "battery.show-percent",
            &mut [0; SETTINGS_HEADER_BYTES + 1]
        ),
        Ok(Value::Bool(true))
    );
    ctx.result.set(Some(i32::MAX));
    assert_eq!(
        read(&host, "battery.show-percent", &mut [0; 16]),
        Err(Error::NoSpace)
    );
}

#[test]
fn malformed_host_values_and_status_errors_remain_distinct() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 128];
    assert_eq!(
        read(&host, "battery.show-percent", &mut scratch),
        Err(Error::Invalid)
    );
    ctx.set(Value::Bool(true));
    ctx.reply.borrow_mut()[SETTINGS_HEADER_BYTES] = 2;
    assert_eq!(
        read(&host, "battery.show-percent", &mut scratch),
        Err(Error::Invalid)
    );
    ctx.set(Value::Bool(true));
    ctx.reply.borrow_mut()[0] = 99;
    assert_eq!(
        read(&host, "battery.show-percent", &mut scratch),
        Err(Error::Unsupported)
    );
    ctx.set(Value::Text("x"));
    ctx.reply.borrow_mut()[SETTINGS_HEADER_BYTES] = 0xff;
    assert_eq!(
        read(&host, "battery.show-percent", &mut scratch),
        Err(Error::Utf8)
    );
    for (code, expected) in [
        (abi::ERR_NOT_FOUND, Error::NotFound),
        (abi::ERR_DENIED, Error::Denied),
        (-125, Error::Other(-125)),
    ] {
        ctx.result.set(Some(code));
        assert_eq!(
            read(&host, "battery.show-percent", &mut scratch),
            Err(expected)
        );
    }
}

#[test]
fn default_table_is_unsupported() {
    let table = abi::host_table();
    let host = Host {
        table: &table,
        ctx: ptr::null_mut(),
    };
    assert_eq!(
        read(&host, "battery.show-percent", &mut [0; 32]),
        Err(Error::Unsupported)
    );
}
