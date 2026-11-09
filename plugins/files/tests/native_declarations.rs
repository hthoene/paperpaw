//! The plugin's native declarations (screens, actions, menu entries, events)
//! are what the image holds, and the host admits them.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::{
    declarations::{validate_batch, validate_record},
    registrations::records_decode,
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
        ("events", batches!(declarations::events), image::EVENTS),
        ("help", batches!(declarations::help), image::HELP),
    ] {
        assert_eq!(built.len(), image.len(), "{name}");
        for (built, image) in built.iter().zip(image) {
            assert_eq!(built.as_slice(), *image, "{name}");
        }
    }
}

/// The host takes the settings, the screens, the action, the icons, the menu
/// entry and need, and the slot offers (`screens-input.md` section 1).
#[test]
fn the_host_admits_every_declaration() {
    for batch in batches!(declarations::settings)
        .into_iter()
        .chain(batches!(declarations::screens))
    {
        let records = records_decode(&batch).unwrap();
        validate_batch("files", &records).unwrap();
    }
    let mut refused = Vec::new();
    for batch in batches!(declarations::navigation)
        .into_iter()
        .chain(batches!(declarations::events))
    {
        let records = records_decode(&batch).unwrap();
        for record in records.iter() {
            match validate_record("files", &record) {
                Ok(()) => {}
                Err(status) => {
                    assert_eq!(status, ERR_UNSUPPORTED, "{record:?}");
                    refused.push(format!("{record:?}"));
                }
            }
        }
    }
    assert!(
        refused.is_empty(),
        "the host admits every record now: {refused:?}"
    );
}

/// The library subscribes to its changes (books, status, the walk, a ready
/// cover) and tells how to use its list in five hints.
#[test]
fn the_library_listens_for_changes_and_helps() {
    let events = batches!(declarations::events);
    assert_eq!(events.len(), 1);
    let records = records_decode(&events[0]).unwrap();
    assert_eq!(records.iter().count(), 1);
    // The host takes a help hint when it can (`validate_record`); until then it
    // refuses it as unsupported, and the plugin goes on without hints.
    let hints = batches!(declarations::help);
    for batch in &hints {
        for record in records_decode(batch).unwrap().iter() {
            let admitted = validate_record("files", &record);
            assert!(
                admitted.is_ok() || admitted == Err(ERR_UNSUPPORTED),
                "{admitted:?}"
            );
        }
    }
    let records = records_decode(&hints[0]).unwrap();
    assert_eq!(records.iter().count(), 5);
    assert!(
        format!("{:?}", records.iter().collect::<Vec<_>>()).contains("files.browser"),
        "the hints are of the library's screen"
    );
}
