//! Test-only: what a plugin's native declaration batches say, decoded, next to
//! what its interface-6 registration says, as lines of text that must be
//! equal. A record the host admits (groups, settings, their extras, screens,
//! actions) must pass `validate_record`; the others are checked and then
//! refused as unsupported until the host takes them (anything else, such as
//! an invalid record, fails the test).
#![allow(dead_code, clippy::unwrap_used, reason = "test support")]
use paperpaw_core::native_host::declarations::validate_record;
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_sdk::{Contribution, Editor, SettingKind, SettingValue};

/// `ERR_UNSUPPORTED` of `paperpaw_native::host_glue` (a plugin has no such dependency).
const UNSUPPORTED: i32 = -1;

/// The decoded batches, as lines.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Lines {
    pub groups: Vec<String>,
    pub specs: Vec<String>,
    /// Only extras that say something.
    pub extras: Vec<String>,
    pub screens: Vec<String>,
    /// The kind of every record that is none of these.
    pub others: Vec<String>,
}

fn default_text(bytes: &[u8]) -> String {
    let payload = &bytes[12..];
    match u32::from_le_bytes(bytes[4..8].try_into().unwrap()) {
        1 => format!("b:{}", payload[0]),
        2 => format!("i:{}", i64::from_le_bytes(payload.try_into().unwrap())),
        3 => format!("t:{}", std::str::from_utf8(payload).unwrap()),
        kind => format!("?{kind}"),
    }
}

/// The fields of a setting's extra, as either side has them.
struct Extra<'a> {
    name: &'a str,
    unit: &'a str,
    section: &'a str,
    follows: &'a str,
    shown: (&'a str, &'a str),
    editor: (u32, &'a str),
    step: i64,
    flags: u32,
}

fn extra_line(e: &Extra<'_>) -> Option<String> {
    let step = if e.step == 1 { 0 } else { e.step };
    let empty = e.unit.is_empty()
        && e.section.is_empty()
        && e.follows.is_empty()
        && e.shown.0.is_empty()
        && e.editor.0 == 0
        && step == 0
        && e.flags == 0;
    (!empty).then(|| {
        format!(
            "{}|{}|{}|{}|{}={}|{}:{}|{step}|{}",
            e.name,
            e.unit,
            e.section,
            e.follows,
            e.shown.0,
            e.shown.1,
            e.editor.0,
            e.editor.1,
            e.flags
        )
    })
}

/// Decodes `batches`, checks each record with the host's rules and returns
/// what they declare.
pub fn native(plugin: &str, batches: &[Vec<u8>]) -> Lines {
    let mut out = Lines::default();
    for batch in batches {
        let records = records_decode(batch).unwrap();
        for record in records.iter() {
            let admitted = matches!(
                record,
                Record::SettingGroup(_)
                    | Record::SettingSpec(_)
                    | Record::SettingExtra(_)
                    | Record::ScreenSpec(_)
                    | Record::ActionSpec(_)
                    | Record::Registration(_)
            );
            match validate_record(plugin, &record) {
                Ok(()) => assert!(admitted, "{record:?}"),
                Err(status) => {
                    assert_eq!(status, UNSUPPORTED, "{record:?}");
                    assert!(!admitted, "{record:?}");
                }
            }
            match record {
                Record::SettingGroup(g) => out.groups.push(format!(
                    "{}|{}|{}|{}|{}",
                    g.name, g.title, g.order, g.page, g.parent
                )),
                Record::SettingSpec(s) => out.specs.push(format!(
                    "{}|{}|{}|{}|{}|{}|{}|{}|{}|{:?}",
                    s.name,
                    s.kind,
                    s.flags,
                    s.group,
                    s.title,
                    s.help,
                    default_text(s.default),
                    s.min,
                    s.max,
                    s.choices
                )),
                Record::SettingExtra(e) => out.extras.extend(extra_line(&Extra {
                    name: e.name,
                    unit: e.unit,
                    section: e.section,
                    follows: e.follows,
                    shown: (e.shown_id, e.shown_value),
                    editor: (e.editor_kind, e.editor),
                    step: i64::from(e.step),
                    flags: e.flags,
                })),
                Record::ScreenSpec(s) => out.screens.push(format!("{}|{}", s.name, s.title)),
                other => {
                    let text = format!("{other:?}");
                    let kind = text.split(['(', ' ']).next().unwrap_or("");
                    out.others.push(kind.to_string());
                }
            }
        }
    }
    out
}

