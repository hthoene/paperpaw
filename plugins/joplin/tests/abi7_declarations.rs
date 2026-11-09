//! The native declarations name what the interface-6 registration named: the
//! four settings (kinds, defaults, flags, titles, help, the set-up screen as
//! their editor), the screen and its instance policy, the actions, the place
//! in the tools and the icons; and the host's rules accept or refuse them as
//! they should.
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

/// What the batches say, decoded and checked by the host's rules, is what the
/// interface-6 registration said (pinned here since it was removed).
#[test]
fn the_decoded_batches_say_what_interface_6_registered() {
    let mut all = Vec::new();
    all.extend(batches(|d| declarations::settings(d).unwrap()));
    all.extend(batches(|d| declarations::screens(d).unwrap()));
    all.extend(batches(|d| declarations::navigation(d).unwrap()));
    all.extend(batches(|d| declarations::events(d).unwrap()));
    let native = declared::native("joplin", &all);
    assert!(native.groups.is_empty());
    assert_eq!(
        native.specs,
        [
            "service|2|4|core.group.library|joplin.service|joplin.service.help|t:joplin.service.cloud|0|0|\"joplin.service.cloud\\njoplin.service.server\\njoplin.service.webdav\"",
            "address|3|20|core.group.library|joplin.address|joplin.address.help|t:|0|0|\"\"",
            "user|3|20|core.group.library|joplin.user|joplin.user.help|t:|0|0|\"\"",
            "notebook|3|20|core.group.library|joplin.notebook|joplin.notebook.help|t:PaperPaw|0|0|\"\"",
        ]
    );
    assert_eq!(
        native.extras,
        [
            "service||||=|1:setup|0|0",
            "address||||=|1:setup|0|0",
            "user||||=|1:setup|0|0",
            "notebook||||=|1:setup|0|0",
        ]
    );
    assert_eq!(native.screens, ["setup|joplin.title"]);
    assert_eq!(native.actions, ["open|joplin.title", "send|joplin.send"]);
    assert_eq!(native.menu_entries, ["core.tools|joplin.open|49"]);
    assert_eq!(native.icons, ["joplin.open|note", "joplin.send|upload"]);
    // Started, the end of a request, the work due and a setting changed.
    assert_eq!(native.events, ["103:5", "32:3", "64:6", "99:4"]);
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
                new.push((format!("joplin.{}", s.name), (s.flags >> 6) & 3));
            }
        }
    }
    assert_eq!(new, [("joplin.setup".to_string(), 1)]);
}
