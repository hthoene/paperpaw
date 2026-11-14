//! What Mail declares to the host, field by field, against what the
//! interface-6 plugin registered (kept in `testdata/pins/`): the groups, the
//! 42 settings (title, help, group, level, kept on reset, secret, kind and
//! range, default, options, step), the actions, the Tools entry, the icons and
//! the screens.
//!
//! The differences, all intended: the group is `mail.group` (interface 6:
//! `mail`) as every ported plugin names its group, and the home widgets'
//! settings come with the widgets.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "only the builders are compared")]
mod declarations;
use paperpaw_core::native_host::registrations::{Record, records_decode};
mod pin;

/// What is compared of one setting.
#[derive(Debug, Default, PartialEq, Eq)]
struct Row {
    name: String,
    title: String,
    help: String,
    group: String,
    advanced: bool,
    kept: bool,
    secret: bool,
    /// `bool`, `int:min:max`, `text` or `choice`.
    kind: String,
    default: String,
    options: Vec<String>,
    step: i64,
}

/// The declared default: `[version u32][kind u32][length u32][value]`.
fn default_text(bytes: &[u8]) -> String {
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let value = &bytes[12..12 + word(8) as usize];
    match word(4) {
        1 => (value[0] != 0).to_string(),
        2 => i64::from_le_bytes(value.try_into().unwrap()).to_string(),
        3 => String::from_utf8(value.to_vec()).unwrap(),
        other => panic!("kind {other}"),
    }
}

fn declared() -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    declarations::settings(|batch: &[u8]| {
        for record in records_decode(batch).unwrap().iter() {
            match record {
                Record::SettingSpec(spec) => rows.push(Row {
                    name: spec.name.to_string(),
                    title: spec.title.to_string(),
                    help: spec.help.to_string(),
                    group: spec.group.to_string(),
                    advanced: spec.flags & 16 != 0,
                    kept: spec.flags & (4 | 8) != 0,
                    secret: spec.flags & 8 != 0,
                    kind: match spec.kind {
                        0 => "bool".to_string(),
                        1 => format!("int:{}:{}", spec.min, spec.max),
                        2 => "choice".to_string(),
                        3 => "text".to_string(),
                        other => panic!("kind {other}"),
                    },
                    default: default_text(spec.default),
                    options: if spec.kind == 2 {
                        spec.choices.split('\n').map(str::to_string).collect()
                    } else {
                        Vec::new()
                    },
                    step: 1,
                }),
                Record::SettingExtra(extra) => {
                    let row = rows.iter_mut().find(|r| r.name == extra.name).unwrap();
                    row.step = i64::from(extra.step.max(1));
                    assert!(extra.unit.is_empty() && extra.section.is_empty());
                    assert!(extra.editor.is_empty() && extra.follows.is_empty());
                }
                Record::SettingGroup(_) => {}
                other => panic!("{other:?}"),
            }
        }
        Ok(())
    })
    .unwrap();
    rows
}

#[test]
fn every_setting_is_declared_as_interface_6_registered_it() {
    let mut pin = pin::Pin::new("every_setting_is_declared_as_interface_6_registered_it");
    let new = declared();
    assert_eq!(new.len(), 42, "three accounts of 13 and three for all");
    for row in &new {
        pin.check(&row.name, row);
    }
    pin.finish();
}

#[test]
fn only_the_password_is_a_secret_and_nothing_secret_is_scoped() {
    let rows = declared();
    let secrets: Vec<_> = rows.iter().filter(|r| r.secret).map(|r| &r.name).collect();
    assert_eq!(secrets, ["1.password", "2.password", "3.password"]);
    // A secret is kept through a reset, as in interface 6.
    assert!(rows.iter().filter(|r| r.secret).all(|r| r.kept));
}