/// What the interface-6 registration of `plugin` says, in the same lines:
/// the group `group` (a full id) and the settings in it.
pub fn legacy(plugin: &str, group: &str, contributions: Vec<Contribution>) -> Lines {
    let prefix = format!("{plugin}.");
    let relative = |id: &str| id.strip_prefix(&prefix).unwrap_or(id).to_string();
    let mut out = Lines::default();
    let mut specs = Vec::new();
    for contribution in contributions {
        match contribution {
            Contribution::SettingGroup(g) if g.id.as_str() == group => out.groups.push(format!(
                "{}|{}|{}|{}|{}",
                relative(g.id.as_str()),
                g.title,
                g.order,
                g.page
                    .as_ref()
                    .map_or(String::new(), |p| relative(p.as_str())),
                g.parent.as_ref().map_or("", |p| p.as_str())
            )),
            Contribution::FixedSettings(list) => specs.extend(list.iter().cloned()),
            Contribution::Setting(s) => specs.push(*s),
            Contribution::FixedScreens(list) => {
                for s in list.iter() {
                    out.screens
                        .push(format!("{}|{}", relative(s.id().as_str()), s.title()));
                }
            }
            _ => {}
        }
    }
    for s in specs {
        if s.group.as_str() != group {
            continue;
        }
        let name = relative(s.id.as_str());
        let (kind, min, max, choices) = match &s.kind {
            SettingKind::Bool => (0, 0, 0, String::new()),
            SettingKind::Int { min, max } => (1, *min, *max, String::new()),
            SettingKind::Choice(c) => (2, 0, 0, c.iter().collect::<Vec<_>>().join("\n")),
            SettingKind::Text => (3, 0, 0, String::new()),
            SettingKind::Language => (4, 0, 0, String::new()),
            SettingKind::List => (5, 0, 0, String::new()),
            other => panic!("{other:?}"),
        };
        let e = s.extra();
        let flags = u32::from(s.scoped)
            | u32::from(s.guarded) << 1
            | u32::from(s.is_kept_on_reset()) << 2
            | u32::from(e.secret) << 3
            | u32::from(format!("{:?}", s.level) == "Advanced") << 4;
        let default = match &s.default {
            SettingValue::Bool(v) => format!("b:{}", u8::from(*v)),
            SettingValue::Int(v) => format!("i:{v}"),
            SettingValue::Text(v) => format!("t:{v}"),
            other => panic!("{other:?}"),
        };
        out.specs.push(format!(
            "{name}|{kind}|{flags}|{}|{}|{}|{default}|{min}|{max}|{choices:?}",
            s.group,
            s.title,
            s.description.as_deref().unwrap_or("")
        ));
        let editor = match &e.editor {
            None => (0, String::new()),
            Some(Editor::Screen(id)) => (1, relative(id.as_str())),
            Some(Editor::Slot(id)) => (2, id.as_str().to_string()),
            Some(other) => panic!("{other:?}"),
        };
        let flags = u32::from(e.slider)
            | u32::from(e.section_of_plugin) << 1
            | u32::from(e.keeps_unknown) << 2;
        out.extras.extend(extra_line(&Extra {
            name: &name,
            unit: e.unit.as_deref().unwrap_or(""),
            section: e.section.as_deref().unwrap_or(""),
            follows: e.follows.as_ref().map_or("", |f| f.as_str()),
            shown: e
                .shown_if
                .as_ref()
                .map_or(("", ""), |(id, value)| (id.as_str(), *value)),
            editor: (editor.0, &editor.1),
            step: e.step,
            flags,
        }));
    }
    out
}
