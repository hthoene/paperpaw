//! The screens, the action, the Tools entry and the icons the native plugin
//! declares are what the image holds, the host admits them, and they say what
//! interface 6 registered (kept here as the values it registered).
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
#![allow(dead_code, reason = "the declaration test uses part of the builders")]
use paperpaw_core::native_host::{
    declarations::validate_batch,
    registrations::{Record, records_decode},
};

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
    let batch = format_batches().remove(0);
    let records = records_decode(&batch).unwrap();
    let spec = records
        .iter()
        .find_map(|r| match r {
            Record::FormatSpec(spec) => Some(spec),
            _ => None,
        })
        .unwrap();
    // Beta 3 registered `notes-vault.markdown` for `md` and `markdown`.
    assert_eq!(format!("notes-vault.{}", spec.name), "notes-vault.markdown");
    assert_eq!(spec.title, "notes-vault.markdown");
    assert_eq!(
        spec.extensions.split('\n').collect::<Vec<_>>(),
        ["md", "markdown"]
    );
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
    let s = |a: &str, b: &str| (a.to_string(), b.to_string());
    // What Beta 3 registered.
    let mut screens = vec![
        s("notes-vault.screen", "notes-vault.title"),
        s("notes-vault.list", "notes-vault.title"),
        s("notes-vault.tags", "notes-vault.tags"),
    ];
    let actions = vec![s("notes-vault.open", "notes-vault.title")];
    let menus = vec![("core.tools".to_string(), "notes-vault.open".to_string(), 38)];
    let mut icons = vec![
        s("notes-vault.open", "note"),
        s("notes-vault.group", "note"),
    ];
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
