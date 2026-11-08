//! The setting, the screen, the action, the menu entry and the icon declared,
//! against what the interface-6 plugin registered, field by field.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "only the builders are compared")]
mod declarations;
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_plugin_file_manager::FileManager;
use paperpaw_sdk::{Action, Contribution, Id, Level, Plugin, Registrar, SettingKind, SettingValue};

fn old() -> Registrar {
    let mut r = Registrar::new(Id::new_static("file-manager"));
    FileManager.register(&mut r).unwrap();
    r
}

#[test]
fn the_setting_is_as_interface_6_registered_it() {
    let mut rows = Vec::new();
    for c in old().into_contributions() {
        let Contribution::FixedSettings(list) = c else {
            continue;
        };
        for spec in list {
            rows.push((
                spec.id.as_str().to_string(),
                spec.title.to_string(),
                spec.description.as_ref().map(ToString::to_string),
                spec.group.as_str().to_string(),
                matches!(spec.level, Level::Advanced),
                matches!(spec.kind, SettingKind::Bool),
                matches!(spec.default, SettingValue::Bool(false)),
            ));
        }
    }
    let mut declared = Vec::new();
    declarations::settings(|batch: &[u8]| {
        for record in records_decode(batch).unwrap().iter() {
            let Record::SettingSpec(spec) = record else {
                panic!("{record:?}");
            };
            declared.push((
                format!("file-manager.{}", spec.name),
                spec.title.to_string(),
                Some(spec.help.to_string()),
                spec.group.to_string(),
                spec.flags & 16 != 0,
                spec.kind == 0,
                // A bool default: [version][kind 1][length 1][0].
                spec.default.get(12) == Some(&0),
            ));
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(declared, rows);
}

#[test]
fn the_screen_action_menu_entry_and_icon_are_as_interface_6_registered_them() {
    let (mut screens, mut actions, mut menus, mut icons) = (vec![], vec![], vec![], vec![]);
    for c in old().into_contributions() {
        match c {
            Contribution::FixedScreens(list) => {
                for s in list {
                    screens.push((s.id().as_str().to_string(), s.title().to_string()));
                }
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
    declarations::ui(|batch: &[u8]| {
        for record in records_decode(batch).unwrap().iter() {
            match record {
                Record::ScreenSpec(s) => {
                    dscreens.push((format!("file-manager.{}", s.name), s.title.to_string()));
                }
                Record::ActionSpec(a) => {
                    dactions.push((format!("file-manager.{}", a.name), a.title.to_string()));
                }
                Record::MenuEntry(e) => {
                    dmenus.push((e.menu.to_string(), e.item.to_string(), e.order));
                }
                Record::IconSpec(i) => dicons.push((i.item.to_string(), i.icon.to_string())),
                other => panic!("{other:?}"),
            }
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(dscreens, screens);
    assert_eq!(dactions, actions);
    assert_eq!(dmenus, menus);
    assert_eq!(dicons, icons);
}
