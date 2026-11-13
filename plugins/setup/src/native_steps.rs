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
    text::copy,
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
    /// The translated unit of a number, as text that holds `{count}` (the form for
    /// 1, and the others).
    pub unit_one: String,
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
            (|| {
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
            })()
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
        let (unit_one, unit) = if unit.is_empty() {
            (String::new(), String::new())
        } else {
            paperpaw_ui_kit::Messages::new()?.unit_forms(self.host, &unit)?
        };
        Ok(Setting {
            key: copy(key)?,
            title,
            help,
            unit_one,
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
                let unit = if *value == 1 {
                    &self.unit_one
                } else {
                    &self.unit
                };
                paperpaw_ui_kit::text::with_unit(*value, unit)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(values: &[&str], at: Option<usize>) -> Setting {
        Setting {
            key: "core.language".into(),
            title: "Language".into(),
            help: String::new(),
            unit_one: String::new(),
            unit: String::new(),
            editor: None,
            control: Control::Choice {
                options: values
                    .iter()
                    .map(|v| Choice {
                        value: (*v).into(),
                        title: v.to_uppercase(),
                    })
                    .collect(),
                at,
            },
        }
    }

    fn number(value: i64) -> Setting {
        Setting {
            key: "core.font-size".into(),
            title: "Size".into(),
            help: String::new(),
            unit_one: "pt".into(),
            unit: "pt".into(),
            editor: None,
            control: Control::Number {
                value,
                min: 8,
                max: 24,
                step: 2,
            },
        }
    }

    #[test]
    fn a_switch_toggles() {
        let mut s = Setting {
            control: Control::Switch(false),
            ..number(0)
        };
        assert_eq!(s.next(1), Some(NewValue::Bool(true)));
        s.apply(&NewValue::Bool(true));
        assert_eq!(s.next(-1), Some(NewValue::Bool(false)));
        assert_eq!(s.label("On", "Off").unwrap(), "On");
    }

    #[test]
    fn a_number_moves_by_its_step_and_stops_at_its_bounds() {
        assert_eq!(number(12).next(1), Some(NewValue::Int(14)));
        assert_eq!(number(12).next(-1), Some(NewValue::Int(10)));
        assert_eq!(number(24).next(1), None);
        assert_eq!(number(8).next(-1), None);
        assert_eq!(number(23).next(1), Some(NewValue::Int(24)));
        // A value beyond the bound comes back to it, without overflowing.
        assert_eq!(number(i64::MAX).next(1), Some(NewValue::Int(24)));
        assert_eq!(number(12).label("On", "Off").unwrap(), "12 pt");
        assert_eq!(number(-3).label("On", "Off").unwrap(), "-3 pt");
    }

    #[test]
    fn a_number_says_its_unit_in_the_form_its_count_takes() {
        let pages = |value| Setting {
            unit_one: "{count} page".into(),
            unit: "{count} pages".into(),
            ..number(value)
        };
        assert_eq!(pages(6).label("On", "Off").unwrap(), "6 pages");
        assert_eq!(pages(1).label("On", "Off").unwrap(), "1 page");
        assert_eq!(pages(0).label("On", "Off").unwrap(), "0 pages");
    }

    #[test]
    fn a_choice_wraps_around_and_starts_at_the_ends_from_nothing() {
        assert_eq!(
            choice(&["en", "de"], Some(0)).next(1),
            Some(NewValue::Text(1))
        );
        assert_eq!(
            choice(&["en", "de"], Some(1)).next(1),
            Some(NewValue::Text(0))
        );
        assert_eq!(
            choice(&["en", "de"], Some(0)).next(-1),
            Some(NewValue::Text(1))
        );
        assert_eq!(
            choice(&["en", "de", "fr"], None).next(1),
            Some(NewValue::Text(0))
        );
        assert_eq!(
            choice(&["en", "de", "fr"], None).next(-1),
            Some(NewValue::Text(2))
        );
        assert_eq!(choice(&[], None).next(1), None);
        let mut s = choice(&["en", "de"], Some(0));
        s.apply(&NewValue::Text(1));
        assert_eq!(s.label("On", "Off").unwrap(), "DE");
    }

    #[test]
    fn an_editor_value_has_nothing_to_step() {
        let s = Setting {
            control: Control::Value("Europe/Berlin".into()),
            ..number(0)
        };
        assert_eq!(s.next(1), None);
        assert_eq!(s.label("On", "Off").unwrap(), "Europe/Berlin");
    }

    #[test]
    fn labels_and_steps_survive_refused_allocations() {
        let s = number(12);
        crate::fail_alloc::sweep(|| s.label("On", "Off").map(|l| l.len()));
        let c = choice(&["en", "de"], Some(0));
        crate::fail_alloc::sweep(|| c.label("On", "Off").map(|l| l.len()));
    }
}
