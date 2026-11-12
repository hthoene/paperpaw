//! Ops 1 `settings`, 2 `setting` and 3 `choices` (`registry.md` 2.2, 2.3, 3).
//! Specs, groups and values are read from the environment's settings; what a
//! value shows is decided here, by the same rule as slot 5
//! ([`visibility_of`](crate::abi7::settings_visibility::visibility_of)).
use super::{
    Asker, Row,
    fixed::{Fixed, caps, clip, translated},
    native_extra::{self, Shown},
    schema,
    sources::Sources,
};
use crate::{
    abi7::{
        settings_visibility::{Visibility, visibility_of},
        settings_wire::{SettingValueRef, settings_encode},
    },
    native_host::registrations::{self as wire, Record},
};
use alloc::vec::Vec;
use core::{mem::MaybeUninit, ops::ControlFlow};
use paperpaw_native::host_glue::{ERR_NOT_FOUND, ERR_UNSUPPORTED};
use paperpaw_sdk::{
    Level, SettingKind, SettingSpec, SettingValue, Settings,
    builtin::{groups::OTHER, slots::SETTINGS},
};

/// `reg_setting.flags`.
const GUARDED: u32 = 1;
const SECRET: u32 = 1 << 1;
const SCOPED: u32 = 1 << 2;
const KEPT_ON_RESET: u32 = 1 << 3;
const ADVANCED: u32 = 1 << 4;
const HIDDEN: u32 = 1 << 5;
const HAS_VALUE: u32 = 1 << 6;
const LONG: u32 = 1 << 7;
const PREVIEW: u32 = 1 << 8;
const SHOWN: u32 = 1 << 9;
const GOVERNS: u32 = 1 << 10;
/// An int setting set with a slider (`SettingExtra::slider`), #257.
const SLIDER: u32 = 1 << 11;

/// `reg_setting.source`.
const FROM_DEFAULT: u32 = 0;
const FROM_PRESET: u32 = 1;
const FROM_PROFILE: u32 = 2;
const FROM_USER: u32 = 3;
const FROM_BOOK: u32 = 4;

/// The prefix of the grants' setting ids (decisions 126 and 131).
const GRANT: &str = "core.grant.";

/// The most bytes of `reg_setting.value`.
const VALUE: usize = 64;

/// The most bytes of a choice value in `reg_choice.value`.
const CHOICE_VALUE: usize = 64;

/// A setting value in the `settings_value` encoding, on the stack.
pub(super) struct Encoded<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> Encoded<N> {
    /// `value` encoded, or `None` where it does not fit `N` bytes (or is a
    /// kind this build cannot encode).
    pub(super) fn of(value: &SettingValue) -> Option<Self> {
        let value = match value {
            SettingValue::Bool(value) => SettingValueRef::Bool(*value),
            SettingValue::Int(value) => SettingValueRef::Int(*value),
            SettingValue::Text(text) => SettingValueRef::Text(text),
            _ => return None,
        };
        let mut raw = [MaybeUninit::<u8>::uninit(); N];
        let required = settings_encode(value, &mut raw).ok()?;
        if required > N {
            return None;
        }
        let mut bytes = [0; N];
        for (to, from) in bytes.iter_mut().zip(raw.iter().take(required)) {
            // SAFETY: the encoder initialized the first `required` slots.
            *to = unsafe { from.assume_init() };
        }
        Some(Self {
            bytes,
            len: required,
        })
    }

    pub(super) fn as_bytes(&self) -> &[u8] {
        self.bytes.get(..self.len).unwrap_or(&[])
    }
}

/// The plugin that fills the settings slot: the system settings screen,
/// which alone may see grants.
fn editor<'a>(sources: &Sources<'a>) -> Option<&'a str> {
    sources
        .env
        .slot_screen(SETTINGS.static_str())
        .map(paperpaw_sdk::Id::namespace)
}

/// What `asker` may see of the value of `spec`: slot 5's rule, except that a
/// secret is never shown to anyone.
pub(super) fn authority(
    sources: &Sources<'_>,
    asker: &Asker<'_>,
    spec: &SettingSpec,
) -> Visibility {
    let grant = spec.id.as_str().starts_with(GRANT);
    visibility_of(asker.plugin, grant, spec.extra().secret, editor(sources))
}

/// The group `spec` is shown in: its own where one is registered, else the
/// catch-all group.
fn group_of<'a>(sources: &Sources<'a>, spec: &'a SettingSpec) -> &'a str {
    let known = sources
        .env
        .settings()
        .groups()
        .any(|group| group.id == spec.group);
    if known {
        spec.group.as_str()
    } else {
        OTHER.static_str()
    }
}

/// The `setting.spec` kind number of `kind`; a kind a later build adds reads
/// as text.
fn kind_number(kind: &SettingKind) -> u32 {
    match kind {
        SettingKind::Bool => 0,
        SettingKind::Int { .. } => 1,
        SettingKind::Choice(_) => 2,
        SettingKind::Language => 4,
        SettingKind::List => 5,
        _ => 3,
    }
}

