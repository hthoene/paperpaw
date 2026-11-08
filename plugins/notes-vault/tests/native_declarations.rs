//! The plugin's native declarations are what the image holds, the host admits
//! them, and they say what interface 6 registered.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_core::native_host::{
    declarations::validate_batch,
    registrations::{Record, records_decode},
};
use paperpaw_plugin_notes_vault::NotesVault;
use paperpaw_sdk::{Contribution, Editor, Id, Level, Plugin, Registrar, SettingKind, SettingValue};

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

/// The group and the setting declared, against what the interface-6 plugin
/// registered, field by field. The one intended difference is the folder
/// chooser: interface 6 named the screen `core.folder-editor`, the native
/// setting names the slot `core.choose-folder` that screen fills (any
/// plugin taking the slot's place edits the setting too).
#[test]
fn the_group_the_setting_and_its_extras_are_as_interface_6_registered_them() {
    let mut registrar = Registrar::new(Id::new_static("notes-vault"));
    NotesVault.register(&mut registrar).unwrap();
    let (mut groups, mut specs) = (Vec::new(), Vec::new());
    let mut editor = None;
    for c in registrar.into_contributions() {
        match c {
            // The widgets' groups come with the widgets.
            Contribution::SettingGroup(group) if group.id.as_str() == "notes-vault.group" => groups
                .push((
                    group.id.as_str().to_string(),
                    group.title.to_string(),
                    group.order,
                    group.page.as_ref().map(|p| p.as_str().to_string()),
                    group.parent.as_ref().map(|p| p.as_str().to_string()),
                )),
            Contribution::FixedSettings(list) => {
                // The widgets' settings come with the widgets.
                for spec in list
                    .iter()
                    .filter(|s| s.id.as_str() == "notes-vault.folder")
                {
                    let extra = spec.extra();
                    editor = extra.editor.clone();
                    assert!(extra.unit.is_none() && extra.section.is_none());
                    assert!(!extra.slider && !extra.secret && !extra.section_of_plugin);
                    specs.push((
                        spec.id.as_str().to_string(),
                        spec.title.to_string(),
                        spec.description.as_ref().map(ToString::to_string),
                        spec.group.as_str().to_string(),
                        matches!(spec.level, Level::Advanced),
                        spec.is_kept_on_reset(),
                        matches!(spec.kind, SettingKind::Text),
                        match &spec.default {
                            SettingValue::Text(t) => t.to_string(),
                            other => panic!("{other:?}"),
                        },
                    ));
                }
            }
            _ => {}
        }
    }
    let (mut dgroups, mut dspecs) = (Vec::new(), Vec::new());
    let mut dextras = Vec::new();
    {
        let mut d = paperpaw_sdk::native::declare::Declarations::new(|batch: &[u8]| {
            for record in records_decode(batch).unwrap().iter() {
                match record {
                    Record::SettingGroup(g) => dgroups.push((
                        format!("notes-vault.{}", g.name),
                        g.title.to_string(),
                        g.order,
                        (!g.page.is_empty()).then(|| g.page.to_string()),
                        (!g.parent.is_empty()).then(|| g.parent.to_string()),
                    )),
                    Record::SettingSpec(s) => dspecs.push((
                        format!("notes-vault.{}", s.name),
                        s.title.to_string(),
                        Some(s.help.to_string()),
                        s.group.to_string(),
                        s.flags & 16 != 0,
                        s.flags & 4 != 0,
                        s.kind == 3,
                        text_default(s.default),
                    )),
                    Record::SettingExtra(e) => dextras.push((
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
    assert_eq!(dgroups, groups);
    assert_eq!(dspecs, specs);
    // The extras: nothing but the editor, as interface 6 had nothing else.
    assert_eq!(dextras.len(), 1);
    let (name, unit, section, follows, kind, slot, step) = &dextras[0];
    assert_eq!(name, "notes-vault.folder");
    assert!(unit.is_empty() && section.is_empty() && follows.is_empty());
    assert!(*step <= 1);
    // The deliberate difference: the folder chooser as a slot (kind 2).
    assert_eq!(
        editor,
        Some(Editor::Screen(
            paperpaw_sdk::builtin::screens::FOLDER_EDITOR
        ))
    );
    assert_eq!((*kind, slot.as_str()), (2, "core.choose-folder"));
    assert_eq!(
        paperpaw_sdk::builtin::slots::CHOOSE_FOLDER.as_str(),
        "core.choose-folder"
    );
}
