//! `command.apply_preset` (1546, `docs/abi/registry.md` 4.3): checked whole
//! at submit, applied after the callback with the other commands.
//!
//! A preset sets the values it names, and every setting of its kind that it
//! leaves out returns to its default (interface 6 `Preset::changes`). What a
//! kind covers (Kernel ruling): `core.theme` the appearance group but the
//! size and contrast of the interface, the logo and the frame of all widgets;
//! `core.widget-look` exactly the looks of the widgets there are and of all
//! of them; any other kind only what its presets name. A value a preset names
//! outside the set of its kind is left out, one that does not fit its setting
//! too (the setting returns to its default where the kind covers it). The
//! walk takes no memory: the sets are tested setting by setting, and
//! [`walk`] is the same one at submit and at apply.
//!
//! At submit: the preset exists (`ERR_NOT_FOUND`), nothing in it, or covered
//! by its kind, is another plugin's guarded setting unless the submitter may
//! set it (`ERR_INVALID`, as `set_setting`), and no flag is set
//! (`ERR_UNSUPPORTED`: the preview of a preset waits for the preview
//! commands). Secret settings and grants are never changed by a preset.
//! Applied, in submit order after the callback's other commands, each
//! setting that does not hold its value already is set or reset, a few at a
//! time, so the host never holds more than one chunk of commands. Before
//! the first setting of a preset is changed, the guarded settings are
//! checked again (earlier commands of the callback may have changed what a
//! kind covers): a preset that would change one the submitter may not is
//! dropped whole, with a warning.
//!
//! Nothing here allocates infallibly: the ids, the values and the commands of
//! a chunk are reserved with `try_reserve` ([`plan`]), and a refusal ends the
//! apply with a warning. The settings are changed one by one, as
//! `set_setting` does, so an out-of-memory in the middle of a preset leaves
//! the chunks before it applied (it is never reported as applied).
use super::{CommandAuthority, CommandBatch, CommandStage, WireCommand};
use crate::Kernel;
use alloc::{borrow::Cow, boxed::Box, string::String, vec::Vec};
use core::ops::ControlFlow;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_UNSUPPORTED};
use paperpaw_sdk::{
    Command, Id, IdError, SettingKind, SettingSpec, SettingValue, Settings,
    presets::{self, Preset},
    widgets::Widgets,
};

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the production native command path takes it with the shell wiring"
    )
)]
/// The most settings one chunk of an apply holds commands for.
const CHUNK: usize = 16;

/// What [`walk`] asks about each setting: its key, its spec and the value
/// the preset gives it (`None`: the default).
pub(crate) type Each<'a> = dyn FnMut(&str, &SettingSpec, Option<Given<'_>>) -> Result<(), i32> + 'a;

/// A value a preset gives a setting, as written and checked against the
/// setting's kind, not yet copied: nothing is allocated to tell it.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Given<'p> {
    Bool(bool),
    Int(i64),
    Text(&'p str),
}

impl<'p> Given<'p> {
    /// `text` as `spec` takes it (what `SettingKind::parse` accepts), or
    /// `None` for a value that does not fit.
    fn of(spec: &SettingSpec, text: &'p str) -> Option<Self> {
        match &spec.kind {
            SettingKind::Bool | SettingKind::Int { .. } => match spec.kind.parse(text)? {
                SettingValue::Bool(value) => Some(Self::Bool(value)),
                SettingValue::Int(value) => Some(Self::Int(value)),
                _ => None,
            },
            SettingKind::Choice(options) => options.contains(text).then_some(Self::Text(text)),
            SettingKind::Language => {
                let tag = !text.is_empty()
                    && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
                tag.then_some(Self::Text(text))
            }
            SettingKind::Text | SettingKind::List | SettingKind::Extended { .. } => {
                Some(Self::Text(text))
            }
            _ => None,
        }
    }

    /// Whether `value` is this one.
    fn equals(&self, value: &SettingValue) -> bool {
        match (self, value) {
            (Self::Bool(a), SettingValue::Bool(b)) => a == b,
            (Self::Int(a), SettingValue::Int(b)) => a == b,
            (Self::Text(a), SettingValue::Text(b)) => *a == b.as_ref(),
            _ => false,
        }
    }

