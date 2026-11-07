//! The file of profiles (`profiles.ini`, version 2) and what a profile's
//! values are, without the host.
use alloc::{format, string::String, vec::Vec};
use paperpaw_sdk::config;

/// A named set of setting values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// Its name.
    pub name: String,
    /// Setting ids and values as in `paperpaw.ini`.
    pub values: Vec<(String, String)>,
}

/// The file's version this plugin writes.
const VERSION: u32 = 2;

/// Version 1 to 2: the names out of the section headers (version 1 had the
/// name as the section, `[Night]`, which an ini file cannot hold with spaces).
fn upgrade_names(v1: &str) -> String {
    let mut profiles: Vec<Profile> = Vec::new();
    for line in v1.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            profiles.push(Profile {
                name: name.trim().into(),
                values: Vec::new(),
            });
        } else if let (Some(profile), Some((id, value))) =
            (profiles.last_mut(), line.split_once('='))
            && is_id(id.trim())
        {
            profile.values.push((id.trim().into(), value.trim().into()));
        }
    }
    text(&profiles)
}

/// A setting id: lower case letters, digits and `.-_`, with a dot.
#[must_use]
pub fn is_id(id: &str) -> bool {
    id.contains('.')
        && id.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'_')
        })
}

/// The profiles as the file's text (without its version).
fn text(profiles: &[Profile]) -> String {
    let mut text = String::new();
    for (i, profile) in profiles.iter().enumerate() {
        if i > 0 {
            text.push('\n');
        }
        text.push_str(&format!("[profile.{}]\n", i + 1));
        text.push_str(&format!("name = {}\n", config::quote(&profile.name)));
        for (id, value) in &profile.values {
            text.push_str(&format!("{id} = {}\n", config::quote(value)));
        }
    }
    text
}

/// The file's text for `profiles`: its version, then the profiles.
#[must_use]
pub fn encode(profiles: &[Profile]) -> String {
    format!("# format {VERSION}\n{}", text(profiles))
}

/// The profiles of a file; `None` where it cannot be read as one (a newer
/// version, or not text): then nothing may be saved over it.
#[must_use]
pub fn decode(bytes: &[u8]) -> Option<Vec<Profile>> {
    let source = core::str::from_utf8(bytes).ok()?;
    let version = source
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("# format "))
        .map_or(Some(1), |v| v.trim().parse::<u32>().ok())?;
    match version {
        1 => Some(parse(&upgrade_names(source))),
        VERSION => Some(parse(source)),
        _ => None,
    }
}

/// The profiles of the file's text (in this version).
fn parse(text: &str) -> Vec<Profile> {
    let mut profiles: Vec<(&str, Profile)> = Vec::new();
    for entry in config::parse(text).filter_map(Result::ok) {
        if !entry.section.starts_with("profile.") {
            continue;
        }
        if profiles.last().is_none_or(|(s, _)| *s != entry.section) {
            profiles.push((
                entry.section,
                Profile {
                    name: String::new(),
                    values: Vec::new(),
                },
            ));
        }
        let Some((_, profile)) = profiles.last_mut() else {
            continue;
        };
        let value = String::from(entry.value.as_str());
        if entry.key == "name" {
            profile.name = value;
        } else if is_id(entry.key) {
            profile.values.push((entry.key.into(), value));
        }
    }
    profiles.into_iter().map(|(_, p)| p).collect()
}

/// Whether `a` and `b` name the same profile: as they read, whatever the case.
#[must_use]
pub fn same_name(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// A name as typed: words joined by single spaces, without brackets; `None`
/// where nothing is left.
#[must_use]
pub fn clean(name: &str) -> Option<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let name = name.replace(['[', ']'], "");
    (!name.trim().is_empty()).then(|| name.trim().into())
}

/// The kind of value a setting holds, as far as profiles care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// On or off.
    Bool,
    /// A whole number between `min` and `max`.
    Int { min: i64, max: i64 },
    /// One of a list of ids.
    Choice,
}

/// A value of the kind in the text of a configuration file, as the profile
/// keeps it, or in the typed form: `None` where the setting does not take it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A switch.
    Bool(bool),
    /// A number.
    Int(i64),
    /// A choice.
    Text(String),
}

impl Value {
    /// How a profile keeps it.
    #[must_use]
    pub fn to_text(&self) -> String {
        match self {
            Self::Bool(true) => "on".into(),
            Self::Bool(false) => "off".into(),
            Self::Int(v) => format!("{v}"),
            Self::Text(v) => v.clone(),
        }
    }

