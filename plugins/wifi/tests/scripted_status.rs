//! The status item over a scripted `core.device`: the host asks the plugin
//! which of its items show (`core.status`), and the plugin answers from where
//! the radio is. Before this the plugin declared the item and answered no
//! call, so every ask ended with `ERR_INVALID` in the background.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{
    Record, ServiceDevWifi, StageCall, record_encode_one,
};
use paperpaw_plugin_wifi::NATIVE_ENTRY;
use paperpaw_scripted_host::{ScriptedHost, event_batch};
use paperpaw_sdk::native::stage_schemas::core_status_v1 as status;
use std::cell::RefCell;
use std::rc::Rc;

/// `core.device` op `wifi`.
const DEVICE_WIFI: u32 = 8;
/// The token of the status item.
const STATUS: u32 = 2;
const OFF: u32 = 0;
const ONLINE: u32 = 3;

fn wifi_reply(state: u32) -> Vec<u8> {
    let mut one = vec![0; 4096];
    let used = record_encode_one(
        &Record::ServiceDevWifi(ServiceDevWifi {
            state,
            ssid: if state == ONLINE { "Home" } else { "" },
            address: if state == ONLINE { "192.168.1.7" } else { "" },
            host_name: "reader",
            guests: 0,
            flags: 0,
        }),
        &mut one,
    )
    .unwrap();
    one.truncate(used);
    one
}

/// The ask of the host for the item of the plugin, as a `core.status` call.
fn ask() -> Vec<u8> {
    let mut payload = [0; 1024];
    let used = status::input_encode(
        &[status::Record::StatusAsk(status::StatusAsk::new(
            "wifi.status",
            &[],
        ))],
        &mut payload,
    )
    .unwrap();
    event_batch(&Record::StageCall(StageCall {
        stage: "core.status",
        schema: 1,
        payload: &payload[..used],
    }))
    .unwrap()
}

fn run(state: u32) -> (i32, usize) {
    let host = ScriptedHost::new();
    host.add_catalogs_in(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"))
        .unwrap();
    let asked = Rc::new(RefCell::new(0));
    let seen = asked.clone();
    host.service("core.device", 1, 0, move |op, _| match op {
        DEVICE_WIFI => {
            *seen.borrow_mut() += 1;
            Ok(wifi_reply(state))
        }
        _ => Err(-1),
    });
    host.service("core.ui", 1, 0, |_, _| Ok(Vec::new()));
    let plugin = host.load(NATIVE_ENTRY.entry()).unwrap();
    let call = plugin.event(STATUS, &ask()).unwrap();
    let asks = *asked.borrow();
    (call.status, asks)
}

/// `ERR_NO_SPACE`: the scripted host lends an event no reply buffer, so the
/// answer, built from where the radio is, has nowhere to go here.
const NO_REPLY_BUFFER: i32 = -3;

#[test]
fn the_status_item_asks_where_the_radio_is_and_builds_its_answer() {
    for state in [OFF, ONLINE] {
        let (status, asks) = run(state);
        // Before, the plugin answered no ask (`ERR_INVALID` at the host) and
        // never read the radio.
        assert_eq!(asks, 1, "radio state {state}: the radio is read once");
        assert!(
            status == 0 || status == NO_REPLY_BUFFER,
            "radio state {state}: {status}"
        );
    }
}
