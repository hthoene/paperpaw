//! What Reading goal declares to the host. The settings are pinned byte for byte
//! (`testdata/declarations.bin.v1`, as the build script checks them); the events
//! and the subscription are checked by what they say. The interface-6 plugin
//! registered the same three settings in the statistics' group (`content` 5 to
//! 95 and `happy` 50 to 200 in percent, steps of 5, and a folder of pictures).
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "the build script also uses the builders")]
mod declarations;
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_sdk::native::declare::Declarations;

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

fn records(build: Build) -> String {
    let mut all = String::new();
    for batch in batches(build) {
        for record in records_decode(&batch).unwrap().iter() {
            all.push_str(&format!("{record:?}\n"));
        }
    }
    all
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
fn the_settings_keep_their_keys_ranges_defaults_and_places() {
    let mut specs = Vec::new();
    let mut extras = Vec::new();
    for batch in batches(|sink| declarations::settings(sink)) {
        for record in records_decode(&batch).unwrap().iter() {
            match record {
                Record::SettingSpec(spec) => specs.push(format!("{spec:?}")),
                Record::SettingExtra(extra) => extras.push(format!("{extra:?}")),
                other => panic!("{other:?}"),
            }
        }
    }
    assert_eq!(specs.len(), 3);
    assert_eq!(extras.len(), 3);
    let all = format!("{specs:?}{extras:?}");
    for needle in [
        "content",
        "reading-goal.setting.content.help",
        "happy",
        "reading-goal.setting.happy.help",
        "pictures",
        "reading-goal.setting.pictures.help",
        "stats.group",
        "core.unit.percent",
        "core.choose-folder",
        "reading-goal.title",
    ] {
        assert!(all.contains(needle), "{needle} is declared: {all}");
    }
    // Ranges and defaults: 5 to 95 from 50, and 50 to 200 from 100.
    assert!(specs[0].contains("8, 0, 0, 0, 50, 0, 0, 0") && specs[0].contains("min: 5, max: 95"));
    assert!(
        specs[1].contains("8, 0, 0, 0, 100, 0, 0, 0") && specs[1].contains("min: 50, max: 200")
    );
    // All three are advanced settings, as on interface 6.
    assert!(specs.iter().all(|s| s.contains("flags: 16")), "{specs:?}");
}

#[test]
fn the_events_are_the_ones_the_figure_is_written_at() {
    let mut count = 0;
    for batch in batches(|sink| declarations::events(sink)) {
        count += records_decode(&batch).unwrap().len();
    }
    // The start, a book opened and closed, suspend and stopping, the minute
    // tick, the end of a picture asked for and a changed setting (the pictures'
    // folder).
    assert_eq!(count, 8);
}

#[test]
fn the_figure_is_asked_by_a_stage_of_its_own() {
    let all = records(|sink| declarations::stages(sink));
    assert!(
        all.contains("StageSpec") && all.contains("read-today"),
        "{all}"
    );
    // A day in (4 bytes), a day and the seconds out (8), one reply.
    assert!(
        all.contains("max_input: 4") && all.contains("max_reply: 8"),
        "{all}"
    );
    assert!(all.contains("max_records: 1"), "{all}");
}

/// Every record is well formed for the host, and the host admits every one of
/// them: none is refused as unsupported.
#[test]
fn every_record_is_valid_for_the_host_and_admitted() {
    use paperpaw_core::native_host::declarations::validate_record;
    use paperpaw_native::host_glue::ERR_UNSUPPORTED;
    let builders: [(&str, Build); 5] = [
        ("settings", |sink| declarations::settings(sink)),
        ("events", |sink| declarations::events(sink)),
        ("stages", |sink| declarations::stages(sink)),
        ("screens", |sink| {
            declarations::screens(&mut Declarations::new(sink))
        }),
        ("navigation", |sink| {
            declarations::navigation(&mut Declarations::new(sink))
        }),
    ];
    let mut waiting = Vec::new();
    for (name, build) in builders {
        for batch in batches(build) {
            for record in records_decode(&batch).unwrap().iter() {
                match validate_record("reading-goal", &record) {
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
    assert!(
        waiting.is_empty(),
        "the host admits every record now: {waiting:?}"
    );
}

#[test]
fn the_sleep_screen_is_offered_below_the_sleep_plugins_own_and_chosen_by_sleep_show() {
    let screens = records(|sink| declarations::screens(&mut Declarations::new(sink)));
    assert!(
        screens.contains("reading-goal.sleep") && screens.contains("10"),
        "{screens}"
    );
    let all = records(|sink| declarations::navigation(&mut Declarations::new(sink)));
    // The sleep plugin offers its own screen at priority 0.
    assert!(
        all.contains("SlotOffer") && all.contains("core.sleep") && all.contains("priority: -1"),
        "{all}"
    );
    assert!(
        all.contains("SettingChoice")
            && all.contains("sleep.show")
            && all.contains("reading-goal.sleep"),
        "{all}"
    );
}
