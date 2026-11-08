//! The plugin's native declarations are what the image holds, and the host
//! admits them.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::{declarations::validate_batch, registrations::records_decode};

#[path = "../declarations.rs"]
mod declarations;

mod image {
    include!(concat!(env!("OUT_DIR"), "/dictionary_declarations.rs"));
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
        (
            "providers",
            batches!(declarations::providers),
            image::PROVIDERS,
        ),
    ] {
        assert_eq!(built.len(), image.len(), "{name}");
        for (built, image) in built.iter().zip(image) {
            assert_eq!(built.as_slice(), *image, "{name}");
        }
    }
}

#[test]
fn the_host_admits_the_setting_and_the_provider() {
    for batch in batches!(declarations::settings)
        .into_iter()
        .chain(batches!(declarations::providers))
    {
        let records = records_decode(&batch).unwrap();
        validate_batch("dictionary", &records).unwrap();
    }
}
