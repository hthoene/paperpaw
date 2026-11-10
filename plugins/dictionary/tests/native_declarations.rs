//! The plugin's native declarations are what the image holds, and the host
//! admits them.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::{
    declarations::{validate_batch, validate_record},
    registrations::records_decode,
};
use paperpaw_native::host_glue::ERR_UNSUPPORTED;

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
        ("screens", batches!(declarations::screens), image::SCREENS),
    ] {
        assert_eq!(built.len(), image.len(), "{name}");
        for (built, image) in built.iter().zip(image) {
            assert_eq!(built.as_slice(), *image, "{name}");
        }
    }
}

/// The host takes the setting; it refuses the lookup provider as unsupported
/// until its lookup broker admits providers (`document.md` section 7): `make`
/// is refused the same way until then, as for the other ports.
#[test]
fn the_host_admits_the_setting_and_names_what_it_refuses() {
    for batch in batches!(declarations::settings) {
        let records = records_decode(&batch).unwrap();
        validate_batch("dictionary", &records).unwrap();
    }
    for batch in batches!(declarations::providers) {
        let records = records_decode(&batch).unwrap();
        let mut refused = 0;
        for record in records.iter() {
            // The work event is admitted; only the provider is refused.
            if format!("{record:?}").starts_with("Registration(Event") {
                validate_record("dictionary", &record).unwrap();
                continue;
            }
            assert_eq!(
                validate_record("dictionary", &record),
                Err(ERR_UNSUPPORTED),
                "{record:?}"
            );
            refused += 1;
        }
        assert_eq!(refused, 1, "the provider");
    }
}

/// What the host says of the panel's screen and of its offer of `core.lookup`
/// is printed by the first failing assertion; the screen itself is admitted.
#[test]
fn the_host_admits_the_panel_screen() {
    for batch in batches!(declarations::screens) {
        let records = records_decode(&batch).unwrap();
        validate_batch("dictionary", &records).unwrap();
    }
}