    /// The value, copied where it is text.
    fn value(&self) -> Result<SettingValue, i32> {
        Ok(match *self {
            Self::Bool(value) => SettingValue::Bool(value),
            Self::Int(value) => SettingValue::Int(value),
            Self::Text(text) => {
                let mut owned = String::new();
                owned
                    .try_reserve_exact(text.len())
                    .map_err(|_| ERR_NO_MEMORY)?;
                owned.push_str(text);
                SettingValue::Text(Cow::Owned(owned))
            }
        })
    }
}

/// A preset's kind covers a set of settings (`core.theme`,
/// `core.widget-look`); another kind covers none.
fn covering(kind: &str) -> bool {
    matches!(kind, presets::THEME | presets::WIDGET_LOOK)
}

/// A theme's settings: the appearance group, but what the reader needs to
/// read the interface at all and the frame of all widgets
/// ([`presets::theme_settings`]).
fn theme_covers(settings: &Settings, spec: &SettingSpec) -> bool {
    use paperpaw_sdk::builtin::settings::{UI_CONTRAST, UI_SIZE, WIDGET_FRAME};
    let kept = [
        UI_SIZE.static_str(),
        UI_CONTRAST.static_str(),
        paperpaw_sdk::logo::LOGO.static_str(),
        WIDGET_FRAME.static_str(),
    ];
    // `group_of(spec) == LOOK` without the copy of an owned group id.
    let look = paperpaw_sdk::builtin::groups::LOOK;
    spec.group == look
        && settings.groups().any(|group| group.id == look)
        && !kept.contains(&spec.id.as_str())
}

/// A widget look's settings: each widget's own look and that of all
/// ([`presets::widget_look_settings`]).
fn widget_look_covers(widgets: &Widgets, spec: &SettingSpec) -> bool {
    use paperpaw_sdk::builtin::settings as s;
    const PARTS: [&str; 9] = [
        ".frame",
        ".padding",
        ".size",
        ".align",
        ".paper",
        ".ink",
        ".corners",
        ".frame-line",
        ".drop-shadow",
    ];
    let all = [
        s::WIDGET_FRAME.static_str(),
        s::WIDGET_PAPER.static_str(),
        s::WIDGET_INK.static_str(),
        s::WIDGET_CORNERS.static_str(),
        s::WIDGET_FRAME_LINE.static_str(),
        s::WIDGET_SHADOW.static_str(),
        s::WIDGET_PADDING.static_str(),
        s::WIDGET_GAP.static_str(),
    ];
    let id = spec.id.as_str();
    all.contains(&id)
        || PARTS.iter().any(|part| {
            id.strip_suffix(part)
                .is_some_and(|widget| widgets.has(widget))
        })
}

/// Whether a preset of `kind` covers `spec`.
pub(crate) fn covers(
    kind: &str,
    settings: &Settings,
    widgets: &Widgets,
    spec: &SettingSpec,
) -> bool {
    match kind {
        presets::THEME => theme_covers(settings, spec),
        presets::WIDGET_LOOK => widget_look_covers(widgets, spec),
        _ => false,
    }
}

/// What a preset never changes: a secret, a grant.
fn untouchable(key: &str, spec: &SettingSpec) -> bool {
    spec.extra().secret || key.starts_with("core.grant.")
}

/// The last value `preset` gives setting `id` (a later line wins).
fn last_value<'p>(preset: &'p Preset, id: &str) -> Option<&'p str> {
    preset
        .values()
        .filter(|(key, _)| *key == id)
        .map(|(_, text)| text)
        .last()
}

