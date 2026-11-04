//! Presets: ready-made sets of setting values, chosen as a whole, such as
//! a power mode (how long work may wait, how soon the device sleeps).
//!
//! A preset has a kind, which the plugin that offers them names (e.g.
//! `core.power-mode`): that plugin lists the presets of its kind
//! ([`Presets::of_kind`], through `env.settings().presets()`), shows each
//! as it would look ([`Command::PreviewSettings`] with
//! [`Preset::changes`]) and applies the one chosen ([`commands`]). Any
//! plugin adds presets, also for another plugin's kind: kept in the
//! program ([`Registrar::add_presets`], costing next to no memory) or made
//! at start ([`Registrar::add_preset`]). A data plugin brings them in a
//! `presets.ini`, so that anyone can make one and share it:
//!
//! ```ini
//! [night-train]
//! kind = core.power-mode
//! name = Night train
//! description = Offline, the light low, asleep soon.
//! power.level = offline
//! core.sleep-after <= 3
//! ```
//!
//! The name and the description are translation keys or, where no
//! catalog has them, the text itself. A preset's values are written as in
//! `paperpaw.ini`, a setting's id and its value a line; the settings of
//! its kind that it leaves out go back to their defaults when it is
//! applied ([`Preset::changes`]).
//!
//! [`Registrar::add_presets`]: crate::Registrar::add_presets
//! [`Registrar::add_preset`]: crate::Registrar::add_preset

use alloc::borrow::Cow;
use alloc::vec::Vec;

use crate::command::Command;
use crate::id::Id;
use crate::settings::{SettingValue, Settings};

/// A ready-made set of setting values.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Preset {
    /// In the namespace of the plugin that adds it.
    pub id: Id,
    /// What it is for, named by the plugin that offers such presets (e.g.
    /// `core.power-mode`).
    pub kind: Id,
    /// Translation key (or text) of its name.
    pub title: Cow<'static, str>,
    /// Translation key (or text) of a sentence on it.
    pub description: Cow<'static, str>,
    /// Its values, a line each: `setting.id = value`, written as in
    /// `paperpaw.ini`.
    pub values: Cow<'static, str>,
}

impl Preset {
    /// A preset made at start (e.g. from a data plugin's file).
    pub fn new(
        id: Id,
        kind: Id,
        title: impl Into<Cow<'static, str>>,
        description: impl Into<Cow<'static, str>>,
        values: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            id,
            kind,
            title: title.into(),
            description: description.into(),
            values: values.into(),
        }
    }

    /// A preset kept in the program, for a `static` list
    /// ([`Registrar::add_presets`](crate::Registrar::add_presets)).
    pub const fn fixed(
        id: &'static str,
        kind: &'static str,
        title: &'static str,
        description: &'static str,
        values: &'static str,
    ) -> Self {
        Self {
            id: Id::new_static(id),
            kind: Id::new_static(kind),
            title: Cow::Borrowed(title),
            description: Cow::Borrowed(description),
            values: Cow::Borrowed(values),
        }
    }

    /// Its values, `(setting, value)` as written, in order; blank lines
    /// and those starting with `#` left out.
    pub fn values(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values.lines().filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (id, value) = line.split_once('=')?;
            Some((id.trim(), value.trim()))
        })
    }

    /// The value it gives setting `id`, as written.
    pub fn value(&self, id: &str) -> Option<&str> {
        self.values().find(|(key, _)| *key == id).map(|(_, v)| v)
    }

    /// What applying it makes of the settings `covered` and of its own:
    /// each with the value it gives it, `None` (the default) where it
    /// leaves a covered one out. Settings not known here and values that
    /// do not fit their setting are left out, so a preset made for
    /// plugins this device lacks still applies the rest.
    pub fn changes(&self, settings: &Settings, covered: &[Id]) -> Vec<(Id, Option<SettingValue>)> {
        let mut out: Vec<(Id, Option<SettingValue>)> = covered
            .iter()
            .filter(|id| settings.spec(id.as_str()).is_some())
            .map(|id| (id.clone(), None))
            .collect();
        for (id, text) in self.values() {
            let Some(value) = settings.spec(id).and_then(|spec| spec.kind.parse(text)) else {
                continue;
            };
            let Ok(id) = Id::parse(id) else {
                continue;
            };
            match out.iter_mut().find(|(c, _)| *c == id) {
                Some(slot) => slot.1 = Some(value),
                None => out.push((id, Some(value))),
            }
        }
        out
    }
}

/// The kind of presets that are themes: values for the appearance
/// settings, chosen as a whole at the top of them (where any plugin
/// brings one), so two themes installed do not fight. A theme leaves
/// out what it does not change; applying it gives those their defaults
/// ([`theme_settings`] are the settings it covers).
pub const THEME: &str = "core.theme";

