//! What the guide reads from the host: the steps, a setting with its
//! choices, whether a book was opened, and the done mark in the data folder.
use crate::model::{Kind, PageKind, Setting, Step, Val};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    library::{Library, Query, Show},
    registry::{Registry, RegistrySettingKind, RegistrySetupPageKind},
    settings::SettingValue,
    storage::{self, Root},
};
use paperpaw_ui_kit::{
    mem::{buffer, push},
    rows,
    text::copy,
};

/// The file whose being there says the setup was done or skipped.
pub const DONE: &str = "done";
/// The file written before the guide opens: where it cannot be, the done
/// mark could not be either.
pub const TRIAL: &str = "trial";

/// The choices read for a setting at most (a time zone list is the longest).
const MAX_CHOICES: usize = 512;

/// Whether the data folder has `name`. A refusal other than "not there" counts as
/// there: the guide must not nag where the card cannot be read.
pub fn exists(host: &Host<'_>, name: &str) -> bool {
    !matches!(
        storage::size(host, Root::Data, name),
        Err(Error::NotFound)
    )
}

/// Notes that the setup was done or skipped. `false` where it cannot be written.
pub fn mark_done(host: &Host<'_>) -> bool {
    if storage::write(host, Root::Data, DONE, b"").is_err() {
        return false;
    }
    let _ = storage::remove(host, Root::Data, TRIAL);
    true
}

/// Whether a book was opened before: not a first start then.
///
/// # Errors
/// The library's refusal.
pub fn books_opened(host: &Host<'_>) -> Result<bool, Error> {
    let library = Library::open(host)?;
    let result = (|| {
        let mut input = buffer(256)?;
        let mut out = buffer(4096)?;
        for show in [Show::Reading, Show::Finished] {
            let page = library.query(host, &Query::new().show(show), &mut input, &mut out)?;
            if !page.is_empty() {
                return Ok(true);
            }
        }
        Ok(false)
    })();
    let released = library.release(host);
    let value = result?;
    released?;
    Ok(value)
}

fn value(v: Option<SettingValue<'_>>) -> Result<Option<Val>, Error> {
    Ok(match v {
        Some(SettingValue::Bool(v)) => Some(Val::Bool(v)),
        Some(SettingValue::Int(v)) => Some(Val::Int(v)),
        Some(SettingValue::Text(v)) => Some(Val::Text(copy(v)?)),
        _ => None,
    })
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
    /// Allocation and host refusals.
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
    /// `result`'s, else the release's.
    pub fn finish<T>(self, result: Result<T, Error>) -> Result<T, Error> {
        let released = self.registry.release(self.host);
        let value = result?;
        released?;
        Ok(value)
    }

    /// The steps that apply on this device now, by their order.
    ///
    /// # Errors
    /// Host and allocation refusals.
    pub fn steps(&mut self) -> Result<Vec<Step>, Error> {
        let mut found: Vec<(i32, Step)> = Vec::new();
        rows!(cursor => self.registry.setup(self.host, cursor, &mut self.input, &mut self.output), |row| {
            // Bit 0: applies now (rules of the host, section 6 of registry.md).
            let kind = match row.page_kind() {
                _ if row.flags() & 1 == 0 => None,
                RegistrySetupPageKind::Setting => Some(PageKind::Setting),
                RegistrySetupPageKind::Screen => Some(PageKind::Screen),
                _ => None,
            };
            match kind {
                Some(kind) => push(&mut found, (row.order(), Step {
                    id: copy(row.id())?,
                    kind,
                    page: copy(row.page())?,
                    title: copy(row.title())?,
                })),
                None => Ok(()),
            }
        })?;
        paperpaw_ui_kit::mem::stable_sort_by(&mut found, |a, b| a.0.cmp(&b.0));
        let mut steps = Vec::new();
        steps
            .try_reserve_exact(found.len())
            .map_err(|_| Error::NoMemory)?;
        steps.extend(found.into_iter().map(|(_, step)| step));
        Ok(steps)
    }

    /// The setting `key` in full: value, range, unit, help and choices.
    ///
    /// # Errors
    /// Host and allocation refusals.
    pub fn setting(&mut self, key: &str) -> Result<Setting, Error> {
        let detail = self
            .registry
            .setting(self.host, key, &mut self.input, &mut self.output)?;
        let row = detail.setting();
        let extra = detail.extra();
        let kind = match row.kind() {
            RegistrySettingKind::Bool => Kind::Bool,
            RegistrySettingKind::Int => Kind::Int,
            RegistrySettingKind::Choice | RegistrySettingKind::Language => Kind::Choice,
            _ => Kind::Other,
        };
        let mut setting = Setting {
            key: copy(row.key())?,
            kind,
            title: copy(row.title())?,
            help: copy(detail.help().unwrap_or(""))?,
            value: value(row.value())?,
            min: i64::from(extra.min()),
            max: i64::from(extra.max()),
            step: i64::from(extra.step()),
            unit: copy(extra.unit())?,
            choices: Vec::new(),
            editor: !extra.editor().is_empty(),
        };
        if kind == Kind::Choice {
            setting.choices = self.choices(key)?;
        }
        Ok(setting)
    }

    fn choices(&mut self, key: &str) -> Result<Vec<(String, String)>, Error> {
        let mut found = Vec::new();
        rows!(cursor => self.registry.choices(self.host, key, cursor, &mut self.input, &mut self.output), |row| {
            if found.len() >= MAX_CHOICES {
                Ok(())
            } else {
                push(&mut found, (copy(row.value())?, copy(row.title())?))
            }
        })?;
        Ok(found)
    }
}
