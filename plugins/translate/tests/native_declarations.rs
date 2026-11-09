//! The native declarations name what the interface-6 registration named (the
//! setting with its choices, default, flags, title, help and section; the
//! panel; its offer for the slot `core.translate`; pinned here), are what the
//! image holds, and the host admits them.
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

use paperpaw_sdk::native::declare::Declarations;

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

#[test]
fn the_decoded_batches_say_what_interface_6_registered() {
    let mut all = batches(|d| declarations::settings(d).unwrap());
    all.extend(batches(|d| declarations::screen(d).unwrap()));
    all.extend(batches(|d| declarations::events(d).unwrap()));
    all.extend(batches(|d| declarations::offer(d).unwrap()));
    let native = declared::native("translate", &all);
    // What the interface-6 plugin registered (pinned from it before it was
    // removed): the language setting, a choice of the reader's own language and
    // the 32 of DeepL, advanced, in the reading group, under the section.
    assert!(native.groups.is_empty());
    let choices: Vec<&str> = std::iter::once(declarations::AUTO)
        .chain(declarations::LANGUAGES)
        .collect();
    let joined = choices.join("\n");
    assert_eq!(
        native.specs,
        [format!(
            "to|2|16|core.group.reading|translate.setting.to|translate.setting.to.help|t:translate.to.auto|0|0|{joined:?}"
        )]
    );
    assert_eq!(native.extras, ["to||translate.section||=|0:|0|0"]);
    assert_eq!(native.screens, ["panel|translate.title"]);
    assert_eq!(native.slots, ["core.translate|translate.panel|0"]);
    assert_eq!(native.slots.len(), 1);
    assert!(
        native.slots[0].starts_with("core.translate|translate.panel"),
        "{:?}",
        native.slots
    );
    assert!(native.others.is_empty(), "{:?}", native.others);
}

/// The language setting is a choice of 33 values, within the 64 a
/// `setting.spec` takes since draft 29.
#[test]
fn the_language_setting_has_its_33_values_and_the_host_admits_it() {
    assert_eq!(declarations::TARGETS.len(), 33);
    for batch in image::SETTINGS {
        let records = records_decode(batch).unwrap();
        assert_eq!(validate_batch("translate", &records), Ok(()));
    }
}

#[test]
fn the_image_is_what_the_builders_make() {
    for (built, image) in [
        (
            batches(|d| declarations::settings(d).unwrap()),
            image::SETTINGS,
        ),
        (batches(|d| declarations::screen(d).unwrap()), image::SCREEN),
        (batches(|d| declarations::events(d).unwrap()), image::EVENTS),
        (batches(|d| declarations::offer(d).unwrap()), image::OFFER),
    ] {
        assert_eq!(built.len(), image.len());
        for (built, image) in built.iter().zip(image) {
            assert_eq!(built.as_slice(), *image);
        }
    }
}

#[test]
fn the_host_admits_the_screen_and_the_subscription_to_the_end_of_a_request() {
    for batch in image::SCREEN.iter().chain(image::EVENTS) {
        let records = records_decode(batch).unwrap();
        assert_eq!(records.iter().count(), 1);
        validate_batch("translate", &records).unwrap();
    }
}

/// The host admits the slot offer; `make` submits `image::OFFER` after the
/// screen (`tests/native_asking.rs`).
#[test]
fn the_host_admits_a_slot_offer() {
    for batch in image::OFFER {
        let records = records_decode(batch).unwrap();
        assert_eq!(records.iter().count(), 1);
        assert_eq!(validate_batch("translate", &records), Ok(()));
    }
}