/// `value` as an `i32`, held to its range.
fn clamp32(value: i64) -> i32 {
    i32::try_from(value.clamp(i64::from(i32::MIN), i64::from(i32::MAX))).unwrap_or_default()
}

/// Where the value of `spec` comes from now, in the order `Settings::get`
/// looks: a power mode, the book, the user, a data plugin's defaults, else
/// the setting's own default.
fn source(sources: &Sources<'_>, spec: &SettingSpec) -> u32 {
    let settings = sources.env.settings();
    let id = spec.id.as_str();
    if settings.is_worn(id) {
        FROM_PRESET
    } else if settings.scoped_value(id).is_some() {
        FROM_BOOK
    } else if settings.is_set(id) {
        FROM_USER
    } else if settings.default_from(id).is_some() {
        FROM_PROFILE
    } else {
        FROM_DEFAULT
    }
}

/// The ids of the settings whose value another setting's showing depends on
/// ([`Settings::governs`], and a native plugin's declared `shown_if`), sorted
/// and each once: one pass over the specs
/// for a whole list instead of one per row (a list of every setting was
/// quadratic, #248). Freed when the list ends; `None` where there is no room
/// for it, and each row then asks `governs` itself.
pub(super) fn governing<'s>(sources: &Sources<'_>, all: &'s Settings) -> Option<Vec<&'s str>> {
    let mut declared = 0_usize;
    native_extra::each_governing(sources, all, &mut |_| declared += 1);
    let count = all
        .specs()
        .filter(|spec| spec.extra().shown_if.is_some())
        .count();
    let mut ids = Vec::new();
    ids.try_reserve_exact(count.checked_add(declared)?).ok()?;
    ids.extend(
        all.specs()
            .filter_map(|spec| spec.extra().shown_if.as_ref())
            .map(|(of, _)| of.as_str()),
    );
    // Within the room counted: the store is changed by the kernel alone,
    // never during a call.
    native_extra::each_governing(sources, all, &mut |of| {
        if ids.len() < ids.capacity() {
            ids.push(of);
        }
    });
    ids.sort_unstable();
    ids.dedup();
    Some(ids)
}

/// The `reg_setting` row of `spec`, handed to `f` (its strings live on this
/// stack). `governing` is what [`governing`] found for a list, `None` for one
/// row alone.
fn with_row<R>(
    sources: &Sources<'_>,
    asker: &Asker<'_>,
    spec: &SettingSpec,
    governing: Option<&[&str]>,
    f: impl FnOnce(&Record<'_>) -> R,
) -> R {
    let env = sources.env;
    let settings = env.settings();
    let id = spec.id.as_str();
    let extra = spec.extra();
    // A native plugin's declared extra is read from the store; while the
    // store is being changed the spec's own counts (the list is asked again).
    let (slider, shown) = native_extra::with_extra(sources, spec, |shown| {
        (shown.slider, native_extra::shows(settings, shown))
    })
    .unwrap_or_else(|_| (extra.slider, settings.shows(spec)));
    let mut flags = 0;
    for (on, bit) in [
        (spec.guarded, GUARDED),
        (extra.secret, SECRET),
        (spec.scoped, SCOPED),
        (spec.is_kept_on_reset(), KEPT_ON_RESET),
        (spec.level == Level::Advanced, ADVANCED),
        (shown, SHOWN),
        (
            governing.map_or_else(
                || settings.governs(id) || native_extra::governs(sources, settings, id),
                |ids| ids.binary_search(&id).is_ok(),
            ),
            GOVERNS,
        ),
        (
            slider && matches!(spec.kind, SettingKind::Int { .. }),
            SLIDER,
        ),
    ] {
        if on {
            flags |= bit;
        }
    }
    let mut encoded = None;
    match authority(sources, asker, spec) {
        Visibility::Hidden => flags |= HIDDEN,
        Visibility::Redacted => {
            // Never shown, only whether there is one.
            flags |= HIDDEN;
            if settings.is_set(id) || env.secrets().has(id) {
                flags |= HAS_VALUE;
            }
        }
        Visibility::Readable => {
            if let Some(value) = settings.get(id) {
                flags |= HAS_VALUE;
                encoded = Encoded::<VALUE>::of(value);
                if encoded.is_none() {
                    flags |= LONG;
                }
                // In the active preview set, whether or not the value shown
                // differs from the kept one.
                if settings.is_previewed(id) {
                    flags |= PREVIEW;
                }
            }
        }
    }
    f(&Record::ServiceRegSetting(wire::ServiceRegSetting {
        key: id,
        group: group_of(sources, spec),
        kind: kind_number(&spec.kind),
        flags,
        // Where a value comes from is as much the caller's business as the
        // value: a row it cannot read shows the default source, so a hidden
        // grant or a secret does not tell whether the user set it.
        source: if flags & HIDDEN == 0 {
            source(sources, spec)
        } else {
            FROM_DEFAULT
        },
        title: translated(env.i18n(), &spec.title, caps::TITLE),
        value: encoded.as_ref().map_or(&[], Encoded::as_bytes),
    }))
}

pub(super) fn rows(
    sources: &Sources<'_>,
    op: u32,
    scope: &str,
    asker: &Asker<'_>,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    match op {
        schema::OP_SETTINGS => settings(sources, scope, asker, visit),
        schema::OP_CHOICES => choices(sources, scope, visit),
        _ => Err(ERR_UNSUPPORTED),
    }
}

