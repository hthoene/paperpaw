//! Settings, groups, menus and presets as the registry lists them, copied into
//! the owned types of [`crate::native_model`].
use crate::native_model::{Detail, Group, Kind, PageRow, Setting, Val};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    registry::{
        MenuQuery, PageCursor, Registry, RegistryMenuKind, RegistrySetting, RegistrySettingKind,
        RegistryValueSource, ValuesQuery,
    },
    settings::{self, SettingValue},
};
use paperpaw_ui_kit::{
    mem::{buffer, push},
    rows,
    text::copy,
};

/// A value as the settings editor keeps it.
fn value(v: Option<SettingValue<'_>>) -> Result<Option<Val>, Error> {
    Ok(match v {
        Some(SettingValue::Bool(v)) => Some(Val::Bool(v)),
        Some(SettingValue::Int(v)) => Some(Val::Int(v)),
        Some(SettingValue::Text(v)) => Some(Val::Text(copy(v)?)),
        _ => None,
    })
}

/// How much of a setting is copied out of its row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fields {
    /// Every field.
    Full,
    /// What decides whether it shows (key, group, kind, flags, source): no
    /// title and no value.
    Light,
}

fn setting(row: &RegistrySetting<'_>, fields: Fields) -> Result<Setting, Error> {
    let kind = match row.kind() {
        RegistrySettingKind::Bool => Kind::Bool,
        RegistrySettingKind::Int => Kind::Int,
        RegistrySettingKind::Choice => Kind::Choice,
        RegistrySettingKind::Text => Kind::Text,
        RegistrySettingKind::Language => Kind::Language,
        RegistrySettingKind::List => Kind::List,
        _ => Kind::Other,
    };
    let source = row.source();
    Ok(Setting {
        key: copy(row.key())?,
        group: copy(row.group())?,
        kind,
        flags: row.flags(),
        is_default: source == RegistryValueSource::Default,
        in_scope: source == RegistryValueSource::BookScope,
        title: if fields == Fields::Full {
            copy(row.title())?
        } else {
            String::new()
        },
        value: if fields == Fields::Full {
            value(row.value())?
        } else {
            None
        },
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

    /// The transport buffers (input, reply), for another service's call made
    /// while the reader is open: no second pair is held beside them.
    pub fn buffers(&mut self) -> (&mut [u8], &mut [u8]) {
        (&mut self.input, &mut self.output)
    }

    /// Releases the registry, whatever `result` is.
    pub fn finish<T>(self, result: Result<T, Error>) -> Result<T, Error> {
        let released = self.registry.release(self.host);
        let value = result?;
        released?;
        Ok(value)
    }

    /// Visits the settings of `scope` (empty for all) that `keep` takes, one
    /// page of the registry at a time: only the kept rows of the page in hand
    /// are copied, and `visit` may ask the registry for more (a detail) while
    /// no page is held. Nothing is kept after the visit, so a caller that wants
    /// few of the 250 settings holds few.
    ///
    /// # Errors
    /// The registry's refusals, `NoMemory`, and what `visit` returns.
    pub fn each_setting(
        &mut self,
        scope: &str,
        fields: Fields,
        keep: impl Fn(&RegistrySetting<'_>) -> bool,
        mut visit: impl FnMut(&mut Self, Setting) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let mut cursor = PageCursor::start();
        loop {
            let mut batch: Vec<Setting> = Vec::new();
            let next = {
                let page = self.registry.settings(
                    self.host,
                    scope,
                    cursor,
                    &mut self.input,
                    &mut self.output,
                )?;
                let wanted = page.rows().filter(|row| keep(row)).count();
                batch
                    .try_reserve_exact(wanted)
                    .map_err(|_| Error::NoMemory)?;
                for row in page.rows().filter(|row| keep(row)) {
                    batch.push(setting(&row, fields)?);
                }
                page.end().next_cursor()
            };
            for found in batch {
                visit(self, found)?;
            }
            match next {
                Some(next) => cursor = next,
                None => return Ok(()),
            }
        }
    }

    /// One setting by its key: its row, copied as `fields` says (`None`: the
    /// registry knows no such setting). One call, where finding it among the
    /// list's rows walks them all.
    ///
    /// # Errors
    /// The registry's refusals other than "not found", and `NoMemory`.
    pub fn setting_row(&mut self, key: &str, fields: Fields) -> Result<Option<Setting>, Error> {
        // No setting has an empty key: the registry refuses to look for one
        // (`Invalid`), which is no failure here.
        if key.is_empty() {
            return Ok(None);
        }
        match self
            .registry
            .setting(self.host, key, &mut self.input, &mut self.output)
        {
            Ok(detail) => setting(&detail.setting(), fields).map(Some),
            Err(Error::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Every group, parts included.
    pub fn groups(&mut self) -> Result<Vec<Group>, Error> {
        let mut found: Vec<Group> = Vec::new();
        let mut parents: Vec<String> = Vec::new();
        push(&mut parents, String::new())?;
        let mut at = 0;
        while at < parents.len() {
            let parent = copy(&parents[at])?;
            rows!(cursor => self.registry.groups(self.host, &parent, cursor, &mut self.input, &mut self.output), |row| {
                if !found.iter().any(|g| g.id == row.id()) {
                    push(&mut found, Group {
                        id: copy(row.id())?,
                        parent: copy(row.parent())?,
                        page: copy(row.page())?,
                        order: row.order(),
                        title: copy(row.title())?,
                    })?;
                    push(&mut parents, copy(row.id())?)?;
                }
                Ok(())
            })?;
            at += 1;
        }
        Ok(found)
    }

    /// A setting in full: its range, unit, editor and description.
    pub fn detail(&mut self, key: &str) -> Result<Detail, Error> {
        let detail = self
            .registry
            .setting(self.host, key, &mut self.input, &mut self.output)?;
        let extra = detail.extra();
        Ok(Detail {
            min: i64::from(extra.min()),
            max: i64::from(extra.max()),
            step: i64::from(extra.step()),
            unit: copy(extra.unit())?,
            section: copy(extra.section())?,
            editor_kind: extra.editor_kind(),
            editor: copy(extra.editor())?,
            choices: extra.choices(),
            help: copy(detail.help().unwrap_or(""))?,
        })
    }

    /// The options of a choice setting: value and translated name.
    pub fn choices(&mut self, key: &str) -> Result<Vec<(String, String)>, Error> {
        let mut found = Vec::new();
        rows!(cursor => self.registry.choices(self.host, key, cursor, &mut self.input, &mut self.output), |row| {
            push(&mut found, (copy(row.value())?, copy(row.title())?))
        })?;
        Ok(found)
    }

    /// The rows of a menu as the user sees them.
    pub fn menu(&mut self, menu: &str) -> Result<Vec<PageRow>, Error> {
        let mut found = Vec::new();
        rows!(cursor => self.registry.menu(self.host, menu, MenuQuery::Current, cursor, &mut self.input, &mut self.output), |row| {
            let section = row.kind() == RegistryMenuKind::Section;
            push(&mut found, PageRow {
                action: if section { String::new() } else { copy(row.item())? },
                section,
                order: row.order(),
                title: copy(row.title())?,
                icon: copy(row.icon())?,
            })
        })?;
        Ok(found)
    }

    /// The ids of the widgets.
    pub fn widget_ids(&mut self) -> Result<Vec<String>, Error> {
        let mut found = Vec::new();
        rows!(cursor => self.registry.widgets(self.host, "", cursor, &mut self.input, &mut self.output), |row| {
            push(&mut found, copy(row.id())?)
        })?;
        Ok(found)
    }

    /// The presets of a kind: id and name.
    pub fn presets(&mut self, kind: &str) -> Result<Vec<(String, String)>, Error> {
        let mut found = Vec::new();
        rows!(cursor => self.registry.presets(self.host, kind, cursor, &mut self.input, &mut self.output), |row| {
            push(&mut found, (copy(row.id())?, copy(row.title())?))
        })?;
        Ok(found)
    }

    /// The settings a placement (`widgets.clock@2`) sets itself; every other
    /// setting of its widget has the widget's own value.
    pub fn placement_keys(&mut self, placement: &str) -> Result<Vec<String>, Error> {
        let mut found = Vec::new();
        rows!(cursor => self.registry.values(self.host, ValuesQuery::Placement(placement), cursor, &mut self.input, &mut self.output), |row| {
            push(&mut found, copy(row.key())?)
        })?;
        Ok(found)
    }

    /// What a preset sets: setting and value (`None`: back to the default).
    pub fn preset_values(&mut self, preset: &str) -> Result<Vec<(String, Option<Val>)>, Error> {
        let mut found = Vec::new();
        rows!(cursor => self.registry.values(self.host, ValuesQuery::Preset(preset), cursor, &mut self.input, &mut self.output), |row| {
            let absent = row.flags() & 1 != 0;
            let v = if absent { None } else { value(row.value())? };
            push(&mut found, (copy(row.key())?, v))
        })?;
        Ok(found)
    }
}

/// Room for the longest value of a setting and its frame: the most `settings.read`
/// takes (`TEXT_MAX`, `max_bytes` 4096 in `abi.def`; a bigger output is
/// `ERR_INVALID`, which no screen of this plugin could open with), and the most it
/// gives (a value that does not fit it is `ERR_NO_SPACE`).
const VALUE_BUFFER: usize = 4096;

/// The value of one setting now (the effective one, previews included).
pub fn read(host: &Host<'_>, key: &str) -> Result<Option<Val>, Error> {
    // A text setting is up to 4,096 bytes, encoded with its frame.
    let mut bytes = paperpaw_ui_kit::mem::buffer(VALUE_BUFFER)?;
    value(Some(settings::read_setting(host, key, &mut bytes)?))
}

/// Whether a switch is on.
pub fn read_bool(host: &Host<'_>, key: &str) -> Result<bool, Error> {
    Ok(matches!(read(host, key)?, Some(Val::Bool(true))))
}