/// Every setting `preset` changes, in order, with the value it gets (`None`:
/// the default), until `each` refuses; the preset's own and, for a kind that
/// covers some, those it leaves out. Nothing is held between the calls.
pub(crate) fn walk(
    settings: &Settings,
    widgets: &Widgets,
    preset: &Preset,
    each: &mut Each<'_>,
) -> Result<(), i32> {
    let kind = preset.kind.as_str();
    if covering(kind) {
        // One spec at a time: none a plugin describes is kept for this.
        let mut refused = Ok(());
        let walked = settings.walk(paperpaw_sdk::Pick::All, &mut |spec| {
            let key = spec.id.as_str();
            if !covers(kind, settings, widgets, spec) || untouchable(key, spec) {
                return ControlFlow::Continue(());
            }
            let value = last_value(preset, key).and_then(|text| Given::of(spec, text));
            refused = each(key, spec, value);
            match refused {
                Ok(()) => ControlFlow::Continue(()),
                Err(_) => ControlFlow::Break(()),
            }
        });
        refused?;
        return walked.map_err(|_| ERR_NO_MEMORY);
    }
    for (key, text) in preset.values() {
        let Some(spec) = settings.spec(key) else {
            continue;
        };
        if untouchable(key, spec) {
            continue;
        }
        if let Some(value) = Given::of(spec, text) {
            each(key, spec, Some(value))?;
        }
    }
    Ok(())
}

/// A setting of another plugin that is guarded may only be changed by who
/// the host exempts (the settings editor, the kernel), as `set_setting`.
fn permitted(owner: &Id, exempt: bool, key: &str, spec: &SettingSpec) -> Result<(), i32> {
    if spec.guarded && !exempt && key.split('.').next() != Some(owner.as_str()) {
        Err(ERR_INVALID)
    } else {
        Ok(())
    }
}

/// What it takes to make setting `key` hold `given` (`None`: its default,
/// not set), or `None` where it holds it already: as
/// [`presets::commands`] decides, without allocating but for the command.
fn change(
    settings: &Settings,
    key: &str,
    spec: &SettingSpec,
    given: Option<Given<'_>>,
) -> Result<Option<Command>, i32> {
    // A widget's own frame is kept where it is named, its default too: it is
    // the widget's own, over the frame of all.
    let frame = key.ends_with(".frame")
        && key != paperpaw_sdk::builtin::settings::WIDGET_FRAME.static_str();
    let default = settings
        .spec(key)
        .filter(|known| known.id.as_str() == key)
        .map(|known| &known.default);
    let kept = given.filter(|value| frame || !default.is_some_and(|d| value.equals(d)));
    let holds = match &kept {
        Some(value) => {
            (!frame || settings.is_set(key))
                && settings.global(key).is_some_and(|now| value.equals(now))
        }
        None => !settings.is_set(key),
    };
    if holds {
        return Ok(None);
    }
    let id = if key == spec.id.as_str() {
        spec.id.try_clone().ok_or(ERR_NO_MEMORY)?
    } else {
        match Id::try_parse(key) {
            Ok(id) => id,
            Err(IdError::NoMemory) => return Err(ERR_NO_MEMORY),
            Err(_) => return Ok(None),
        }
    };
    Ok(Some(match &kept {
        Some(value) => Command::SetSetting(id, value.value()?),
        None => Command::ResetSetting(id),
    }))
}

/// Ends a walk once a chunk is full (never an answer to anybody).
const STOP: i32 = i32::MIN;

/// The commands for the next [`CHUNK`] settings of `preset` after the first
/// `from` of its walk, pushed on `out` (room for them is reserved first);
/// whether there are more. Nothing is allocated but through `try_reserve`: a
/// refusal is `ERR_NO_MEMORY` and `out` holds what was planned before it.
pub(crate) fn plan(
    settings: &Settings,
    widgets: &Widgets,
    preset: &Preset,
    from: usize,
    out: &mut Vec<Command>,
) -> Result<bool, i32> {
    out.try_reserve_exact(CHUNK).map_err(|_| ERR_NO_MEMORY)?;
    let (mut seen, mut more) = (0_usize, false);
    let walked = walk(settings, widgets, preset, &mut |key, spec, given| {
        seen += 1;
        if seen <= from {
            return Ok(());
        }
        if seen > from + CHUNK {
            more = true;
            return Err(STOP);
        }
        if let Some(command) = change(settings, key, spec, given)? {
            // The room for a chunk is reserved.
            out.push(command);
        }
        Ok(())
    });
    match walked {
        Err(STOP) => Ok(more),
        other => other.map(|()| false),
    }
}