/// Op 1: the settings in registry order; with a scope, those of the group of
/// that id (those shown in it, not in its parts), else those of the plugin.
fn settings(
    sources: &Sources<'_>,
    scope: &str,
    asker: &Asker<'_>,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    let all = sources.env.settings();
    let is_group = scope == OTHER.static_str() || all.groups().any(|g| g.id.as_str() == scope);
    let governing = governing(sources, all);
    for spec in all.specs() {
        let hit = scope.is_empty()
            || if is_group {
                group_of(sources, spec) == scope
            } else {
                spec.id.namespace() == scope
            };
        if hit && with_row(sources, asker, spec, governing.as_deref(), |row| visit(row)).is_break()
        {
            break;
        }
    }
    Ok(())
}

/// Op 3: the values of choice setting `scope`, the setting's own first and
/// then those other plugins added (the setting keeps them in that order).
/// Nothing for a setting that is not there or is no choice.
fn choices(
    sources: &Sources<'_>,
    scope: &str,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    let Some(spec) = exact(sources, scope) else {
        return Ok(());
    };
    let SettingKind::Choice(values) = &spec.kind else {
        return Ok(());
    };
    for value in values.iter() {
        let row = Record::ServiceRegChoice(wire::ServiceRegChoice {
            value,
            title: translated(sources.env.i18n(), value, caps::TITLE),
        });
        if visit(&row).is_break() {
            break;
        }
    }
    Ok(())
}

/// The setting `id` itself (a placement's id names no setting here).
pub(super) fn exact<'a>(sources: &Sources<'a>, id: &str) -> Option<&'a SettingSpec> {
    sources
        .env
        .settings()
        .spec(id)
        .filter(|spec| spec.id.as_str() == id)
}

/// How many values of `spec` a `reg_choice` row can carry.
fn choice_count(spec: &SettingSpec) -> u32 {
    match &spec.kind {
        SettingKind::Choice(values) => {
            let count = values.iter().filter(|v| v.len() <= CHOICE_VALUE).count();
            u32::try_from(count).unwrap_or(u32::MAX)
        }
        _ => 0,
    }
}

/// Op 2: the row, what the editor needs besides it, and the help text.
pub(super) fn detail(
    sources: &Sources<'_>,
    key: &str,
    asker: &Asker<'_>,
    visit: &mut dyn FnMut(&dyn Row) -> Result<(), i32>,
) -> Result<(), i32> {
    let spec = exact(sources, key).ok_or(ERR_NOT_FOUND)?;
    with_row(sources, asker, spec, None, |row| visit(row))?;
    native_extra::with_extra(sources, spec, |shown| extra_row(spec, shown, visit))??;
    let help = spec.description.as_deref().unwrap_or("");
    if help.is_empty() {
        return Ok(());
    }
    let text = translated(sources.env.i18n(), help, caps::TEXT);
    if text.is_empty() {
        return Ok(());
    }
    visit(&Record::ServiceRegText(wire::ServiceRegText { text }))
}

/// Op 2's `reg_setting_extra` of `spec`, with what its extra says.
fn extra_row(
    spec: &SettingSpec,
    shown: &Shown<'_>,
    visit: &mut dyn FnMut(&dyn Row) -> Result<(), i32>,
) -> Result<(), i32> {
    let (min, max) = match &spec.kind {
        SettingKind::Int { min, max } => (clamp32(*min), clamp32(*max)),
        _ => (0, 0),
    };
    // The section's key of a setting made for each plugin is that plugin's
    // name: built here, on the stack.
    let plugin_section = if shown.section_of_plugin {
        let plugin = spec.id.as_str().rsplit('.').next().unwrap_or_default();
        Fixed::<{ caps::ID }>::of(&[plugin, ".plugin.name"])
    } else {
        None
    };
    let section = plugin_section.as_ref().map_or(shown.section, Fixed::as_str);
    visit(&Record::ServiceRegSettingExtra(
        wire::ServiceRegSettingExtra {
            min,
            max,
            step: clamp32(shown.step),
            unit: clip(shown.unit, 96),
            section: clip(section, caps::ID),
            follows: shown.follows,
            shown_id: shown.shown_id,
            shown_value: clip(shown.shown_value, caps::ID),
            editor_kind: shown.editor_kind,
            editor: shown.editor,
            choices: choice_count(spec),
        },
    ))
}
