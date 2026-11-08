//! The declared settings, field by field and in the order, against what the
//! interface-6 plugin registered: id, name, help, group, level, kind, range and
//! choices, default, and the extra (unit, step).
//!
//! One difference is meant: the daily goal starts at 20 minutes (interface 6
//! started at none, and Reading goal set 20 where it was installed; now Stats
//! owns the setting and Reading goal reads it).
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "only the settings are compared")]
mod declarations;
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_sdk::{Contribution, Id, Level, Plugin, Registrar, SettingKind, SettingValue};

/// What is compared of one setting.
#[derive(Debug, Default, PartialEq, Eq)]
struct Row {
    name: String,
    title: String,
    help: String,
    group: String,
    advanced: bool,
    /// `bool`, `int:min:max` or `choice:a,b,c`.
    kind: String,
    default: String,
    unit: String,
    step: i64,
}

fn interface6() -> Vec<Row> {
    let mut r = Registrar::new(Id::new_static("stats"));
    paperpaw_plugin_stats::Stats.register(&mut r).unwrap();
    let mut rows = Vec::new();
    for c in r.into_contributions() {
        let Contribution::FixedSettings(list) = c else {
            continue;
        };
        for spec in list.iter().filter(|s| s.id.as_str().starts_with("stats.")) {
            let extra = spec.extra();
            rows.push(Row {
                name: spec.id.as_str().strip_prefix("stats.").unwrap().to_string(),
                title: spec.title.to_string(),
                help: spec
                    .description
                    .as_ref()
                    .map_or(String::new(), ToString::to_string),
                group: spec
                    .group
                    .as_str()
                    .strip_prefix("stats.")
                    .unwrap_or(spec.group.as_str())
                    .to_string(),
                advanced: matches!(spec.level, Level::Advanced),
                kind: match &spec.kind {
                    SettingKind::Bool => "bool".to_string(),
                    SettingKind::Int { min, max } => format!("int:{min}:{max}"),
                    SettingKind::Choice(choices) => {
                        format!("choice:{}", choices.iter().collect::<Vec<_>>().join(","))
                    }
                    other => panic!("{other:?}"),
                },
                default: match &spec.default {
                    SettingValue::Bool(b) => b.to_string(),
                    SettingValue::Int(n) => n.to_string(),
                    SettingValue::Text(t) => t.to_string(),
                    other => panic!("{other:?}"),
                },
                unit: extra
                    .unit
                    .as_ref()
                    .map_or(String::new(), ToString::to_string),
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
                Record::SettingGroup(_) => {}
                Record::SettingSpec(spec) => rows.push(Row {
                    name: spec.name.to_string(),
                    title: spec.title.to_string(),
                    help: spec.help.to_string(),
                    group: spec.group.to_string(),
                    // Bit 4 of the flags: advanced.
                    advanced: spec.flags & 16 != 0,
                    kind: match spec.kind {
                        0 => "bool".to_string(),
                        1 => format!("int:{}:{}", spec.min, spec.max),
                        2 => format!("choice:{}", spec.choices.replace('\n', ",")),
                        other => panic!("kind {other}"),
                    },
                    default: default_text(spec.default),
                    ..Row::default()
                }),
                Record::SettingExtra(extra) => {
                    let row = rows.iter_mut().find(|r| r.name == extra.name).unwrap();
                    row.unit = extra.unit.to_string();
                    row.step = i64::from(extra.step.max(1));
                }
                other => panic!("{other:?}"),
            }
        }
        Ok(())
    })
    .unwrap();
    rows
}

#[test]
fn every_setting_is_declared_as_interface_6_registered_it_in_the_same_order() {
    let mut old = interface6();
    let new = declared();
    // The goal comes first, as the widgets registered it before the rest.
    assert_eq!(
        old.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        ["goal", "enabled", "min-seconds", "max-seconds"]
    );
    assert_eq!(old[0].default, "stats.goal.off");
    old[0].default = "stats.goal.20".to_string();
    assert_eq!(new, old);
}
