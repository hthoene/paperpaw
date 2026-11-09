//! The plugin's native declarations are what the image holds, and the host
//! admits them.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::{declarations::validate_batch, registrations::records_decode};

#[path = "../declarations.rs"]
mod declarations;

mod image {
    include!(concat!(env!("OUT_DIR"), "/kosync_declarations.rs"));
}

type Declare = fn(
    &mut paperpaw_sdk::native::declare::Declarations<&mut dyn FnMut(&[u8]) -> Result<(), i32>>,
) -> Result<(), paperpaw_sdk::native::declare::DeclareError>;

fn built(declare: Declare) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    {
        let mut sink = |batch: &[u8]| {
            out.push(batch.to_vec());
            Ok(())
        };
        let mut d = paperpaw_sdk::native::declare::Declarations::new(&mut sink as &mut dyn FnMut(&[u8]) -> Result<(), i32>);
        declare(&mut d).unwrap();
    }
    out
}

fn batches() -> Vec<Vec<u8>> {
    built(declarations::settings)
}

fn all() -> [(Vec<Vec<u8>>, &'static [&'static [u8]]); 4] {
    [
        (built(declarations::settings), image::SETTINGS),
        (built(declarations::screens), image::SCREENS),
        (built(declarations::navigation), image::NAVIGATION),
        (built(declarations::events), image::EVENTS),
    ]
}

#[test]
fn the_image_is_what_the_builder_makes() {
    for (built, image) in all() {
        assert_eq!(built.len(), image.len());
        for (built, image) in built.iter().zip(image) {
            assert_eq!(built.as_slice(), *image);
        }
    }
}

#[test]
fn the_host_admits_every_batch() {
    for (built, _) in all() {
        assert!(!built.is_empty());
        for batch in built {
            validate_batch("kosync", &records_decode(&batch).unwrap()).unwrap();
        }
    }
}

#[test]
fn the_tokens_are_distinct() {
    use declarations::*;
    let mut tokens = [
        ACCOUNT, SYNC_NOW, STARTED, SETTING_CHANGED, BOOK_OPENED, BOOK_CLOSED, PAGE_TURNED,
        SUSPEND, RESUME, WORK_DUE, WORK_ALLOWED, HTTP_DONE, SCREEN_SHOWN,
    ];
    tokens.sort_unstable();
    assert!(tokens.windows(2).all(|w| w[0] != w[1]));
}

#[test]
fn the_host_admits_the_group_and_the_three_settings() {
    for batch in batches() {
        let records = records_decode(&batch).unwrap();
        assert_eq!(records.iter().count(), 4);
        validate_batch("kosync", &records).unwrap();
    }
}
