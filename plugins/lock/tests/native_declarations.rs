//! The plugin's native declarations are what the image holds, and the host
//! admits them.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::{declarations::validate_batch, registrations::records_decode};

#[path = "../declarations.rs"]
mod declarations;

mod image {
    include!(concat!(env!("OUT_DIR"), "/lock_declarations.rs"));
}

/// The batches a builder makes.
macro_rules! batches {
    ($build:path) => {{
        let mut out: Vec<Vec<u8>> = Vec::new();
        {
            let mut d = paperpaw_sdk::native::declare::Declarations::new(|batch: &[u8]| {
                out.push(batch.to_vec());
                Ok(())
            });
            $build(&mut d).unwrap();
        }
        out
    }};
}

/// Each group of declarations: its name, what the builder makes, what the image
/// holds and how many records each batch has (the settings: the group, three
/// settings and the kind's editor).
fn groups() -> [(&'static str, Vec<Vec<u8>>, &'static [&'static [u8]], usize); 4] {
    [
        (
            "settings",
            batches!(declarations::settings),
            image::SETTINGS,
            5,
        ),
        (
            "screens",
            batches!(declarations::screens),
            image::SCREENS,
            1,
        ),
        (
            "navigation",
            batches!(declarations::navigation),
            image::NAVIGATION,
            1,
        ),
        ("events", batches!(declarations::events), image::EVENTS, 2),
    ]
}

#[test]
fn the_image_is_what_the_builder_makes() {
    for (name, built, image, _) in groups() {
        assert_eq!(built.len(), image.len(), "{name}");
        for (built, image) in built.iter().zip(image) {
            assert_eq!(built.as_slice(), *image, "{name}");
        }
    }
}

#[test]
fn the_host_admits_the_declarations() {
    for (name, built, _, count) in groups() {
        for batch in built {
            let records = records_decode(&batch).unwrap();
            assert_eq!(records.iter().count(), count, "{name}");
            validate_batch("lock", &records).unwrap();
        }
    }
}
