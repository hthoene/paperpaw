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
//! time, so the host never holds more than one chunk of commands.
use super::{CommandBatch, CommandStage, WireCommand};
use crate::Kernel;
use alloc::{boxed::Box, vec::Vec};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_UNSUPPORTED};
use paperpaw_sdk::{
    Command, Id, SettingSpec, SettingValue, Settings,
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
pub(crate) type Each<'a> =
    dyn FnMut(&str, &SettingSpec, Option<SettingValue>) -> Result<(), i32> + 'a;

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
    settings.group_of(spec) == paperpaw_sdk::builtin::groups::LOOK
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
        for spec in settings.specs() {
            let key = spec.id.as_str();
            if !covers(kind, settings, widgets, spec) || untouchable(key, spec) {
                continue;
            }
            let value = last_value(preset, key).and_then(|text| spec.kind.parse(text));
            each(key, spec, value)?;
        }
        return Ok(());
    }
    for (key, text) in preset.values() {
        let Some(spec) = settings.spec(key) else {
            continue;
        };
        if untouchable(key, spec) {
            continue;
        }
        if let Some(value) = spec.kind.parse(text) {
            each(key, spec, Some(value))?;
        }
    }
    Ok(())
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
                if spec.guarded
                    && !self.guarded_exempt
                    && key.split('.').next() != Some(self.owner.as_str())
                {
                    Err(ERR_INVALID)
                } else {
                    Ok(())
                }
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
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "the production native command path takes it with the shell wiring"
        )
    )]
    /// Applies the presets a callback submitted, in order, after its other
    /// commands. A preset gone since (its plugin stopped) is dropped with a
    /// warning. Each setting that does not hold its value already is set or
    /// reset, [`CHUNK`] settings at a time (a setting is told from its place
    /// in the walk, which is the same each time).
    pub(crate) fn apply_native_presets(&mut self, presets: Vec<Box<str>>) {
        for name in presets {
            let mut done = 0;
            loop {
                let mut commands: Vec<Command> = Vec::new();
                let mut seen = 0;
                let mut more = false;
                let outcome = match self.shared.settings.presets().get(&name) {
                    Some(preset) => walk(
                        &self.shared.settings,
                        &self.shared.widgets,
                        preset,
                        &mut |key, _, value| {
                            seen += 1;
                            if seen <= done {
                                return Ok(());
                            }
                            if seen > done + CHUNK {
                                more = true;
                                return Err(ERR_NO_MEMORY);
                            }
                            let Ok(id) = Id::parse(key) else {
                                return Ok(());
                            };
                            let change = [(id, value)];
                            let needed = presets::commands(&self.shared.settings, &change);
                            commands
                                .try_reserve(needed.len())
                                .map_err(|_| ERR_NO_MEMORY)?;
                            commands.extend(needed);
                            Ok(())
                        },
                    ),
                    None => Err(ERR_NOT_FOUND),
                };
                match outcome {
                    Err(ERR_NOT_FOUND) => {
                        self.warn_runtime("native preset dropped: it is gone");
                        break;
                    }
                    Err(_) if !more => {
                        self.warn_runtime("native preset not applied: no memory");
                        break;
                    }
                    _ => {}
                }
                for command in commands {
                    self.apply(command);
                }
                if !more {
                    break;
                }
                done += CHUNK;
            }
        }
    }
}
