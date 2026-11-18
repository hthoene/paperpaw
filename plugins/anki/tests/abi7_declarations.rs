//! What the native declarations say, and that the host's rules accept them:
//! the settings (the text ones edited on a screen of the plugin), the three
//! screens and their instance policy, the actions, the place in the tools, the
//! icons and the events the sync needs.
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

#[test]
fn the_decoded_batches_say_what_the_plugin_declares() {
    let mut all = Vec::new();
    all.extend(batches(|d| declarations::settings(d).unwrap()));
    all.extend(batches(|d| declarations::screens(d).unwrap()));
    all.extend(batches(|d| declarations::navigation(d).unwrap()));
    all.extend(batches(|d| declarations::events(d).unwrap()));
    let native = declared::native("anki", &all);
    assert_eq!(native.specs.len(), 5, "{:?}", native.specs);
    for name in ["host|", "deck|", "port|", "new-cards|", "sync-on-open|"] {
        assert!(native.specs.iter().any(|s| s.starts_with(name)), "{name}");
    }
    assert!(
        native
            .specs
            .iter()
            .all(|s| s.contains("core.group.library"))
    );
    // The computer and the deck are edited on screens of the plugin.
    assert!(native.extras.iter().any(|e| e.contains("1:home")));
    assert!(native.extras.iter().any(|e| e.contains("1:decks")));
    assert_eq!(
        native.screens,
        ["home|anki.title", "decks|anki.deck", "study|anki.study"]
    );
    assert_eq!(native.actions, ["open|anki.title", "sync|anki.sync"]);
    assert_eq!(native.menu_entries, ["core.tools|anki.open|47"]);
    assert_eq!(native.icons, ["anki.open|stack-2", "anki.sync|refresh"]);
    // The plugin stopping (35) and the end of a request (99).
    let mut events = native.events.clone();
    events.sort();
    assert_eq!(events, ["35:4", "99:3"]);
    assert!(native.groups.is_empty());
    assert!(native.others.is_empty(), "{:?}", native.others);
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
    assert_eq!(new.len(), 3);
    assert!(new.iter().all(|(_, policy)| *policy == 1), "{new:?}");
}
