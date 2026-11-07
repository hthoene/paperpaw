//! The page's native declarations (screen, action, menu place, icon) are what the
//! image holds, and the host accepts them.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::{declarations::validate_batch, registrations::records_decode};

#[path = "../declarations.rs"]
mod declarations;

mod image {
    include!(concat!(env!("OUT_DIR"), "/about_declarations.rs"));
}

fn batches(
    declare: impl Fn(
        &mut paperpaw_sdk::native::declare::Declarations<&mut dyn FnMut(&[u8]) -> Result<(), i32>>,
    ) -> Result<(), paperpaw_sdk::native::declare::DeclareError>,
) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut sink = |batch: &[u8]| -> Result<(), i32> {
        out.push(batch.to_vec());
        Ok(())
    };
    let mut d = paperpaw_sdk::native::declare::Declarations::new(&mut sink as &mut dyn FnMut(_) -> _);
    declare(&mut d).unwrap();
    drop(d);
    out
}

#[test]
fn the_image_is_what_the_builder_makes_and_the_host_accepts_it() {
    for (name, built, image) in [
        ("screens", batches(declarations::screens), image::SCREENS),
        (
            "navigation",
            batches(declarations::navigation),
            image::NAVIGATION,
        ),
    ] {
        assert_eq!(built.len(), image.len(), "{name}");
        for (built, image) in built.iter().zip(image) {
            assert_eq!(built.as_slice(), *image, "{name}");
            let records = records_decode(built).unwrap();
            validate_batch("about", &records).unwrap();
        }
    }
}
