//! The page's native declarations (screen, action, menu place, icon) are what the
//! image holds, and the host accepts them.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::{
    declarations::{validate_batch, validate_record},
    registrations::{Record, records_decode},
};
use paperpaw_native::host_glue::ERR_UNSUPPORTED;

#[path = "../declarations.rs"]
mod declarations;

mod image {
    include!(concat!(env!("OUT_DIR"), "/about_declarations.rs"));
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
        ("screens", batches!(declarations::screens), image::SCREENS),
        (
            "navigation",
            batches!(declarations::navigation),
            image::NAVIGATION,
        ),
        ("stages", batches!(declarations::stages), image::STAGE_SPECS),
    ] {
        assert_eq!(built.len(), image.len(), "{name}");
        for (built, image) in built.iter().zip(image) {
            assert_eq!(built.as_slice(), *image, "{name}");
        }
    }
}

/// The host takes the screen; it checks the menu place and the icon and
/// then refuses them until its menus take them (`screens-input.md` section
/// 1), so `make` is refused as unsupported until then: the same state as
/// Keys and Profiles, pending #944.
#[test]
fn the_host_admits_the_screen_and_the_action_and_refuses_the_menu_place_and_icon() {
    for batch in batches!(declarations::screens) {
        let records = records_decode(&batch).unwrap();
        validate_batch("about", &records).unwrap();
    }
    let mut refused = 0;
    for batch in batches!(declarations::navigation) {
        let records = records_decode(&batch).unwrap();
        for record in records.iter() {
            match validate_record("about", &record) {
                Ok(()) => assert!(matches!(record, Record::ActionSpec(_)), "{record:?}"),
                Err(status) => {
                    assert_eq!(status, ERR_UNSUPPORTED, "{record:?}");
                    assert!(matches!(record, Record::MenuEntry(_) | Record::IconSpec(_)));
                    refused += 1;
                }
            }
        }
    }
    assert_eq!(refused, 2, "the menu place and the icon");
    // The stage spec is checked, then refused until the host's stages are
    // wired, as the menu place is.
    for batch in batches!(declarations::stages) {
        let records = records_decode(&batch).unwrap();
        for record in records.iter() {
            assert!(matches!(record, Record::StageSpec(_)), "{record:?}");
            assert_eq!(validate_record("about", &record), Err(ERR_UNSUPPORTED));
        }
    }
}
