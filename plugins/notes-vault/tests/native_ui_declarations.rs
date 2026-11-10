//! The screens, the action, the Tools entry and the icons the native plugin
//! declares are what the image holds, the host admits them, and they say what
//! interface 6 registered.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#![allow(dead_code, reason = "the declaration test uses part of the builders")]
use paperpaw_core::native_host::{
    declarations::validate_batch,
    registrations::{Record, records_decode},
};
use paperpaw_plugin_notes_vault::NotesVault;
use paperpaw_sdk::{Action, Contribution, Id, Plugin, Registrar};

#[path = "../declarations.rs"]
mod declarations;

mod image {
    include!(concat!(env!("OUT_DIR"), "/notes_vault_declarations.rs"));
}

fn batches() -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    {
        let mut d = paperpaw_sdk::native::declare::Declarations::new(|batch: &[u8]| {
            out.push(batch.to_vec());
            Ok(())
        });
        declarations::ui(&mut d).unwrap();
    }
    out
}

#[test]
fn the_image_is_what_the_builder_makes() {
    let built = batches();
    assert_eq!(built.len(), image::UI.len());
    for (built, image) in built.iter().zip(image::UI) {
        assert_eq!(built.as_slice(), *image);
    }
}

fn format_batches() -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    {
        let mut d = paperpaw_sdk::native::declare::Declarations::new(|batch: &[u8]| {
            out.push(batch.to_vec());
            Ok(())
        });
        declarations::formats(&mut d).unwrap();
    }
    out
}

#[test]
fn the_format_image_is_what_the_builder_makes() {
    let built = format_batches();
    assert_eq!(built.len(), image::FORMATS.len());
    for (built, image) in built.iter().zip(image::FORMATS) {
        assert_eq!(built.as_slice(), *image);
    }
}

/// The format declaration is valid (the host checks its name, title and
/// extensions) and the host admits it (`document.md` section 1).
#[test]
fn the_host_checks_the_format_and_admits_it() {
    for batch in format_batches() {
        let records = records_decode(&batch).unwrap();
        validate_batch("notes-vault", &records).unwrap();
    }
}

#[test]
fn the_format_is_as_interface_6_registered_it() {
    let mut registrar = Registrar::new(Id::new_static("notes-vault"));
    NotesVault.register(&mut registrar).unwrap();
    let old = registrar
        .into_contributions()
        .into_iter()
        .find_map(|c| match c {
            Contribution::Format(spec) => Some(spec),
            _ => None,
        })
        .unwrap();
    let batch = format_batches().remove(0);
    let records = records_decode(&batch).unwrap();
    let spec = records
        .iter()
        .find_map(|r| match r {
            Record::FormatSpec(spec) => Some(spec),
            _ => None,
        })
        .unwrap();
    assert_eq!(format!("notes-vault.{}", spec.name), old.id.as_str());
    assert_eq!(spec.title, old.title);
    let wanted: Vec<&str> = old.extensions.iter().map(|e| &**e).collect();
    assert_eq!(spec.extensions.split('\n').collect::<Vec<_>>(), wanted);
}

#[test]
fn the_host_admits_the_declarations() {
    for batch in batches() {
        let records = records_decode(&batch).unwrap();
        validate_batch("notes-vault", &records).unwrap();
    }
}

#[test]
fn the_screens_action_menu_entry_and_icons_are_as_interface_6_registered_them() {
    let mut registrar = Registrar::new(Id::new_static("notes-vault"));
    NotesVault.register(&mut registrar).unwrap();
    let (mut screens, mut actions, mut menus, mut icons) = (vec![], vec![], vec![], vec![]);
    for c in registrar.into_contributions() {
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
    let mut registrations = 0;
    for batch in batches() {
        for record in records_decode(&batch).unwrap().iter() {
            match record {
                Record::ScreenSpec(s) => {
                    dscreens.push((format!("notes-vault.{}", s.name), s.title.to_string()));
                }
                Record::ActionSpec(a) => {
                    dactions.push((format!("notes-vault.{}", a.name), a.title.to_string()));
                }
                Record::MenuEntry(e) => {
                    dmenus.push((e.menu.to_string(), e.item.to_string(), e.order));
                }
                Record::IconSpec(i) => dicons.push((i.item.to_string(), i.icon.to_string())),
                // The subscription to the index's work.
                _ => registrations += 1,
            }
        }
    }
    assert_eq!(registrations, 1);
    screens.sort();
    dscreens.sort();
    assert_eq!(dscreens, screens);
    assert_eq!(dactions, actions);
    assert_eq!(dmenus, menus);
    icons.sort();
    dicons.sort();
    assert_eq!(dicons, icons);
}
