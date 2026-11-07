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

    /// What applying `profile` would change: only settings that differ, and
    /// only values their settings accept.
    pub fn differences(
        &mut self,
        profile: &Profile,
        current: &[Current],
    ) -> Result<Vec<(String, Value)>, Error> {
        let mut changes = Vec::new();
        for (key, text) in &profile.values {
            let Some(now) = current.iter().find(|c| c.key == *key) else {
                continue;
            };
            let mut kind = now.kind;
            // Bounds only where the value differs: most do not.
            if matches!(kind, Kind::Int { .. }) && !now.value.text_is(text) {
                let (min, max) = self.bounds(key)?;
                kind = Kind::Int { min, max };
            }
            let Some(value) = Value::parse(kind, text)? else {
                continue;
            };
            if value != now.value {
                changes.try_reserve(1).map_err(|_| Error::NoMemory)?;
                changes.push((copy(key)?, value));
            }
        }
        Ok(changes)
    }
}

/// Stages the setting changes.
pub fn apply(
    commands: &mut CommandSender<'_, '_>,
    changes: &[(String, Value)],
) -> Result<(), Error> {
    for (key, value) in changes {
        match value {
            Value::Bool(v) => commands.set_setting(key, SettingValue::Bool(*v))?,
            Value::Int(v) => commands.set_setting(key, SettingValue::Int(*v))?,
            Value::Text(v) => commands.set_setting(key, SettingValue::Text(v))?,
        }
    }
    Ok(())
}
