//! The rows of a menu from the registry (`core.registry` op 12), copied into
//! the owned rows of [`crate::native_model`] with fallible allocation.
use crate::native_model::{Kind, Row, SettingInfo, SettingKind, Value};
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error, Host,
    registry::{MenuQuery, Registry, RegistryMenuKind, RegistryMenuRow, RegistrySettingKind},
    settings::SettingValue,
};
use paperpaw_ui_kit::{
    mem::{buffer, push},
    rows,
    text::copy,
};

/// A row of the registry, owned. A kind a newer host adds is left out by the
/// caller (`None`).
fn row(row: &RegistryMenuRow<'_>) -> Result<Option<Row>, Error> {
    let kind = match row.kind() {
        RegistryMenuKind::Action => Kind::Action,
        RegistryMenuKind::Setting => Kind::Setting,
        RegistryMenuKind::Submenu => Kind::Submenu,
        RegistryMenuKind::Section => Kind::Section,
        _ => return Ok(None),
    };
    Ok(Some(Row {
        item: if kind == Kind::Section {
            alloc::string::String::new()
        } else {
            copy(row.item())?
        },
        kind,
        flags: row.flags(),
        title: copy(row.title())?,
        icon: copy(row.icon())?,
        badge: copy(row.badge())?,
        describe: copy(row.describe())?,
    }))
}

/// An open registry with its transport buffers.
pub struct Reader<'h, 'cb> {
    host: &'h Host<'cb>,
    registry: Registry,
    input: Vec<u8>,
    output: Vec<u8>,
}

impl<'h, 'cb> Reader<'h, 'cb> {
    /// Buffers first: a refused allocation must not leave a handle open.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals.
    pub fn open(host: &'h Host<'cb>) -> Result<Self, Error> {
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
    ///
    /// # Errors
    /// The error of `result`, else of the release.
    pub fn finish<T>(self, result: Result<T, Error>) -> Result<T, Error> {
        let released = self.registry.release(self.host);
        let value = result?;
        released?;
        Ok(value)
    }

    /// The rows `menu` shows now: the user's own definition if there is one,
    /// else the declared rows in order, without those hidden or needing what
    /// the device lacks.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals; nothing is returned in part.
    pub fn menu(&mut self, menu: &str) -> Result<Vec<Row>, Error> {
        let mut found: Vec<Row> = Vec::new();
        rows!(cursor => self.registry.menu(self.host, menu, MenuQuery::Current, cursor, &mut self.input, &mut self.output), |r| {
            match row(&r)? {
                Some(one) => push(&mut found, one),
                None => Ok(()),
            }
        })?;
        Ok(found)
    }

    /// The values of choice setting `key`, in order.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals.
    pub fn choice_values(&mut self, key: &str) -> Result<Vec<alloc::string::String>, Error> {
        let mut found = Vec::new();
        rows!(cursor => self.registry.choices(self.host, key, cursor, &mut self.input, &mut self.output), |c| {
            push(&mut found, copy(c.value())?)
        })?;
        Ok(found)
    }

    /// The setting `key` with what its row needs; `None` for an id that is
    /// no setting (the host answers `NotFound`).
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals but `NotFound`.
    pub fn setting(&mut self, key: &str) -> Result<Option<SettingInfo>, Error> {
        let found = match self
            .registry
            .setting(self.host, key, &mut self.input, &mut self.output)
        {
            Ok(found) => found,
            Err(Error::NotFound) => return Ok(None),
            Err(other) => return Err(other),
        };
        let basic = found.setting();
        let kind = match basic.kind() {
            RegistrySettingKind::Bool => SettingKind::Bool,
            RegistrySettingKind::Int => SettingKind::Int,
            RegistrySettingKind::Choice => SettingKind::Choice,
            RegistrySettingKind::Text => SettingKind::Text,
            RegistrySettingKind::Language => SettingKind::Language,
            RegistrySettingKind::List => SettingKind::List,
            _ => SettingKind::Other,
        };
        let extra = found.extra();
        Ok(Some(SettingInfo {
            key: copy(key)?,
            kind,
            flags: basic.flags(),
            value: match basic.value() {
                Some(SettingValue::Bool(v)) => Some(Value::Bool(v)),
                Some(SettingValue::Int(v)) => Some(Value::Int(v)),
                Some(SettingValue::Text(v)) => Some(Value::Text(copy(v)?)),
                _ => None,
            },
            min: i64::from(extra.min()),
            max: i64::from(extra.max()),
            step: i64::from(extra.step()),
            unit: copy(extra.unit())?,
            editor_kind: extra.editor_kind(),
            editor: copy(extra.editor())?,
            choices: extra.choices(),
        }))
    }
}
