//! What Mail declares to the host, field by field, against what the
//! interface-6 plugin registered: the groups, the 42 settings (title, help,
//! group, level, kept on reset, secret, kind and range, default, options,
//! step), the actions, the Tools entry, the icons and the screens.
//!
//! The differences, all intended: the group is `mail.group` (interface 6:
//! `mail`) as every ported plugin names its group, and the home widgets'
//! settings come with the widgets.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "only the builders are compared")]
mod declarations;
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_plugin_mail::Mail;
use paperpaw_sdk::{
    Action, Contribution, Id, Instances, Level, Plugin, Registrar, SettingKind, SettingValue,
};

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

fn old() -> Registrar {
    let mut r = Registrar::new(Id::new_static("mail"));
    Mail.register(&mut r).unwrap();
    r
}

/// The group as the native plugin names it.
fn renamed(group: &str) -> String {
    if group == "mail" {
        "mail.group".to_string()
    } else {
        group.to_string()
    }
}

fn mine(name: &str) -> bool {
    let rest = name.strip_prefix("mail.").unwrap_or("");
    matches!(rest, "fetch" | "mark-read" | "signature")
        || matches!(rest.split_once('.'), Some(("1" | "2" | "3", _)))
}

fn interface6() -> Vec<Row> {
    let mut rows = Vec::new();
    for c in old().into_contributions() {
        let Contribution::FixedSettings(list) = c else {
            continue;
        };
        for spec in list.iter().filter(|s| mine(s.id.as_str())) {
            let extra = spec.extra();
            rows.push(Row {
                name: spec.id.as_str().strip_prefix("mail.").unwrap().to_string(),
                title: spec.title.to_string(),
                help: spec
                    .description
                    .as_ref()
                    .map_or(String::new(), ToString::to_string),
                group: renamed(spec.group.as_str()),
                advanced: matches!(spec.level, Level::Advanced),
                kept: spec.is_kept_on_reset(),
                secret: extra.secret,
                kind: match &spec.kind {
                    SettingKind::Bool => "bool".to_string(),
                    SettingKind::Int { min, max } => format!("int:{min}:{max}"),
                    SettingKind::Text => "text".to_string(),
                    SettingKind::Choice(_) => "choice".to_string(),
                    other => panic!("{other:?}"),
                },
                default: match &spec.default {
                    SettingValue::Bool(b) => b.to_string(),
                    SettingValue::Int(n) => n.to_string(),
                    SettingValue::Text(t) => t.to_string(),
                    other => panic!("{other:?}"),
                },
                options: match &spec.kind {
                    SettingKind::Choice(choices) => choices.iter().map(str::to_string).collect(),
                    _ => Vec::new(),
                },
                step: extra.step.max(1),
            });
        }
    }
    rows
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
    let (old, new) = (interface6(), declared());
    assert_eq!(old.len(), 42, "three accounts of 13 and three for all");
    for (a, b) in old.iter().zip(&new) {
        assert_eq!(a, b);
    }
    assert_eq!(new.len(), old.len());
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
    let mut old_groups = Vec::new();
    for c in old().into_contributions() {
        // The widgets' groups come with the widgets.
        if let Contribution::SettingGroup(g) = c
            && !g.id.as_str().starts_with("mail.widget.")
        {
            old_groups.push((
                renamed(g.id.as_str()),
                g.title.to_string(),
                g.order,
                g.parent
                    .as_ref()
                    .map(|p| renamed(p.as_str()))
                    .unwrap_or_default(),
            ));
        }
    }
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
    assert_eq!(groups, old_groups);
}

#[test]
fn the_screens_actions_menu_entry_and_icons_are_as_interface_6_registered_them() {
    let (mut screens, mut actions, mut menus, mut icons) = (vec![], vec![], vec![], vec![]);
    for c in old().into_contributions() {
        match c {
            Contribution::FixedScreens(list) => {
                for s in list {
                    screens.push((
                        s.id().as_str().to_string(),
                        s.title().to_string(),
                        matches!(s.how_many(), Instances::SINGLE),
                    ));
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
    assert_eq!(dscreens, screens);
    assert_eq!(dactions, actions);
    assert_eq!(dmenus, menus);
    assert_eq!(dicons, icons);
}

/// Every record is well formed for the host; the ones it does not admit yet
/// fail only with `ERR_UNSUPPORTED` (`native_host/declarations.rs`), and these
/// are the only ones that do.
#[test]
fn every_record_is_valid_for_the_host_or_waits_for_its_admission() {
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
    // The menu entry and the icons, as for every plugin of this port.
    assert_eq!(waiting, ["MenuEntry", "IconSpec", "IconSpec"]);
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
