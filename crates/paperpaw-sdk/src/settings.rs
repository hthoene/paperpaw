use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use crate::collections::VecMap;
use crate::id::Id;
use crate::plugin::RegisterError;
use specs::Specs;

/// A setting's value.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SettingValue {
    Bool(bool),
    Int(i64),
    Text(Cow<'static, str>),
}

/// The type and valid range of a setting. Settings UIs are generated from it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SettingKind {
    Bool,
    Int {
        min: i64,
        max: i64,
    },
    /// One of a fixed set of values.
    Choice(Choices),
    Text,
    /// A UI language tag; settings UIs offer the installed translations.
    Language,
    /// Ids separated by commas, e.g. the status items of a status line;
    /// shown by their names, chosen on the setting's editor screen.
    List,
    /// A kind a later build adds (a time, a colour, a slider), by its key,
    /// with its parameters as names and values (`("step", "5")`): data that
    /// a `static` list of settings can hold. A build that doesn't know it
    /// shows the setting read-only, its value as text.
    Extended {
        key: crate::Id,
        params: &'static [(&'static str, &'static str)],
    },
}

/// The values of a [`SettingKind::Choice`], each the translation key of
/// its name: a list in the program ([`Choices::Fixed`], costing no
/// memory) or one made at run time.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Choices {
    Fixed(&'static [&'static str]),
    Owned(Vec<Cow<'static, str>>),
}

impl Choices {
    /// The values in their order.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        let (fixed, owned): (&[&str], &[Cow<'static, str>]) = match self {
            Choices::Fixed(values) => (values, &[]),
            Choices::Owned(values) => (&[], values),
        };
        fixed
            .iter()
            .copied()
            .chain(owned.iter().map(|v| v.as_ref()))
    }

    pub fn len(&self) -> usize {
        match self {
            Choices::Fixed(values) => values.len(),
            Choices::Owned(values) => values.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Value `index`.
    pub fn get(&self, index: usize) -> Option<&str> {
        match self {
            Choices::Fixed(values) => values.get(index).copied(),
            Choices::Owned(values) => values.get(index).map(|v| v.as_ref()),
        }
    }

    /// Where `value` is among them.
    pub fn position(&self, value: &str) -> Option<usize> {
        self.iter().position(|v| v == value)
    }

    pub fn contains(&self, value: &str) -> bool {
        self.position(value).is_some()
    }
}

impl From<Vec<Cow<'static, str>>> for Choices {
    fn from(values: Vec<Cow<'static, str>>) -> Self {
        Choices::Owned(values)
    }
}

impl FromIterator<Cow<'static, str>> for Choices {
    fn from_iter<I: IntoIterator<Item = Cow<'static, str>>>(values: I) -> Self {
        Choices::Owned(values.into_iter().collect())
    }
}

impl From<&'static [&'static str]> for Choices {
    fn from(values: &'static [&'static str]) -> Self {
        Choices::Fixed(values)
    }
}

impl SettingKind {
    /// Parses a value as written in a configuration file.
    pub fn parse(&self, text: &str) -> Option<SettingValue> {
        let value = match self {
            SettingKind::Bool => SettingValue::Bool(match text {
                "true" | "yes" | "on" | "1" => true,
                "false" | "no" | "off" | "0" => false,
                _ => return None,
            }),
            SettingKind::Int { .. } => SettingValue::Int(text.parse().ok()?),
            SettingKind::Choice(_)
            | SettingKind::Text
            | SettingKind::Language
            | SettingKind::List
            | SettingKind::Extended { .. } => SettingValue::Text(Cow::Owned(String::from(text))),
        };
        self.accepts(&value).then_some(value)
    }

    pub fn accepts(&self, value: &SettingValue) -> bool {
        match (self, value) {
            (SettingKind::Bool, SettingValue::Bool(_)) => true,
            (SettingKind::Int { min, max }, SettingValue::Int(v)) => (min..=max).contains(&v),
            (SettingKind::Choice(options), SettingValue::Text(v)) => options.contains(v),
            // A kind of a later build keeps its value as text, unchecked.
            (
                SettingKind::Text | SettingKind::List | SettingKind::Extended { .. },
                SettingValue::Text(_),
            ) => true,
            (SettingKind::Language, SettingValue::Text(v)) => {
                !v.is_empty() && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            }
            _ => false,
        }
    }
}

impl SettingValue {
    /// The value as written in configuration files.
    pub fn to_config_text(&self) -> String {
        match self {
            SettingValue::Bool(true) => String::from("on"),
            SettingValue::Bool(false) => String::from("off"),
            SettingValue::Int(v) => alloc::format!("{v}"),
            SettingValue::Text(v) => String::from(v.as_ref()),
        }
    }
}

/// A setting contribution, made with [`SettingSpec::new`] or the typed
/// constructors ([`SettingSpec::switch`] and the others) and the builder
/// methods.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SettingSpec {
    /// `<plugin>.<name>`; in configuration files written as `[<plugin>] <name> = …`.
    pub id: Id,
    /// Translation key of the setting's display name.
    pub title: Cow<'static, str>,
    pub kind: SettingKind,
    pub default: SettingValue,
    /// The group it is shown in, e.g.
    /// [`builtin::groups::READING`](crate::builtin::groups::READING).
    pub group: Id,
    pub level: Level,
    /// Translation key of a short help text.
    pub description: Option<Cow<'static, str>>,
    /// May differ per scope, e.g. per book (see [`Settings::scope`]).
    pub scoped: bool,
    /// It keeps the device safe (a lock's): changed by its own plugin and
    /// the settings editor alone, never by another plugin's defaults,
    /// presets or commands ([`SettingSpec::guarded`]).
    pub guarded: bool,
    /// The reader's own, not a matter of taste: an account, a folder, a
    /// list typed in. "Reset to defaults" and "Reset all" leave
    /// it as it is, as they leave every secret
    /// ([`SettingSpec::kept_on_reset`], [`SettingSpec::is_kept_on_reset`]).
    pub kept_on_reset: bool,
    /// What few settings have ([`SettingSpec::extra`]), kept apart: the
    /// others take less memory.
    extra: Option<Extra>,
}

