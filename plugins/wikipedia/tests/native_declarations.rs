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

/// The provider is declared but not submitted: the host refuses a lookup
/// provider until the stage `core.lookup` is live. When this fails because
/// the host admits it, submit `image::PROVIDER` in `make` and make the case
/// `the-provider-answers-the-dictionary-panel-with-the-summary` real.
#[test]
fn the_host_does_not_admit_a_lookup_provider_yet() {
    for batch in batches!(declarations::provider) {
        let records = records_decode(&batch).unwrap();
        assert_eq!(records.iter().count(), 1);
        assert_eq!(
            validate_batch("wikipedia", &records),
            Err(paperpaw_native::host_glue::ERR_UNSUPPORTED)
        );
    }
}
