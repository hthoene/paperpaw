//! Literata's bold and italic from the card: the lean firmware (the X4
//! family's) has regular Literata alone, and the store fetches the data
//! plugin `literata-styles` by itself at the first start after the
//! first-run setup, once and quietly; a firmware that has the faces, or a
//! card that has the plugin, asks for nothing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use common::{DEVICE, Shared};
use paperpaw_core::{HttpCall, HttpResponse, Io, Kernel};
use paperpaw_sdk::builtin::fonts;
use paperpaw_sdk::{
    Font, FontSpec, Id, Instant, Manifest, Plugin, Priority, RegisterError, Registrar, Storage,
    Version,
};
use paperpaw_text::{Glyph, LineMetrics, PackBuilder};

/// What the lean firmware has of Literata: the regular face alone.
struct Lean;

impl Plugin for Lean {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("fonts-default"),
            "Fonts",
            Version::new(1, 0, 0),
        )
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let mut pack = PackBuilder::new();
        let metrics = LineMetrics::new(8.0, 2.0, 12.0);
        pack.add_strike(10.0, metrics, vec![('a', Glyph::empty(5.0))]);
        let font = Font::from_data(pack.build(), &[]).unwrap();
        let id = Id::new_static("fonts-default.literata");
        r.add_font(FontSpec::new(id.clone(), font).family("Literata", false, false))?;
        r.bind_font(fonts::READING, id, Priority::DEFAULT)
    }
}

const SETUP_DONE: &str = ".paperpaw/plugins/setup/done";
const PAGE: &str = "https://paperpaw.org/api/v1/plugins/literata-styles.json?device=x4&lang=en";

/// A kernel started on `storage`, with the fonts of the firmware `lean`
/// or not, and what it asks the network for.
fn begun(storage: Shared, lean: bool) -> (Kernel, Vec<HttpCall>) {
    let mut builder = Kernel::builder(DEVICE)
        .storage(storage)
        .network(true)
        .catalog_device("x4")
        .plugin(common::notice_box::NoticeBox)
        .plugin(paperpaw_plugin_store::Store);
    builder = if lean {
        builder.plugin(Lean)
    } else {
        builder.plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
    };
    let mut kernel = builder.build().unwrap();
    kernel.start(Instant(0));
    let calls = kernel
        .take_io()
        .into_iter()
        .filter_map(|io| match io {
            Io::Http(call) => Some(call),
            _ => None,
        })
        .collect();
    (kernel, calls)
}

/// What a start asks the network for.
fn started(storage: Shared, lean: bool) -> Vec<String> {
    begun(storage, lean).1.into_iter().map(|c| c.url).collect()
}

/// A start whose first request is answered with `status` (0: no
/// connection at all).
fn started_and_answered(storage: Shared, status: u16) -> Vec<String> {
    let (mut kernel, calls) = begun(storage, true);
    let urls = calls.iter().map(|c| c.url.clone()).collect();
    if let Some(call) = calls.first() {
        let response = HttpResponse {
            id: call.id,
            status,
            ..HttpResponse::default()
        };
        kernel.http_response(Instant(100), response);
    }
    urls
}

fn after_setup() -> Shared {
    let storage = Shared::default();
    storage.write(SETUP_DONE, b"").unwrap();
    storage
}

#[test]
fn the_lean_firmware_asks_for_the_faces_once_the_setup_is_over() {
    let storage = after_setup();
    assert_eq!(started(storage.clone(), true), [PAGE]);
    let queue = paperpaw_sdk::read_file(&storage, ".paperpaw/plugins/store/queue.txt", 4096);
    assert_eq!(
        String::from_utf8(queue.unwrap()).unwrap(),
        "> ~literata-styles\n"
    );
}

#[test]
fn not_before_the_setup_is_over() {
    assert!(started(Shared::default(), true).is_empty());
}

#[test]
fn a_firmware_with_the_faces_asks_for_nothing() {
    assert!(started(after_setup(), false).is_empty());
}

#[test]
fn a_card_with_the_plugin_asks_for_nothing() {
    let storage = after_setup();
    storage
        .write(
            "plugins/literata-styles/plugin.ini",
            b"[plugin]\nid = literata-styles\nname = Styles\nversion = 1.0.0\n",
        )
        .unwrap();
    assert!(started(storage, true).is_empty());
}

const TRIES: &str = ".paperpaw/plugins/store/styles-tries.txt";
const HINTED: &str = ".paperpaw/plugins/store/styles-hinted.txt";

/// Not asked again and again: three answers of the store at most (the
/// page came wrong), then it is the reader's to find.
#[test]
fn it_is_tried_three_times_at_most() {
    let storage = after_setup();
    for round in 1..=3 {
        assert_eq!(
            started_and_answered(storage.clone(), 500),
            [PAGE],
            "start {round}"
        );
    }
    assert!(started(storage.clone(), true).is_empty());
    assert_eq!(
        String::from_utf8(paperpaw_sdk::read_file(&storage, TRIES, 16).unwrap()).unwrap(),
        "3"
    );
}

/// Starts without a network are no tries: a reader who is rarely
/// online gets it at the first start with a network, however many starts
/// there were before, and is told once to connect.
#[test]
fn starts_without_a_network_cost_no_try_and_say_it_once() {
    let storage = after_setup();
    for round in 1..=5 {
        assert_eq!(
            started_and_answered(storage.clone(), 0),
            [PAGE],
            "start {round}"
        );
        assert!(!storage.exists(TRIES), "start {round} counted");
        assert!(storage.exists(HINTED), "start {round} did not say it");
    }
}

/// A start while it still waits (no network yet) does not put it in
/// twice.
#[test]
fn it_waits_in_the_queue_without_doubling() {
    let storage = after_setup();
    storage
        .write(".paperpaw/plugins/store/queue.txt", b"~literata-styles\n")
        .unwrap();
    let asked = started(storage.clone(), true);
    // The queue goes on by itself (the reader asked for it by updating).
    assert_eq!(asked, [PAGE]);
    let queue = paperpaw_sdk::read_file(&storage, ".paperpaw/plugins/store/queue.txt", 4096);
    assert_eq!(
        String::from_utf8(queue.unwrap()).unwrap(),
        "> ~literata-styles\n"
    );
}
