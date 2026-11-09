//! The plugin's native declarations are what the image holds, and the host
//! admits them.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::{
    declarations::validate_batch,
    registrations::{Record, records_decode},
};

#[path = "../declarations.rs"]
#[allow(dead_code, reason = "tokens the plugin uses, not the declarations")]
mod declarations;

mod image {
    include!(concat!(env!("OUT_DIR"), "/kosync_declarations.rs"));
}

/// The batches a declare function sends (a macro: each function is
/// instantiated with the sink's own lifetime).
macro_rules! built {
    ($declare:path) => {{
        let mut out: Vec<Vec<u8>> = Vec::new();
        {
            let mut sink = |batch: &[u8]| -> Result<(), i32> {
                out.push(batch.to_vec());
                Ok(())
            };
            let mut d = paperpaw_sdk::native::declare::Declarations::new(&mut sink);
            $declare(&mut d).unwrap();
        }
        out
    }};
}

fn batches() -> Vec<Vec<u8>> {
    built!(declarations::settings)
}

type Image = &'static [&'static [u8]];

fn all() -> [(Vec<Vec<u8>>, Image); 4] {
    [
        (built!(declarations::settings), image::SETTINGS),
        (built!(declarations::screens), image::SCREENS),
        (built!(declarations::navigation), image::NAVIGATION),
        (built!(declarations::events), image::EVENTS),
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
        ACCOUNT,
        SYNC_NOW,
        STARTED,
        SETTING_CHANGED,
        BOOK_OPENED,
        BOOK_CLOSED,
        PAGE_TURNED,
        SUSPEND,
        RESUME,
        WORK_DUE,
        WORK_ALLOWED,
        HTTP_DONE,
        SCREEN_SHOWN,
    ];
    tokens.sort_unstable();
    assert!(tokens.windows(2).all(|w| w[0] != w[1]));
}

#[test]
fn the_host_admits_the_group_and_the_three_settings() {
    for batch in batches() {
        let records = records_decode(&batch).unwrap();
        assert_eq!(records.iter().count(), 6);
        validate_batch("kosync", &records).unwrap();
    }
}

/// As in interface 6, the user name and the server are set on the account
/// screen (a tap on either opens it); the switch for syncing by itself is a
/// plain row.
#[test]
fn the_account_screen_edits_the_user_and_the_server() {
    let mut edited = Vec::new();
    for batch in batches() {
        let records = records_decode(&batch).unwrap();
        for record in records.iter() {
            if let Record::SettingExtra(extra) = record {
                // Kind 1: a screen of the plugin.
                assert_eq!((extra.editor_kind, extra.editor), (1, "account"));
                edited.push(extra.name.to_string());
            }
        }
    }
    edited.sort();
    assert_eq!(edited, ["server", "user"]);
}