/// Where a spec's [`SettingExtra`] is: in the program, or on the heap
/// for a setting made from data ([`SettingSpec::with_owned_extra`]).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Extra {
    Fixed(&'static SettingExtra),
    Owned(Box<SettingExtra>),
}

/// Where a setting is edited (see [`SettingExtra::editor`]), opened with
/// the parameter [`ID`](crate::builtin::params::ID), the setting's id.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Editor {
    /// A screen of a plugin's, e.g. one that asks for a code.
    Screen(Id),
    /// Whatever screen fills a slot, e.g.
    /// [`CHOOSE_FOLDER`](crate::builtin::slots::CHOOSE_FOLDER), so that a
    /// plugin taking the slot's place edits the setting too.
    Slot(Id),
}

impl Editor {
    /// The command that opens it for setting `setting`.
    pub fn open(&self, setting: &Id) -> crate::Command {
        let params =
            crate::Params::new().with(crate::builtin::params::ID, String::from(setting.as_str()));
        match self {
            Editor::Screen(screen) => crate::Command::OpenScreen(screen.clone(), params),
            Editor::Slot(slot) => crate::Command::OpenSlot(slot.clone(), params),
        }
    }
}

/// What few settings have (see [`SettingSpec::extra`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SettingExtra {
    /// Translation key that formats a number with its unit, with plural
    /// forms, e.g. `core.unit.pt.other = {count} pt`.
    pub unit: Option<Cow<'static, str>>,
    /// Where the value is edited instead of on the settings page, e.g.
    /// a folder chooser for a path ([`Editor::open`] opens it).
    pub editor: Option<Editor>,
    /// How much one step changes a number (e.g. 10 for percentages).
    pub step: i64,
    /// A number better set by eye than by its value, such as the line
    /// spacing or a light's brightness: on touch screens settings pages
    /// and panels show it as a slider ([`ui::slider`](crate::ui::slider)),
    /// in its steps; with buttons alone it keeps `−` and `+`.
    pub slider: bool,
    /// Translation key of the part of its group it belongs to (e.g.
    /// [`group_sections::FONT`](crate::builtin::group_sections::FONT)),
    /// shown as a heading above it in large groups.
    pub section: Option<Cow<'static, str>>,
    /// Its part is the name of the plugin its id ends in
    /// (`core.network.weather`: the weather plugin's), for settings made
    /// for each plugin: one fixed extra serves them all
    /// ([`SettingSpec::section_key`]).
    pub section_of_plugin: bool,
    /// A secret such as an API key: shown as dots, entered afresh rather
    /// than edited, and kept with the secrets, not the settings: read it
    /// with `env.secrets().get(id)` (see [`secrets`](crate::secrets)).
    pub secret: bool,
    /// Another setting of the same kind whose value this one has until it
    /// is set itself, e.g. the margins at the top and bottom those at the
    /// sides (one step only: what that one follows is not followed).
    pub follows: Option<Id>,
    /// A choice that keeps values its options lack (one a plugin offered
    /// that is not installed now, such as a reading app): kept as they
    /// are, without a warning, for whoever reads it to pass over.
    pub keeps_unknown: bool,
    /// Shown only while another setting has a value, e.g. the server's
    /// address while syncing is on (see [`Settings::shows`]): the
    /// setting's id and the value, as written in `paperpaw.ini`.
    pub shown_if: Option<(Id, &'static str)>,
    /// What a value must be: a value it refuses is not set, and the
    /// reader is told why (see [`SettingCheck`]).
    pub check: Option<SettingCheck>,
}

/// Checks a value before it is set: `Err` with the translation key of
/// why it is refused (see [`SettingExtra::check`]).
#[derive(Clone, Copy)]
pub struct SettingCheck(pub fn(&Settings, &SettingValue) -> Result<(), &'static str>);

impl fmt::Debug for SettingCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SettingCheck")
    }
}

impl PartialEq for SettingCheck {
    fn eq(&self, other: &Self) -> bool {
        self.0 as usize == other.0 as usize
    }
}

impl Eq for SettingCheck {}

/// The extra of settings without one.
static NO_EXTRA: SettingExtra = SettingExtra::new();

impl SettingExtra {
    /// None of them: no unit, editor or section, a step of 1. Made in the
    /// program for fixed settings with the builder methods below and given
    /// to [`SettingSpec::with_extra`]:
    ///
    /// ```
    /// use paperpaw_sdk::{Id, SettingExtra, SettingSpec};
    /// const GROUP: Id = Id::new_static("my.page");
    /// static PERCENT: SettingExtra = SettingExtra::new().unit("my.unit.percent").step(10);
    /// static SETTINGS: [SettingSpec; 1] = [
    ///     SettingSpec::number("my.zoom", "my.zoom", 50, 200, 100, GROUP)
    ///         .with_extra(&PERCENT),
    /// ];
    /// ```
    pub const fn new() -> Self {
        Self {
            unit: None,
            editor: None,
            step: 1,
            slider: false,
            section: None,
            secret: false,
            follows: None,
            keeps_unknown: false,
            shown_if: None,
            check: None,
            section_of_plugin: false,
        }
    }

    /// Its part is the name of the plugin its id ends in
    /// ([`SettingExtra::section_of_plugin`]).
    pub const fn section_of_plugin(mut self) -> Self {
        self.section_of_plugin = true;
        self
    }

    /// A choice keeping values its options lack
    /// ([`SettingExtra::keeps_unknown`]).
    pub const fn keeps_unknown(mut self) -> Self {
        self.keeps_unknown = true;
        self
    }

