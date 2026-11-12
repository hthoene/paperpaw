//! The declared settings, field by field, against what the interface-6 plugin
//! registered: id, name, help, group, level, kind and range, default, and the
//! extra (unit, step, slider, part of the group, editor).
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "only the settings are compared")]
mod declarations;
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_sdk::{Contribution, Id, Plugin, Registrar};
use paperpaw_sdk::{Editor, Level, SettingKind, SettingValue};

/// What keeps the values of a choice apart in the declaration.
const CHOICE_SEPARATOR: char = '\n';

const NAMES: [&str; 15] = [
    "collect",
    "mark",
    "new-per-day",
    "retention",
    "cards-from-lookups",
    "review-on-wake",
    "sd-auto",
    "sd-folder",
    // The role of the words marked in books.
    "learning.background",
    "learning.ink",
    "learning.line",
    "learning.line-tone",
    "learning.thickness",
    "learning.frame",
    "learning.struck",
];

/// What is compared of one setting.
#[derive(Debug, Default, PartialEq, Eq)]
struct Row {
    name: String,
    title: String,
    help: String,
    group: String,
    advanced: bool,
    /// `int:min:max`, `text`, `bool` or `choice`.
    kind: String,
    /// The values of a choice, in their order.
    choices: Vec<String>,
    /// The default as text.
    default: String,
    unit: String,
    step: i64,
    slider: bool,
    section: String,
    /// `slot:<id>`, `screen:<id>` or empty.
    editor: String,
}

fn interface6() -> Vec<Row> {
    let mut r = Registrar::new(Id::new_static("vocabulary"));
    paperpaw_plugin_vocabulary::Vocabulary
        .register(&mut r)
        .unwrap();
    let mut rows = Vec::new();
    for c in r.into_contributions() {
        let Contribution::FixedSettings(list) = c else {
            continue;
        };
        // The eight of the plugin and the role of the marked words (seven more).
        for spec in list.iter().filter(|s| {
            NAMES
                .map(|n| format!("vocabulary.{n}"))
                .contains(&s.id.as_str().to_string())
        }) {
            let extra = spec.extra();
            rows.push(Row {
                name: spec
                    .id
                    .as_str()
                    .strip_prefix("vocabulary.")
                    .unwrap()
                    .to_string(),
                title: spec.title.to_string(),
                help: spec
                    .description
                    .as_ref()
                    .map_or(String::new(), ToString::to_string),
                group: spec.group.as_str().to_string(),
                advanced: matches!(spec.level, Level::Advanced),
                kind: match &spec.kind {
                    SettingKind::Bool => "bool".to_string(),
                    SettingKind::Int { min, max } => format!("int:{min}:{max}"),
                    SettingKind::Text => "text".to_string(),
                    SettingKind::Choice(_) => "choice".to_string(),
                    other => panic!("{other:?}"),
                },
                choices: match &spec.kind {
                    SettingKind::Choice(choices) => choices.iter().map(String::from).collect(),
                    _ => Vec::new(),
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
                slider: extra.slider,
                section: extra
                    .section
                    .as_ref()
                    .map_or(String::new(), ToString::to_string),
                editor: match &extra.editor {
                    Some(Editor::Slot(id)) => format!("slot:{}", id.as_str()),
                    Some(Editor::Screen(id)) => format!("screen:{}", id.as_str()),
                    None => String::new(),
                    Some(other) => panic!("{other:?}"),
                },
            });
        }
    }
    rows
}

/// The declared default: `[version u32][kind u32][length u32][value]`, an
/// integer as `i64` little-endian or text as UTF-8.
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
                    // Bit 4 of the flags: advanced (the others are not used here).
                    advanced: spec.flags & 16 != 0,
                    kind: match spec.kind {
                        0 => "bool".to_string(),
                        1 => format!("int:{}:{}", spec.min, spec.max),
                        2 => "choice".to_string(),
                        3 => "text".to_string(),
                        other => panic!("kind {other}"),
                    },
                    // The values of a choice are one text, apart by a separator.
                    choices: spec
                        .choices
                        .split(CHOICE_SEPARATOR)
                        .filter(|value| !value.is_empty())
                        .map(String::from)
                        .collect(),
                    default: default_text(spec.default),
                    ..Row::default()
                }),
                Record::SettingExtra(extra) => {
                    let row = rows.iter_mut().find(|r| r.name == extra.name).unwrap();
                    row.unit = extra.unit.to_string();
                    row.step = i64::from(extra.step.max(1));
                    row.slider = extra.flags & 1 != 0;
                    row.section = extra.section.to_string();
                    row.editor = match extra.editor_kind {
                        0 => String::new(),
                        2 => format!("slot:{}", extra.editor),
                        1 => format!("screen:{}", extra.editor),
                        other => panic!("editor kind {other}"),
                    };
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
fn every_setting_is_declared_as_interface_6_registered_it() {
    let (old, new) = (interface6(), declared());
    assert_eq!(old.len(), 15);
    assert_eq!(new, old);
}
