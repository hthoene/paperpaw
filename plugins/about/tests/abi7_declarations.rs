//! The page's native declarations (screen, action, menu place, icon) are what the
//! image holds, and the host accepts them.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::{
    declarations::validate_batch,
    registrations::{Record, records_decode},
};

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

/// The host admits every declaration the page makes: the screen, the action,
/// the menu place, the icon (`screens-input.md` section 1) and the stage
/// (`stages-services.md` section 1).
#[test]
fn the_host_admits_every_declaration_of_the_page() {
    for batch in batches!(declarations::screens) {
        let records = records_decode(&batch).unwrap();
        validate_batch("about", &records).unwrap();
    }
    let (mut actions, mut places, mut icons) = (0, 0, 0);
    for batch in batches!(declarations::navigation) {
        let records = records_decode(&batch).unwrap();
        validate_batch("about", &records).unwrap();
        for record in records.iter() {
            match record {
                Record::ActionSpec(_) => actions += 1,
                Record::MenuEntry(_) => places += 1,
                Record::IconSpec(_) => icons += 1,
                other => panic!("an unexpected record: {other:?}"),
            }
        }
    }
    assert_eq!((actions, places, icons), (1, 1, 1));
    let mut stages = 0;
    for batch in batches!(declarations::stages) {
        let records = records_decode(&batch).unwrap();
        validate_batch("about", &records).unwrap();
        for record in records.iter() {
            assert!(matches!(record, Record::StageSpec(_)), "{record:?}");
            stages += 1;
        }
    }
    assert_eq!(stages, 1);
}