impl CommandStage<'_> {
    /// Whether `command` applies a preset.
    pub(super) fn is_preset(command: &WireCommand<'_>) -> bool {
        matches!(command, WireCommand::ApplyPreset { .. })
    }

    /// Takes the admitted presets, in order, for the host to apply after the
    /// callback.
    #[must_use]
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "the production native command path takes it with the shell wiring"
        )
    )]
    pub(crate) fn take_presets(&mut self) -> Vec<Box<str>> {
        core::mem::take(&mut self.staged_presets)
    }

    /// The preset records of `batch`, checked and copied; their bytes are
    /// the second part of the answer.
    #[inline(never)]
    pub(super) fn validate_presets(
        &self,
        batch: &CommandBatch<'_>,
    ) -> Result<(Vec<Box<str>>, usize), i32> {
        let mut names: Vec<Box<str>> = Vec::new();
        let count = batch.iter().filter(Self::is_preset).count();
        if count == 0 {
            return Ok((names, 0));
        }
        let ui = self.ui.as_ref().ok_or(ERR_UNSUPPORTED)?;
        let widgets = ui.env().widgets();
        names.try_reserve_exact(count).map_err(|_| ERR_NO_MEMORY)?;
        let mut bytes = 0_usize;
        for command in batch.iter() {
            let WireCommand::ApplyPreset { preset, flags } = command else {
                continue;
            };
            if flags != 0 {
                return Err(ERR_UNSUPPORTED);
            }
            let found = self.settings.presets().get(preset).ok_or(ERR_NOT_FOUND)?;
            walk(self.settings, widgets, found, &mut |key, spec, _| {
                permitted(self.owner, self.guarded_exempt, key, spec)
            })?;
            let mut name = alloc::string::String::new();
            name.try_reserve_exact(preset.len())
                .map_err(|_| ERR_NO_MEMORY)?;
            name.push_str(preset);
            names.push(name.into_boxed_str());
            bytes = bytes.saturating_add(preset.len());
        }
        Ok((names, bytes))
    }
}

impl Kernel {
    /// Applies the presets a callback of `owner` submitted, in order, after
    /// its other commands. A preset gone since (its plugin stopped), or one
    /// that would now change a guarded setting `owner` may not, is dropped
    /// whole with a warning; one that cannot be planned for want of memory
    /// ends there with a warning, what its earlier chunks changed staying
    /// changed. Each setting that does not hold its value already is set or
    /// reset, [`CHUNK`] settings at a time ([`plan`]).
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "the production native command path takes it with the shell wiring"
        )
    )]
    pub(crate) fn apply_native_presets(
        &mut self,
        owner: &Id,
        authority: CommandAuthority,
        presets: Vec<Box<str>>,
    ) {
        for name in presets {
            let checked = match self.shared.settings.presets().get(&name) {
                Some(preset) => walk(
                    &self.shared.settings,
                    &self.shared.widgets,
                    preset,
                    &mut |key, spec, _| permitted(owner, authority.guarded_exempt, key, spec),
                ),
                None => Err(ERR_NOT_FOUND),
            };
            match checked {
                Ok(()) => {}
                Err(ERR_NOT_FOUND) => {
                    self.warn_runtime("native preset dropped: it is gone");
                    self.native_registry_dropped(owner);
                    continue;
                }
                Err(_) => {
                    self.warn_runtime("native preset dropped: it changes a guarded setting");
                    self.native_registry_dropped(owner);
                    continue;
                }
            }
            let mut from = 0;
            loop {
                let mut commands: Vec<Command> = Vec::new();
                let planned = match self.shared.settings.presets().get(&name) {
                    Some(preset) => plan(
                        &self.shared.settings,
                        &self.shared.widgets,
                        preset,
                        from,
                        &mut commands,
                    ),
                    None => Err(ERR_NOT_FOUND),
                };
                let Ok(more) = planned else {
                    self.warn_runtime("native preset not applied: no memory");
                    break;
                };
                for command in commands {
                    self.apply(command);
                }
                if !more {
                    break;
                }
                from += CHUNK;
            }
        }
    }
}
