//! What Setup declares to the host: the screen `setup.guide` (one instance),
//! the slot offer `core.setup` → `setup.guide`, the action `setup.open`, its
//! entry at order 97 in the device's part of the settings with the `wand`
//! icon (as the interface-6 plugin registered), and the five events it follows.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "the build script also uses the builders")]
mod declarations;
use paperpaw_core::native_host::{declarations::validate_record, registrations::records_decode};
use paperpaw_native::host_glue::ERR_UNSUPPORTED;
use paperpaw_sdk::native::declare::DeclareError;

type Build = fn(&mut dyn FnMut(&[u8]) -> Result<(), i32>) -> Result<(), DeclareError>;

fn batches(build: Build) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    build(&mut |batch: &[u8]| {
        out.push(batch.to_vec());
        Ok(())
    })
    .unwrap();
    out
}

const BUILDERS: [(&str, Build); 2] = [
    ("ui", |sink| declarations::ui(sink)),
    ("events", |sink| declarations::events(sink)),
];

#[test]
fn the_ui_names_what_the_interface_6_plugin_registered() {
    let mut text = String::new();
    for batch in batches(BUILDERS[0].1) {
        for record in records_decode(&batch).unwrap().iter() {
            text.push_str(&format!("{record:?}\n"));
        }
    }
    for needle in [
        "setup.title",
        "core.setup",
        "setup.guide",
        "setup.open",
        "core.settings-pages",
        "wand",
    ] {
        assert!(text.contains(needle), "{needle}");
    }
}

/// The host does not admit menu entries and icons yet
/// (`native_host/declarations.rs`: checked, then `ERR_UNSUPPORTED`); they must
/// fail only that way, and these are the only ones that do.
#[test]
fn every_record_is_valid_for_the_host_or_waits_for_its_admission() {
    let mut waiting = Vec::new();
    for (name, build) in BUILDERS {
        for batch in batches(build) {
            for record in records_decode(&batch).unwrap().iter() {
                match validate_record("setup", &record) {
                    Ok(()) => {}
                    Err(ERR_UNSUPPORTED) => {
                        let text = format!("{record:?}");
                        waiting.push(format!("{name}: {}", text.split('(').next().unwrap()));
                    }
                    Err(error) => panic!("{name}: {record:?}: {error}"),
                }
            }
        }
    }
    assert_eq!(waiting, ["ui: MenuEntry", "ui: IconSpec"]);
}

#[test]
fn the_events_are_the_five_the_guide_follows() {
    let mut count = 0;
    for batch in batches(BUILDERS[1].1) {
        count += records_decode(&batch).unwrap().len();
    }
    // Started, Answered, SettingChanged, WifiChanged and RegistryChanged.
    assert_eq!(count, 5);
}
