//! The settings a profile keeps, read from and applied through the host.
use crate::store::{Kind, Profile, Value};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::builtin::{groups, settings};
use paperpaw_sdk::native::{
    Error, Host,
    commands::CommandSender,
    registry::{Registry, RegistrySettingKind},
    settings::SettingValue,
};
use paperpaw_ui_kit::{rows, text::copy};

/// The groups whose settings a profile keeps: everything about reading, the
/// display and the status lines.
/// A setting a profile keeps and its value now.
#[derive(Debug, PartialEq, Eq)]
pub struct Current {
    /// Its id.
    pub key: String,
    /// What kind of value it takes (an integer's bounds unknown: see
    /// [`Reader::bounds`]).
    pub kind: Kind,
    /// Its value now.
    pub value: Value,
}

/// An open registry with its transport buffers.
pub struct Reader<'h, 'cb> {
    host: &'h Host<'cb>,
    registry: Registry,
    input: Vec<u8>,
    output: Vec<u8>,
}

fn buffer(len: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
    bytes.resize(len, 0);
    Ok(bytes)
}

impl<'h, 'cb> Reader<'h, 'cb> {
    pub fn open(host: &'h Host<'cb>) -> Result<Self, Error> {
        // Buffers first: a refused allocation must not leave a registry handle open.
        let input = buffer(512)?;
        let output = buffer(4096)?;
        Ok(Self {
            host,
            registry: Registry::open(host)?,
            input,
            output,
        })
    }

    /// Releases the registry, whatever `result` is.
    pub fn finish<T>(self, result: Result<T, Error>) -> Result<T, Error> {
        let released = self.registry.release(self.host);
        let value = result?;
        released?;
        Ok(value)
    }

    /// The groups within the ones profiles keep, those included.
    fn kept_groups(&mut self) -> Result<Vec<String>, Error> {
        let mut found: Vec<String> = Vec::new();
        let roots = [groups::READING, groups::DISPLAY, groups::STATUS_LINES];
        for root in &roots {
            found.try_reserve(1).map_err(|_| Error::NoMemory)?;
            found.push(copy(root.as_str())?);
        }
        let mut at = 0;
        while at < found.len() {
            let parent = copy(&found[at])?;
            rows!(cursor => self.registry.groups(self.host, &parent, cursor, &mut self.input, &mut self.output), |row| {
                if !found.iter().any(|g| g == row.id()) {
                    found.try_reserve(1).map_err(|_| Error::NoMemory)?;
                    found.push(copy(row.id())?);
                }
                Ok(())
            })?;
            at += 1;
        }
        Ok(found)
    }

    /// The settings a profile keeps and their values now.
    pub fn current(&mut self) -> Result<Vec<Current>, Error> {
        let kept = self.kept_groups()?;
        // Not ways of reading, whatever their group.
        let (scope_only, notice) = (settings::SCOPE_ONLY, settings::NOTICE_SECONDS);
        let left_out = [scope_only.as_str(), notice.as_str()];
        let mut found = Vec::new();
        rows!(cursor => self.registry.settings(self.host, "", cursor, &mut self.input, &mut self.output), |row| {
            (|| {
                let kind = match row.kind() {
                    RegistrySettingKind::Bool => Kind::Bool,
                    RegistrySettingKind::Int => Kind::Int { min: i64::MIN, max: i64::MAX },
                    RegistrySettingKind::Choice | RegistrySettingKind::List => Kind::Choice,
                    // Folders and languages belong to the device, not to a way of reading.
                    _ => return Ok(()),
                };
                if !kept.iter().any(|g| g == row.group()) || left_out.contains(&row.key()) {
                    return Ok(());
                }
                let value = match row.value() {
                    Some(SettingValue::Bool(v)) => Value::Bool(v),
                    Some(SettingValue::Int(v)) => Value::Int(v),
                    Some(SettingValue::Text(v)) => Value::Text(copy(v)?),
                    _ => return Ok(()),
                };
                found.try_reserve(1).map_err(|_| Error::NoMemory)?;
                found.push(Current { key: copy(row.key())?, kind, value });
                Ok(())
            })()
        })?;
        Ok(found)
    }

    /// The bounds of an integer setting.
    pub fn bounds(&mut self, key: &str) -> Result<(i64, i64), Error> {
        let detail = self
            .registry
            .setting(self.host, key, &mut self.input, &mut self.output)?;
        let extra = detail.extra();
        Ok((i64::from(extra.min()), i64::from(extra.max())))
    }

