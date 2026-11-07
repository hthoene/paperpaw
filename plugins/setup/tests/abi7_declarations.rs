//! The native declarations (screens, navigation, events) are what the image holds,
//! and the host takes them as far as it has wired them.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::{
    declarations::{validate_batch, validate_record},
    registrations::records_decode,
};
use paperpaw_native::host_glue::ERR_UNSUPPORTED;

#[path = "../declarations.rs"]
mod declarations;

mod image {
    include!(concat!(env!("OUT_DIR"), "/setup_declarations.rs"));
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
        ("slots", batches!(declarations::slots), image::SLOTS),
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

/// The host takes the screens; the other families it refuses as unsupported
/// until it wires them (`screens-input.md` section 1), so `make` is refused
/// until then: the same state as the other native plugins, pending #944.
#[test]
fn the_host_admits_the_screens_and_refuses_the_rest_as_unsupported() {
    for batch in batches!(declarations::screens) {
        let records = records_decode(&batch).unwrap();
        validate_batch("setup", &records).unwrap();
    }
    for batch in batches!(declarations::slots)
        .into_iter()
        .chain(batches!(declarations::navigation))
        .chain(batches!(declarations::events))
    {
        let records = records_decode(&batch).unwrap();
        for record in records.iter() {
            if let Err(status) = validate_record("setup", &record) {
                assert_eq!(status, ERR_UNSUPPORTED, "{record:?}");
            }
        }
    }
}
