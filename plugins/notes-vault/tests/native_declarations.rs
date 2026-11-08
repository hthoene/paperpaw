//! The plugin's native declarations are what the image holds, the host admits
//! them, and they say what interface 6 registered.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::{declarations::validate_batch, registrations::records_decode};

#[path = "../declarations.rs"]
mod declarations;

mod image {
    include!(concat!(env!("OUT_DIR"), "/notes_vault_declarations.rs"));
}

fn batches() -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    {
        let mut d = paperpaw_sdk::native::declare::Declarations::new(|batch: &[u8]| {
            out.push(batch.to_vec());
            Ok(())
        });
        declarations::settings(&mut d).unwrap();
    }
    out
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
fn the_host_admits_the_group_the_setting_and_its_editor() {
    for batch in batches() {
        let records = records_decode(&batch).unwrap();
        // The group, the setting and the extras of its editor.
        assert_eq!(records.iter().count(), 3);
        validate_batch("notes-vault", &records).unwrap();
    }
}