    /// With the translation key of its unit ([`SettingExtra::unit`]).
    pub const fn unit(mut self, key: &'static str) -> Self {
        core::mem::forget(self.unit.replace(Cow::Borrowed(key)));
        self
    }

    /// Edited on screen `screen` ([`SettingExtra::editor`]).
    pub const fn editor(mut self, screen: Id) -> Self {
        core::mem::forget(self.editor.replace(Editor::Screen(screen)));
        self
    }

    /// Edited on whatever screen fills slot `slot`
    /// ([`SettingExtra::editor`]), e.g.
    /// [`CHOOSE_FOLDER`](crate::builtin::slots::CHOOSE_FOLDER).
    pub const fn editor_slot(mut self, slot: Id) -> Self {
        core::mem::forget(self.editor.replace(Editor::Slot(slot)));
        self
    }

    /// One step (a `+` or `−`) changes a number by `step`.
    pub const fn step(mut self, step: i64) -> Self {
        self.step = if step < 1 { 1 } else { step };
        self
    }

    /// Set with a slider ([`SettingExtra::slider`]).
    pub const fn slider(mut self) -> Self {
        self.slider = true;
        self
    }

    /// In the part `key` of its group ([`SettingExtra::section`]).
    pub const fn section(mut self, key: &'static str) -> Self {
        core::mem::forget(self.section.replace(Cow::Borrowed(key)));
        self
    }

    /// A secret ([`SettingExtra::secret`]).
    pub const fn secret(mut self) -> Self {
        self.secret = true;
        self
    }

    /// With the value of setting `id` until set itself
    /// ([`SettingExtra::follows`]).
    pub const fn follows(mut self, id: Id) -> Self {
        core::mem::forget(self.follows.replace(id));
        self
    }

    /// Shown only while setting `id` has `value` ([`SettingExtra::shown_if`]).
    pub const fn shown_if(mut self, id: Id, value: &'static str) -> Self {
        core::mem::forget(self.shown_if.replace((id, value)));
        self
    }

    /// Values are checked with `check` before they are set
    /// ([`SettingExtra::check`]).
    pub const fn check(
        mut self,
        check: fn(&Settings, &SettingValue) -> Result<(), &'static str>,
    ) -> Self {
        self.check = Some(SettingCheck(check));
        self
    }
}

impl Default for SettingExtra {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingSpec {
    /// A setting of `kind` with its `default`, called `title` (a
    /// translation key), in `group`. Every builder of a spec is `const`, so
    /// a plugin declares its settings as a `static` list registered with
    /// [`Registrar::add_settings`](crate::Registrar::add_settings): it
    /// takes next to no memory (on small devices the list stays in flash).
    /// Its unit, section, editor and the like are a [`SettingExtra`], kept
    /// in the program too ([`SettingSpec::with_extra`]).
    ///
    /// ```
    /// use paperpaw_sdk::{Id, SettingExtra, SettingSpec};
    /// const GROUP: Id = Id::new_static("my.clock");
    /// static PERCENT: SettingExtra = SettingExtra::new().unit("my.unit.percent").step(10);
    /// static SETTINGS: [SettingSpec; 3] = [
    ///     SettingSpec::switch("my.clock.date", "my.clock.date", true, GROUP),
    ///     SettingSpec::choice(
    ///         "my.clock.face",
    ///         "my.clock.face",
    ///         &["my.plain", "my.ticks"],
    ///         "my.ticks",
    ///         GROUP,
    ///     )
    ///     .description("my.clock.face.help"),
    ///     SettingSpec::number("my.clock.zoom", "my.clock.zoom", 50, 200, 100, GROUP)
    ///         .with_extra(&PERCENT),
    /// ];
    /// ```
    ///
    /// A spec whose id is known only at run time (one for each plugin, say)
    /// is made with this, and registered with
    /// [`Registrar::add_setting`](crate::Registrar::add_setting).
    pub const fn new(
        id: Id,
        title: &'static str,
        kind: SettingKind,
        default: SettingValue,
        group: Id,
    ) -> Self {
        Self {
            id,
            title: Cow::Borrowed(title),
            kind,
            default,
            group,
            level: Level::Basic,
            description: None,
            scoped: false,
            guarded: false,
            kept_on_reset: false,
            extra: None,
        }
    }

    /// A switch, on or off ([`SettingSpec::new`]).
    pub const fn switch(id: &'static str, title: &'static str, default: bool, group: Id) -> Self {
        Self::new(
            Id::new_static(id),
            title,
            SettingKind::Bool,
            SettingValue::Bool(default),
            group,
        )
    }

    /// A whole number from `min` to `max` ([`SettingSpec::new`]).
    pub const fn number(
        id: &'static str,
        title: &'static str,
        min: i64,
        max: i64,
        default: i64,
        group: Id,
    ) -> Self {
        Self::new(
            Id::new_static(id),
            title,
            SettingKind::Int { min, max },
            SettingValue::Int(default),
            group,
        )
    }

    /// One of `options`, each the translation key of its name
    /// ([`SettingSpec::new`]).
    pub const fn choice(
        id: &'static str,
        title: &'static str,
        options: &'static [&'static str],
        default: &'static str,
        group: Id,
    ) -> Self {
        Self::new(
            Id::new_static(id),
            title,
            SettingKind::Choice(Choices::Fixed(options)),
            SettingValue::Text(Cow::Borrowed(default)),
            group,
        )
    }

    /// Text, typed in ([`SettingSpec::new`]).
    pub const fn text(
        id: &'static str,
        title: &'static str,
        default: &'static str,
        group: Id,
    ) -> Self {
        Self::new(
            Id::new_static(id),
            title,
            SettingKind::Text,
            SettingValue::Text(Cow::Borrowed(default)),
            group,
        )
    }

