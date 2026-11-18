//! The native declarations are what the interface-6 registration declared: the
//! group and its settings, the screens, the action, the status item, the
//! icons, the needs and the first-run step. Callbacks and the radio's own
//! events are a later slice (`PORTING-7.md`).
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "tokens the plugin uses later")]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
use paperpaw_sdk::native::declare::Declarations;

fn batches(
    declare: impl FnOnce(&mut Declarations<&mut dyn FnMut(&[u8]) -> Result<(), i32>>),
) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut sink = |b: &[u8]| -> Result<(), i32> {
            out.push(b.to_vec());
            Ok(())
        };
        let mut d = Declarations::new(&mut sink as &mut dyn FnMut(&[u8]) -> Result<(), i32>);
        declare(&mut d);
    }
    out
}

/// The subscriptions, decoded: the radio's own events 92-95 go to their
/// callbacks, with the four of the device's the plugin had before.
#[test]
fn the_decoded_subscriptions_name_every_event_the_plugin_handles() {
    use paperpaw_core::native_host::registrations::{Record, Registration, records_decode};
    let mut found = Vec::new();
    for batch in batches(|d| declarations::events(d).unwrap()) {
        for record in records_decode(&batch).unwrap().iter() {
            match record {
                Record::Registration(Registration::Event { event_tag, token }) => {
                    found.push((event_tag, token));
                }
                other => panic!("{other:?}"),
            }
        }
    }
    // The device's four (not `screen_shown`: 38 goes to the declared screens' tokens, `screens-input.md` 2), then the radio's four: scanned 92, done 93,
    // network wanted 94 and idle 95 (`docs/abi/device.md`), then the work's and the served page's (server, request),
    // then the stack's change (81: any screen on top, back home or in a book the radio goes off).
    let tokens: Vec<u32> = found.iter().map(|(_, token)| *token).collect();
    assert_eq!(
        tokens,
        [
            declarations::STARTED,
            declarations::STATUS_CHANGED,
            declarations::WIFI_CHANGED,
            declarations::POWER_CHANGED,
            declarations::WIFI_SCANNED,
            declarations::WIFI_DONE,
            declarations::NETWORK_WANTED,
            declarations::NETWORK_IDLE,
            declarations::WORK,
            declarations::SERVER_EVENT,
            declarations::SERVED_REQUEST,
            declarations::STACK_CHANGED,
        ]
    );
    let radio: Vec<u32> = found[4..8].iter().map(|(tag, _)| *tag).collect();
    assert_eq!(radio, [92, 93, 94, 95]);
    // The timers (idle off, retry) come due as work (`event.work_due`).
    assert_eq!(found[8].0, 103, "work_due");
    assert_eq!(found[0].0, 32, "started");
    assert_eq!(found[11].0, 81, "stack_changed");
}

/// What the interface-6 registration declared, fixed when its code went: the
/// group and its four settings, the three single screens, the action, the
/// status item, the icons, the `Wifi` needs of the settings and the first-run
/// step. Every record passes the host's rules (`declared::native` checks it).
#[test]
fn the_declarations_are_what_the_interface_6_registration_declared() {
    let mut all = Vec::new();
    all.extend(batches(|d| declarations::settings(d).unwrap()));
    all.extend(batches(|d| declarations::screens(d).unwrap()));
    all.extend(batches(|d| declarations::navigation(d).unwrap()));
    all.extend(batches(|d| declarations::events(d).unwrap()));
    let lines = declared::native("wifi", &all);
    assert_eq!(lines.groups, ["group|wifi.title|85|page|"]);
    let idle: Vec<&str> = declarations::IDLE_OFF.to_vec();
    assert_eq!(idle.len(), 5);
    assert_eq!(
        lines.specs,
        [
            "on-demand|0|0|wifi.group|wifi.setting.on-demand|wifi.setting.on-demand.help|b:1|0|0|\"\"",
            "idle-off|2|0|wifi.group|wifi.setting.idle-off|wifi.setting.idle-off.help|t:wifi.idle-off.1m|0|0|\"wifi.idle-off.30s\\nwifi.idle-off.1m\\nwifi.idle-off.2m\\nwifi.idle-off.5m\\nwifi.idle-off.10m\"",
            "while-charging|0|0|wifi.group|wifi.setting.while-charging|wifi.setting.while-charging.help|b:0|0|0|\"\"",
            "join-first|2|0|wifi.group|wifi.setting.join-first|wifi.setting.join-first.help|t:wifi.join-first.picked|0|0|\"wifi.join-first.picked\\nwifi.join-first.strongest\"",
        ]
    );
    assert_eq!(
        lines.screens,
        [
            "page|wifi.title",
            "networks|wifi.networks.title",
            "phone|wifi.phone.title"
        ]
    );
    assert_eq!(lines.actions, ["open|wifi.title"]);
    assert_eq!(lines.icons, ["wifi.group|wifi", "wifi.open|wifi"]);
    assert_eq!(
        lines.needs,
        [
            "wifi.idle-off|Wifi",
            "wifi.join-first|Wifi",
            "wifi.on-demand|Wifi",
            "wifi.while-charging|Wifi",
        ]
    );
    // The status item (`wifi.status`, tap `wifi.open`) and the first-run step
    // (`wifi.setup`, order 50, `wifi.setup.help`) are records of their own.
    assert_eq!(lines.others, ["StatusItem", "SetupStep"]);
    assert_eq!(batches(|d| declarations::settings(d).unwrap()).len(), 1);
    assert_eq!(batches(|d| declarations::screens(d).unwrap()).len(), 1);
    assert!(!batches(|d| declarations::navigation(d).unwrap()).is_empty());
    // Twelve subscriptions (the radio's four, the timers' work and the stack's
    // change among them) in two batches.
    assert_eq!(batches(|d| declarations::events(d).unwrap()).len(), 2);
}
