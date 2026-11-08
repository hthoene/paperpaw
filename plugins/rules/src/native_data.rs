//! What the rules ask the registry: the actions there are and the switches and
//! numbers among the settings, with their translated titles.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    registry::{Registry, RegistrySettingKind},
};
use paperpaw_ui_kit::{rows, text::copy};

/// An action or a setting: its id and title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    pub id: String,
    pub title: String,
}

/// What kind of setting a row is, as far as rules care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    /// An on/off setting: a switch.
    Switch,
    /// A whole number (its bounds: [`Reader::bounds`]).
    Number,
}

/// A switch or a number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting {
    pub id: String,
    pub title: String,
    pub sort: Sort,
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
    /// Opens the registry; the buffers come first, so a refused allocation
    /// leaves no handle open.
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
    pub fn finish<T>(self, result: Result<T, Error>) -> Result<T, Error> {
        let released = self.registry.release(self.host);
        let value = result?;
        released?;
        Ok(value)
    }

    /// Every action: id and translated title.
    pub fn actions(&mut self) -> Result<Vec<Named>, Error> {
        let mut found = Vec::new();
        rows!(cursor => self.registry.actions(self.host, "", cursor, &mut self.input, &mut self.output), |row| {
            found.try_reserve(1).map_err(|_| Error::NoMemory)?;
            found.push(Named { id: copy(row.id())?, title: copy(row.title())? });
            Ok(())
        })?;
        Ok(found)
    }

    /// Every switch and number setting.
    pub fn settings(&mut self) -> Result<Vec<Setting>, Error> {
        let mut found = Vec::new();
        rows!(cursor => self.registry.settings(self.host, "", cursor, &mut self.input, &mut self.output), |row| {
            (|| {
                let sort = match row.kind() {
                    RegistrySettingKind::Bool => Sort::Switch,
                    RegistrySettingKind::Int => Sort::Number,
                    _ => return Ok(()),
                };
                found.try_reserve(1).map_err(|_| Error::NoMemory)?;
                found.push(Setting { id: copy(row.key())?, title: copy(row.title())?, sort });
                Ok(())
            })()
        })?;
        Ok(found)
    }

    /// The bounds of a number setting.
    pub fn bounds(&mut self, key: &str) -> Result<(i64, i64), Error> {
        let detail = self
            .registry
            .setting(self.host, key, &mut self.input, &mut self.output)?;
        let extra = detail.extra();
        Ok((i64::from(extra.min()), i64::from(extra.max())))
    }
}