    /// A language: the tag of one of the installed translations
    /// ([`SettingSpec::new`]).
    pub const fn language(
        id: &'static str,
        title: &'static str,
        default: &'static str,
        group: Id,
    ) -> Self {
        Self::new(
            Id::new_static(id),
            title,
            SettingKind::Language,
            SettingValue::Text(Cow::Borrowed(default)),
            group,
        )
    }

    /// Ids separated by commas, chosen on the setting's editor screen
    /// ([`SettingKind::List`], [`SettingSpec::new`]).
    pub const fn list(
        id: &'static str,
        title: &'static str,
        default: &'static str,
        group: Id,
    ) -> Self {
        Self::new(
            Id::new_static(id),
            title,
            SettingKind::List,
            SettingValue::Text(Cow::Borrowed(default)),
            group,
        )
    }

    /// With a help text, the translation key `key`.
    pub const fn description(mut self, key: &'static str) -> Self {
        core::mem::forget(self.description.replace(Cow::Borrowed(key)));
        self
    }

    /// Shown only with advanced settings.
    pub const fn advanced(mut self) -> Self {
        self.level = Level::Advanced;
        self
    }

    /// May be set per scope, e.g. for one book only.
    pub const fn scoped(mut self) -> Self {
        self.scoped = true;
        self
    }

    /// It keeps the device safe: only its own plugin and the settings
    /// editor (the plugin of the settings slot) change it; other plugins'
    /// defaults, presets and commands are refused.
    pub const fn guarded(mut self) -> Self {
        self.guarded = true;
        self
    }

    /// Left as it is by the resets of the settings pages: the reader's
    /// own, such as an account's server or user, a folder or a list typed
    /// in (see the field of the same name).
    pub const fn kept_on_reset(mut self) -> Self {
        self.kept_on_reset = true;
        self
    }

    /// With `extra`: its unit, section, editor, step and the like, kept in
    /// the program too.
    pub const fn with_extra(mut self, extra: &'static SettingExtra) -> Self {
        core::mem::forget(self.extra.replace(Extra::Fixed(extra)));
        self
    }

    /// With `extra` made at run time, for a setting made from data (a
    /// data plugin's), whose section or unit only the data names. Settings
    /// a plugin's code declares use [`SettingSpec::with_extra`], which
    /// takes no memory.
    pub fn with_owned_extra(mut self, extra: SettingExtra) -> Self {
        self.extra = Some(Extra::Owned(Box::new(extra)));
        self
    }

    /// The translation key of the part of its group it is shown in
    /// ([`SettingExtra::section`], [`SettingExtra::section_of_plugin`]).
    pub fn section_key(&self) -> Option<Cow<'static, str>> {
        let extra = self.extra();
        if extra.section_of_plugin {
            let plugin = self.id.as_str().rsplit('.').next().unwrap_or_default();
            return Some(Cow::Owned(alloc::format!("{plugin}.plugin.name")));
        }
        extra.section.clone()
    }

    /// Its unit, editor, step, slider, section and whether it is a secret
    /// (the defaults for a setting that sets none of them).
    pub fn extra(&self) -> &SettingExtra {
        match &self.extra {
            Some(Extra::Fixed(extra)) => extra,
            Some(Extra::Owned(extra)) => extra,
            None => &NO_EXTRA,
        }
    }

    /// Whether it takes `value`: one of its kind, or for a choice keeping
    /// unknown values ([`SettingExtra::keeps_unknown`]) any word.
    pub fn accepts(&self, value: &SettingValue) -> bool {
        self.kind.accepts(value)
            || (self.extra().keeps_unknown
                && matches!((&self.kind, value),
                    (SettingKind::Choice(_), SettingValue::Text(v)) if !v.is_empty()))
    }

    /// Parses a value as written in a configuration file, as
    /// [`SettingSpec::accepts`] takes it.
    pub fn parse(&self, text: &str) -> Option<SettingValue> {
        let value = match self.kind.parse(text) {
            Some(value) => value,
            None => SettingValue::Text(Cow::Owned(String::from(text))),
        };
        self.accepts(&value).then_some(value)
    }

    /// Whether the resets of the settings pages leave it as it is: marked
    /// so, or a secret (a reset never removes a password).
    pub fn is_kept_on_reset(&self) -> bool {
        self.kept_on_reset || self.extra().secret
    }
}

/// How often a setting is needed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Level {
    /// What most readers change (font size, language, …).
    #[default]
    Basic,
    /// Rarely needed; shown when the user asks for advanced settings.
    Advanced,
}

/// A group of settings, shown together on one page.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SettingGroupSpec {
    pub id: Id,
    /// Translation key of the group's name.
    pub title: Cow<'static, str>,
    /// By order, smallest first; 0 names none (`builtin::settings_page`).
    pub order: i32,
    /// A screen of the group's plugin shown instead of the generated page
    /// (opened with [`params::ID`](crate::builtin::params::ID), the
    /// group), e.g. Wi-Fi with its networks; it shows the group's
    /// settings itself.
    pub page: Option<Id>,
    /// The group it is a part of, shown on that group's page (a page of
    /// its own one level down), not on the main page; `None` for a group
    /// of the main page.
    pub parent: Option<Id>,
}

impl SettingGroupSpec {
    /// Group `id`, called `title` (a translation key); the rest as set
    /// with its methods. All are `const`: a plugin declares its groups as
    /// statics and registers a reference
    /// ([`Registrar::add_setting_group`](crate::Registrar::add_setting_group)),
    /// which takes next to no memory.
    pub const fn new(id: Id, title: &'static str) -> Self {
        Self {
            id,
            title: Cow::Borrowed(title),
            order: 0,
            page: None,
            parent: None,
        }
    }

