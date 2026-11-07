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
    assert!(!commands.is_empty());
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

#[test]
fn screen_commands_and_refresh_modes_use_the_generated_tags_and_scalars() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 256];
    let mut sender = CommandSender::new(&host, &mut scratch);
    sender.open_screen("foreign.main").unwrap();
    sender.run_action("foreign.refresh").unwrap();
    for mode in [
        RedrawMode::RedrawOnly,
        RedrawMode::Fast,
        RedrawMode::Partial,
        RedrawMode::Full,
    ] {
        sender.queue_redraw(mode).unwrap();
    }
    sender.close_screen().unwrap();
    let expected = [
        (
            56,
            wire::Command::OpenScreen {
                screen: "foreign.main",
            },
        ),
        (
            57,
            wire::Command::RunAction {
                action: "foreign.refresh",
            },
        ),
        (58, wire::Command::Redraw { mode: 0 }),
        (58, wire::Command::Redraw { mode: 1 }),
        (58, wire::Command::Redraw { mode: 2 }),
        (58, wire::Command::Redraw { mode: 3 }),
        (59, wire::Command::CloseScreen),
    ];
    let batches = ctx.batches.borrow();
    assert_eq!(batches.len(), expected.len());
    for (bytes, (tag, command)) in batches.iter().zip(expected) {
        assert_eq!(bytes[1], tag);
        assert_eq!(
            wire::commands_decode(bytes)
                .unwrap()
                .iter()
                .collect::<Vec<_>>(),
            [command]
        );
    }
}

#[test]
fn empty_and_overlong_screen_action_ids_are_invalid_without_effects() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [77; 256];
    let long = "x".repeat(wire::COMMAND_MAX_SCREEN_BYTES + 1);
    for id in ["", long.as_str()] {
        let mut sender = CommandSender::new(&host, &mut scratch);
        assert_eq!(sender.open_screen(id), Err(Error::Invalid));
        assert_eq!(sender.run_action(id), Err(Error::Invalid));
        assert_eq!(scratch, [77; 256]);
    }
    assert!(ctx.batches.borrow().is_empty());
}

#[test]
fn short_screen_command_buffers_are_unchanged_and_never_submitted() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [77; 1];
    let mut sender = CommandSender::new(&host, &mut scratch);
    assert_eq!(sender.open_screen("battery.main"), Err(Error::NoSpace));
    assert_eq!(sender.run_action("battery.refresh"), Err(Error::NoSpace));
    assert_eq!(
        sender.queue_redraw(RedrawMode::Partial),
        Err(Error::NoSpace)
    );
    assert_eq!(sender.close_screen(), Err(Error::NoSpace));
    assert_eq!(scratch, [77; 1]);
    assert!(ctx.batches.borrow().is_empty());
}

#[test]
fn screen_command_host_errors_are_preserved_and_default_table_is_unsupported() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 128];
    let mut sender = CommandSender::new(&host, &mut scratch);
    for (code, error) in [
        (abi::ERR_NOT_FOUND, Error::NotFound),
        (abi::ERR_BUSY, Error::Busy),
        (abi::ERR_STATE, Error::State),
        (-901, Error::Other(-901)),
    ] {
        ctx.result.set(code);
        assert_eq!(sender.open_screen("battery.main"), Err(error));
        assert_eq!(sender.run_action("battery.refresh"), Err(error));
        assert_eq!(sender.queue_redraw(RedrawMode::Full), Err(error));
        assert_eq!(sender.close_screen(), Err(error));
    }
    let table = abi::host_table();
    let host = Host {
        table: &table,
        ctx: ptr::null_mut(),
    };
    let mut scratch = [0; 128];
    let mut sender = CommandSender::new(&host, &mut scratch);
    assert_eq!(sender.open_screen("battery.main"), Err(Error::Unsupported));
    assert_eq!(
        sender.run_action("battery.refresh"),
        Err(Error::Unsupported)
    );
    assert_eq!(
        sender.queue_redraw(RedrawMode::Partial),
        Err(Error::Unsupported)
    );
    assert_eq!(sender.close_screen(), Err(Error::Unsupported));
}

#[test]
fn similar_setting_prefixes_are_not_mistaken_for_user_grants() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 128];
    CommandSender::new(&host, &mut scratch)
        .set_setting("core.granted", SettingValue::Bool(true))
        .unwrap();
    let batches = ctx.batches.borrow();
    assert_eq!(batches.len(), 1);
    assert_eq!(
        wire::commands_decode(&batches[0])
            .unwrap()
            .iter()
            .collect::<Vec<_>>(),
        [wire::Command::SetSetting {
            key: "core.granted",
            value: SettingValueRef::Bool(true)
        }]
    );
}

