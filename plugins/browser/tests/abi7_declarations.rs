//! The native declarations name what the interface-6 registration names: the
//! screen, the slot the browser fills, the action that opens it, its place in
//! the tools menu, its icon, and (native only) the event that brings a page.
//! Texts, the manual and the network need come with the native entry
//! (`PORTING-7.md`).
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_plugin_browser::Browser;
use paperpaw_sdk::native::declare::Declarations;
use paperpaw_sdk::{Contribution, Id, Plugin, Registrar};

fn legacy() -> Vec<Contribution> {
    let mut r = Registrar::new(Id::new_static("browser"));
    Browser.register(&mut r).unwrap();
    r.into_contributions()
}

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
/// what the registration says; nothing else is declared.
#[test]
fn the_decoded_batches_say_what_the_registration_says() {
    let native = declared::native("browser", &all());
    let old = declared::legacy("browser", "", legacy());
    // The screen is added at run time (`add_screen`): its id, title and the
    // default instances (one per parameters) are compared here.
    let screens: Vec<String> = legacy()
        .into_iter()
        .filter_map(|c| match c {
            Contribution::Screen(s) => {
                assert_eq!(s.instances, paperpaw_sdk::Instances::PER_PARAMS);
                let id = s.id.as_str().strip_prefix("browser.").unwrap().to_string();
                Some(format!("{id}|{}", s.title))
            }
            _ => None,
        })
        .collect();
    assert_eq!(native.screens, screens);
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
    assert_eq!(native.slots, old.slots);
    assert_eq!(native.slots, ["core.browser|browser.screen|0"]);
    assert_eq!(native.actions, old.actions);
    assert_eq!(native.actions, ["open|browser.title"]);
    assert_eq!(native.menu_entries, old.menu_entries);
    assert_eq!(native.menu_entries, ["core.tools|browser.open|43"]);
    assert_eq!(native.icons, old.icons);
    assert_eq!(native.icons, ["browser.open|world"]);
    assert_eq!(native.needs, old.needs);
    assert!(native.needs.is_empty());
    // Settings, groups, widgets, status items and keys: the browser has none.
    for lines in [&native, &old] {
        assert!(lines.groups.is_empty() && lines.specs.is_empty() && lines.extras.is_empty());
        assert!(lines.widget_places.is_empty() && lines.status_places.is_empty());
        assert!(lines.keys.is_empty() && lines.others.is_empty());
    }
    // Native only: the event that brings the answer to a request.
    assert_eq!(native.events.len(), 1);
    assert!(old.events.is_empty());
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
