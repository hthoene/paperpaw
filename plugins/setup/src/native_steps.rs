//! What the guide reads of the registry: the steps that apply on this device
//! and, for the page shown, the setting it asks for with its choices.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    registry::{Registry, RegistrySettingKind, RegistrySetupPageKind},
    settings::SettingValue,
};
use paperpaw_ui_kit::{
    mem::{buffer, push, stable_sort_by},
    rows,
    text::{concat, copy, decimal_u64},
};

/// The flag of a step row that says it applies now.
const APPLIES: u32 = 1;
/// Page buffer for registry replies.
const PAGE: usize = 4096;

/// What a step shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Page {
    /// A setting of any plugin, by key.
    Setting(String),
    /// A screen of the step's plugin, by id.
    Screen(String),
}

/// A step of the guide, as the registry lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub id: String,
    /// Its own title; empty where the setting's title does.
    pub title: String,
    pub order: i32,
    pub page: Page,
}

/// An option of a choice: its value and its translated name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub value: String,
    pub title: String,
}

/// What kind of answer a setting takes, with its value as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Control {
    Switch(bool),
    Number {
        value: i64,
        min: i64,
        max: i64,
        step: i64,
    },
    /// One of the options (`at` is the current one, if it is among them).
    Choice {
        options: Vec<Choice>,
        at: Option<usize>,
    },
    /// Anything else: shown as its value, changed on its own editor.
    Value(String),
}

/// The setting a page asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting {
    pub key: String,
    pub title: String,
    pub help: String,
    /// The translated unit of a number.
    pub unit: String,
    /// The screen or slot that edits it, if it has one: `(is_slot, id)`.
    pub editor: Option<(bool, String)>,
    pub control: Control,
}

/// The open registry with transport buffers.
pub struct Reader<'h, 'cb> {
    host: &'h Host<'cb>,
    registry: Registry,
    input: Vec<u8>,
    output: Vec<u8>,
}

