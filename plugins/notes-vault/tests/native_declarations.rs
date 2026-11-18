//! The plugin's native declarations are what the image holds, the host admits
//! them, and they say what interface 6 registered (kept here as the values it
//! registered: the group, the setting, its editor).
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
        declarations::settings(&mut d).unwrap();
    }
    out
}

#[test]
fn the_image_is_what_the_builder_makes() {
    let built = batches();
    assert_eq!(built.len(), image::SETTINGS.len());
    for (built, image) in built.iter().zip(image::SETTINGS) {
        assert_eq!(built.as_slice(), *image);
    }
}

#[test]
fn the_host_admits_the_group_the_setting_and_its_editor() {
    for batch in batches() {
        let records = records_decode(&batch).unwrap();
        // The group, the setting and the extras of its editor.
        assert_eq!(records.iter().count(), 3);
        validate_batch("notes-vault", &records).unwrap();
    }
}

/// The default of a text setting: `[version u32][kind u32][length u32][value]`.
fn text_default(bytes: &[u8]) -> String {
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    assert_eq!(word(4), 3, "a text default");
    String::from_utf8(bytes[12..12 + word(8) as usize].to_vec()).unwrap()
}

/// The group, the setting and its extras, as interface 6 registered them
/// (Beta 3), field by field. The one intended difference is the folder
/// chooser: interface 6 named the screen `core.folder-editor`, the native
/// setting names the slot `core.choose-folder` that screen fills (any plugin
/// taking the slot's place edits the setting too).
#[test]
fn the_group_the_setting_and_its_extras_are_as_interface_6_registered_them() {
    let (mut groups, mut specs, mut extras) = (Vec::new(), Vec::new(), Vec::new());
    {
        let mut d = paperpaw_sdk::native::declare::Declarations::new(|batch: &[u8]| {
            for record in records_decode(batch).unwrap().iter() {
                match record {
                    Record::SettingGroup(g) => groups.push((
                        format!("notes-vault.{}", g.name),
                        g.title.to_string(),
                        g.order,
                        g.page.to_string(),
                        g.parent.to_string(),
                    )),
                    Record::SettingSpec(s) => specs.push((
                        format!("notes-vault.{}", s.name),
                        s.title.to_string(),
                        s.help.to_string(),
                        s.group.to_string(),
                        s.flags & 16 != 0,
                        s.flags & 4 != 0,
                        s.kind == 3,
                        text_default(s.default),
                    )),
                    Record::SettingExtra(e) => extras.push((
                        format!("notes-vault.{}", e.name),
                        e.unit.to_string(),
                        e.section.to_string(),
                        e.follows.to_string(),
                        e.editor_kind,
                        e.editor.to_string(),
                        e.step,
                    )),
                    other => panic!("{other:?}"),
                }
            }
            Ok(())
        });
        declarations::settings(&mut d).unwrap();
    }
    assert_eq!(
        groups,
        [(
            "notes-vault.group".to_string(),
            "notes-vault.title".to_string(),
            0,
            String::new(),
            String::new()
        )]
    );
    assert_eq!(
        specs,
        [(
            "notes-vault.folder".to_string(),
            "notes-vault.setting.folder".to_string(),
            "notes-vault.setting.folder.help".to_string(),
            "notes-vault.group".to_string(),
            false,
            false,
            true,
            String::new()
        )]
    );
    // The extras: nothing but the editor, as interface 6 had nothing else.
    assert_eq!(extras.len(), 1);
    let (name, unit, section, follows, kind, slot, step) = &extras[0];
    assert_eq!(name, "notes-vault.folder");
    assert!(unit.is_empty() && section.is_empty() && follows.is_empty());
    assert!(*step <= 1);
    // The deliberate difference: the folder chooser as a slot (kind 2).
    assert_eq!((*kind, slot.as_str()), (2, "core.choose-folder"));
    assert_eq!(
        paperpaw_sdk::builtin::slots::CHOOSE_FOLDER.as_str(),
        "core.choose-folder"
    );
}
