//! The plugin's native declarations are what the image holds, and the host
//! admits them.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::{declarations::validate_batch, registrations::records_decode};

#[path = "../declarations.rs"]
mod declarations;

mod image {
    include!(concat!(env!("OUT_DIR"), "/wikipedia_declarations.rs"));
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
    let screen = batches!(declarations::screen);
    assert_eq!(screen.len(), image::SCREEN.len());
    for (built, image) in screen.iter().zip(image::SCREEN) {
        assert_eq!(built.as_slice(), *image);
    }
    let provider = batches!(declarations::provider);
    assert_eq!(provider.len(), image::PROVIDER.len());
    for (built, image) in provider.iter().zip(image::PROVIDER) {
        assert_eq!(built.as_slice(), *image);
    }
}

#[test]
fn the_host_admits_the_screen_its_slot_and_its_subscription() {
    for batch in batches!(declarations::screen) {
        let records = records_decode(&batch).unwrap();
        // The screen, the slot offer and the subscription to `http_done`.
        assert_eq!(records.iter().count(), 3);
        validate_batch("wikipedia", &records).unwrap();
    }
}

/// The host admits the lookup provider `make` submits (`lookup.spec`).
#[test]
fn the_host_admits_the_lookup_provider() {
    for batch in batches!(declarations::provider) {
        let records = records_decode(&batch).unwrap();
        assert_eq!(records.iter().count(), 1);
        validate_batch("wikipedia", &records).unwrap();
    }
}

#[test]
fn the_host_admits_the_about_subscription_and_the_image_has_it() {
    let built = batches!(declarations::about);
    assert_eq!(built.len(), image::ABOUT.len());
    for (built, image) in built.iter().zip(image::ABOUT) {
        assert_eq!(built.as_slice(), *image);
        let records = records_decode(built).unwrap();
        assert_eq!(records.iter().count(), 1);
        validate_batch("wikipedia", &records).unwrap();
    }
}

/// The schema is a copy of the owner's: `about.lines@1` never changes once
/// published, so the copy cannot drift legitimately.
#[test]
fn the_about_schema_is_byte_for_byte_the_owners() {
    let mine = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/stages/subscribed/about.lines@1.def"
    ))
    .unwrap();
    let owners = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../about/stages/about.lines@1.def"
    ))
    .unwrap();
    assert_eq!(mine, owners);
}