#[test]
fn full_length_screen_and_action_ids_remain_accepted() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let id = alloc::format!("battery.{}", "a".repeat(wire::COMMAND_MAX_SCREEN_BYTES - 8));
    let mut scratch = [0; 256];
    let mut sender = CommandSender::new(&host, &mut scratch);
    sender.open_screen(&id).unwrap();
    sender.run_action(&id).unwrap();
    let batches = ctx.batches.borrow();
    assert_eq!(batches.len(), 2);
    assert_eq!(
        wire::commands_decode(&batches[0])
            .unwrap()
            .iter()
            .collect::<Vec<_>>(),
        [wire::Command::OpenScreen { screen: &id }]
    );
    assert_eq!(
        wire::commands_decode(&batches[1])
            .unwrap()
            .iter()
            .collect::<Vec<_>>(),
        [wire::Command::RunAction { action: &id }]
    );
}

#[test]
fn registry_list_limits_preserve_every_id_and_refuse_excess_before_submit() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; wire::COMMAND_MAX_BYTES];
    let mut sender = CommandSender::new(&host, &mut scratch);
    let menu = ["keys.open"; wire::COMMAND_SET_MENU_ITEMS_MAX_ITEMS];
    sender.set_menu("core.main", Some(&menu)).unwrap();
    let plugins = ["dictionary"; wire::COMMAND_REQUEST_PLUGINS_PLUGINS_MAX_ITEMS];
    sender.request_plugins(&plugins).unwrap();
    assert_eq!(
        sender.set_menu(
            "core.main",
            Some(&["keys.open"; wire::COMMAND_SET_MENU_ITEMS_MAX_ITEMS + 1])
        ),
        Err(Error::NoSpace)
    );
    assert_eq!(
        sender
            .request_plugins(&["dictionary"; wire::COMMAND_REQUEST_PLUGINS_PLUGINS_MAX_ITEMS + 1]),
        Err(Error::NoSpace)
    );
    for invalid in ["", "keys.open\nother.open"] {
        assert_eq!(
            sender.set_menu("core.main", Some(&[invalid])),
            Err(Error::Invalid)
        );
        assert_eq!(sender.request_plugins(&[invalid]), Err(Error::Invalid));
    }
    let batches = ctx.batches.borrow();
    assert_eq!(batches.len(), 2);
    let decoded = wire::commands_decode(&batches[0]).unwrap();
    let Some(wire::Command::SetMenu {
        items, flags: 0, ..
    }) = decoded.iter().next()
    else {
        panic!("expected menu");
    };
    assert_eq!(items.split('\n').collect::<Vec<_>>(), menu);
    let decoded = wire::commands_decode(&batches[1]).unwrap();
    let Some(wire::Command::RequestPlugins { plugins: actual }) = decoded.iter().next() else {
        panic!("expected requests");
    };
    assert_eq!(actual.split('\n').collect::<Vec<_>>(), plugins);
}

#[test]
fn registry_defaults_and_empty_customization_remain_distinct() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; wire::COMMAND_MAX_BYTES];
    let mut sender = CommandSender::new(&host, &mut scratch);
    sender.set_menu("core.main", None).unwrap();
    sender.set_menu("core.main", Some(&[])).unwrap();
    sender
        .bind_key("global", "button.menu", KeyBinding::None)
        .unwrap();
    sender
        .bind_key("global", "button.menu", KeyBinding::Default)
        .unwrap();
    sender.bind_slot("core.home", None).unwrap();
    sender
        .reset_setting(ResetTarget::Placement("widgets.clock@2"))
        .unwrap();
    sender.reset_setting(ResetTarget::All).unwrap();
    sender
        .apply_preset("power.day", PresetMode::Preview)
        .unwrap();
    sender
        .set_plugin_enabled("dictionary", PluginEnabled::Off)
        .unwrap();
    sender.set_scope(None).unwrap();
    sender.end_preview().unwrap();
    let expected = [
        wire::Command::SetMenu {
            menu: "core.main",
            items: "",
            flags: 1,
        },
        wire::Command::SetMenu {
            menu: "core.main",
            items: "",
            flags: 0,
        },
        wire::Command::BindKey {
            layer: "global",
            input: "button.menu",
            action: "",
            flags: 0,
        },
        wire::Command::BindKey {
            layer: "global",
            input: "button.menu",
            action: "",
            flags: 1,
        },
        wire::Command::BindSlot {
            slot: "core.home",
            screen: "",
        },
        wire::Command::ResetSetting {
            key: "widgets.clock@2",
            flags: 8,
        },
        wire::Command::ResetSetting { key: "", flags: 4 },
        wire::Command::ApplyPreset {
            preset: "power.day",
            flags: 1,
        },
        wire::Command::SetPluginEnabled {
            plugin: "dictionary",
            enabled: 0,
        },
        wire::Command::SetScope { path: "" },
        wire::Command::PreviewEnd { flags: 0 },
    ];
    let batches = ctx.batches.borrow();
    assert_eq!(batches.len(), expected.len());
    for (bytes, expected) in batches.iter().zip(expected) {
        let decoded = wire::commands_decode(bytes).unwrap();
        assert_eq!(decoded.iter().next(), Some(expected));
    }
}

