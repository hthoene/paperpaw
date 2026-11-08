//! The native declarations name what the interface-6 registration names (the
//! setting with its choices, default, flags, title, help and section; the
//! panel; its offer for the slot `core.translate`), are what the image
//! holds, and the host admits them.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "tokens the plugin uses later")]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
mod image {
    include!(concat!(env!("OUT_DIR"), "/translate_declarations.rs"));
}
use paperpaw_core::native_host::{declarations::validate_batch, registrations::records_decode};
use paperpaw_plugin_translate::Translate;
use paperpaw_sdk::native::declare::Declarations;
use paperpaw_sdk::{Contribution, Id, Plugin, Registrar};

fn batches(
    declare: impl FnOnce(&mut Declarations<&mut dyn FnMut(&[u8]) -> Result<(), i32>>),
) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut sink = |b: &[u8]| -> Result<(), i32> {
            out.push(b.to_vec());
            Ok(())
        };
        let mut d = Declarations::new(&mut sink as &mut dyn FnMut(&[u8]) -> Result<(), i32>);
        declare(&mut d);
    }
    out
}

fn legacy() -> Vec<Contribution> {
    let mut r = Registrar::new(Id::new_static("translate"));
    Translate.register(&mut r).unwrap();
    r.into_contributions()
}

#[test]
fn the_decoded_batches_say_what_the_registration_says() {
    let mut all = batches(|d| declarations::screen(d).unwrap());
    all.extend(batches(|d| declarations::offer(d).unwrap()));
    let native = declared::native("translate", &all);
    // Without the setting: its choice has 33 values (see below).
    let without_setting = legacy()
        .into_iter()
        .filter(|c| !matches!(c, Contribution::FixedSettings(_)))
        .collect();
    let old = declared::legacy("translate", "core.group.reading", without_setting);
    assert_eq!(native.screens, old.screens);
    assert_eq!(native.screens.len(), 1);
    assert_eq!(native.slots, old.slots);
    assert_eq!(native.slots.len(), 1);
    assert!(
        native.slots[0].starts_with("core.translate|translate.panel"),
        "{:?}",
        native.slots
    );
    assert!(native.others.is_empty(), "{:?}", native.others);
}

/// The language setting is built but not in the image: its choice has 33
/// values and a `setting.spec` takes 32. When this fails because draft 29 raised
/// the limit, emit `SETTINGS` in `build.rs`, submit them in `make`, and compare
/// the setting with the interface-6 one as `the_decoded_batches...` did (the
/// spec `to` with its 33 choices, its help, `advanced`, and the extra with the
/// section `translate.section`).
#[test]
fn the_language_setting_is_refused_until_a_choice_may_have_33_values() {
    let mut out = Vec::new();
    let mut d = Declarations::new(|b: &[u8]| -> Result<(), i32> {
        out.push(b.to_vec());
        Ok(())
    });
    assert_eq!(
        declarations::settings(&mut d),
        Err(paperpaw_sdk::native::declare::DeclareError::Choices)
    );
    assert_eq!(declarations::TARGETS.len(), 33);
}

#[test]
fn the_languages_are_the_interface_6_languages() {
    assert_eq!(
        declarations::LANGUAGES,
        paperpaw_plugin_translate::LANGUAGES
    );
    assert_eq!(declarations::AUTO, paperpaw_plugin_translate::AUTO);
}

#[test]
fn the_image_is_what_the_builders_make() {
    for (built, image) in [
        (batches(|d| declarations::screen(d).unwrap()), image::SCREEN),
        (batches(|d| declarations::offer(d).unwrap()), image::OFFER),
    ] {
        assert_eq!(built.len(), image.len());
        for (built, image) in built.iter().zip(image) {
            assert_eq!(built.as_slice(), *image);
        }
    }
}

#[test]
fn the_host_admits_the_screen() {
    for batch in image::SCREEN {
        let records = records_decode(batch).unwrap();
        assert_eq!(records.iter().count(), 1);
        validate_batch("translate", &records).unwrap();
    }
}

/// The offer is declared but not submitted: the host checks a slot offer and
/// refuses it until it admits registry declarations. When this fails because
/// the host admits it, submit `image::OFFER` in `make`.
#[test]
fn the_host_does_not_admit_a_slot_offer_yet() {
    for batch in image::OFFER {
        let records = records_decode(batch).unwrap();
        assert_eq!(records.iter().count(), 1);
        assert_eq!(
            validate_batch("translate", &records),
            Err(paperpaw_native::host_glue::ERR_UNSUPPORTED)
        );
    }
}