    /// Its place among the groups, smallest first.
    pub const fn order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }

    /// Screen `page` shows the group instead of the generated page.
    pub const fn page(mut self, page: Id) -> Self {
        core::mem::forget(self.page.replace(page));
        self
    }

    /// A part of group `parent`: shown on its page, one level down, by
    /// order among its other parts.
    pub const fn within(mut self, parent: Id) -> Self {
        core::mem::forget(self.parent.replace(parent));
        self
    }
}

/// A group of settings as registered: one kept in the program (a
/// `static` [`SettingGroupSpec`], a reference), or one made at run time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingGroup(GroupAt);

#[derive(Debug, Clone, PartialEq, Eq)]
enum GroupAt {
    Fixed(&'static SettingGroupSpec),
    Owned(Box<SettingGroupSpec>),
}

impl core::ops::Deref for SettingGroup {
    type Target = SettingGroupSpec;

    fn deref(&self) -> &SettingGroupSpec {
        match &self.0 {
            GroupAt::Fixed(group) => group,
            GroupAt::Owned(group) => group,
        }
    }
}

impl From<&'static SettingGroupSpec> for SettingGroup {
    fn from(group: &'static SettingGroupSpec) -> Self {
        Self(GroupAt::Fixed(group))
    }
}

impl From<SettingGroupSpec> for SettingGroup {
    fn from(group: SettingGroupSpec) -> Self {
        Self(GroupAt::Owned(Box::new(group)))
    }
}

/// Why a setting could not be changed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SettingError {
    Unknown,
    InvalidValue,
    /// No place is left to keep a changed spec (more than 32,768
    /// settings of a kind).
    Full,
    /// The setting's check refused the value: the translation key of why
    /// (see [`SettingExtra::check`]).
    Refused(&'static str),
}

impl fmt::Display for SettingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SettingError::Unknown => "unknown setting",
            SettingError::InvalidValue => "invalid value for this setting",
            SettingError::Full => "no place left to change this setting",
            SettingError::Refused(why) => why,
        })
    }
}

impl core::error::Error for SettingError {}

/// All registered settings and their current values.
///
/// Plugins receive it read-only through [`Env`](crate::Env) and change values
/// with [`Command::SetSetting`](crate::Command::SetSetting).
///
/// Something placed more than once, such as a widget on two pages, has
/// values of its own for each placement: the setting `widgets.clock.face`
/// of the placement `widgets.clock@2` is `widgets.clock@2.face` (see
/// [`Id::placement`]), with the spec of `widgets.clock.face`; it is set,
/// saved and reset like any other, and where it is not set the setting's
/// own value applies. While a placement is shown, its values are those of
/// the setting itself ([`Settings::with_placement`]), so a widget reads
/// its settings as always.
///
/// Values may be shown for a while without being kept, e.g. a layout
/// before it is chosen (`Settings::set_preview`, asked for with
/// [`Command::PreviewSettings`](crate::Command::PreviewSettings)): they
/// are what [`Settings::get`] gives while they last; what is kept
/// ([`Settings::global`]) stays as it is. The ready-made sets of values
/// plugins offer are kept here too ([`Settings::presets`]).
#[derive(Debug, Default)]
pub struct Settings {
    specs: Specs,
    values: VecMap<Id, SettingValue>,
    /// The placement whose values its settings have now, e.g.
    /// `widgets.clock@2` (see [`Settings::with_placement`]).
    placement: core::cell::RefCell<Option<String>>,
    /// Groups by order.
    groups: Vec<SettingGroup>,
    /// The values of the active scope (e.g. the open book), if any.
    scope: Option<VecMap<Id, SettingValue>>,
    /// Values shown without being kept; `None`: as if not set.
    preview: VecMap<Id, Option<SettingValue>>,
    /// Values a power mode wears over the reader's own, while it is worn
    /// (see [`Settings::wear`]).
    worn: VecMap<Id, SettingValue>,
    /// Upper limits of numbers while a power mode is worn.
    limits: VecMap<Id, SettingValue>,
    /// The mode's values the reader changed while it was worn: theirs
    /// until the next mode (see [`Settings::released`]).
    released: Vec<Id>,
    presets: crate::presets::Presets,
}