#[test]
fn plugin_answer_is_submitted_and_host_denial_is_reported() {
    let table = table();
    let mut ctx = Context::default();
    ctx.result.set(abi::ERR_DENIED);
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 256];
    assert_eq!(
        CommandSender::new(&host, &mut scratch).answer_plugin(
            7,
            "dictionary",
            PluginOutcome::Installed
        ),
        Err(Error::Denied)
    );
    let batches = ctx.batches.borrow();
    assert_eq!(batches.len(), 1);
    assert_eq!(
        wire::commands_decode(&batches[0]).unwrap().iter().next(),
        Some(wire::Command::AnswerPlugin {
            request: 7,
            plugin: "dictionary",
            outcome: 0
        })
    );
}

#[test]
fn preview_sets_are_one_batch_and_reject_overflow_without_submission() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; wire::COMMAND_MAX_BYTES];
    let mut value_scratch = [0; 2048];
    let mut sender = CommandSender::new(&host, &mut scratch);
    let values =
        [PreviewSetting::new("profiles.size", SettingValue::Int(4)); wire::COMMAND_MAX_RECORDS];
    sender
        .preview_settings(&values, &mut value_scratch)
        .unwrap();
    assert_eq!(
        sender.preview_settings(
            &[PreviewSetting::new("profiles.size", SettingValue::Int(4));
                wire::COMMAND_MAX_RECORDS + 1],
            &mut value_scratch
        ),
        Err(Error::NoSpace)
    );
    assert_eq!(
        sender.preview_settings(&values, &mut [0; 1]),
        Err(Error::NoSpace)
    );
    assert_eq!(
        sender.preview_settings(&[], &mut value_scratch),
        Err(Error::Invalid)
    );
    assert_eq!(
        sender.preview_settings(
            &[PreviewSetting::new(
                "core.grant.style",
                SettingValue::Bool(true)
            )],
            &mut value_scratch
        ),
        Err(Error::Unsupported)
    );
    let batches = ctx.batches.borrow();
    assert_eq!(batches.len(), 1);
    let batch = wire::commands_decode(&batches[0]).unwrap();
    assert_eq!(batch.len(), wire::COMMAND_MAX_RECORDS);
    for command in batch.iter() {
        let wire::Command::PreviewSetting { key, value, flags } = command else {
            panic!("expected preview");
        };
        assert_eq!(key, "profiles.size");
        assert_eq!(flags, 0);
        assert_eq!(
            super::super::wire::settings_wire::settings_decode(value).unwrap(),
            SettingValueRef::Int(4)
        );
    }
}

#[test]
fn plugin_commands_use_loader_identity_rules_before_submit() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; wire::COMMAND_MAX_BYTES];
    let mut sender = CommandSender::new(&host, &mut scratch);
    let long = "a".repeat(41);
    for id in ["", "Upper", "bad.id", long.as_str()] {
        assert_eq!(sender.request_plugins(&[id]), Err(Error::Invalid));
        assert_eq!(
            sender.set_plugin_enabled(id, PluginEnabled::On),
            Err(Error::Invalid)
        );
        assert_eq!(
            sender.answer_plugin(1, id, PluginOutcome::Failed),
            Err(Error::Invalid)
        );
    }
    assert!(ctx.batches.borrow().is_empty());
}

#[test]
fn preview_key_value_budget_is_checked_before_host_submission() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; wire::COMMAND_MAX_BYTES];
    let mut value_scratch = [0; 4096];
    let mut sender = CommandSender::new(&host, &mut scratch);
    let text = "x".repeat(470);
    let row = PreviewSetting::new("profiles.name", SettingValue::Text(&text));
    sender
        .preview_settings(&[row; 4], &mut value_scratch)
        .unwrap();
    assert_eq!(
        sender.preview_settings(&[row; 5], &mut value_scratch),
        Err(Error::NoSpace)
    );
    assert_eq!(ctx.batches.borrow().len(), 1);
}
