#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::*;
use alloc::{string::String, vec::Vec};
use core::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr,
};
use paperpaw_native::host_glue::{self as abi, PpHost};

#[derive(Default)]
struct Context {
    batches: RefCell<Vec<Vec<u8>>>,
    result: Cell<i32>,
}

unsafe extern "C" fn submit(ctx: *mut c_void, batch: PpBytes) -> i32 {
    // SAFETY: tests retain the context; sender lends initialized batch bytes.
    let (ctx, bytes) = unsafe {
        (
            &*ctx.cast::<Context>(),
            core::slice::from_raw_parts(batch.ptr, batch.len),
        )
    };
    let commands = wire::commands_decode(bytes).unwrap();
    assert_eq!(commands.len(), 1);
    ctx.batches.borrow_mut().push(bytes.to_vec());
    ctx.result.get()
}

fn table() -> PpHost {
    let mut table = abi::host_table();
    table.commands_submit = submit;
    table
}

#[test]
fn generated_host_decodes_each_typed_command_in_call_order() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 128];
    let mut sender = CommandSender::new(&host, &mut scratch);
    for value in [
        SettingValue::Bool(false),
        SettingValue::Bool(true),
        SettingValue::Int(i64::MIN),
        SettingValue::Int(i64::MAX),
        SettingValue::Text("Grüße"),
    ] {
        sender.set_setting("battery.enabled", value).unwrap();
    }
    let batches = ctx.batches.borrow();
    let expected = [
        SettingValueRef::Bool(false),
        SettingValueRef::Bool(true),
        SettingValueRef::Int(i64::MIN),
        SettingValueRef::Int(i64::MAX),
        SettingValueRef::Text("Grüße"),
    ];
    assert_eq!(batches.len(), expected.len());
    for (bytes, value) in batches.iter().zip(expected) {
        let decoded = wire::commands_decode(bytes).unwrap();
        assert_eq!(
            decoded.iter().collect::<Vec<_>>(),
            [wire::Command::SetSetting {
                key: "battery.enabled",
                value
            }]
        );
    }
}

#[test]
fn short_scratch_and_invalid_values_leave_output_unchanged_without_calling_host() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut short = [77; 2];
    assert_eq!(
        CommandSender::new(&host, &mut short)
            .set_setting("battery.enabled", SettingValue::Bool(true)),
        Err(Error::NoSpace)
    );
    assert_eq!(short, [77; 2]);
    let mut scratch = [77; 128];
    let long_key = String::from_utf8(alloc::vec![b'x'; wire::COMMAND_MAX_KEY_BYTES + 1]).unwrap();
    for (key, value, error) in [
        ("battery.enabled", SettingValue::Absent, Error::Invalid),
        ("", SettingValue::Bool(true), Error::Invalid),
        (long_key.as_str(), SettingValue::Bool(true), Error::NoSpace),
    ] {
        assert_eq!(
            CommandSender::new(&host, &mut scratch).set_setting(key, value),
            Err(error)
        );
        assert_eq!(scratch, [77; 128]);
    }
    assert!(ctx.batches.borrow().is_empty());
}

#[test]
fn user_grants_are_denied_before_encoding_or_host_calls() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [77; 128];
    for key in [
        "core.grant.",
        "core.grant.style",
        "core.grant.install",
        "core.grant.style.battery",
        "core.grant.install.foreign",
    ] {
        assert_eq!(
            CommandSender::new(&host, &mut scratch).set_setting(key, SettingValue::Bool(true)),
            Err(Error::Denied)
        );
        assert_eq!(scratch, [77; 128]);
    }
    assert!(ctx.batches.borrow().is_empty());
}

#[test]
fn host_statuses_are_preserved_and_do_not_latch_the_sender() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 128];
    let mut sender = CommandSender::new(&host, &mut scratch);
    for (code, error) in [
        (abi::ERR_DENIED, Error::Denied),
        (abi::ERR_STATE, Error::State),
        (abi::ERR_NO_SPACE, Error::NoSpace),
        (abi::ERR_NO_MEMORY, Error::NoMemory),
        (-912, Error::Other(-912)),
        (2, Error::Other(2)),
    ] {
        ctx.result.set(code);
        assert_eq!(
            sender.set_setting("battery.enabled", SettingValue::Bool(false)),
            Err(error)
        );
        ctx.result.set(abi::OK);
        assert_eq!(
            sender.set_setting("battery.enabled", SettingValue::Bool(true)),
            Ok(())
        );
    }
    assert_eq!(ctx.batches.borrow().len(), 12);
}

#[test]
fn dropping_a_sender_never_submits() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 128];
    {
        let mut sender = CommandSender::new(&host, &mut scratch);
        assert_eq!(
            sender.set_setting("battery.enabled", SettingValue::Absent),
            Err(Error::Invalid)
        );
    }
    assert!(ctx.batches.borrow().is_empty());
    {
        let mut sender = CommandSender::new(&host, &mut scratch);
        sender
            .set_setting("battery.enabled", SettingValue::Bool(true))
            .unwrap();
    }
    assert_eq!(ctx.batches.borrow().len(), 1);
}

#[test]
fn default_table_reports_unsupported() {
    let table = abi::host_table();
    let host = Host {
        table: &table,
        ctx: ptr::null_mut(),
    };
    assert_eq!(
        CommandSender::new(&host, &mut [0; 128])
            .set_setting("battery.enabled", SettingValue::Bool(true)),
        Err(Error::Unsupported)
    );
}
