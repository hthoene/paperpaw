//! The native declarations name what the interface-6 registration named (pinned
//! here from that code, which is gone): the screen, the slot the browser
//! fills, the action that opens it, its place in the tools menu, its icon, and
//! the event that brings a page.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
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
    all.extend(batches(|d| declarations::screens(d).unwrap()));
    all.extend(batches(|d| declarations::navigation(d).unwrap()));
    all.extend(batches(|d| declarations::events(d).unwrap()));
    all
}

/// Every field of every record, decoded and checked by the host's rules, is
/// what interface 6 registered; nothing else is declared.
#[test]
fn the_decoded_batches_say_what_the_registration_said() {
    let native = declared::native("browser", &all());
    // The screen, with one instance per parameters (flags 0, stated, not
    // defaulted).
    // The decoded flags: one instance per parameters (0), stated, not defaulted.
    let flags: Vec<u32> = all()
        .iter()
        .flat_map(|batch| {
            let records = records_decode(batch).unwrap();
            records
                .iter()
                .filter_map(|r| match r {
                    Record::ScreenSpec(s) => Some(s.flags),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(flags, [0]);
    assert_eq!(native.screens, ["screen|browser.title"]);
    assert_eq!(native.slots, ["core.browser|browser.screen|0"]);
    assert_eq!(native.actions, ["open|browser.title"]);
    assert_eq!(native.menu_entries, ["core.tools|browser.open|43"]);
    assert_eq!(native.icons, ["browser.open|world"]);
    assert!(native.needs.is_empty());
    // Settings, groups, widgets, status items and keys: the browser has none.
    assert!(native.groups.is_empty() && native.specs.is_empty() && native.extras.is_empty());
    assert!(native.widget_places.is_empty() && native.status_places.is_empty());
    assert!(native.keys.is_empty() && native.others.is_empty());
    // The event that brings the answer to a request.
    assert_eq!(native.events.len(), 1);
}

/// Every text the declarations ask for exists in both languages.
#[test]
fn the_declared_texts_have_words() {
    let (en, de) = (
        include_str!("../i18n/en.ini"),
        include_str!("../i18n/de.ini"),
    );
    for key in ["browser.title"] {
        for (name, catalog) in [("en", en), ("de", de)] {
            assert!(
                catalog.lines().any(|l| l.starts_with(&format!("{key} = "))),
                "{key} missing in {name}"
            );
        }
    }
}
