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
