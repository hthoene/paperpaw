//! The plugin's native declarations (screens, actions, menu entries, events)
//! are what the image holds, and the host admits them.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::{
    declarations::{validate_batch, validate_record},
    registrations::records_decode,
};

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

/// The host takes the screens, the actions, the events, the menu places and
/// the icons (`screens-input.md` section 1).
#[test]
fn the_host_admits_the_screens_the_actions_the_events_the_menu_places_and_the_icons() {
    for batch in batches!(declarations::screens)
        .into_iter()
        .chain(batches!(declarations::events))
    {
        let records = records_decode(&batch).unwrap();
        validate_batch("plugin-manager", &records).unwrap();
    }
    let mut admitted = 0;
    for batch in batches!(declarations::navigation) {
        let records = records_decode(&batch).unwrap();
        for record in records.iter() {
            match validate_record("plugin-manager", &record) {
                Ok(()) => admitted += 1,
                Err(status) => panic!("{record:?}: {status}"),
            }
        }
    }
    // The actions, the two menu places and the two icons.
    assert_eq!(admitted, 6);
}
