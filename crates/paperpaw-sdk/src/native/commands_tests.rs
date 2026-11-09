#![allow(clippy::unwrap_used, reason = "fixture errors must fail tests")]
use super::*;
use crate::native::ui::Open;
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
        Err(Error::Invalid)
    );
    assert_eq!(
        sender
            .request_plugins(&["dictionary"; wire::COMMAND_REQUEST_PLUGINS_PLUGINS_MAX_ITEMS + 1]),
        Err(Error::Invalid)
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
fn bluetooth_commands_are_one_record_each_and_name_the_plugin_to_give_to() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; wire::COMMAND_MAX_BYTES];
    let mut sender = CommandSender::new(&host, &mut scratch);
    sender.bt_scan(10_000).unwrap();
    sender.bt_stop_scan().unwrap();
    sender.bt_pair(3).unwrap();
    sender.bt_connect(3).unwrap();
    sender.bt_disconnect(3).unwrap();
    sender.bt_forget(3).unwrap();
    sender.bt_give(4, "weather").unwrap();
    sender.bt_take_back(4).unwrap();
    sender.bt_keys(5, true).unwrap();
    sender.bt_keys(5, false).unwrap();
    // A plugin id is named, and no longer than the contract's 64 bytes.
    assert_eq!(sender.bt_give(4, ""), Err(Error::Invalid));
    assert_eq!(sender.bt_give(4, "x".repeat(65)), Err(Error::Invalid));
    assert!(sender.bt_give(4, "x".repeat(64)).is_ok());
    let expected = [
        wire::Command::BtScan { lasting_ms: 10_000 },
        wire::Command::BtStopScan {},
        wire::Command::BtPair { device: 3 },
        wire::Command::BtConnect { device: 3 },
        wire::Command::BtDisconnect { device: 3 },
        wire::Command::BtForget { device: 3 },
        wire::Command::BtGive {
            device: 4,
            plugin: "weather",
        },
        wire::Command::BtTakeBack { device: 4 },
        wire::Command::BtKeys { device: 5, on: 1 },
        wire::Command::BtKeys { device: 5, on: 0 },
    ];
    let batches = ctx.batches.borrow();
    // The two refused gives submitted nothing; the last, 64 bytes, did.
    assert_eq!(batches.len(), expected.len() + 1);
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
            plugin_request_id(7),
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
            sender.answer_plugin(plugin_request_id(1), id, PluginOutcome::Failed),
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

fn plugin_request_id(raw: u32) -> crate::native::events::PluginRequestId {
    use crate::native::{declare::records_wire as records, events::Event};
    let mut bytes = [0; 128];
    let count = records::records_encode(
        &[records::Record::EventPluginsRequested(
            records::EventPluginsRequested {
                now_ms: 0,
                request: raw,
                by: "fixture",
                plugins: "dictionary",
            },
        )],
        &mut bytes,
    )
    .unwrap();
    let Event::PluginsRequested { request, .. } = Event::decode(&bytes[..count]).unwrap() else {
        panic!("expected plugin request");
    };
    request
}

#[test]
fn menu_list_item_and_total_limits_keep_distinct_errors() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; wire::COMMAND_MAX_BYTES];
    let mut sender = CommandSender::new(&host, &mut scratch);
    let oversized = "a".repeat(wire::COMMAND_SET_MENU_ITEMS_ITEM_MAX_BYTES + 1);
    assert_eq!(
        sender.set_menu("core.main", Some(&[&oversized])),
        Err(Error::Invalid)
    );
    let item = "a".repeat(wire::COMMAND_SET_MENU_ITEMS_ITEM_MAX_BYTES);
    let items = [&item[..]; wire::COMMAND_SET_MENU_ITEMS_MAX_ITEMS];
    assert_eq!(
        sender.set_menu("core.main", Some(&items)),
        Err(Error::NoSpace)
    );
    assert!(ctx.batches.borrow().is_empty());
}

fn decoded(ctx: &Context) -> Vec<Vec<String>> {
    ctx.batches
        .borrow()
        .iter()
        .map(|bytes| {
            wire::commands_decode(bytes)
                .unwrap()
                .iter()
                .map(|command| alloc::format!("{command:?}"))
                .collect()
        })
        .collect()
}