    /// Whether `value` is one of the choices of setting `key`.
    fn accepts(&mut self, key: &str, value: &str) -> Result<bool, Error> {
        let mut found = false;
        rows!(cursor => self.registry.choices(self.host, key, cursor, &mut self.input, &mut self.output), |choice| {
            found |= choice.value() == value;
            Ok(())
        })?;
        Ok(found)
    }

    /// What applying `profile` would change: only settings that differ, and
    /// only values their settings accept.
    pub fn differences(
        &mut self,
        profile: &Profile,
        current: &[Current],
    ) -> Result<Vec<(String, Value)>, Error> {
        differences(self, profile, current)
    }
}

/// What a profile's values are checked against: the bounds of an integer
/// setting and the choices of a choice setting.
pub trait Limits {
    /// The bounds of an integer setting.
    fn bounds(&mut self, key: &str) -> Result<(i64, i64), Error>;
    /// Whether `value` is one of the choices of the setting `key`.
    fn accepts(&mut self, key: &str, value: &str) -> Result<bool, Error>;
}

impl Limits for Reader<'_, '_> {
    fn bounds(&mut self, key: &str) -> Result<(i64, i64), Error> {
        Reader::bounds(self, key)
    }
    fn accepts(&mut self, key: &str, value: &str) -> Result<bool, Error> {
        Reader::accepts(self, key, value)
    }
}

/// What applying `profile` would change: only settings that differ and only
/// values their settings accept. A stale value (a choice that was removed, an
/// integer out of bounds, a word that is no boolean) is skipped, as in
/// interface 6, and the rest of the profile still applies.
pub fn differences(
    limits: &mut impl Limits,
    profile: &Profile,
    current: &[Current],
) -> Result<Vec<(String, Value)>, Error> {
    let mut changes = Vec::new();
    for (key, text) in &profile.values {
        let Some(now) = current.iter().find(|c| c.key == *key) else {
            continue;
        };
        let differs = !now.value.text_is(text);
        let mut kind = now.kind;
        // Limits only where the value differs: most do not.
        if differs && matches!(kind, Kind::Int { .. }) {
            let (min, max) = limits.bounds(key)?;
            kind = Kind::Int { min, max };
        }
        let Some(value) = Value::parse(kind, text)? else {
            continue;
        };
        if differs && matches!(kind, Kind::Choice) && !limits.accepts(key, text)? {
            continue;
        }
        if value != now.value {
            changes.try_reserve(1).map_err(|_| Error::NoMemory)?;
            changes.push((copy(key)?, value));
        }
    }
    Ok(changes)
}

