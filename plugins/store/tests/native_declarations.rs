//! The plugin's native declarations are what the image holds, and the host
//! admits them.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::{declarations::validate_batch, registrations::records_decode};

#[path = "../declarations.rs"]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
use paperpaw_plugin_store::Store;
use paperpaw_sdk::native::declare::Declarations;
use paperpaw_sdk::{Id, Plugin, Registrar};

mod image {
    include!(concat!(env!("OUT_DIR"), "/store_declarations.rs"));
}

type Batches = Vec<Vec<u8>>;

macro_rules! batches_of {
    ($declare:path) => {{
        let mut out: Batches = Vec::new();
        {
            let mut d = Declarations::new(|batch: &[u8]| {
                out.push(batch.to_vec());
                Ok(())
            });
            $declare(&mut d).unwrap();
        }
        out
    }};
}

fn batches() -> Batches {
    batches_of!(declarations::settings)
}

#[test]
fn the_image_is_what_the_builder_makes() {
    let built = batches();
    assert_eq!(built.len(), image::SETTINGS.len());
    for (built, image) in built.iter().zip(image::SETTINGS) {
        assert_eq!(built.as_slice(), *image);
    }
}

#[test]
fn the_host_admits_the_two_settings() {
    for batch in batches() {
        let records = records_decode(&batch).unwrap();
        assert_eq!(records.iter().count(), 2);
        validate_batch("store", &records).unwrap();
    }
}

/// The two settings are the interface-6 registration's: kinds, defaults, the
/// group, the titles and helps, and the guarded flag of the community switch.
#[test]
fn the_decoded_batches_say_what_the_registration_says() {
    // The settings sit in the built-in groups (developer, system), not in a
    // group of the store's own: the registration is read once for each.
    let old = |group: &str| {
        let mut registrar = Registrar::new(Id::new_static("store"));
        Store.register(&mut registrar).unwrap();
        declared::legacy("store", group, registrar.into_contributions())
    };
    let (developer, system) = (old("core.group.developer"), old("core.group.system"));
    let new = declared::native("store", &batches());
    assert_eq!(new.specs.len(), 2);
    let old_specs: Vec<_> = developer
        .specs
        .iter()
        .chain(&system.specs)
        .cloned()
        .collect();
    assert_eq!(new.specs, old_specs);
    let old_extras: Vec<_> = developer
        .extras
        .iter()
        .chain(&system.extras)
        .cloned()
        .collect();
    assert_eq!(new.extras, old_extras);
    // The guard is on the community switch alone.
    let guarded = |line: &String| {
        let flags: u32 = line.split('|').nth(2).unwrap().parse().unwrap();
        flags & 2 != 0
    };
    let on: Vec<_> = new.specs.iter().filter(|l| guarded(l)).collect();
    assert_eq!(on.len(), 1);
    assert!(on[0].starts_with("community|"));
}

#[test]
fn the_action_menu_entries_and_event_are_what_the_image_holds_and_the_host_admits_them() {
    let navigation = batches_of!(declarations::navigation);
    assert_eq!(navigation.len(), image::NAVIGATION.len());
    for (built, image) in navigation.iter().zip(image::NAVIGATION) {
        assert_eq!(built.as_slice(), *image);
        validate_batch("store", &records_decode(built).unwrap()).unwrap();
    }
    let events = batches_of!(declarations::events);
    assert_eq!(events.len(), image::EVENTS.len());
    for (built, image) in events.iter().zip(image::EVENTS) {
        assert_eq!(built.as_slice(), *image);
        validate_batch("store", &records_decode(built).unwrap()).unwrap();
    }
}
