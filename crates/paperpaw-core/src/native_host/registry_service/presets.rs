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
    native_host::{
        declaration_store::Kind,
        registrations::{self as wire, Record},
    },
};
use core::ops::ControlFlow;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_UNSUPPORTED};
use paperpaw_sdk::Pick;
use paperpaw_sdk::presets::Preset;

/// The most bytes of `reg_value.value`.
const VALUE: usize = 512;

/// `reg_value.flags` bit 0: "as if not set" (a preset that resets the key).
const RESET: u32 = 1;
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
            return Ok(());
        }
    }
    native_presets(sources, scope, visit)
}

/// The `preset.spec` declarations of native plugins (the preset's id is the
/// plugin's id and the declared name), after those of the environment.
fn native_presets(
    sources: &Sources<'_>,
    scope: &str,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    let Some(directory) = sources.directory else {
        return Ok(());
    };
    let store = directory.store.try_borrow().map_err(|_| ERR_BUSY)?;
    let mut stopped = false;
    store.keys(Kind::PresetSpec, &mut |key| {
        if stopped {
            return;
        }
        let Some(plugin) = store.plugin_of(key) else {
            return;
        };
        store.with_record(key, &mut |record| {
            let Record::PresetSpec(spec) = record else {
                return;
            };
            if !scope.is_empty() && spec.kind != scope {
                return;
            }
            let Some(id) = Fixed::<{ caps::ID }>::of(&[plugin, ".", spec.name]) else {
                return;
            };
            stopped = visit(&native_item(sources, id.as_str(), plugin, &spec)).is_break();
        });
    });
    Ok(())
}

fn native_item<'a>(
    sources: &'a Sources<'_>,
    id: &'a str,
    plugin: &'a str,
    spec: &wire::PresetSpec<'a>,
) -> Record<'a> {
    Record::ServiceRegItem(wire::ServiceRegItem {
        id,
        plugin,
        flags: 0,
        title: translated(sources.env.i18n(), spec.title, caps::TITLE),
    })
}

/// Op 17: the row and, where the preset has a description, its text.
pub(super) fn detail(
    sources: &Sources<'_>,
    key: &str,
    visit: &mut dyn FnMut(&dyn Row) -> Result<(), i32>,
) -> Result<(), i32> {
    let Some(preset) = sources.env.settings().presets().get(key) else {
        return native_detail(sources, key, visit);
    };
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

/// Op 17 for a preset a native plugin declared.
fn native_detail(
    sources: &Sources<'_>,
    key: &str,
    visit: &mut dyn FnMut(&dyn Row) -> Result<(), i32>,
) -> Result<(), i32> {
    let directory = sources.directory.ok_or(ERR_NOT_FOUND)?;
    let store = directory.store.try_borrow().map_err(|_| ERR_BUSY)?;
    let mut result = Err(ERR_NOT_FOUND);
    store.keys(Kind::PresetSpec, &mut |declared| {
        if result.is_ok() {
            return;
        }
        let Some(plugin) = store.plugin_of(declared) else {
            return;
        };
        store.with_record(declared, &mut |record| {
            let Record::PresetSpec(spec) = record else {
                return;
            };
            let Some(id) = Fixed::<{ caps::ID }>::of(&[plugin, ".", spec.name]) else {
                return;
            };
            if id.as_str() != key {
                return;
            }
            result = visit(&native_item(sources, id.as_str(), plugin, &spec)).and_then(|()| {
                let text = translated(sources.env.i18n(), spec.help, caps::TEXT);
                if text.is_empty() {
                    return Ok(());
                }
                visit(&Record::ServiceRegText(wire::ServiceRegText { text }))
            });
        });
    });
    result
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
            placement(sources, scope, plain, asker, visit)?;
        }
        return Ok(());
    }
    if let Some(preset) = sources.env.settings().presets().get(scope) {
        preset_values(sources, preset, asker, visit);
        return Ok(());
    }
    native_values(sources, scope, asker, visit)
}

/// The `preset.value` declarations of native plugins for preset `scope`: a
/// value as declared, or with `RESET` where the preset sets the key "back to
/// default". Keys no setting here has are left out.
fn native_values(
    sources: &Sources<'_>,
    scope: &str,
    asker: &Asker<'_>,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    let Some(directory) = sources.directory else {
        return Ok(());
    };
    let store = directory.store.try_borrow().map_err(|_| ERR_BUSY)?;
    let mut stopped = false;
    store.keys(Kind::PresetValue, &mut |key| {
        if stopped {
            return;
        }
        store.with_record(key, &mut |record| {
            let Record::PresetValue(line) = record else {
                return;
            };
            if line.preset != scope || line.value.len() > VALUE {
                return;
            }
            let Some(spec) = sources.env.settings().spec(line.key) else {
                return;
            };
            let reset = if line.flags & 1 != 0 { RESET } else { 0 };
            let flow = if authority(sources, asker, spec) == Visibility::Readable {
                visit_value(line.key, line.value, reset, visit)
            } else {
                visit_value(line.key, &[], reset | HIDDEN, visit)
            };
            stopped = flow.is_break();
        });
    });
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
) -> Result<(), i32> {
    let settings = sources.env.settings();
    // The widget's plugin alone, one spec at a time (#248).
    let plugin = plain.split('.').next().unwrap_or_default();
    let walked = settings.walk(Pick::Of(plugin), &mut |spec| {
        let Some(rest) = spec
            .id
            .as_str()
            .strip_prefix(plain)
            .filter(|rest| rest.starts_with('.'))
        else {
            return ControlFlow::Continue(());
        };
        let Some(key) = Fixed::<{ caps::ID }>::of(&[placement, rest]) else {
            return ControlFlow::Continue(());
        };
        if !settings.is_set(key.as_str()) {
            return ControlFlow::Continue(());
        }
        let Some(value) = settings.global(key.as_str()) else {
            return ControlFlow::Continue(());
        };
        put(sources, asker, key.as_str(), spec, value, visit)
    });
    walked.map_err(|_| ERR_NO_MEMORY)
}
