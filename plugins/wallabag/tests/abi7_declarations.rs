//! What the native declarations say: the three settings (kinds, defaults, flags,
//! titles, help, the set-up screen as their editor), the screen and its
//! instance policy, the actions, the place in the tools, the icons and the
//! events the fetch needs; and the host's rules accept or refuse them as they
//! should.
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
    let native = declared::native("wallabag", &all);
    // The three texts, in the library group, kept on reset (the user name and
    // the client's ID are advanced), edited on the set-up screen.
    assert_eq!(
        native.specs,
        [
            "server|3|4|core.group.library|wallabag.server|wallabag.server.help|t:|0|0|\"\"",
            "user|3|20|core.group.library|wallabag.user|wallabag.user.help|t:|0|0|\"\"",
            "client-id|3|20|core.group.library|wallabag.client-id|wallabag.client-id.help|t:|0|0|\"\"",
        ]
    );
    assert_eq!(native.extras.len(), 3);
    assert!(native.extras.iter().all(|e| e.contains("1:setup")));
    assert_eq!(native.screens, ["setup|wallabag.title"]);
    assert_eq!(
        native.actions,
        ["fetch|wallabag.fetch", "open|wallabag.title"]
    );
    assert_eq!(native.menu_entries, ["core.tools|wallabag.open|48"]);
    assert_eq!(
        native.icons,
        ["wallabag.fetch|refresh", "wallabag.open|article"]
    );
    // The plugin stopping (35), the reader's page turns (37), the end of a
    // request (99) and the work that is due (103).
    // (the order the host keeps them in is not the plugin's)
    let mut events = native.events.clone();
    events.sort();
    assert_eq!(events, ["103:6", "35:5", "37:4", "99:3"]);
    assert!(native.groups.is_empty());
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
                new.push((format!("wallabag.{}", s.name), (s.flags >> 6) & 3));
            }
        }
    }
    assert_eq!(new, [("wallabag.setup".to_string(), 1)]);
}