#[test]
fn the_groups_are_the_page_of_each_account_inside_the_group() {
    let mut pin = pin::Pin::new("the_groups_are_the_page_of_each_account_inside_the_group");
    let mut groups = Vec::new();
    declarations::settings(|batch: &[u8]| {
        for record in records_decode(batch).unwrap().iter() {
            if let Record::SettingGroup(g) = record {
                assert!(g.page.is_empty());
                groups.push((
                    format!("mail.{}", g.name),
                    g.title.to_string(),
                    g.order,
                    g.parent.to_string(),
                ));
            }
        }
        Ok(())
    })
    .unwrap();
    pin.check("groups", &groups);
    pin.finish();
}

#[test]
fn the_screens_actions_menu_entry_and_icons_are_as_interface_6_registered_them() {
    let mut pin = pin::Pin::new(
        "the_screens_actions_menu_entry_and_icons_are_as_interface_6_registered_them",
    );
    let (mut dscreens, mut dactions, mut dmenus, mut dicons) = (vec![], vec![], vec![], vec![]);
    let mut build = |batch: &[u8]| {
        for record in records_decode(batch).unwrap().iter() {
            match record {
                // Flags: the bits 6 and 7 are the instance policy; 1 is Single.
                Record::ScreenSpec(s) => dscreens.push((
                    format!("mail.{}", s.name),
                    s.title.to_string(),
                    (s.flags >> 6) & 3 == 1,
                )),
                Record::ActionSpec(a) => {
                    dactions.push((format!("mail.{}", a.name), a.title.to_string()));
                }
                Record::MenuEntry(e) => {
                    assert_eq!(e.place, 0, "placed by order, as interface 6");
                    dmenus.push((e.menu.to_string(), e.item.to_string(), e.order));
                }
                Record::IconSpec(i) => dicons.push((i.item.to_string(), i.icon.to_string())),
                other => panic!("{other:?}"),
            }
        }
        Ok(())
    };
    declarations::ui(&mut build).unwrap();
    {
        let mut d = paperpaw_sdk::native::declare::Declarations::new(&mut build);
        declarations::screens(&mut d).unwrap();
    }
    pin.check("screens", &dscreens);
    pin.check("actions", &dactions);
    pin.check("menu entries", &dmenus);
    pin.check("icons", &dicons);
    pin.finish();
}

/// Every record is well formed for the host, and the host admits every one of
/// them: none is refused as unsupported.
#[test]
fn every_record_is_valid_for_the_host_and_admitted() {
    use paperpaw_core::native_host::declarations::validate_record;
    use paperpaw_native::host_glue::ERR_UNSUPPORTED;
    let (mut count, mut waiting) = (0, Vec::new());
    let mut check = |batch: &[u8]| {
        for record in records_decode(batch).unwrap().iter() {
            match validate_record("mail", &record) {
                Ok(()) => {}
                Err(ERR_UNSUPPORTED) => {
                    waiting.push(format!("{record:?}").split('(').next().unwrap().to_string());
                }
                Err(error) => panic!("{record:?}: {error}"),
            }
            count += 1;
        }
        Ok(())
    };
    declarations::settings(&mut check).unwrap();
    declarations::ui(&mut check).unwrap();
    {
        let mut d = paperpaw_sdk::native::declare::Declarations::new(&mut check);
        declarations::screens(&mut d).unwrap();
    }
    // 4 groups, 42 settings, 1 extra, 2 actions, 1 entry, 2 icons, 4 screens.
    assert_eq!(count, 4 + 42 + 1 + 2 + 1 + 2 + 4);
    // The menu entry and the icons are admitted now.
    assert!(waiting.is_empty(), "{waiting:?}");
}

#[test]
fn the_settings_are_the_golden() {
    let mut framed = Vec::new();
    declarations::settings(|batch: &[u8]| {
        framed.extend_from_slice(&u32::try_from(batch.len()).unwrap().to_le_bytes());
        framed.extend_from_slice(batch);
        Ok(())
    })
    .unwrap();
    assert!(
        framed == include_bytes!("../testdata/declarations.bin.v1"),
        "the settings changed: read the difference and bless the golden on purpose"
    );
}