impl<'h, 'cb> Reader<'h, 'cb> {
    /// Opens the registry; the buffers come first, so a refused allocation
    /// leaves no handle open.
    pub fn open(host: &'h Host<'cb>) -> Result<Self, Error> {
        let input = buffer(512)?;
        let output = buffer(PAGE)?;
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

    /// The steps that apply now, in their order.
    pub fn steps(&mut self) -> Result<Vec<Step>, Error> {
        let mut found: Vec<Step> = Vec::new();
        rows!(cursor => self.registry.setup(self.host, cursor, &mut self.input, &mut self.output), |row| {
            if row.flags() & APPLIES == 0 {
                return Ok(());
            }
            let page = match row.page_kind() {
                RegistrySetupPageKind::Setting => Page::Setting(copy(row.page())?),
                RegistrySetupPageKind::Screen => Page::Screen(copy(row.page())?),
                _ => return Ok(()),
            };
            push(&mut found, Step {
                id: copy(row.id())?,
                title: copy(row.title())?,
                order: row.order(),
                page,
            })
        })?;
        stable_sort_by(&mut found, |a, b| a.order.cmp(&b.order));
        Ok(found)
    }

    /// The setting `key`: its metadata, help, value and options.
    pub fn setting(&mut self, key: &str) -> Result<Setting, Error> {
        let (kind, title, help, unit, editor, bounds, count, value) = {
            let detail =
                self.registry
                    .setting(self.host, key, &mut self.input, &mut self.output)?;
            let (row, extra) = (detail.setting(), detail.extra());
            let editor = match extra.editor_kind() {
                1 => Some((false, copy(extra.editor())?)),
                2 => Some((true, copy(extra.editor())?)),
                _ => None,
            };
            let value = match row.value() {
                Some(SettingValue::Bool(on)) => Value::Bool(on),
                Some(SettingValue::Int(number)) => Value::Int(number),
                Some(SettingValue::Text(text)) => Value::Text(copy(text)?),
                _ => Value::None,
            };
            (
                row.kind(),
                copy(row.title())?,
                copy(detail.help().unwrap_or(""))?,
                copy(extra.unit())?,
                editor,
                (
                    i64::from(extra.min()),
                    i64::from(extra.max()),
                    i64::from(extra.step()),
                ),
                extra.choices(),
                value,
            )
        };
        let control = match (kind, value) {
            (RegistrySettingKind::Bool, Value::Bool(on)) => Control::Switch(on),
            (RegistrySettingKind::Bool, _) => Control::Switch(false),
            (RegistrySettingKind::Int, value) => Control::Number {
                value: match value {
                    Value::Int(number) => number,
                    _ => bounds.0,
                },
                min: bounds.0,
                max: bounds.1,
                step: bounds.2.max(1),
            },
            (RegistrySettingKind::Choice | RegistrySettingKind::Language, value) if count > 0 => {
                let options = self.options(key)?;
                let current = match &value {
                    Value::Text(text) => options.iter().position(|o| o.value == *text),
                    _ => None,
                };
                Control::Choice {
                    options,
                    at: current,
                }
            }
            (_, Value::Text(text)) => Control::Value(text),
            _ => Control::Value(String::new()),
        };
        Ok(Setting {
            key: copy(key)?,
            title,
            help,
            unit,
            editor,
            control,
        })
    }

    fn options(&mut self, key: &str) -> Result<Vec<Choice>, Error> {
        let mut found = Vec::new();
        rows!(cursor => self.registry.choices(self.host, key, cursor, &mut self.input, &mut self.output), |row| {
            push(&mut found, Choice { value: copy(row.value())?, title: copy(row.title())? })
        })?;
        Ok(found)
    }
}

enum Value {
    None,
    Bool(bool),
    Int(i64),
    Text(String),
}

impl Setting {
    /// The name of the current value, as the card shows it.
    pub fn label(&self, on: &str, off: &str) -> Result<String, Error> {
        match &self.control {
            Control::Switch(true) => copy(on),
            Control::Switch(false) => copy(off),
            Control::Number { value, .. } => {
                let mut digits = [0; 20];
                let number = if *value < 0 {
                    concat(&["-", decimal_u64(value.unsigned_abs(), &mut digits)])?
                } else {
                    copy(decimal_u64(value.unsigned_abs(), &mut digits))?
                };
                if self.unit.is_empty() {
                    Ok(number)
                } else {
                    concat(&[&number, " ", &self.unit])
                }
            }
            Control::Choice { options, at } => match at.and_then(|at| options.get(at)) {
                Some(option) => copy(&option.title),
                None => Ok(String::new()),
            },
            Control::Value(text) => copy(text),
        }
    }

    /// The value after changing by `delta` (one option or step), as a command
    /// would set it; `None` where there is nothing to change to.
    #[must_use]
    pub fn next(&self, delta: i64) -> Option<NewValue> {
        match &self.control {
            Control::Switch(on) => Some(NewValue::Bool(!on)),
            Control::Number {
                value,
                min,
                max,
                step,
            } => {
                let moved = value.saturating_add(delta.saturating_mul(*step));
                let next = moved.clamp(*min, *max);
                (next != *value).then_some(NewValue::Int(next))
            }
            Control::Choice { options, at } => {
                let count = i64::try_from(options.len()).ok()?;
                if count == 0 {
                    return None;
                }
                let from = at.map_or(if delta < 0 { 0 } else { -1 }, |at| at as i64);
                let to = (from + delta).rem_euclid(count);
                Some(NewValue::Text(usize::try_from(to).ok()?))
            }
            Control::Value(_) => None,
        }
    }

    /// Applies `value` to the copy held, as the host will once it has the
    /// command.
    pub fn apply(&mut self, value: &NewValue) {
        match (&mut self.control, value) {
            (Control::Switch(on), NewValue::Bool(now)) => *on = *now,
            (Control::Number { value, .. }, NewValue::Int(now)) => *value = *now,
            (Control::Choice { at, .. }, NewValue::Text(index)) => *at = Some(*index),
            _ => {}
        }
    }
}

/// A value to set: a switch, a number, or the index of an option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewValue {
    Bool(bool),
    Int(i64),
    Text(usize),
}
