//! What Diagnostics declares to the host. The settings are pinned byte for
//! byte (`testdata/declarations.bin.v1`, as the build script checks them); the
//! screens, actions, Tools entry and events are checked by what they say. The
//! interface-6 plugin registered the same: the setting `diagnostics.pages`
//! (1 to 10,000, 20) in the developer group, the screens `diagnostics.screen`
//! (one instance) and `diagnostics.check`, the actions `diagnostics.reset` and
//! `diagnostics.open-check`, the Tools entry at order 99 for firmware, its
//! `list-check` icon and the held Select on the test screen.
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

const BUILDERS: [(&str, Build); 3] = [
    ("settings", |sink| declarations::settings(sink)),
    ("ui", |sink| declarations::ui(sink)),
    ("events", |sink| declarations::events(sink)),
];

#[test]
fn the_settings_are_the_golden() {
    let mut framed = Vec::new();
    for batch in batches(BUILDERS[0].1) {
        framed.extend_from_slice(&u32::try_from(batch.len()).unwrap().to_le_bytes());
        framed.extend_from_slice(&batch);
    }
    assert!(
        framed == include_bytes!("../testdata/declarations.bin.v1"),
        "the settings changed: read the difference and bless the golden on purpose"
    );
}

#[test]
fn the_ui_names_what_the_interface_6_plugin_registered() {
    let mut text = String::new();
    for batch in batches(BUILDERS[1].1) {
        for record in records_decode(&batch).unwrap().iter() {
            text.push_str(&format!("{record:?}\n"));
        }
    }
    for needle in [
        "diagnostics.title",
        "diagnostics.check.title",
        "diagnostics.action.reset",
        "core.tools",
        "diagnostics.open-check",
        "list-check",
        "long.confirm",
        "diagnostics.reset",
    ] {
        assert!(text.contains(needle), "{needle}");
    }
}

/// The host does not admit menu entries, needs, icons and key bindings yet
/// (`native_host/declarations.rs`: checked, then `ERR_UNSUPPORTED`); they must
/// fail only that way, and these are the only ones that do.
#[test]
fn every_record_is_valid_for_the_host_or_waits_for_its_admission() {
    let mut waiting = Vec::new();
    for (name, build) in BUILDERS {
        for batch in batches(build) {
            for record in records_decode(&batch).unwrap().iter() {
                match validate_record("diagnostics", &record) {
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
    assert_eq!(
        waiting,
        [
            "ui: MenuEntry",
            "ui: MenuNeed",
            "ui: IconSpec",
            "ui: KeyBind"
        ]
    );
}

#[test]
fn the_events_are_the_ones_the_check_follows() {
    let mut count = 0;
    for batch in batches(BUILDERS[2].1) {
        count += records_decode(&batch).unwrap().len();
    }
    // Tick, suspend, resume, battery, charge and Wi-Fi.
    assert_eq!(count, 6);
}
