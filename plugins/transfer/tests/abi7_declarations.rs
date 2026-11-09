//! What the plugin declares: the transfer screen, the phone's keyboard screen
//! with its slot, the cable's screen with its slot (built, not submitted), the
//! action that opens the transfer, its place in the tools and its icon, and the
//! events it hears. The batches are checked by the host's rules, read back and
//! compared with what is written here, and pinned byte for byte by a reviewed
//! golden (`PAPERPAW_BLESS=1` writes it anew).
#![allow(clippy::unwrap_used, reason = "test assertions")]
#![allow(
    clippy::redundant_closure,
    reason = "the declaring functions are generic over the sink: a closure fixes it"
)]
#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the plugin use the builders and tokens"
)]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_sdk::native::declare::{Declarations, DeclareError};

fn batches(
    declare: impl FnOnce(
        &mut Declarations<&mut dyn FnMut(&[u8]) -> Result<(), i32>>,
    ) -> Result<(), DeclareError>,
) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut sink = |b: &[u8]| -> Result<(), i32> {
            out.push(b.to_vec());
            Ok(())
        };
        let mut d = Declarations::new(&mut sink as &mut dyn FnMut(&[u8]) -> Result<(), i32>);
        declare(&mut d).unwrap();
    }
    out
}

/// What `make` submits: the screens with the phone's slot, the navigation and
/// the events.
fn submitted() -> Vec<Vec<u8>> {
    let mut all = Vec::new();
    all.extend(batches(|d| declarations::screens(d)));
    all.extend(batches(|d| declarations::navigation(d)));
    all.extend(batches(|d| declarations::events(d)));
    all
}

#[test]
fn the_declarations_are_the_reviewed_golden() {
    let mut all = submitted();
    all.extend(batches(|d| declarations::cable_later(d)));
    let mut framed = Vec::new();
    for batch in all {
        framed.extend_from_slice(&u32::try_from(batch.len()).unwrap().to_le_bytes());
        framed.extend_from_slice(&batch);
    }
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/declarations.bin.v1");
    if std::env::var_os("PAPERPAW_BLESS").is_some() {
        std::fs::write(path, &framed).unwrap();
    }
    assert!(
        framed == std::fs::read(path).unwrap(),
        "the declarations changed: review, then PAPERPAW_BLESS=1 cargo test"
    );
}

/// What the batches say, decoded by the host's rules: the transfer and the
/// phone's screens, the action, the menu entry, the icon and the phone's slot;
/// the cable's screen and slot are built but not submitted.
#[test]
fn the_decoded_batches_say_what_the_plugin_declares() {
    let native = declared::native("transfer", &submitted());
    assert_eq!(
        native.screens,
        ["screen|transfer.title", "phone|transfer.phone.title"]
    );
    assert_eq!(native.actions, ["open|transfer.title"]);
    assert_eq!(native.menu_entries, ["core.tools|transfer.open|46"]);
    assert_eq!(native.icons, ["transfer.open|transfer"]);
    assert_eq!(native.slots, ["core.remote-keyboard|transfer.phone|0"]);
    assert!(native.groups.is_empty() && native.specs.is_empty());
    assert!(native.others.is_empty(), "{:?}", native.others);
    let later = declared::native("transfer", &batches(|d| declarations::cable_later(d)));
    assert_eq!(later.screens, ["cable|transfer.cable.title"]);
    assert_eq!(later.slots, ["core.cable|transfer.cable|0"]);
}

/// The instance policy of each screen, as the decoded `screen.spec` flags say
/// it (bits 6 and 7: 0 per parameters, 1 single, 2 many): the transfer and the
/// cable open one at a time, the phone's keyboard screen is kept per
/// parameters (stated, not left to the default).
#[test]
fn the_instances_are_one_at_a_time_but_the_phones() {
    let mut policies = Vec::new();
    let mut all = submitted();
    all.extend(batches(|d| declarations::cable_later(d)));
    for batch in all {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::ScreenSpec(s) = record {
                policies.push((format!("transfer.{}", s.name), (s.flags >> 6) & 3));
            }
        }
    }
    policies.sort();
    assert_eq!(
        policies,
        [
            ("transfer.cable".to_string(), 1),
            ("transfer.phone".to_string(), 0),
            ("transfer.screen".to_string(), 1),
        ]
    );
}

#[test]
fn the_events_are_the_servers_the_install_the_network_and_the_timer() {
    let events = batches(|d| declarations::events(d));
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