/// The settings a theme covers: those of the appearance group
/// ([`groups::LOOK`](crate::builtin::groups::LOOK)) but what the reader
/// needs to read the interface at all, which a theme never changes (the
/// size of its text, its contrast, the logo), and the frame of all
/// widgets, which a widget look covers ([`widget_look_settings`]).
pub fn theme_settings(settings: &Settings) -> Vec<Id> {
    use crate::builtin::settings::{UI_CONTRAST, UI_SIZE, WIDGET_FRAME};
    let kept = [
        UI_SIZE.static_str(),
        UI_CONTRAST.static_str(),
        crate::logo::LOGO.static_str(),
        WIDGET_FRAME.static_str(),
    ];
    settings
        .specs()
        .filter(|spec| settings.group_of(spec) == crate::builtin::groups::LOOK)
        .filter(|spec| !kept.contains(&spec.id.as_str()))
        .map(|spec| spec.id.clone())
        .collect()
}

/// What applying `theme` makes of the settings it covers
/// ([`theme_settings`]): what it names outside them (the size of the
/// interface's text, say) is left out.
pub fn theme_changes(settings: &Settings, theme: &Preset) -> Vec<(Id, Option<SettingValue>)> {
    let covered = theme_settings(settings);
    let mut changes = theme.changes(settings, &covered);
    changes.retain(|(id, _)| covered.contains(id));
    changes
}

/// The kind of presets that give widgets a look as a whole: their
/// frames, room, text sizes and alignment, and the frame of all
/// ([`widget_look_settings`]). The settings plugin offers them at the top
/// of the appearance settings, as themes.
pub const WIDGET_LOOK: &str = "core.widget-look";

/// The settings a widget look covers: each registered widget's frame,
/// padding, size, alignment and colours (`<widget>.frame` …, of
/// `widgets`), and the frame and colours of all
/// ([`WIDGET_FRAME`](crate::builtin::settings::WIDGET_FRAME) …); not the
/// settings of other things that end so (the status lines' size).
pub fn widget_look_settings(settings: &Settings, widgets: &crate::widgets::Widgets) -> Vec<Id> {
    use crate::builtin::settings::{WIDGET_FRAME, WIDGET_INK, WIDGET_PAPER};
    const PARTS: [&str; 6] = [".frame", ".padding", ".size", ".align", ".paper", ".ink"];
    let all = [
        WIDGET_FRAME.static_str(),
        WIDGET_PAPER.static_str(),
        WIDGET_INK.static_str(),
    ];
    let of_widget = |id: &str| {
        PARTS.iter().any(|part| {
            id.strip_suffix(part)
                .is_some_and(|widget| widgets.has(widget))
        })
    };
    settings
        .specs()
        .filter(|spec| all.contains(&spec.id.as_str()) || of_widget(spec.id.as_str()))
        .map(|spec| spec.id.clone())
        .collect()
}

/// What `look` changes among [`widget_look_settings`].
pub fn widget_look_changes(
    settings: &Settings,
    widgets: &crate::widgets::Widgets,
    look: &Preset,
) -> Vec<(Id, Option<SettingValue>)> {
    let covered = widget_look_settings(settings, widgets);
    let mut changes = look.changes(settings, &covered);
    changes.retain(|(id, _)| covered.contains(id));
    changes
}

/// The commands that give the settings the values of `changes` (see
/// [`Preset::changes`]): those that have them already are left alone, and
/// a setting's default is not written down but the setting reset.
pub fn commands(settings: &Settings, changes: &[(Id, Option<SettingValue>)]) -> Vec<Command> {
    changes
        .iter()
        .filter(|(id, value)| !has(settings, id, value.as_ref()))
        .map(|(id, value)| match kept(settings, id, value.as_ref()) {
            Some(value) => Command::SetSetting(id.clone(), value.clone()),
            None => Command::ResetSetting(id.clone()),
        })
        .collect()
}

/// Whether the settings have the values of `changes` now, e.g. to tell
/// which preset is the one in use.
pub fn applied(settings: &Settings, changes: &[(Id, Option<SettingValue>)]) -> bool {
    changes
        .iter()
        .all(|(id, value)| has(settings, id, value.as_ref()))
}

/// Whether setting `id` has `value` (`None`: is not set) as kept.
fn has(settings: &Settings, id: &Id, value: Option<&SettingValue>) -> bool {
    match kept(settings, id, value) {
        // A frame kept only where it is written down (see `kept`).
        Some(value) if widget_frame(id) => {
            settings.is_set(id.as_str()) && settings.global(id.as_str()) == Some(value)
        }
        Some(value) => settings.global(id.as_str()) == Some(value),
        None => !settings.is_set(id.as_str()),
    }
}

/// `value` as kept for setting `id`: `None` (not set) for its default,
/// but a widget's frame named, its default too: a frame chosen is the
/// widget's own, over the frame of all
/// ([`WIDGET_FRAME`](crate::builtin::settings::WIDGET_FRAME)).
fn kept<'a>(
    settings: &Settings,
    id: &Id,
    value: Option<&'a SettingValue>,
) -> Option<&'a SettingValue> {
    let default = settings
        .spec(id.as_str())
        .filter(|spec| spec.id == *id)
        .map(|spec| &spec.default);
    value.filter(|value| widget_frame(id) || Some(*value) != default)
}