#[test]
fn device_commands_encode_in_order_and_keep_the_password_secret() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 256];
    let mut sender = CommandSender::new(&host, &mut scratch);
    sender.wifi_on().unwrap();
    sender.wifi_scan().unwrap();
    sender.wifi_join(7, "home", "hunter2!", true).unwrap();
    sender.wifi_host(8, "me", "12345678").unwrap();
    sender.go_online(600_000).unwrap();
    sender.wear("power.saving", true).unwrap();
    sender.sleep().unwrap();
    sender.power_off().unwrap();
    sender.restart().unwrap();
    sender.wifi_leave().unwrap();
    sender.network_none().unwrap();
    sender.wifi_off().unwrap();
    let batches = decoded(&ctx);
    assert_eq!(batches.len(), 12);
    assert_eq!(batches[0], ["WifiOn"]);
    assert!(batches[2][0].contains("request: 7") && batches[2][0].contains("flags: 1"));
    assert!(
        batches
            .iter()
            .flatten()
            .all(|text| !text.contains("hunter2"))
    );
    assert!(batches[5][0].contains("power.saving") && batches[5][0].contains("flags: 1"));
    assert_eq!(batches[6], ["Sleep"]);
    assert_eq!(batches[7], ["PowerOff"]);
    assert_eq!(batches[8], ["Restart"]);
}

#[test]
fn device_commands_refuse_bad_input_before_the_host() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 256];
    let mut sender = CommandSender::new(&host, &mut scratch);
    let long = "x".repeat(200);
    assert_eq!(sender.wifi_join(0, "home", "", false), Err(Error::Invalid));
    assert_eq!(sender.wifi_join(1, "", "", false), Err(Error::Invalid));
    assert_eq!(sender.wifi_host(1, "me", "short"), Err(Error::Invalid));
    assert_eq!(sender.go_online(0), Err(Error::Invalid));
    assert_eq!(sender.go_online(600_001), Err(Error::Invalid));
    assert_eq!(sender.wear("", false), Err(Error::Invalid));
    assert!(sender.wear(&long, false).is_err());
    assert!(sender.wifi_join(1, &long, "", false).is_err());
    assert!(ctx.batches.borrow().is_empty());
}

#[test]
fn a_screen_and_a_power_command_travel_in_one_batch() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 256];
    let mut sender = CommandSender::new(&host, &mut scratch);
    sender
        .show_then(Open::slot("core.sleep").unwrap(), PowerAction::PowerOff)
        .unwrap();
    assert_eq!(
        sender.show_then(Open::slot("core.sleep").unwrap(), PowerAction::Sleep),
        Ok(())
    );
    let batches = decoded(&ctx);
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[0].len(), 2);
    assert!(batches[0][0].contains("core.sleep"));
    assert_eq!(batches[0][1], "PowerOff");
    assert_eq!(batches[1][1], "Sleep");
}

#[test]
fn lookup_ready_names_the_text_and_language_of_the_ask() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; wire::COMMAND_MAX_BYTES];
    let mut sender = CommandSender::new(&host, &mut scratch);
    sender.lookup_ready("serendipity", "en").unwrap();
    // A language may be empty; the limits are the contract's 1,024 and 35.
    sender.lookup_ready("x".repeat(1024), "").unwrap();
    sender.lookup_ready("a", "l".repeat(35)).unwrap();
    assert_eq!(
        sender.lookup_ready("word", "l".repeat(36)),
        Err(Error::Invalid)
    );
    assert_eq!(
        sender.lookup_ready("x".repeat(1025), "en"),
        Err(Error::Invalid)
    );
    let batches = ctx.batches.borrow();
    assert_eq!(batches.len(), 3, "refused asks stage nothing");
    let first = wire::commands_decode(&batches[0]).unwrap();
    assert_eq!(
        first.iter().next(),
        Some(wire::Command::LookupReady {
            text: "serendipity",
            language: "en"
        })
    );
}

#[test]
fn a_host_refusal_of_lookup_ready_comes_back() {
    let table = table();
    let mut ctx = Context::default();
    ctx.result.set(abi::ERR_STATE);
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 128];
    let mut sender = CommandSender::new(&host, &mut scratch);
    assert_eq!(sender.lookup_ready("word", "en"), Err(Error::State));
}

#[test]
fn install_and_remove_commands_encode_with_their_flags() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 256];
    let mut sender = CommandSender::new(&host, &mut scratch);
    sender
        .install_plugin(3, InstallRoot::Data, "staging/fonts-x", true, false)
        .unwrap();
    sender
        .install_plugin(4, InstallRoot::Files, "a.ppn", false, true)
        .unwrap();
    sender.remove_plugin(5, "fonts-x").unwrap();
    let batches = decoded(&ctx);
    assert_eq!(batches.len(), 3);
    assert!(batches[0][0].contains("request: 3") && batches[0][0].contains("root: 0"));
    assert!(batches[0][0].contains("flags: 1") && batches[0][0].contains("staging/fonts-x"));
    assert!(batches[1][0].contains("root: 1") && batches[1][0].contains("flags: 2"));
    assert!(batches[2][0].contains("request: 5") && batches[2][0].contains("fonts-x"));
}

