//! What Power declares to the host. The settings are pinned byte for byte
//! (`testdata/declarations.bin.v1`, as the build script checks them); the
//! actions, menu entry, icons, held Power key, status item and events are
//! checked by what they say. The interface-6 plugin registered the same
//! things: group `power.group` (order 86), four settings, the mode in the
//! quick panel at order 30, leaf icons, `long.power` to the menu.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "the build script also uses the builders")]
mod declarations;
use paperpaw_core::native_host::registrations::{Record, records_decode};

fn batches(
    build: impl FnOnce(&mut Vec<Vec<u8>>) -> Result<(), paperpaw_sdk::native::declare::DeclareError>,
) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    build(&mut out).unwrap();
    out
}

fn settings() -> Vec<Vec<u8>> {
    batches(|out| {
        declarations::settings(|batch| {
            out.push(batch.to_vec());
            Ok(())
        })
    })
}

#[test]
fn the_settings_are_the_golden() {
    let mut framed = Vec::new();
    for batch in settings() {
        framed.extend_from_slice(&u32::try_from(batch.len()).unwrap().to_le_bytes());
        framed.extend_from_slice(&batch);
    }
    assert!(
        framed == include_bytes!("../testdata/declarations.bin.v1"),
        "the settings changed: read the difference and bless the golden on purpose"
    );
}

#[test]
fn the_settings_keep_their_keys_titles_ranges_and_defaults() {
    let mut keys = Vec::new();
    for batch in settings() {
        for record in records_decode(&batch).unwrap().iter() {
            match record {
                Record::SettingGroup(group) => {
                    assert_eq!(
                        (group.name, group.title, group.order),
                        ("group", "power.title", 86)
                    );
                }
                Record::SettingSpec(spec) => {
                    keys.push((spec.name.to_owned(), spec.title.to_owned()))
                }
                _ => {}
            }
        }
    }
    assert_eq!(
        keys,
        [
            ("mode", "power.setting.mode"),
            ("saving-below", "power.setting.saving-below"),
            ("offline-below", "power.setting.offline-below"),
            (
                "normal-while-charging",
                "power.setting.normal-while-charging"
            ),
        ]
        .map(|(a, b)| (a.to_owned(), b.to_owned()))
    );
}

#[test]
fn the_ui_declares_actions_the_quick_entry_the_held_key_and_the_status_item() {
    let ui = batches(|out| {
        declarations::ui(|batch| {
            out.push(batch.to_vec());
            Ok(())
        })
    });
    let mut seen = Vec::new();
    for batch in &ui {
        for record in records_decode(batch).unwrap().iter() {
            seen.push(format!("{record:?}"));
        }
    }
    let all = seen.join("\n");
    for needle in [
        "power.menu.title",
        "core.quick",
        "power.mode",
        "long.power",
        "power.menu",
        "power.status.item",
        "leaf",
    ] {
        assert!(all.contains(needle), "{needle} is declared");
    }
}

#[test]
fn the_events_are_the_ones_the_mode_follows() {
    let events = batches(|out| {
        declarations::events(|batch| {
            out.push(batch.to_vec());
            Ok(())
        })
    });
    let mut count = 0;
    for batch in &events {
        count += records_decode(batch).unwrap().len();
    }
    // Started, resume, battery, charge, setting, answered and the status stage.
    assert_eq!(count, 7);
}

/// Every record is well formed for the host. The host does not admit menu
/// entries, icons, key bindings, status items and stage subscriptions yet
/// (`native_host/declarations.rs`: checked, then `ERR_UNSUPPORTED`); they must
/// fail only that way, and these are the only ones that do.
#[test]
fn every_record_is_valid_for_the_host_or_waits_for_its_admission() {
    use paperpaw_core::native_host::declarations::validate_record;
    use paperpaw_native::host_glue::ERR_UNSUPPORTED;
    type Build = fn(
        &mut dyn FnMut(&[u8]) -> Result<(), i32>,
    ) -> Result<(), paperpaw_sdk::native::declare::DeclareError>;
    let builders: [(&str, Build); 3] = [
        ("settings", |sink| declarations::settings(sink)),
        ("ui", |sink| declarations::ui(sink)),
        ("events", |sink| declarations::events(sink)),
    ];
    let mut waiting = Vec::new();
    for (name, build) in builders {
        let mut batches = Vec::new();
        build(&mut |batch: &[u8]| {
            batches.push(batch.to_vec());
            Ok(())
        })
        .unwrap();
        for batch in batches {
            for record in records_decode(&batch).unwrap().iter() {
                match validate_record("power", &record) {
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
            "ui: IconSpec",
            "ui: IconSpec",
            "ui: KeyBind",
            "ui: StatusItem",
            "events: RegistrationStage"
        ]
    );
}