impl Settings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Releases unused capacity once registration is complete.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.specs.shrink_to_fit();
        self.values.shrink_to_fit();
        self.groups.shrink_to_fit();
        self.presets.shrink_to_fit();
    }

    /// The presets every plugin added (see [`presets`](crate::presets)).
    pub fn presets(&self) -> &crate::presets::Presets {
        &self.presets
    }

    /// Adds presets (see [`presets`](crate::presets)).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn presets_mut(&mut self) -> &mut crate::presets::Presets {
        &mut self.presets
    }

    /// Shows `values` in place of those kept (`None`: as if not set),
    /// until they are replaced; empty, the kept ones show again. Values of
    /// unknown settings and those that do not fit are left out.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_preview(&mut self, values: Vec<(Id, Option<SettingValue>)>) {
        self.preview = VecMap::default();
        for (id, value) in values {
            let fits = self
                .spec(id.as_str())
                .is_some_and(|spec| value.as_ref().is_none_or(|value| spec.accepts(value)));
            if fits {
                self.preview.insert(id, value);
            }
        }
    }

    /// Wears a power mode's values over the reader's own, and keeps
    /// numbers below its limits, until another mode is worn (see
    /// [`power`](crate::power)): nothing is written, and the reader's own
    /// values show again when the mode is taken off. Values of unknown
    /// settings and those that do not fit are left out. The ids whose
    /// shown value may have changed come back.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn wear(&mut self, values: Vec<(Id, SettingValue)>, limits: Vec<(Id, i64)>) -> Vec<Id> {
        let mut touched: Vec<Id> = self
            .worn
            .keys()
            .chain(self.limits.keys())
            .cloned()
            .collect();
        self.worn = VecMap::default();
        self.limits = VecMap::default();
        self.released.clear();
        for (id, value) in values {
            if self
                .spec(id.as_str())
                .is_some_and(|spec| spec.accepts(&value))
            {
                touched.push(id.clone());
                self.worn.insert(id, value);
            }
        }
        for (id, limit) in limits {
            let value = SettingValue::Int(limit);
            if self
                .spec(id.as_str())
                .is_some_and(|spec| spec.accepts(&value))
            {
                touched.push(id.clone());
                self.limits.insert(id, value);
            }
        }
        touched.sort_unstable();
        touched.dedup();
        touched
    }

    /// The mode's values the reader changed while it was worn, which are
    /// theirs until the next mode, e.g. to keep across a restart.
    pub fn released(&self) -> &[Id] {
        &self.released
    }

    /// Lets go of the worn mode's value for `id`, as when the reader
    /// changed it (e.g. after a restart, see [`Settings::released`]).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn release(&mut self, id: &Id) {
        let value = self.worn.remove(id.as_str()).is_some();
        let limit = self.limits.remove(id.as_str()).is_some();
        if (value || limit) && !self.released.contains(id) {
            self.released.push(id.clone());
        }
    }

    /// Whether a power mode holds `id` (a value or a limit).
    pub fn is_worn(&self, id: &str) -> bool {
        self.worn.contains_key(id) || self.limits.contains_key(id)
    }

    /// Whether values are shown without being kept.
    pub fn previewing(&self) -> bool {
        !self.preview.is_empty()
    }

    /// Whether a value is kept for `id` (it is not at its default).
    pub fn is_set(&self, id: &str) -> bool {
        self.values.contains_key(id)
    }

    /// Whether setting `id` has a value of the reader's choosing now: kept
    /// ([`Settings::is_set`]), shown while one is chosen
    /// ([`Command::PreviewSettings`](crate::Command::PreviewSettings)), or
    /// set by the placement whose values are active
    /// ([`Settings::with_placement`]). Not its default alone.
    pub fn is_chosen(&self, id: &str) -> bool {
        if self.placed_value(id).is_some() {
            return true;
        }
        // Shown while one is chosen: that decides, also a default shown.
        match self.preview.get(id) {
            Some(shown) => shown.is_some(),
            None => self.values.contains_key(id),
        }
    }

    /// Registers a group of settings.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add_group(&mut self, group: impl Into<SettingGroup>) -> Result<(), RegisterError> {
        let group = group.into();
        if self.groups.iter().any(|g| g.id == group.id) {
            return Err(RegisterError::Duplicate {
                id: group.id.clone(),
            });
        }
        let at = self
            .groups
            .partition_point(|g| (g.order, g.id.as_str()) <= (group.order, group.id.as_str()));
        self.groups.insert(at, group);
        Ok(())
    }

    /// Groups by order.
    pub fn groups(&self) -> impl Iterator<Item = &SettingGroupSpec> {
        self.groups.iter().map(|g| &**g)
    }

    /// The group `group` is a part of, if it is one
    /// ([`SettingGroupSpec::within`]).
    pub fn parent_of(&self, group: &Id) -> Option<&Id> {
        self.groups
            .iter()
            .find(|g| g.id == *group)
            .and_then(|g| g.parent.as_ref())
    }

    /// Whether `group` is `ancestor` or one of its parts, at any depth.
    pub fn within(&self, group: &Id, ancestor: &Id) -> bool {
        let mut at = group;
        // Groups name their parents; a loop of them is no part of any.
        for _ in 0..8 {
            if at == ancestor {
                return true;
            }
            match self.parent_of(at) {
                Some(parent) => at = parent,
                None => return false,
            }
        }
        false
    }

    /// The group a setting is shown in: its own if registered, else
    /// [`OTHER`](crate::builtin::groups::OTHER).
    pub fn group_of(&self, spec: &SettingSpec) -> Id {
        if self.groups.iter().any(|g| g.id == spec.group) {
            spec.group.clone()
        } else {
            crate::builtin::groups::OTHER
        }
    }

    /// Registers a setting. Its default must be valid.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn define(&mut self, spec: SettingSpec) -> Result<(), RegisterError> {
        if self.specs.get(spec.id.as_str()).is_some() {
            return Err(RegisterError::Duplicate { id: spec.id });
        }
        if !spec.kind.accepts(&spec.default) {
            return Err(RegisterError::Invalid {
                message: Cow::Borrowed("default value does not match the setting kind"),
            });
        }
        if !self.specs.insert(spec) {
            return Err(RegisterError::Invalid {
                message: Cow::Borrowed("no place left for another setting"),
            });
        }
        Ok(())
    }

    /// Registers spec `at` of a list of fixed settings
    /// (a `static` list), kept by a reference to the list. Its
    /// default must be valid.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn define_fixed(
        &mut self,
        list: &'static [SettingSpec],
        at: usize,
    ) -> Result<(), RegisterError> {
        let Some(spec) = list.get(at) else {
            return Ok(());
        };
        if self.specs.get(spec.id.as_str()).is_some() {
            return Err(RegisterError::Duplicate {
                id: spec.id.clone(),
            });
        }
        if !spec.kind.accepts(&spec.default) {
            return Err(RegisterError::Invalid {
                message: Cow::Borrowed("default value does not match the setting kind"),
            });
        }
        if !self.specs.insert_fixed(list, at) {
            return Err(RegisterError::Invalid {
                message: Cow::Borrowed("no place left for another setting"),
            });
        }
        Ok(())
    }

    /// Replaces the options of a [`SettingKind::Choice`] setting, e.g. once
    /// all contributions it chooses from are known.
    ///
    /// Whether it did: not for an unknown setting, nor where no place is
    /// left to keep the changed spec ([`SettingError::Full`]).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_choices(&mut self, id: &str, options: impl Into<Choices>) -> bool {
        match self.specs.get_mut(id) {
            Ok(spec) => {
                spec.kind = SettingKind::Choice(options.into());
                true
            }
            Err(_) => false,
        }
    }

    /// The spec of setting `id`; for a placement's (`widgets.clock@2.face`)
    /// the setting's own (`widgets.clock.face`).
    pub fn spec(&self, id: &str) -> Option<&SettingSpec> {
        match crate::id::unplaced(id) {
            Some(plain) => self.specs.get(&plain),
            None => self.specs.get(id),
        }
    }

    /// The id a value of `id` is kept under: the spec's, or the
    /// placement's own.
    fn value_id(&self, id: &str) -> Result<Id, SettingError> {
        let spec = self.spec(id).ok_or(SettingError::Unknown)?;
        if id == spec.id.as_str() {
            Ok(spec.id.clone())
        } else {
            Id::parse(id).map_err(|_| SettingError::Unknown)
        }
    }

    /// Whether the showing of another setting depends on setting `id`
    /// ([`SettingExtra::shown_if`]): screens listing settings make
    /// themselves anew when it changes.
    pub fn governs(&self, id: &str) -> bool {
        self.specs().any(|spec| {
            spec.extra()
                .shown_if
                .as_ref()
                .is_some_and(|(of, _)| of == id)
        })
    }

    /// Whether `spec` shows now: not while the setting its
    /// [`SettingExtra::shown_if`] names has another value.
    pub fn shows(&self, spec: &SettingSpec) -> bool {
        let Some((id, value)) = &spec.extra().shown_if else {
            return true;
        };
        // The value read as that setting reads it (`on`, `true` …); a
        // setting not there does not hide this one.
        let Some(condition) = self.spec(id.as_str()) else {
            return true;
        };
        condition
            .kind
            .parse(value)
            .is_some_and(|value| self.get(id.as_str()) == Some(&value))
    }

    /// Sets a value after validating it.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set(&mut self, id: &str, value: SettingValue) -> Result<(), SettingError> {
        let spec = self.spec(id).ok_or(SettingError::Unknown)?;
        if !spec.accepts(&value) {
            return Err(SettingError::InvalidValue);
        }
        if let Some(SettingCheck(check)) = spec.extra().check {
            check(self, &value).map_err(SettingError::Refused)?;
        }
        let id = self.value_id(id)?;
        // The reader's change wins over a mode's value until the next mode.
        self.release(&id);
        self.values.insert(id, value);
        Ok(())
    }

    /// Makes the values of `placement` (e.g. `widgets.clock@2`, see
    /// [`Id::placement`]) those of its settings while `f` runs: a widget
    /// placed there reads `widgets.clock.face` and gets
    /// `widgets.clock@2.face` where that is set. `None` runs `f` as it is.
    pub fn with_placement<R>(&self, placement: Option<&str>, f: impl FnOnce() -> R) -> R {
        let Some(placement) = placement.filter(|p| p.contains('@')) else {
            return f();
        };
        let before = self.placement.replace(Some(String::from(placement)));
        let out = f();
        self.placement.replace(before);
        out
    }

    /// The value the active placement has for `id`, if it sets one.
    fn placed_value(&self, id: &str) -> Option<&SettingValue> {
        let placement = self.placement.borrow();
        let placed = placement.as_deref()?;
        let plain = &placed[..placed.find('@')?];
        let rest = id.strip_prefix(plain).filter(|r| r.starts_with('.'))?;
        let key = alloc::format!("{placed}{rest}");
        match self.preview.get(key.as_str()) {
            Some(shown) => shown.as_ref(),
            None => self.values.get(key.as_str()),
        }
    }

    /// The ids of the values `placement` sets (e.g. to forget them when it
    /// goes).
    pub fn placement_values(&self, placement: &str) -> Vec<Id> {
        self.values
            .keys()
            .filter(|id| {
                id.as_str()
                    .strip_prefix(placement)
                    .is_some_and(|rest| rest.starts_with('.'))
            })
            .cloned()
            .collect()
    }

    /// Forgets the value set for `id`, so that its default applies again;
    /// where changes of it go to the active scope
    /// ([`Settings::goes_to_scope`]), the scope's value, so that the
    /// global one applies again (for "only this book", the scope's values
    /// all go, as when it is switched off).
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn reset(&mut self, id: &str) -> Result<(), SettingError> {
        let id = self.value_id(id)?;
        if self.goes_to_scope(id.as_str()) {
            if let Some(scope) = self.scope.as_mut() {
                if id == crate::builtin::settings::SCOPE_ONLY {
                    scope.clear();
                } else {
                    scope.remove(id.as_str());
                }
            }
        } else {
            self.values.remove(id.as_str());
        }
        Ok(())
    }

    /// Sets a value from configuration-file text.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_from_text(&mut self, id: &str, text: &str) -> Result<(), SettingError> {
        let spec = self.spec(id).ok_or(SettingError::Unknown)?;
        let value = spec.parse(text).ok_or(SettingError::InvalidValue)?;
        let id = self.value_id(id)?;
        self.values.insert(id, value);
        Ok(())
    }

    /// The current value: the active scope's if it sets one (see
    /// [`Settings::scope`]), else the global value, else the default.
    pub fn get(&self, id: &str) -> Option<&SettingValue> {
        if let Some(value) = self.placed_value(id) {
            return Some(value);
        }
        if let Some(shown) = self.preview.get(id) {
            return match (shown, crate::id::unplaced(id)) {
                (Some(value), _) => Some(value),
                (None, Some(plain)) => self.get(&plain),
                (None, None) => self.specs.get(id).map(|spec| &spec.default),
            };
        }
        if let Some(plain) = crate::id::unplaced(id) {
            return self.values.get(id).or_else(|| self.get(&plain));
        }
        if let Some(value) = self.worn.get(id) {
            return Some(value);
        }
        if let Some(limit @ SettingValue::Int(most)) = self.limits.get(id) {
            let own = self.own(id);
            return match own {
                Some(SettingValue::Int(value)) if value <= most => own,
                _ => Some(limit),
            };
        }
        self.own(id)
    }

    /// The value of `id` without a power mode: the scope's, the reader's,
    /// the one it follows, or the default.
    fn own(&self, id: &str) -> Option<&SettingValue> {
        self.scoped_value(id)
            .or_else(|| self.values.get(id))
            .or_else(|| {
                self.followed_or_default(id, |other| {
                    self.scoped_value(other).or_else(|| self.values.get(other))
                })
            })
    }

    /// The value of `id` where none is set: that of the setting it
    /// follows until it is set ([`SettingExtra::follows`]), as `set`
    /// finds it or by default, else its own default. Its spec is looked
    /// up once.
    fn followed_or_default<'s>(
        &'s self,
        id: &str,
        set: impl FnOnce(&str) -> Option<&'s SettingValue>,
    ) -> Option<&'s SettingValue> {
        let spec = self.specs.get(id)?;
        let followed = spec.extra().follows.as_ref().and_then(|other| {
            let other = other.as_str();
            set(other).or_else(|| self.specs.get(other).map(|spec| &spec.default))
        });
        followed.or(Some(&spec.default))
    }

    /// The value outside any scope, or the default if none was set; for a
    /// placement's, the setting's own where the placement sets none.
    pub fn global(&self, id: &str) -> Option<&SettingValue> {
        if let Some(plain) = crate::id::unplaced(id) {
            return self.values.get(id).or_else(|| self.global(&plain));
        }
        self.values
            .get(id)
            .or_else(|| self.followed_or_default(id, |other| self.values.get(other)))
    }

    /// The active scope's value of `id`, if it applies: the setting is
    /// [`scoped`](SettingSpec::scoped) and the scope is switched on
    /// ([`SCOPE_ONLY`](crate::builtin::settings::SCOPE_ONLY)).
    pub fn scoped_value(&self, id: &str) -> Option<&SettingValue> {
        let scope = self.scope.as_ref()?;
        let only = crate::builtin::settings::SCOPE_ONLY;
        if id != only.as_str() && scope.get(only.as_str()) != Some(&SettingValue::Bool(true)) {
            return None;
        }
        let value = scope.get(id)?;
        self.specs.get(id).filter(|spec| spec.scoped)?;
        Some(value)
    }

    /// The values of the active scope, e.g. of the book being read: scoped
    /// settings changed while it is switched on are kept there. `None`
    /// outside any scope.
    pub fn scope(&self) -> Option<&VecMap<Id, SettingValue>> {
        self.scope.as_ref()
    }

    /// Activates a scope with its values (only valid scoped ones are
    /// kept), or ends it.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_scope(&mut self, values: Option<VecMap<Id, SettingValue>>) {
        self.scope = values.map(|mut values| {
            let ids: Vec<Id> = values.keys().cloned().collect();
            for id in ids {
                let valid = match (self.specs.get(id.as_str()), values.get(id.as_str())) {
                    (Some(spec), Some(value)) => spec.scoped && spec.accepts(value),
                    _ => false,
                };
                if !valid {
                    values.remove(id.as_str());
                }
            }
            values
        });
    }

    /// Sets a scoped setting in the active scope. Switching the scope off
    /// ([`SCOPE_ONLY`](crate::builtin::settings::SCOPE_ONLY)) drops its
    /// values.
    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn set_in_scope(&mut self, id: &str, value: SettingValue) -> Result<(), SettingError> {
        let spec = self.specs.get(id).ok_or(SettingError::Unknown)?;
        if !spec.scoped || !spec.accepts(&value) {
            return Err(SettingError::InvalidValue);
        }
        let id = spec.id.clone();
        let scope = self.scope.as_mut().ok_or(SettingError::Unknown)?;
        if id == crate::builtin::settings::SCOPE_ONLY && value == SettingValue::Bool(false) {
            scope.clear();
            return Ok(());
        }
        scope.insert(id, value);
        Ok(())
    }

    /// Whether `id` is set in the active scope rather than globally, e.g.
    /// to mark it in menus.
    pub fn is_scoped(&self, id: &str) -> bool {
        id != crate::builtin::settings::SCOPE_ONLY.as_str() && self.scoped_value(id).is_some()
    }

    /// Whether a change of `id` goes to the active scope.
    pub fn goes_to_scope(&self, id: &str) -> bool {
        let Some(scope) = &self.scope else {
            return false;
        };
        let only = crate::builtin::settings::SCOPE_ONLY;
        let scoped = self.specs.get(id).is_some_and(|spec| spec.scoped);
        scoped
            && (id == only.as_str() || scope.get(only.as_str()) == Some(&SettingValue::Bool(true)))
    }

    /// The value of `setting` (see [`setting`](crate::setting)); the
    /// type's empty value if it is unknown.
    pub fn read<T: crate::setting::SettingType>(
        &self,
        setting: &crate::setting::Setting<T>,
    ) -> T::Value<'_> {
        T::read(self.get(setting.as_str()))
    }

    /// The value of a boolean setting by its id; `false` if unknown.
    pub fn bool(&self, id: &str) -> bool {
        matches!(self.get(id), Some(SettingValue::Bool(true)))
    }

    /// The value of an integer setting; `0` if unknown.
    pub fn int(&self, id: &str) -> i64 {
        match self.get(id) {
            Some(SettingValue::Int(v)) => *v,
            _ => 0,
        }
    }

    /// The value of a text or choice setting; `""` if unknown.
    pub fn text(&self, id: &str) -> &str {
        match self.get(id) {
            Some(SettingValue::Text(v)) => v,
            _ => "",
        }
    }

    /// All settings in registration order: plugins registered first come
    /// first, each in the order it declared its settings.
    pub fn specs(&self) -> impl Iterator<Item = &SettingSpec> {
        self.specs.iter()
    }
}

mod declared;
mod specs;
#[cfg(test)]
mod tests;
