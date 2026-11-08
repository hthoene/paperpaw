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
    include!(concat!(env!("OUT_DIR"), "/files_declarations.rs"));
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
        (
            "settings",
            batches!(declarations::settings),
            image::SETTINGS,
        ),
        ("screens", batches!(declarations::screens), image::SCREENS),
        (
            "navigation",
            batches!(declarations::navigation),
            image::NAVIGATION,
        ),
    ] {
        assert_eq!(built.len(), image.len(), "{name}");
        for (built, image) in built.iter().zip(image) {
            assert_eq!(built.as_slice(), *image, "{name}");
        }
    }
}

/// The host takes the settings, the screens, the action and the events; it
/// checks the icons, the menu needs and the slot offer and then refuses what
/// its menus and slots do not take yet (`screens-input.md` section 1, #944):
/// `make` is refused as unsupported until then, the same state as Keys,
/// Profiles, About and the plugin manager.
#[test]
fn the_host_admits_what_it_can_and_names_what_it_refuses() {
    for batch in batches!(declarations::settings)
        .into_iter()
        .chain(batches!(declarations::screens))
    {
        let records = records_decode(&batch).unwrap();
        validate_batch("files", &records).unwrap();
    }
    let mut refused = Vec::new();
    for batch in batches!(declarations::navigation) {
        let records = records_decode(&batch).unwrap();
        for record in records.iter() {
            match validate_record("files", &record) {
                Ok(()) => assert!(matches!(record, Record::ActionSpec(_)), "{record:?}"),
                Err(status) => {
                    assert_eq!(status, ERR_UNSUPPORTED, "{record:?}");
                    refused.push(format!("{record:?}"));
                }
            }
        }
    }
    assert_eq!(
        refused.len(),
        8,
        "the four icons, the menu need, the menu entry and the two slot offers: {refused:?}"
    );
}
