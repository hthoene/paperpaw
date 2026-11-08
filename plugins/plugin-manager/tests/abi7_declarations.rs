//! The plugin's native declarations (screens, actions, menu entries, events)
//! are what the image holds, and the host admits them.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::{
    declarations::{validate_batch, validate_record},
    registrations::{Record, records_decode},
};
use paperpaw_native::host_glue::ERR_UNSUPPORTED;

#[path = "../declarations.rs"]
mod declarations;

mod image {
    include!(concat!(env!("OUT_DIR"), "/plugin_manager_declarations.rs"));
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

/// The host takes the screens, the actions and the events; it checks the
/// menu places and the icons and then refuses them until its menus take them
/// (`screens-input.md` section 1): `make` is refused as unsupported until
/// then, the same state as Keys, Profiles and About (#944).
#[test]
fn the_host_admits_the_screens_the_actions_and_the_events_and_refuses_the_menu_places_and_icons() {
    for batch in batches!(declarations::screens)
        .into_iter()
        .chain(batches!(declarations::events))
    {
        let records = records_decode(&batch).unwrap();
        validate_batch("plugin-manager", &records).unwrap();
    }
    let mut refused = 0;
    for batch in batches!(declarations::navigation) {
        let records = records_decode(&batch).unwrap();
        for record in records.iter() {
            match validate_record("plugin-manager", &record) {
                Ok(()) => assert!(matches!(record, Record::ActionSpec(_)), "{record:?}"),
                Err(status) => {
                    assert_eq!(status, ERR_UNSUPPORTED, "{record:?}");
                    assert!(matches!(record, Record::MenuEntry(_) | Record::IconSpec(_)));
                    refused += 1;
                }
            }
        }
    }
    assert_eq!(refused, 4, "the two menu places and the two icons");
}
