//! The screens, the action, the Tools menu entry and the icon declared,
//! as the interface-6 plugin registered them (pinned here); and every
//! record valid for the host (the menu entry and the icon wait for its
//! admission, as for every plugin of this port).
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "only the builders are compared")]
mod declarations;
use paperpaw_core::native_host::declarations::validate_record;
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_native::host_glue::ERR_UNSUPPORTED;
use paperpaw_sdk::native::declare::Declarations;

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
    let (mut screens, mut actions, mut menus, mut icons) = (vec![], vec![], vec![], vec![]);
    for batch in batches() {
        for record in records_decode(&batch).unwrap().iter() {
            match record {
                Record::ScreenSpec(s) => {
                    // One instance for each set of parameters, as before (bits 6-7 of the flags: 0).
                    screens.push((
                        format!("notes-overview.{}", s.name),
                        s.title.to_string(),
                        (s.flags >> 6) & 3 == 0,
                    ));
                }
                Record::ActionSpec(a) => {
                    actions.push((format!("notes-overview.{}", a.name), a.title.to_string()));
                }
                Record::MenuEntry(e) => {
                    menus.push((e.menu.to_string(), e.item.to_string(), e.order))
                }
                Record::IconSpec(i) => icons.push((i.item.to_string(), i.icon.to_string())),
                other => panic!("{other:?}"),
            }
        }
    }
    screens.sort();
    // What the interface-6 plugin registered (`lib.rs` before this port left it).
    let screen = |id: &str| (id.to_string(), "notes-overview.title".to_string(), true);
    assert_eq!(
        screens,
        [
            screen("notes-overview.book"),
            screen("notes-overview.screen")
        ]
    );
    assert_eq!(
        actions,
        [(
            "notes-overview.open".to_string(),
            "notes-overview.title".to_string()
        )]
    );
    assert_eq!(
        menus,
        [(
            "core.tools".to_string(),
            "notes-overview.open".to_string(),
            37
        )]
    );
    assert_eq!(
        icons,
        [("notes-overview.open".to_string(), "bookmarks".to_string())]
    );
}

/// The host admits every one of them now, so none waits.
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
    assert!(
        waiting.is_empty(),
        "the host admits every record now: {waiting:?}"
    );
}
