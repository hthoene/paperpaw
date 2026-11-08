//! The screens, the action, the Tools menu entry and the icon declared,
//! against what the interface-6 plugin registered, field by field; and every
//! record valid for the host (the menu entry and the icon wait for its
//! admission, as for every plugin of this port).
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "only the builders are compared")]
mod declarations;
use paperpaw_core::native_host::declarations::validate_record;
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_native::host_glue::ERR_UNSUPPORTED;
use paperpaw_plugin_notes_overview::NotesOverview;
use paperpaw_sdk::native::declare::Declarations;
use paperpaw_sdk::{Action, Contribution, Id, Plugin, Registrar};

fn batches() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut d = Declarations::new(|batch: &[u8]| {
        out.push(batch.to_vec());
        Ok(())
    });
    declarations::ui(&mut d).unwrap();
    drop(d);
    out
}

#[test]
fn the_screens_action_menu_entry_and_icon_are_as_interface_6_registered_them() {
    let mut r = Registrar::new(Id::new_static("notes-overview"));
    NotesOverview.register(&mut r).unwrap();
    let (mut screens, mut actions, mut menus, mut icons) = (vec![], vec![], vec![], vec![]);
    for c in r.into_contributions() {
        match c {
            Contribution::Screen(s) => {
                screens.push((s.id.as_str().to_string(), s.title.to_string()));
            }
            Contribution::FixedActions(list) => {
                for a in list {
                    let a = Action::Fixed(a);
                    actions.push((a.id().as_str().to_string(), a.title().to_string()));
                }
            }
            Contribution::FixedMenuEntries(list) => {
                for e in list {
                    menus.push((
                        e.menu.as_str().to_string(),
                        e.item.as_str().to_string(),
                        i32::from(e.order),
                    ));
                }
            }
            Contribution::FixedIcons(list) => {
                for (item, icon) in list {
                    icons.push((item.as_str().to_string(), (*icon).to_string()));
                }
            }
            _ => {}
        }
    }
    let (mut dscreens, mut dactions, mut dmenus, mut dicons) = (vec![], vec![], vec![], vec![]);
    for batch in batches() {
        for record in records_decode(&batch).unwrap().iter() {
            match record {
                Record::ScreenSpec(s) => {
                    // One instance at a time, as before (the flags say it).
                    dscreens.push((format!("notes-overview.{}", s.name), s.title.to_string()));
                }
                Record::ActionSpec(a) => {
                    dactions.push((format!("notes-overview.{}", a.name), a.title.to_string()));
                }
                Record::MenuEntry(e) => {
                    dmenus.push((e.menu.to_string(), e.item.to_string(), e.order))
                }
                Record::IconSpec(i) => dicons.push((i.item.to_string(), i.icon.to_string())),
                other => panic!("{other:?}"),
            }
        }
    }
    dscreens.sort();
    screens.sort();
    assert_eq!(dscreens, screens);
    assert_eq!(dactions, actions);
    assert_eq!(dmenus, menus);
    assert_eq!(dicons, icons);
}

#[test]
fn every_record_is_valid_for_the_host_or_waits_for_its_admission() {
    let mut waiting = Vec::new();
    for batch in batches() {
        for record in records_decode(&batch).unwrap().iter() {
            match validate_record("notes-overview", &record) {
                Ok(()) => {}
                Err(ERR_UNSUPPORTED) => {
                    let text = format!("{record:?}");
                    waiting.push(text.split('(').next().unwrap().to_string());
                }
                Err(error) => panic!("{record:?}: {error}"),
            }
        }
    }
    assert_eq!(waiting, ["MenuEntry", "IconSpec"]);
}