#[test]
fn install_and_remove_commands_refuse_bad_arguments_before_the_host() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 1024];
    let mut sender = CommandSender::new(&host, &mut scratch);
    assert_eq!(
        sender.install_plugin(0, InstallRoot::Data, "a", false, false),
        Err(Error::Invalid)
    );
    assert_eq!(
        sender.install_plugin(1, InstallRoot::Data, "", false, false),
        Err(Error::Invalid)
    );
    assert_eq!(sender.remove_plugin(0, "a"), Err(Error::Invalid));
    assert_eq!(sender.remove_plugin(1, ""), Err(Error::Invalid));
    let long = "x".repeat(600);
    assert!(
        sender
            .install_plugin(1, InstallRoot::Data, &long, false, false)
            .is_err()
    );
    assert!(sender.remove_plugin(1, &long).is_err());
    assert!(ctx.batches.borrow().is_empty());
}

#[test]
fn install_and_remove_commands_keep_the_wire_limits() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 1024];
    let mut sender = CommandSender::new(&host, &mut scratch);
    let (path, id) = ("p".repeat(512), "i".repeat(64));
    sender
        .install_plugin(1, InstallRoot::Data, &path, false, false)
        .unwrap();
    sender.remove_plugin(2, &id).unwrap();
    assert!(
        sender
            .install_plugin(3, InstallRoot::Data, &"p".repeat(513), false, false)
            .is_err()
    );
    assert!(sender.remove_plugin(4, &"i".repeat(65)).is_err());
    assert_eq!(ctx.batches.borrow().len(), 2);
}

#[test]
fn firmware_commands_encode_with_their_options_and_key() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 1024];
    let mut sender = CommandSender::new(&host, &mut scratch);
    let key = [7_u8; 32];
    sender
        .install_firmware(
            1,
            InstallRoot::Data,
            "fw/paperpaw.bin",
            FirmwareOptions::new(),
        )
        .unwrap();
    sender
        .install_firmware(
            2,
            InstallRoot::Files,
            "other.bin",
            FirmwareOptions::new().allow_older().foreign(),
        )
        .unwrap();
    sender
        .install_firmware(
            3,
            InstallRoot::Data,
            "own.bin",
            FirmwareOptions::new().key(&key),
        )
        .unwrap();
    sender.firmware_restart().unwrap();
    let mut seen = Vec::new();
    for bytes in ctx.batches.borrow().iter() {
        for command in wire::commands_decode(bytes).unwrap().iter() {
            seen.push(match command {
                wire::Command::InstallFirmware {
                    request,
                    root,
                    path,
                    key,
                    flags,
                } => (request, root, String::from(path), key.to_vec(), flags),
                wire::Command::FirmwareRestart {} => (0, 0, String::new(), Vec::new(), 0),
                other => panic!("{other:?}"),
            });
        }
    }
    assert_eq!(
        seen,
        [
            (1, 0, String::from("fw/paperpaw.bin"), Vec::new(), 0),
            (2, 1, String::from("other.bin"), Vec::new(), 0b11),
            (3, 0, String::from("own.bin"), key.to_vec(), 0),
            (0, 0, String::new(), Vec::new(), 0),
        ]
    );
}

#[test]
fn firmware_install_refuses_bad_arguments_before_the_host() {
    let table = table();
    let mut ctx = Context::default();
    let host = Host {
        table: &table,
        ctx: ptr::from_mut(&mut ctx).cast(),
    };
    let mut scratch = [0; 1024];
    let mut sender = CommandSender::new(&host, &mut scratch);
    let new = FirmwareOptions::new;
    assert_eq!(
        sender.install_firmware(0, InstallRoot::Data, "a", new()),
        Err(Error::Invalid)
    );
    assert_eq!(
        sender.install_firmware(1, InstallRoot::Data, "", new()),
        Err(Error::Invalid)
    );
    assert!(
        sender
            .install_firmware(1, InstallRoot::Data, &"p".repeat(513), new())
            .is_err()
    );
    assert!(ctx.batches.borrow().is_empty());
    // The longest path is the contract's 512 bytes.
    sender
        .install_firmware(1, InstallRoot::Data, &"p".repeat(512), new())
        .unwrap();
    assert_eq!(ctx.batches.borrow().len(), 1);
}
