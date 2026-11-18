//! What the native declarations say: the setting for where articles are
//! fetched from (kind, choices, default, flags, title, help), the screen and
//! its instance policy, the actions, the place in the tools, the icons and the
//! events the fetch needs (all as the interface-6 registration named them);
//! and the host's rules accept or refuse them as they should.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "tokens the plugin uses later")]
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

/// What the batches say, decoded and checked by the host's rules.
#[test]
fn the_decoded_batches_say_what_the_plugin_declares() {
    let mut all = Vec::new();
    all.extend(batches(|d| declarations::settings(d).unwrap()));
    all.extend(batches(|d| declarations::screens(d).unwrap()));
    all.extend(batches(|d| declarations::navigation(d).unwrap()));
    all.extend(batches(|d| declarations::events(d).unwrap()));
    let native = declared::native("read-later-cloud", &all);
    assert!(native.groups.is_empty());
    // The choice (kept on reset, in the library group, *Later* first), its
    // two values and its help.
    assert_eq!(
        native.specs,
        [
            "from|2|4|core.group.library|read-later-cloud.from|read-later-cloud.from.help|t:read-later-cloud.from.later|0|0|\"read-later-cloud.from.later\\nread-later-cloud.from.inbox\""
        ]
    );
    assert!(native.extras.is_empty(), "{:?}", native.extras);
    assert_eq!(native.screens, ["setup|read-later-cloud.title"]);
    assert_eq!(
        native.actions,
        [
            "fetch|read-later-cloud.fetch",
            "open|read-later-cloud.title"
        ]
    );
    assert_eq!(native.menu_entries, ["core.tools|read-later-cloud.open|49"]);
    assert_eq!(
        native.icons,
        [
            "read-later-cloud.fetch|refresh",
            "read-later-cloud.open|article"
        ]
    );
    // The plugin stopping (35), the reader's page turns (37), the work due
    // (103) and the end of a request (99).
    assert_eq!(native.events, ["103:6", "35:5", "37:4", "99:3"]);
    assert!(native.others.is_empty(), "{:?}", native.others);
}

/// The set-up opens one at a time: the decoded `screen.spec` flags (bits 6 and
/// 7: 0 per parameters, 1 single, 2 many).
#[test]
fn the_set_up_opens_one_at_a_time() {
    let mut new = Vec::new();
    for batch in batches(|d| declarations::screens(d).unwrap()) {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::ScreenSpec(s) = record {
                new.push((format!("read-later-cloud.{}", s.name), (s.flags >> 6) & 3));
            }
        }
    }
    assert_eq!(new, [("read-later-cloud.setup".to_string(), 1)]);
}
