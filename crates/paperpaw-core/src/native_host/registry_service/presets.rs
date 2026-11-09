//! Ops 16 `presets`, 17 `preset` and 18 `values` (`registry.md` 2.2, 2.3, 3):
//! the presets plugins added, and the values a preset or a placement sets.
use super::{
    Asker, Row,
    fixed::{Fixed, caps, translated},
    schema,
    settings::{Encoded, authority},
    sources::Sources,
};
use crate::{
    abi7::settings_visibility::Visibility,
    native_host::registrations::{self as wire, Record},
};
use core::ops::ControlFlow;
use paperpaw_native::host_glue::{ERR_NOT_FOUND, ERR_UNSUPPORTED};
use paperpaw_sdk::presets::Preset;

/// The most bytes of `reg_value.value`.
const VALUE: usize = 512;

/// `reg_value.flags` bit 1: the value is withheld.
const HIDDEN: u32 = 1 << 1;

pub(super) fn rows(
    sources: &Sources<'_>,
    op: u32,
    scope: &str,
    asker: &Asker<'_>,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    match op {
        schema::OP_PRESETS => presets(sources, scope, visit),
        schema::OP_VALUES => values(sources, scope, asker, visit),
        _ => Err(ERR_UNSUPPORTED),
    }
}

/// The `reg_item` of `preset`: who added it (its id's namespace) and its name.
fn item<'a>(sources: &'a Sources<'_>, preset: &'a Preset) -> Record<'a> {
    Record::ServiceRegItem(wire::ServiceRegItem {
        id: preset.id.as_str(),
        plugin: preset.id.namespace(),
        flags: 0,
        title: translated(sources.env.i18n(), &preset.title, caps::TITLE),
    })
}

/// Op 16: the presets in the order they were added; with a scope, those of
/// that kind.
fn presets(
    sources: &Sources<'_>,
    scope: &str,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    for preset in sources.env.settings().presets().iter() {
        if !scope.is_empty() && preset.kind.as_str() != scope {
            continue;
        }
        if visit(&item(sources, preset)).is_break() {
            break;
        }
    }
    Ok(())
}

/// Op 17: the row and, where the preset has a description, its text.
pub(super) fn detail(
    sources: &Sources<'_>,
    key: &str,
    visit: &mut dyn FnMut(&dyn Row) -> Result<(), i32>,
) -> Result<(), i32> {
    let preset = sources
        .env
        .settings()
        .presets()
        .get(key)
        .ok_or(ERR_NOT_FOUND)?;
    visit(&item(sources, preset))?;
    if preset.description.is_empty() {
        return Ok(());
    }
    let text = translated(sources.env.i18n(), &preset.description, caps::TEXT);
    if text.is_empty() {
        return Ok(());
    }
    visit(&Record::ServiceRegText(wire::ServiceRegText { text }))
}

/// One `reg_value`: `value` is where slot 5 would give it to the asker, else
/// withheld. A value too long for its field is not listed.
fn put(
    sources: &Sources<'_>,
    asker: &Asker<'_>,
    key: &str,
    spec: &paperpaw_sdk::SettingSpec,
    value: &paperpaw_sdk::SettingValue,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> ControlFlow<()> {
    if authority(sources, asker, spec) != Visibility::Readable {
        return visit_value(key, &[], HIDDEN, visit);
    }
    match Encoded::<VALUE>::of(value) {
        Some(encoded) => visit_value(key, encoded.as_bytes(), 0, visit),
        None => ControlFlow::Continue(()),
    }
}

fn visit_value(
    key: &str,
    value: &[u8],
    flags: u32,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> ControlFlow<()> {
    visit(&Record::ServiceRegValue(wire::ServiceRegValue {
        key,
        value,
        flags,
    }))
}

/// Op 18: what preset `scope` sets, or (the scope has an `@`: the caller
/// said it is a placement) what that placement sets itself. Nothing for a
/// scope that names neither.
fn values(
    sources: &Sources<'_>,
    scope: &str,
    asker: &Asker<'_>,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    if scope.is_empty() {
        return Ok(());
    }
    if let Some((plain, number)) = scope.split_once('@') {
        if !plain.is_empty()
            && !number.is_empty()
            && number.bytes().all(|byte| byte.is_ascii_digit())
        {
            placement(sources, scope, plain, asker, visit);
        }
        return Ok(());
    }
    if let Some(preset) = sources.env.settings().presets().get(scope) {
        preset_values(sources, preset, asker, visit);
    }
    Ok(())
}

/// The lines of `preset` as the settings read them: those that name a known
/// setting and a value it takes (what `Preset::changes` applies), a setting
/// named twice by its last line that takes.
fn preset_values(
    sources: &Sources<'_>,
    preset: &Preset,
    asker: &Asker<'_>,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) {
    let settings = sources.env.settings();
    for (at, (id, text)) in preset.values().enumerate() {
        let Some(spec) = settings.spec(id) else {
            continue;
        };
        let Some(value) = spec.kind.parse(text) else {
            continue;
        };
        let taken = |line: &str| spec.kind.parse(line).is_some();
        if preset
            .values()
            .skip(at + 1)
            .any(|(later, text)| later == id && taken(text))
        {
            continue;
        }
        if put(sources, asker, id, spec, &value, visit).is_break() {
            break;
        }
    }
}

/// The values placement `placement` (`widgets.clock@2`, of widget `plain`)
/// sets itself, not the fallbacks, in the settings' order.
fn placement(
    sources: &Sources<'_>,
    placement: &str,
    plain: &str,
    asker: &Asker<'_>,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) {
    let settings = sources.env.settings();
    for spec in settings.specs() {
        let Some(rest) = spec
            .id
            .as_str()
            .strip_prefix(plain)
            .filter(|rest| rest.starts_with('.'))
        else {
            continue;
        };
        let Some(key) = Fixed::<{ caps::ID }>::of(&[placement, rest]) else {
            continue;
        };
        if !settings.is_set(key.as_str()) {
            continue;
        }
        let Some(value) = settings.global(key.as_str()) else {
            continue;
        };
        if put(sources, asker, key.as_str(), spec, value, visit).is_break() {
            break;
        }
    }
}
