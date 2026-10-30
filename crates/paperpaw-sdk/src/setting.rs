//! Settings by id and type: a [`Setting<T>`] names a setting and says what
//! its value is, so reading and changing it are checked as the plugin is
//! built.
//!
//! ```
//! use paperpaw_sdk::setting::{Setting, Text};
//!
//! /// The text size of the plugin's page.
//! pub const SIZE: Setting<i64> = Setting::new("demo.size");
//! /// Whether it shows pictures.
//! pub const PICTURES: Setting<bool> = Setting::new("demo.pictures");
//! /// Which theme (a choice).
//! pub const THEME: Setting<Text> = Setting::new("demo.theme");
//!
//! // In a handler, with an `env`:
//! // let size: i64 = env.settings().read(&SIZE);
//! // out.push(Command::set(&PICTURES, false));
//! ```
//!
//! Settings by text (`env.settings().bool("demo.pictures")`) stay for what
//! is only known at run time, e.g. a settings page for any plugin.

use alloc::borrow::Cow;
use core::fmt;
use core::marker::PhantomData;

use crate::id::Id;
use crate::settings::SettingValue;

/// The type of a setting's value: [`bool`] (switches), [`i64`] (numbers)
/// or [`Text`] (choices, languages, lists and text).
pub trait SettingType: 'static {
    /// What reading gives.
    type Value<'a>;
    /// What a change takes.
    type Input;
    /// The value read from a setting's stored one (the type's empty value
    /// if there is none or it is of another type).
    fn read(value: Option<&SettingValue>) -> Self::Value<'_>;
    /// The stored value of a change.
    fn write(input: Self::Input) -> SettingValue;
}

impl SettingType for bool {
    type Value<'a> = bool;
    type Input = bool;

    fn read(value: Option<&SettingValue>) -> bool {
        matches!(value, Some(SettingValue::Bool(true)))
    }

    fn write(input: bool) -> SettingValue {
        SettingValue::Bool(input)
    }
}

impl SettingType for i64 {
    type Value<'a> = i64;
    type Input = i64;

    fn read(value: Option<&SettingValue>) -> i64 {
        match value {
            Some(SettingValue::Int(v)) => *v,
            _ => 0,
        }
    }

    fn write(input: i64) -> SettingValue {
        SettingValue::Int(input)
    }
}

/// The type of settings whose value is text: a choice (its option's id),
/// a language tag, a list of ids separated by commas, or text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Text {}

impl SettingType for Text {
    type Value<'a> = &'a str;
    type Input = Cow<'static, str>;

    fn read(value: Option<&SettingValue>) -> &str {
        match value {
            Some(SettingValue::Text(v)) => v,
            _ => "",
        }
    }

    fn write(input: Cow<'static, str>) -> SettingValue {
        SettingValue::Text(input)
    }
}

/// A setting: its id and the type of its value (see the
/// [module documentation](self)).
pub struct Setting<T: SettingType> {
    id: Id,
    value: PhantomData<fn() -> T>,
}

impl<T: SettingType> Setting<T> {
    /// The setting `id`, e.g. `reader.font-size`; checked as for
    /// [`Id::new_static`].
    pub const fn new(id: &'static str) -> Self {
        Self {
            id: Id::new_static(id),
            value: PhantomData,
        }
    }

    pub const fn id(&self) -> &Id {
        &self.id
    }

    pub fn as_str(&self) -> &str {
        self.id.as_str()
    }

    /// The id as text kept in the program, for a spec kept there too
    /// (e.g. `SettingSpec::switch(SHOW.static_str(), …)`, see
    /// a `static` list). Takes the
    /// setting, so it can be used in `const` and `static` items.
    pub const fn static_str(self) -> &'static str {
        // Settings are only made by `new`, from text in the program.
        let text = self.id.static_text();
        core::mem::forget(self);
        text
    }

    /// The stored value of `input`, e.g. for a default.
    pub fn value(&self, input: T::Input) -> SettingValue {
        T::write(input)
    }
}

impl<T: SettingType> Clone for Setting<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            value: PhantomData,
        }
    }
}

impl<T: SettingType> fmt::Debug for Setting<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Setting({})", self.id)
    }
}

impl<T: SettingType> fmt::Display for Setting<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.id, f)
    }
}

impl<T: SettingType> AsRef<str> for Setting<T> {
    fn as_ref(&self) -> &str {
        self.id.as_str()
    }
}

impl<T: SettingType> From<Setting<T>> for Id {
    fn from(setting: Setting<T>) -> Id {
        setting.id
    }
}

impl<T: SettingType> From<&Setting<T>> for Id {
    fn from(setting: &Setting<T>) -> Id {
        setting.id.clone()
    }
}

impl<T: SettingType> PartialEq<Id> for Setting<T> {
    fn eq(&self, other: &Id) -> bool {
        self.id == *other
    }
}

impl<T: SettingType> PartialEq<Setting<T>> for Id {
    fn eq(&self, other: &Setting<T>) -> bool {
        *self == other.id
    }
}

impl<T: SettingType> PartialEq for Setting<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T: SettingType> Eq for Setting<T> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{SettingKind, SettingSpec, Settings};

    const ON: Setting<bool> = Setting::new("a.on");
    const SIZE: Setting<i64> = Setting::new("a.size");
    const THEME: Setting<Text> = Setting::new("a.theme");

    #[test]
    fn read_by_type() {
        let mut settings = Settings::new();
        settings
            .define(SettingSpec::new(
                Id::from(ON),
                "on",
                SettingKind::Bool,
                ON.value(true),
                crate::builtin::groups::OTHER,
            ))
            .unwrap();
        settings
            .define(SettingSpec::new(
                Id::from(SIZE),
                "size",
                SettingKind::Int { min: 1, max: 20 },
                SIZE.value(9),
                crate::builtin::groups::OTHER,
            ))
            .unwrap();
        settings
            .define(SettingSpec::new(
                Id::from(THEME),
                "theme",
                SettingKind::Text,
                THEME.value(Cow::Borrowed("light")),
                crate::builtin::groups::OTHER,
            ))
            .unwrap();
        assert!(settings.read(&ON));
        assert_eq!(settings.read(&SIZE), 9);
        assert_eq!(settings.read(&THEME), "light");
        settings.set(SIZE.as_str(), SIZE.value(12)).unwrap();
        assert_eq!(settings.read(&SIZE), 12);
        // Unknown: the type's empty value.
        assert_eq!(settings.read(&Setting::<i64>::new("a.none")), 0);
        assert_eq!(Id::new_static("a.size"), SIZE);
    }
}
