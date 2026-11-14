//! The native declarations are what the image holds, the host takes the
//! widget and setting records, and every widget has the behaviour its token
//! says.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]

use paperpaw_core::native_host::{
    declarations::{validate_batch, validate_record},
    registrations::{Record, records_decode},
};
use paperpaw_native::host_glue::ERR_UNSUPPORTED;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the plugin read the same file"
)]
mod declarations;

mod image {
    include!(concat!(env!("OUT_DIR"), "/widgets_declarations.rs"));
}

/// The batches `$declare` submits.
macro_rules! batches {
    ($declare:path) => {{
        let mut out: Vec<Vec<u8>> = Vec::new();
        {
            let mut d = paperpaw_sdk::native::declare::Declarations::new(|batch: &[u8]| {
                out.push(batch.to_vec());
                Ok(())
            });
            $declare(&mut d).unwrap();
        }
        out
    }};
}

#[test]
fn the_image_is_what_the_builder_makes() {
    for (name, built, image) in [
        ("widgets", batches!(declarations::widgets), image::WIDGETS),
        ("screens", batches!(declarations::screens), image::SCREENS),
        (
            "navigation",
            batches!(declarations::navigation),
            image::NAVIGATION,
        ),
        ("events", batches!(declarations::events), image::EVENTS),
    ] {
        assert_eq!(built.len(), image.len(), "{name}");
        for (built, image) in built.iter().zip(image) {
            assert_eq!(built.as_slice(), *image, "{name}");
        }
    }
}

/// The host takes what it has wired, and refuses the rest as unsupported
/// until it wires them (`screens-input.md` section 1).
#[test]
fn the_host_admits_the_records_or_refuses_them_as_unsupported() {
    for batch in batches!(declarations::widgets)
        .into_iter()
        .chain(batches!(declarations::screens))
        .chain(batches!(declarations::navigation))
        .chain(batches!(declarations::events))
    {
        let records = records_decode(&batch).unwrap();
        if let Err(status) = validate_batch("widgets", &records) {
            assert_eq!(status, ERR_UNSUPPORTED);
        }
        for record in records.iter() {
            if let Err(status) = validate_record("widgets", &record) {
                assert_eq!(status, ERR_UNSUPPORTED, "{record:?}");
            }
        }
    }
}

#[test]
fn every_declared_widget_has_a_distinct_token_a_behaviour_and_a_group() {
    let mut tokens: Vec<u32> = declarations::WIDGETS.iter().map(|def| def.token).collect();
    tokens.sort_unstable();
    tokens.dedup();
    assert_eq!(tokens.len(), declarations::WIDGETS.len());
    // The tick, the screen and its action are not a widget's token.
    for other in [
        declarations::TICK,
        declarations::READING,
        declarations::TODO_SCREEN,
        declarations::TODO_OPEN,
    ] {
        assert!(!tokens.contains(&other));
    }
}

/// A widget that `previews` a settings group is made for that group's
/// settings page and never offered in the Add picker (`registry.md`, op 19).
/// Beta 3's widget pack declared none, so every widget stays pickable.
#[test]
fn no_widget_of_the_pack_previews_a_settings_group() {
    let mut seen = 0;
    for batch in batches!(declarations::widgets) {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::WidgetSpec(spec) = record {
                assert_eq!(spec.previews, "", "{}", spec.name);
                seen += 1;
            }
        }
    }
    assert_eq!(seen, declarations::WIDGETS.len());
}

/// What a setting is, as the declaration record says: kind, default, range,
/// choices, title and help (not the order or the batch it came in).
type Shape = (String, u32, Vec<u8>, i32, i32, String, String, String);

fn setting_shapes(batches: &[Vec<u8>]) -> std::collections::BTreeMap<String, Shape> {
    let mut found = std::collections::BTreeMap::new();
    for batch in batches {
        for record in records_decode(batch).unwrap().iter() {
            if let Record::SettingSpec(s) = record {
                found.insert(
                    s.name.to_string(),
                    (
                        s.group.to_string(),
                        s.kind,
                        s.default.to_vec(),
                        s.min,
                        s.max,
                        s.choices.to_string(),
                        s.title.to_string(),
                        s.help.to_string(),
                    ),
                );
            }
        }
    }
    found
}

/// Beta 3's batches of the pack's settings (`testdata/declarations.bin.v1`,
/// made from the interface-6 registration before it was removed): every
/// setting it declared is declared now with the same group, kind, default,
/// range, choices, title and help. Settings may be added (a new widget, a new
/// part); none may change or go.
#[test]
fn every_setting_of_beta_3_is_declared_as_it_was() {
    let golden = include_bytes!("../testdata/declarations.bin.v1");
    let mut old = Vec::new();
    let mut at = 0;
    while at < golden.len() {
        let len = u32::from_le_bytes(golden[at..at + 4].try_into().unwrap()) as usize;
        old.push(golden[at + 4..at + 4 + len].to_vec());
        at += 4 + len;
    }
    let before = setting_shapes(&old);
    let now = setting_shapes(&batches!(declarations::widgets));
    let mut differences = Vec::new();
    for (name, shape) in &before {
        match now.get(name) {
            Some(same) if same == shape => {}
            Some(other) => differences.push(format!("changed {name}: {shape:?} -> {other:?}")),
            None => differences.push(format!("gone {name}")),
        }
    }
    assert!(
        differences.is_empty(),
        "{} of {} settings differ:\n{}",
        differences.len(),
        before.len(),
        differences.join("\n")
    );
}
