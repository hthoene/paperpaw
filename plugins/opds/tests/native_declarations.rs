//! What the native declarations say: the two settings (kinds, defaults, flags,
//! titles, help, the catalogs' part of the library settings), the three screens
//! and their instance policies, the two actions, the place in the tools, the
//! icons and the events the plugin runs on; the image is what the builders
//! make, and the host's rules accept them.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "tokens the plugin uses")]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_sdk::native::declare::Declarations;

mod image {
    include!(concat!(env!("OUT_DIR"), "/opds_declarations.rs"));
}

type Sink<'a> = &'a mut dyn FnMut(&[u8]) -> Result<(), i32>;

fn batches(declare: impl FnOnce(&mut Declarations<Sink<'_>>)) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut sink = |b: &[u8]| -> Result<(), i32> {
            out.push(b.to_vec());
            Ok(())
        };
        let mut d = Declarations::new(&mut sink as Sink<'_>);
        declare(&mut d);
    }
    out
}

fn all() -> Vec<Vec<u8>> {
    let mut all = Vec::new();
    all.extend(batches(|d| declarations::settings(d).unwrap()));
    all.extend(batches(|d| declarations::screens(d).unwrap()));
    all.extend(batches(|d| declarations::navigation(d).unwrap()));
    all.extend(batches(|d| declarations::events(d).unwrap()));
    all
}

#[test]
fn the_image_is_what_the_builders_make() {
    let built = all();
    let image: Vec<&[u8]> = image::SETTINGS
        .iter()
        .chain(image::SCREENS)
        .chain(image::NAVIGATION)
        .chain(image::EVENTS)
        .copied()
        .collect();
    assert_eq!(built.len(), image.len());
    for (built, image) in built.iter().zip(image) {
        assert_eq!(built.as_slice(), image);
    }
}

/// What the batches say, decoded and checked by the host's rules.
#[test]
fn the_decoded_batches_say_what_the_plugin_declares() {
    let native = declared::native("opds", &all());
    // Covers in lists (on) and how many books a sync fetches (10 to 1,000, 50),
    // both in the library group, in the catalogs' part.
    assert_eq!(native.specs.len(), 2, "{:?}", native.specs);
    assert!(native.specs[0].starts_with("covers|"), "{:?}", native.specs);
    assert!(
        native.specs[1].starts_with("sync-limit|"),
        "{:?}",
        native.specs
    );
    assert_eq!(native.extras.len(), 2);
    assert!(native.extras.iter().all(|e| e.contains("opds.section")));
    assert_eq!(
        native.screens,
        [
            "catalogs|opds.title",
            "browse|opds.title",
            "edit|opds.title"
        ]
    );
    assert_eq!(native.actions, ["open|opds.title", "sync|opds.sync-all"]);
    assert_eq!(native.menu_entries, ["core.tools|opds.open|45"]);
    assert_eq!(
        native.icons,
        ["opds.open|book-download", "opds.sync|refresh"]
    );
    // The plugin stopping (35), the end of a request (99) and its progress
    // (100), a picture ready (90), work due (104).
    assert_eq!(native.events.len(), 5, "{:?}", native.events);
    assert!(native.groups.is_empty());
    assert!(native.others.is_empty(), "{:?}", native.others);
}

/// The list opens one at a time, the browsing and the editor as many as are
/// asked for: the decoded `screen.spec` flags (bits 6 and 7: 0 per parameters,
/// 1 single, 2 many).
#[test]
fn the_screens_have_their_instance_policies() {
    let mut new = Vec::new();
    for batch in batches(|d| declarations::screens(d).unwrap()) {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::ScreenSpec(s) = record {
                new.push((s.name.to_string(), (s.flags >> 6) & 3));
            }
        }
    }
    assert_eq!(
        new,
        [
            ("catalogs".to_string(), 1),
            ("browse".to_string(), 2),
            ("edit".to_string(), 2)
        ]
    );
}

#[test]
fn the_tokens_are_each_their_own() {
    use declarations::*;
    let mut tokens = [
        OPEN_TOKEN,
        SYNC_TOKEN,
        HTTP_TOKEN,
        PROGRESS_TOKEN,
        WORK_TOKEN,
        STOP_TOKEN,
        IMAGE_TOKEN,
        CATALOGS_SCREEN,
        BROWSE_SCREEN,
        EDIT_SCREEN,
    ];
    tokens.sort_unstable();
    assert!(tokens.windows(2).all(|w| w[0] != w[1]));
}