/// Whether setting `id` is a frame of its own (`<widget>.frame`).
fn widget_frame(id: &Id) -> bool {
    id.as_str().ends_with(".frame")
        && id.as_str() != crate::builtin::settings::WIDGET_FRAME.static_str()
}

/// Every preset added, those kept in the program first, each in the order
/// it was added.
#[derive(Debug, Default)]
pub struct Presets {
    fixed: Vec<&'static Preset>,
    owned: Vec<Preset>,
}

impl Presets {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds presets kept in the program.
    pub fn add_fixed(&mut self, presets: &'static [Preset]) {
        self.fixed.extend(presets.iter());
    }

    /// Adds a preset made at start.
    pub fn add(&mut self, preset: Preset) {
        self.owned.push(preset);
    }

    /// Forgets the presets made at start by plugin `plugin` (ids
    /// `<plugin>.…`), e.g. of a data plugin removed while running.
    pub fn remove_plugin(&mut self, plugin: &str) {
        self.owned.retain(|p| p.id.namespace() != plugin);
    }

    pub fn shrink_to_fit(&mut self) {
        self.fixed.shrink_to_fit();
        self.owned.shrink_to_fit();
    }

    pub fn iter(&self) -> impl Iterator<Item = &Preset> {
        self.fixed.iter().copied().chain(self.owned.iter())
    }

    /// The presets of `kind`.
    pub fn of_kind<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a Preset> + 'a {
        self.iter().filter(move |p| p.kind.as_str() == kind)
    }

    /// The preset `id`.
    pub fn get(&self, id: &str) -> Option<&Preset> {
        self.iter().find(|p| p.id.as_str() == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{SettingKind, SettingSpec};

    static PRESETS: [Preset; 2] = [
        Preset::fixed(
            "a.calm",
            "a.layout",
            "a.calm",
            "a.calm.help",
            "a.size = 3\n# a comment\n\n a.name = calm ",
        ),
        Preset::fixed("a.other", "b.theme", "", "", "a.size = 99"),
    ];

    fn settings() -> Settings {
        let mut s = Settings::new();
        s.define(SettingSpec::new(
            Id::new_static("a.size"),
            "a.size",
            SettingKind::Int { min: 0, max: 9 },
            SettingValue::Int(1),
            crate::builtin::groups::OTHER,
        ))
        .unwrap();
        s.define(SettingSpec::new(
            Id::new_static("a.name"),
            "a.name",
            SettingKind::Text,
            SettingValue::Text(Cow::Borrowed("")),
            crate::builtin::groups::OTHER,
        ))
        .unwrap();
        s.define(SettingSpec::new(
            Id::new_static("a.dots"),
            "a.dots",
            SettingKind::Bool,
            SettingValue::Bool(true),
            crate::builtin::groups::OTHER,
        ))
        .unwrap();
        s
    }

    #[test]
    fn presets_are_found_by_kind_and_read_line_by_line() {
        let mut presets = Presets::new();
        presets.add_fixed(&PRESETS);
        presets.add(Preset::new(
            Id::new_static("c.nook"),
            Id::new_static("a.layout"),
            "Nook",
            "",
            "a.size = 2",
        ));
        let ids: Vec<&str> = presets.of_kind("a.layout").map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["a.calm", "c.nook"]);
        let calm = presets.get("a.calm").unwrap();
        let values: Vec<(&str, &str)> = calm.values().collect();
        assert_eq!(values, [("a.size", "3"), ("a.name", "calm")]);
        assert_eq!(calm.value("a.name"), Some("calm"));
        assert_eq!(calm.value("a.dots"), None);
    }

    #[test]
    fn applying_sets_its_values_and_resets_what_it_covers() {
        let mut s = settings();
        s.set("a.dots", SettingValue::Bool(false)).unwrap();
        let covered = [
            Id::new_static("a.size"),
            Id::new_static("a.dots"),
            Id::new_static("a.unknown"),
        ];
        let changes = PRESETS[0].changes(&s, &covered);
        assert_eq!(
            changes,
            [
                (Id::new_static("a.size"), Some(SettingValue::Int(3))),
                (Id::new_static("a.dots"), None),
                (
                    Id::new_static("a.name"),
                    Some(SettingValue::Text(Cow::Borrowed("calm")))
                ),
            ]
        );
        let commands = commands(&s, &changes);
        assert_eq!(commands.len(), 3);
        assert!(!applied(&s, &changes));
        s.set("a.size", SettingValue::Int(3)).unwrap();
        s.set_from_text("a.name", "calm").unwrap();
        s.reset("a.dots").unwrap();
        assert!(applied(&s, &changes));
        assert!(super::commands(&s, &changes).is_empty());
        // A default is not written down: the setting is reset.
        s.set("a.size", SettingValue::Int(4)).unwrap();
        let back = [(Id::new_static("a.size"), Some(SettingValue::Int(1)))];
        assert_eq!(
            super::commands(&s, &back),
            [Command::ResetSetting(Id::new_static("a.size"))]
        );
        s.reset("a.size").unwrap();
        assert!(applied(&s, &back));
        // A value that does not fit is left out.
        assert!(PRESETS[1].changes(&s, &[]).is_empty());
    }
}