    /// The value `text` is for a setting of `kind`, if it takes it.
    #[must_use]
    pub fn parse(kind: Kind, text: &str) -> Option<Self> {
        match kind {
            Kind::Bool => match text.trim().to_ascii_lowercase().as_str() {
                "on" | "true" | "yes" | "1" => Some(Self::Bool(true)),
                "off" | "false" | "no" | "0" => Some(Self::Bool(false)),
                _ => None,
            },
            Kind::Int { min, max } => text
                .trim()
                .parse::<i64>()
                .ok()
                .filter(|v| (min..=max).contains(v))
                .map(Self::Int),
            Kind::Choice => (!text.is_empty()).then(|| Self::Text(text.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profiles() -> [Profile; 2] {
        [
            Profile {
                name: "Night".into(),
                values: Vec::from([
                    ("core.night-mode".into(), "on".into()),
                    ("core.font-size".into(), "12".into()),
                ]),
            },
            Profile {
                name: "Large [print]".into(),
                values: Vec::from([("core.font-size".into(), "18".into())]),
            },
        ]
    }

    #[test]
    fn profiles_are_kept_whatever_their_names() {
        assert!(decode(b"").unwrap().is_empty());
        let profiles = profiles();
        let loaded = decode(encode(&profiles).as_bytes()).unwrap();
        assert_eq!(loaded, profiles, "names with brackets and spaces too");
    }

    #[test]
    fn the_file_of_version_2_is_read_as_it_was_written() {
        let loaded = decode(include_bytes!("../testdata/profiles.ini.v2")).unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].name, "Night reading");
        assert_eq!(loaded[1].name, "  Large [print] = big ");
        assert_eq!(loaded[2].name, "Quote \" and \\ back");
        assert_eq!(
            loaded[2].values,
            [("core.line-spacing".into(), String::new())]
        );
        // What this writes is what it read, byte for byte.
        assert_eq!(
            encode(&loaded).as_bytes(),
            include_bytes!("../testdata/profiles.ini.v2")
        );
    }

    #[test]
    fn files_of_version_1_are_read_and_written_as_version_2() {
        let loaded = decode(include_bytes!("../testdata/profiles.ini.v1")).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name, "Night");
        assert_eq!(
            loaded[0].values,
            [
                ("core.night-mode".into(), "on".into()),
                ("core.font-size".into(), "12".into()),
            ]
        );
        assert_eq!(loaded[1].name, "Large print");
        let text = encode(&loaded);
        assert!(text.starts_with("# format 2\n[profile.1]\nname = Night\n"));
        assert_eq!(decode(text.as_bytes()).unwrap(), loaded);
    }

    #[test]
    fn a_file_of_another_version_or_not_text_is_not_read() {
        assert_eq!(decode(b"# format 3\n[profile.1]\nname = x\n"), None);
        assert_eq!(decode(b"# format 0\n"), None);
        assert_eq!(decode(b"# format x\n"), None);
        assert_eq!(decode(&[0xff, 0xfe]), None);
    }

    #[test]
    fn names_are_tidied_and_compared_without_case() {
        assert_eq!(clean("  Night   [mode] "), Some("Night mode".into()));
        assert_eq!(clean(" [ ] "), None);
        assert!(same_name("NIGHT", "night"));
        assert!(!same_name("Night", "Nights"));
    }

    #[test]
    fn values_are_taken_only_where_the_setting_accepts_them() {
        let int = Kind::Int { min: 6, max: 40 };
        assert_eq!(Value::parse(Kind::Bool, "on"), Some(Value::Bool(true)));
        assert_eq!(Value::parse(Kind::Bool, "Off"), Some(Value::Bool(false)));
        assert_eq!(Value::parse(Kind::Bool, "maybe"), None);
        assert_eq!(Value::parse(int, "12"), Some(Value::Int(12)));
        assert_eq!(Value::parse(int, "41"), None);
        assert_eq!(Value::parse(int, "x"), None);
        assert_eq!(
            Value::parse(Kind::Choice, "core.contrast.strong"),
            Some(Value::Text("core.contrast.strong".into()))
        );
        assert_eq!(Value::parse(Kind::Choice, ""), None);
        assert_eq!(Value::Bool(true).to_text(), "on");
        assert_eq!(Value::Int(-3).to_text(), "-3");
    }
}
