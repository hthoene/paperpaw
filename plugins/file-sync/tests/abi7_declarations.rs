//! What the native declarations say, and that the host's rules accept them:
//! the settings, the two screens and their instance policy, the actions, the
//! places in the tools, the widgets and the events the plugin reads.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "tokens the plugin uses elsewhere")]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_sdk::native::declare::Declarations;

fn batches(
    declare: impl FnOnce(&mut Declarations<&mut dyn FnMut(&[u8]) -> Result<(), i32>>),
) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut sink = |b: &[u8]| -> Result<(), i32> {
            out.push(b.to_vec());
            Ok(())
        };
        let mut d = Declarations::new(&mut sink as &mut dyn FnMut(&[u8]) -> Result<(), i32>);
        declare(&mut d);
    }
    out
}

fn all() -> Vec<Vec<u8>> {
    let mut all = Vec::new();
    all.extend(batches(|d| declarations::settings(d).unwrap()));
    all.extend(batches(|d| declarations::screens(d).unwrap()));
    all.extend(batches(|d| declarations::widgets(d).unwrap()));
    all.extend(batches(|d| declarations::navigation(d).unwrap()));
    all.extend(batches(|d| declarations::events(d).unwrap()));
    all
}

#[test]
fn the_decoded_batches_say_what_the_plugin_declares() {
    let native = declared::native("file-sync", &all());
    for name in [
        "when|",
        "folder-1|",
        "mode-1|",
        "server-1|",
        "user-1|",
        "folder-3|",
        "mode-3|",
        "server-3|",
        "user-3|",
    ] {
        assert!(native.specs.iter().any(|s| s.starts_with(name)), "{name}");
    }
    // The folder of each pair is chosen on the folder chooser.
    assert_eq!(
        native
            .extras
            .iter()
            .filter(|e| e.contains("core.choose-folder"))
            .count(),
        3
    );
    assert_eq!(
        native.screens,
        ["home|file-sync.title", "pair|file-sync.pair"]
    );
    assert_eq!(
        native.actions,
        ["open|file-sync.title", "sync|file-sync.sync"]
    );
    assert_eq!(
        native.menu_entries,
        [
            "core.tools|file-sync.open|50",
            "core.tools|file-sync.sync|51"
        ]
    );
    assert!(
        !native.widget_places.is_empty() || !native.others.is_empty() || !native.groups.is_empty()
    );
}

#[test]
fn the_screens_open_one_at_a_time() {
    let mut new = Vec::new();
    for batch in batches(|d| declarations::screens(d).unwrap()) {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::ScreenSpec(s) = record {
                new.push((s.name.to_string(), (s.flags >> 6) & 3));
            }
        }
    }
    assert_eq!(new.len(), 2);
    assert!(new.iter().all(|(_, policy)| *policy == 1), "{new:?}");
}

#[test]
fn the_plugin_reads_the_events_it_needs_and_no_others() {
    let native = declared::native("file-sync", &all());
    let mut tags: Vec<&str> = native
        .events
        .iter()
        .map(|e| e.split(':').next().unwrap())
        .collect();
    tags.sort_unstable();
    // Stopping (35), the start, a setting, the charger, Wi-Fi, the minute and
    // the end of a request (99).
    assert!(tags.contains(&"99") && tags.contains(&"35"));
    assert_eq!(tags.len(), 7, "{tags:?}");
}
