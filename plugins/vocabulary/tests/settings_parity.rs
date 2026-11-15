//! The declared settings, field by field, against `tests/pinned/settings.txt`
//! (written while the interface-6 plugin's registration was still compared with
//! them): id, name, help, group, level, kind and range, default, and the extra
//! (unit, step, slider, part of the group, editor).
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "only the settings are compared")]
mod declarations;
use paperpaw_core::native_host::registrations::{Record, records_decode};

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
fn every_setting_is_declared_as_pinned() {
    let new = declared();
    assert_eq!(new.len(), 15);
    let names: Vec<&str> = new.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, NAMES);
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/pinned/settings.txt");
    let now = format!("{new:#?}\n");
    if std::env::var_os("PAPERPAW_BLESS").is_some() {
        std::fs::write(path, &now).unwrap();
        return;
    }
    let pinned = std::fs::read_to_string(path).unwrap();
    assert_eq!(now, pinned, "{path} differs: PAPERPAW_BLESS=1 writes it");
}
