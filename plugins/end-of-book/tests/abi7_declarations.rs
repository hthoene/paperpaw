//! The plugin's native declarations (switches, screen, book events) are what
//! the image holds, and the host accepts the settings and the screen. The
//! event subscriptions are checked here as well.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::{declarations::validate_batch, registrations::records_decode};

#[path = "../declarations.rs"]
mod declarations;

mod image {
    include!(concat!(env!("OUT_DIR"), "/end_of_book_declarations.rs"));
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
        ("events", batches!(declarations::events), image::EVENTS),
    ] {
        assert_eq!(built.len(), image.len(), "{name}");
        for (built, image) in built.iter().zip(image) {
            assert_eq!(built.as_slice(), *image, "{name}");
        }
    }
}

#[test]
fn the_host_admits_the_settings_and_the_screen() {
    for batch in batches!(declarations::settings)
        .into_iter()
        .chain(batches!(declarations::screens))
    {
        let records = records_decode(&batch).unwrap();
        validate_batch("end-of-book", &records).unwrap();
    }
}

#[test]
fn the_events_are_the_end_and_the_closing_of_a_book() {
    let batch = &batches!(declarations::events)[0];
    let records = records_decode(batch).unwrap();
    assert_eq!(records.iter().count(), 2);
    validate_batch("end-of-book", &records).unwrap();
}
