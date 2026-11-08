//! What Stats declares to the host. The settings are pinned byte for byte
//! (`testdata/declarations.bin.v1`, as the build script checks them); the icon,
//! the events and the stage are checked by what they say. The interface-6
//! plugin registered the same group (`stats.group`, order 25), three settings
//! with the same ranges and defaults, and the chart icon.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "the build script also uses the builders")]
mod declarations;
use paperpaw_core::native_host::registrations::{Record, records_decode};

type Build = fn(
    &mut dyn FnMut(&[u8]) -> Result<(), i32>,
) -> Result<(), paperpaw_sdk::native::declare::DeclareError>;

fn batches(build: Build) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    build(&mut |batch: &[u8]| {
        out.push(batch.to_vec());
        Ok(())
    })
    .unwrap();
    out
}

#[test]
fn the_settings_are_the_golden() {
    let mut framed = Vec::new();
    for batch in batches(|sink| declarations::settings(sink)) {
        framed.extend_from_slice(&u32::try_from(batch.len()).unwrap().to_le_bytes());
        framed.extend_from_slice(&batch);
    }
    assert!(
        framed == include_bytes!("../testdata/declarations.bin.v1"),
        "the settings changed: read the difference and bless the golden on purpose"
    );
}

#[test]
fn the_settings_keep_their_keys_ranges_defaults_and_units() {
    let mut seen = Vec::new();
    for batch in batches(|sink| declarations::settings(sink)) {
        for record in records_decode(&batch).unwrap().iter() {
            match record {
                Record::SettingGroup(group) => {
                    assert_eq!(
                        (group.name, group.title, group.order),
                        ("group", "stats.group", 25)
                    );
                }
                Record::SettingSpec(spec) => seen.push(format!("{spec:?}")),
                Record::SettingExtra(extra) => seen.push(format!("{extra:?}")),
                _ => {}
            }
        }
    }
    let all = seen.join("\n");
    for needle in [
        "enabled",
        "stats.setting.enabled.help",
        "goal",
        "stats.setting.goal",
        "stats.setting.goal.help",
        "stats.goal.20",
        "min-seconds",
        "stats.setting.min",
        "max-seconds",
        "stats.setting.max",
        "stats.unit.seconds",
    ] {
        assert!(all.contains(needle), "{needle} is declared");
    }
}

#[test]
fn the_group_has_the_chart_icon() {
    let mut all = String::new();
    for batch in batches(|sink| declarations::ui(sink)) {
        for record in records_decode(&batch).unwrap().iter() {
            all.push_str(&format!("{record:?}\n"));
        }
    }
    assert!(
        all.contains("stats.group") && all.contains("chart-bar"),
        "{all}"
    );
}

#[test]
fn the_events_are_the_ones_reading_is_recorded_from() {
    let mut count = 0;
    for batch in batches(|sink| declarations::events(sink)) {
        count += records_decode(&batch).unwrap().len();
    }
    // Started, resume, suspend, stopping, page turned, book opened, closed,
    // finished and moved, and the status (the clock becoming known).
    assert_eq!(count, 10);
}

#[test]
fn the_stage_reading_goal_asks_is_subscribed_to() {
    let mut all = String::new();
    for batch in batches(|sink| declarations::stages(sink)) {
        for record in records_decode(&batch).unwrap().iter() {
            all.push_str(&format!("{record:?}\n"));
        }
    }
    assert!(all.contains("reading-goal.read-today"), "{all}");
    assert!(all.contains("RegistrationStage"), "{all}");
}

/// Every record is well formed for the host. The host does not admit icons and stages yet
/// (`native_host/declarations.rs`: checked, then `ERR_UNSUPPORTED`); they must
/// fail only that way, and these are the only ones that do.
#[test]
fn every_record_is_valid_for_the_host_or_waits_for_its_admission() {
    use paperpaw_core::native_host::declarations::validate_record;
    use paperpaw_native::host_glue::ERR_UNSUPPORTED;
    let builders: [(&str, Build); 4] = [
        ("settings", |sink| declarations::settings(sink)),
        ("ui", |sink| declarations::ui(sink)),
        ("events", |sink| declarations::events(sink)),
        ("stages", |sink| declarations::stages(sink)),
    ];
    let mut waiting = Vec::new();
    for (name, build) in builders {
        for batch in batches(build) {
            for record in records_decode(&batch).unwrap().iter() {
                match validate_record("stats", &record) {
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
    // Icons, the menu entries of the action that opens the overview, and the
    // stage subscription wait for the host to admit them; screens, the action and the
    // settings are valid now.
    assert_eq!(
        waiting,
        [
            "ui: IconSpec",
            "ui: MenuEntry",
            "ui: MenuEntry",
            "ui: IconSpec",
            "stages: RegistrationStage"
        ]
    );
}
