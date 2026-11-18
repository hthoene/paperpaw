//! The native declarations name the three screens, the action, its place in the
//! tools, its icon and the two slots; and the host's rules accept what they
//! declare.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the plugin use the builders and tokens"
)]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
use paperpaw_core::native_host::registrations::{Record, records_decode};
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

/// What the batches say, decoded and checked by the host's rules: the screens,
/// the action, the menu entry, the icon and the two slots.
#[test]
fn the_decoded_batches_say_what_the_plugin_offers() {
    let mut all = Vec::new();
    all.extend(batches(|d| declarations::screens(d).unwrap()));
    all.extend(batches(|d| declarations::cable(d).unwrap()));
    all.extend(batches(|d| declarations::navigation(d).unwrap()));
    let native = declared::native("transfer", &all);
    assert_eq!(native.screens.len(), 3);
    assert_eq!(native.actions, ["open|transfer.title"]);
    assert_eq!(native.menu_entries, ["core.tools|transfer.open|46"]);
    assert_eq!(native.icons, ["transfer.open|transfer"]);
    let mut slots = native.slots.clone();
    slots.sort();
    assert_eq!(
        slots,
        [
            "core.cable|transfer.cable|0",
            "core.remote-keyboard|transfer.phone|0"
        ]
    );
    assert!(native.groups.is_empty() && native.specs.is_empty());
    assert!(native.others.is_empty(), "{:?}", native.others);
}

/// The instance policy of each screen, as the decoded `screen.spec` flags say
/// it (bits 6 and 7: 0 per parameters, 1 single, 2 many): the transfer and the
/// cable open one at a time, the phone's keyboard screen is kept per parameters
/// (stated, not left to the default).
#[test]
fn the_instances_are_stated() {
    let mut new = Vec::new();
    let mut screens = batches(|d| declarations::screens(d).unwrap());
    screens.extend(batches(|d| declarations::cable(d).unwrap()));
    for batch in screens {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::ScreenSpec(s) = record {
                new.push((format!("transfer.{}", s.name), (s.flags >> 6) & 3));
            }
        }
    }
    new.sort();
    assert_eq!(
        new,
        [
            ("transfer.cable".to_string(), 1),
            ("transfer.phone".to_string(), 0),
            ("transfer.screen".to_string(), 1),
        ]
    );
}

/// What `make` submits (the transfer, phone and cable screens, the phone's
/// and the cable's slots, the navigation and the events), decoded as the host
/// does: the cable's screen is offered for `core.cable` (one at a time), and
/// the host opens it when a computer talks over the cable.
#[test]
fn what_is_submitted_offers_the_phone_and_the_cable_and_hears_the_events() {
    let mut all = Vec::new();
    all.extend(batches(|d| declarations::screens(d).unwrap()));
    all.extend(batches(|d| declarations::cable(d).unwrap()));
    all.extend(batches(|d| declarations::navigation(d).unwrap()));
    let native = declared::native("transfer", &all);
    let mut slots = native.slots.clone();
    slots.sort();
    assert_eq!(
        slots,
        [
            "core.cable|transfer.cable|0",
            "core.remote-keyboard|transfer.phone|0"
        ]
    );
    assert_eq!(native.screens.len(), 3);
    assert!(native.others.is_empty(), "{:?}", native.others);
    let events = batches(|d| declarations::events(d).unwrap());
    assert_eq!(events.len(), 1);
    let native = declared::native("transfer", &events);
    let mut tokens: Vec<&str> = native
        .events
        .iter()
        .map(|event| event.rsplit(':').next().unwrap())
        .collect();
    tokens.sort_unstable();
    let mut want = [
        declarations::SERVER_EVENT,
        declarations::SERVED_REQUEST,
        declarations::INSTALL_DONE,
        declarations::WIFI_DONE,
        declarations::WORK,
    ]
    .map(|t| t.to_string());
    want.sort_unstable();
    assert_eq!(tokens, want);
    assert!(native.others.is_empty(), "{:?}", native.others);
}

/// The cable's batch alone (what `make` submits for it) holds the screen, one
/// at a time, and its offer for `core.cable`: the slot the host opens when a
/// computer talks over the cable and no server runs.
#[test]
fn the_cable_batch_is_the_screen_and_its_offer() {
    let cable = batches(|d| declarations::cable(d).unwrap());
    assert_eq!(cable.len(), 1);
    let native = declared::native("transfer", &cable);
    assert_eq!(native.screens.len(), 1);
    assert_eq!(native.slots, ["core.cable|transfer.cable|0"]);
    assert!(native.actions.is_empty() && native.menu_entries.is_empty());
    assert!(native.others.is_empty(), "{:?}", native.others);
    let mut found = Vec::new();
    for record in records_decode(&cable[0]).unwrap().iter() {
        if let Record::ScreenSpec(s) = record {
            found.push((s.name.to_string(), (s.flags >> 6) & 3));
        }
    }
    assert_eq!(found, [("cable".to_string(), 1)]);
}