/// Where setting changes are staged.
pub trait Stage {
    /// Stages one change.
    fn set(&mut self, key: &str, value: SettingValue<'_>) -> Result<(), Error>;
}

impl Stage for CommandSender<'_, '_> {
    fn set(&mut self, key: &str, value: SettingValue<'_>) -> Result<(), Error> {
        self.set_setting(key, value)
    }
}

/// Stages the setting changes, then runs `after` (the notes that it was done).
/// The first change the stage refuses (`NoSpace` over its command or byte
/// budget) is *returned*, and `after` does not run: the callback returns that
/// error, so the host drops everything staged in it and a profile is applied
/// whole or not at all.
pub fn commit<S: Stage>(
    stage: &mut S,
    changes: &[(String, Value)],
    after: impl FnOnce(&mut S) -> Result<(), Error>,
) -> Result<(), Error> {
    apply(stage, changes)?;
    after(stage)
}

/// Stages the setting changes, stopping at the first the stage refuses.
fn apply(stage: &mut impl Stage, changes: &[(String, Value)]) -> Result<(), Error> {
    for (key, value) in changes {
        match value {
            Value::Bool(v) => stage.set(key, SettingValue::Bool(*v))?,
            Value::Int(v) => stage.set(key, SettingValue::Int(*v))?,
            Value::Text(v) => stage.set(key, SettingValue::Text(v))?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    /// A setting the profile may carry and what it accepts.
    struct Fake {
        choices: Vec<(&'static str, Vec<&'static str>)>,
        asked: Vec<String>,
    }

    impl Limits for Fake {
        fn bounds(&mut self, _key: &str) -> Result<(i64, i64), Error> {
            Ok((6, 40))
        }
        fn accepts(&mut self, key: &str, value: &str) -> Result<bool, Error> {
            self.asked.push(key.to_string());
            Ok(self
                .choices
                .iter()
                .any(|(k, values)| *k == key && values.contains(&value)))
        }
    }

    fn now(key: &str, kind: Kind, value: Value) -> Current {
        Current {
            key: key.to_string(),
            kind,
            value,
        }
    }

    fn profile(values: &[(&str, &str)]) -> Profile {
        Profile {
            name: "Night".to_string(),
            values: values
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    #[test]
    fn a_stale_choice_is_skipped_and_the_rest_of_the_profile_applies() {
        let current = [
            now("a.font", Kind::Choice, Value::Text("sans".into())),
            now("a.contrast", Kind::Choice, Value::Text("normal".into())),
            now(
                "a.size",
                Kind::Int {
                    min: i64::MIN,
                    max: i64::MAX,
                },
                Value::Int(12),
            ),
            now("a.bold", Kind::Bool, Value::Bool(false)),
        ];
        let mut limits = Fake {
            choices: vec![
                ("a.font", vec!["sans", "serif"]),
                ("a.contrast", vec!["normal", "strong"]),
            ],
            asked: Vec::new(),
        };
        // "removed-font" is no choice any more; the others are fine.
        let kept = profile(&[
            ("a.font", "removed-font"),
            ("a.contrast", "strong"),
            ("a.size", "18"),
            ("a.bold", "on"),
        ]);
        let changes = differences(&mut limits, &kept, &current).unwrap();
        let keys: Vec<&str> = changes.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["a.contrast", "a.size", "a.bold"]);
        assert_eq!(changes[0].1, Value::Text("strong".into()));
    }

    #[test]
    fn a_choice_that_did_not_change_is_not_looked_up() {
        let current = [now("a.font", Kind::Choice, Value::Text("gone".into()))];
        let mut limits = Fake {
            choices: Vec::new(),
            asked: Vec::new(),
        };
        let same = profile(&[("a.font", "gone")]);
        assert!(
            differences(&mut limits, &same, &current)
                .unwrap()
                .is_empty()
        );
        assert!(limits.asked.is_empty());
    }

    #[test]
    fn an_empty_choice_is_skipped_without_a_lookup() {
        let current = [now("a.font", Kind::Choice, Value::Text("sans".into()))];
        let mut limits = Fake {
            choices: vec![("a.font", vec!["sans"])],
            asked: Vec::new(),
        };
        let blank = profile(&[("a.font", "")]);
        assert!(
            differences(&mut limits, &blank, &current)
                .unwrap()
                .is_empty()
        );
        assert!(limits.asked.is_empty());
    }

    /// A stage with a byte budget like the host's: keys and text count.
    struct Budget {
        left: usize,
        staged: Vec<String>,
    }

    impl Stage for Budget {
        fn set(&mut self, key: &str, value: SettingValue<'_>) -> Result<(), Error> {
            let cost = key.len()
                + match value {
                    SettingValue::Text(v) => v.len(),
                    _ => 0,
                };
            self.left = self.left.checked_sub(cost).ok_or(Error::NoSpace)?;
            self.staged.push(key.to_string());
            Ok(())
        }
    }

    #[test]
    fn a_profile_over_the_byte_budget_returns_no_space_and_notes_nothing() {
        let changes = [
            ("a.one".to_string(), Value::Bool(true)),
            ("a.two".to_string(), Value::Text("x".repeat(60))),
            ("a.three".to_string(), Value::Int(7)),
        ];
        let mut stage = Budget {
            left: 40,
            staged: Vec::new(),
        };
        let mut noted = false;
        let result = commit(&mut stage, &changes, |_| {
            noted = true;
            Ok(())
        });
        // The callback's result is the error, so the host drops "a.one"
        // too (commands.md: submit 1); the note is not made.
        assert_eq!(result, Err(Error::NoSpace));
        assert!(!noted);
        assert_eq!(stage.staged, ["a.one"]);
        let mut roomy = Budget {
            left: 1000,
            staged: Vec::new(),
        };
        let result = commit(&mut roomy, &changes, |_| {
            noted = true;
            Ok(())
        });
        assert_eq!(result, Ok(()));
        assert!(noted);
        assert_eq!(roomy.staged, ["a.one", "a.two", "a.three"]);
    }
}
