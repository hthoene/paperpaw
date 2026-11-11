//! The device calls of the plugin over a scripted `core.device`: the shell
//! refuses a reply buffer smaller than an op's bound (4,096 bytes for every
//! `core.device` op) with `NO_SPACE` before the service runs, as the host did
//! for the plugin's start (the mode worn) and its status item.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::environment::{SNAPSHOT_BYTES, SNAPSHOT_VERSION, SettingValueRef};
use paperpaw_core::native_host::registrations::{
    EventStarted, Record, ServiceDevPower, record_encode_one,
};
use paperpaw_plugin_power::NATIVE_ENTRY;
use paperpaw_scripted_host::{ScriptedHost, event_batch};
use std::cell::Cell;
use std::rc::Rc;

/// The environment the plugin reads: a snapshot with no battery.
fn snapshot(host: &ScriptedHost) {
    let mut bytes = vec![0_u8; SNAPSHOT_BYTES];
    bytes[0..4].copy_from_slice(&SNAPSHOT_VERSION.to_le_bytes());
    bytes[4..8].copy_from_slice(&u32::try_from(SNAPSHOT_BYTES).unwrap().to_le_bytes());
    bytes[32..34].copy_from_slice(&480_u16.to_le_bytes());
    bytes[34..36].copy_from_slice(&800_u16.to_le_bytes());
    bytes[36..38].copy_from_slice(&220_u16.to_le_bytes());
    bytes[42] = 1;
    host.set_env(&bytes);
}

/// `core.device` op `power`.
const DEVICE_POWER: u32 = 12;
/// The token of the plugin's events.
const EVENTS_TOKEN: u32 = 1;

fn power_reply() -> Vec<u8> {
    let mut one = vec![0; 4096];
    let used = record_encode_one(
        &Record::ServiceDevPower(ServiceDevPower {
            level: 0,
            mode: "core.normal",
            flags: 0,
            stretch: 1,
            every_ms: 0,
        }),
        &mut one,
    )
    .unwrap();
    one.truncate(used);
    one
}

#[test]
fn the_mode_worn_is_read_into_a_buffer_the_shell_accepts() {
    let host = ScriptedHost::new();
    let asked = Rc::new(Cell::new(0));
    let seen = asked.clone();
    host.service("core.device", 1, 0, move |op, _| match op {
        DEVICE_POWER => {
            seen.set(seen.get() + 1);
            Ok(power_reply())
        }
        _ => Err(-1),
    });
    snapshot(&host);
    host.set_setting("power.mode", SettingValueRef::Text("power.mode.normal"));
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    let started = event_batch(&Record::EventStarted(EventStarted { now_ms: 0 })).unwrap();
    let run = plugin.event(EVENTS_TOKEN, &started).unwrap();
    // Before the fix the call ended with `ERR_NO_SPACE` (-3): the buffer held
    // 512 bytes, the bound is 4,096.
    assert_eq!(asked.get(), 1, "asked; status {}", run.status);
    assert_eq!(run.status, 0);
    assert_eq!(asked.get(), 1, "the device was asked for the power policy");
}
