//! The plugin's native declarations are what the image holds, and the host
//! admits them.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::{
    declarations::{validate_batch, validate_record},
    registrations::records_decode,
};

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

/// The host takes the setting and the lookup provider (`document.md` section
/// 7: `lookup.spec` admission); nothing of these is refused.
#[test]
fn the_host_admits_the_setting_and_the_lookup_provider() {
    for batch in batches!(declarations::settings) {
        let records = records_decode(&batch).unwrap();
        validate_batch("dictionary", &records).unwrap();
    }
    for batch in batches!(declarations::providers) {
        let records = records_decode(&batch).unwrap();
        let mut providers = 0;
        for record in records.iter() {
            validate_record("dictionary", &record).unwrap();
            if !format!("{record:?}").starts_with("Registration(Event") {
                providers += 1;
            }
        }
        assert_eq!(providers, 1, "the provider");
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

/// The panel hears when a provider that answers later kept its answer
/// (`event.lookup_ready`, 110, `document.md` 7; todo 98): its screens' batch
/// subscribes the panel's token to it.
#[test]
fn the_panel_hears_when_a_provider_kept_its_answer() {
    use paperpaw_core::native_host::registrations::{Record, Registration};
    /// `event.lookup_ready` (`abi/abi.def`).
    const EVENT_LOOKUP_READY_TAG: u32 = 110;
    let mut found = Vec::new();
    for batch in batches!(declarations::screens) {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::Registration(Registration::Event { event_tag, token }) = record {
                found.push((event_tag, token));
            }
        }
    }
    assert_eq!(
        found,
        [(EVENT_LOOKUP_READY_TAG, declarations::POPUP_TOKEN)],
        "{found:?}"
    );
}
