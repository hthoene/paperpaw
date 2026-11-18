//! The plugin's native declaration is what the image holds, and says what the
//! provider does (`docs/abi/document.md` section 1).
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::{
    declarations::validate_record,
    registrations::{Record, records_decode},
};

#[path = "../declarations.rs"]
mod declarations;

mod image {
    include!(concat!(env!("OUT_DIR"), "/format_cbz_declarations.rs"));
}

fn batches() -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    {
        let mut d = paperpaw_sdk::native::declare::Declarations::new(|batch: &[u8]| {
            out.push(batch.to_vec());
            Ok(())
        });
        declarations::formats(&mut d).unwrap();
    }
    out
}

#[test]
fn the_image_is_what_the_builder_makes() {
    let built = batches();
    assert_eq!(built.len(), image::FORMATS.len());
    for (built, image) in built.iter().zip(image::FORMATS) {
        assert_eq!(built.as_slice(), *image);
    }
}

#[test]
fn one_format_claims_cbz() {
    let built = batches();
    let [batch] = built.as_slice() else {
        panic!("one batch");
    };
    let records = records_decode(batch).unwrap();
    let rows: Vec<_> = records.iter().collect();
    let [Record::FormatSpec(spec)] = rows.as_slice() else {
        panic!("one format: {rows:?}");
    };
    assert_eq!(
        (spec.name, spec.title, spec.extensions),
        ("cbz", "format-cbz.title", "cbz")
    );
    // Not `prepared`; flags: whole pages (each section is a picture).
    assert_eq!(spec.flags, 0b1000);
    assert_eq!(spec.token, declarations::FORMAT_TOKEN);
}

/// The host checks the format and admits it (`document.md` section 1): its
/// name, title and extensions are valid, so `validate_record` accepts it.
#[test]
fn the_host_checks_the_format_and_names_what_it_refuses() {
    for batch in batches() {
        for record in records_decode(&batch).unwrap().iter() {
            assert_eq!(validate_record("format-cbz", &record), Ok(()), "{record:?}");
        }
    }
}

fn setting_batches() -> Vec<Vec<u8>> {
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

/// The reading settings: the way pages run and panel by panel, each book's
/// own, named in both languages.
#[test]
fn the_settings_are_the_direction_and_panels_each_books_own() {
    let built = setting_batches();
    assert_eq!(built.len(), image::SETTINGS.len());
    for (built, image) in built.iter().zip(image::SETTINGS) {
        assert_eq!(built.as_slice(), *image);
    }
    let en = paperpaw_i18n::Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
    let de = paperpaw_i18n::Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
    let mut names = Vec::new();
    for batch in &built {
        for record in records_decode(batch).unwrap().iter() {
            assert_eq!(validate_record("format-cbz", &record), Ok(()), "{record:?}");
            if let Record::SettingSpec(spec) = record {
                names.push(spec.name.to_string());
                for key in [spec.title, spec.help] {
                    assert!(en.get(key).is_some() && de.get(key).is_some(), "{key}");
                }
            }
        }
    }
    assert_eq!(names, ["direction", "panels"]);
    for choice in declarations::DIRECTIONS {
        assert!(
            en.get(choice).is_some() && de.get(choice).is_some(),
            "{choice}"
        );
    }
}
