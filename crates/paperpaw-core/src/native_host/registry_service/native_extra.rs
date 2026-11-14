//! A native plugin's `setting.extra` (`registry.md` 2.2 and 2.3, todo 541).
//! The setting's spec keeps no extra (`native_values` maps the spec record
//! alone); the record stays in the declaration store and is read from there
//! when a row is asked for, with nothing kept beside it.
use super::{
    fixed::{Fixed, caps},
    sources::Sources,
};
use crate::native_host::{
    declaration_store::Kind,
    registrations::{Record, SettingExtra},
};
use paperpaw_native::host_glue::ERR_BUSY;
use paperpaw_sdk::{Editor, SettingSpec, Settings};

/// `setting_extra.flags` (`paperpaw_sdk::native::declare`).
const SLIDER: u32 = 1;
const SECTION_OF_PLUGIN: u32 = 1 << 1;

/// What a setting's extra says, from either source.
pub(super) struct Shown<'a> {
    pub(super) slider: bool,
    pub(super) section_of_plugin: bool,
    pub(super) step: i64,
    pub(super) unit: &'a str,
    pub(super) section: &'a str,
    pub(super) follows: &'a str,
    pub(super) shown_id: &'a str,
    pub(super) shown_value: &'a str,
    /// 0 none, 1 a screen, 2 a slot; the editor a full id.
    pub(super) editor_kind: u32,
    pub(super) editor: &'a str,
}

impl<'a> Shown<'a> {
    /// The spec's own extra: the kernel's, a linked plugin's.
    fn of(spec: &'a SettingSpec) -> Self {
        let extra = spec.extra();
        let (shown_id, shown_value) = extra
            .shown_if
            .as_ref()
            .map_or(("", ""), |(id, value)| (id.as_str(), *value));
        let (editor_kind, editor) = match &extra.editor {
            Some(Editor::Screen(screen)) => (1, screen.as_str()),
            Some(Editor::Slot(slot)) => (2, slot.as_str()),
            _ => (0, ""),
        };
        Self {
            slider: extra.slider,
            section_of_plugin: extra.section_of_plugin,
            step: extra.step,
            unit: extra.unit.as_deref().unwrap_or(""),
            section: extra.section.as_deref().unwrap_or(""),
            follows: extra.follows.as_ref().map_or("", paperpaw_sdk::Id::as_str),
            shown_id,
            shown_value,
            editor_kind,
            editor,
        }
    }

    /// The record plugin `plugin` declared; `screen` holds the full id of an
    /// editor screen (declared relative to the plugin).
    fn declared(record: &SettingExtra<'a>, screen: Option<&'a str>) -> Self {
        let (editor_kind, editor) = match (record.editor_kind, screen) {
            (1, Some(screen)) => (1, screen),
            (2, _) => (2, record.editor),
            // A screen whose full id would not fit names none.
            _ => (0, ""),
        };
        Self {
            slider: record.flags & SLIDER != 0,
            section_of_plugin: record.flags & SECTION_OF_PLUGIN != 0,
            // The wire writes the default step, 1, as 0.
            step: if record.step == 0 {
                1
            } else {
                i64::from(record.step)
            },
            unit: record.unit,
            section: record.section,
            follows: record.follows,
            shown_id: record.shown_id,
            shown_value: record.shown_value,
            editor_kind,
            editor,
        }
    }
}

/// `ERR_BUSY` while the store is being changed: checked once by an op
/// before its rows (the kernel changes the store between calls alone).
pub(super) fn ready(sources: &Sources<'_>) -> Result<(), i32> {
    match sources.directory {
        Some(directory) => directory.store.try_borrow().map(drop).map_err(|_| ERR_BUSY),
        None => Ok(()),
    }
}

/// Hands `f` the extra of `spec`: its own where it has one, else the one
/// its native plugin declared, else none. `ERR_BUSY` while the store is
/// being changed. The cost is one name lookup: an indexed owner's is a
/// binary search; an image-only owner walks only the batches that declare
/// extras (their kind mask), so a plugin with none costs a mask test a
/// batch.
pub(super) fn with_extra<R>(
    sources: &Sources<'_>,
    spec: &SettingSpec,
    mut f: impl FnMut(&Shown<'_>) -> R,
) -> Result<R, i32> {
    let mut out = None;
    if let (Some(directory), Some((plugin, name))) = (sources.directory, native_name(spec)) {
        let store = directory.store.try_borrow().map_err(|_| ERR_BUSY)?;
        store.find(plugin, Kind::Extra, name, &mut |record| {
            if let Record::SettingExtra(extra) = record {
                let screen = Fixed::<{ caps::ID }>::of(&[plugin, ".", extra.editor]);
                let screen = screen.as_ref().map(Fixed::as_str);
                out = Some(f(&Shown::declared(&extra, screen)));
            }
        });
    }
    Ok(match out {
        Some(out) => out,
        None => f(&Shown::of(spec)),
    })
}

/// The plugin and name of a setting whose spec says nothing of what a
/// `setting.extra` record says, which a native plugin may have declared (a
/// spec that does keeps its own: the kernel's, a linked plugin's).
fn native_name(spec: &SettingSpec) -> Option<(&str, &str)> {
    let own = spec.extra();
    let has_own = own.slider
        || own.section_of_plugin
        || own.step != 1
        || own.unit.is_some()
        || own.section.is_some()
        || own.follows.is_some()
        || own.shown_if.is_some()
        || own.editor.is_some();
    if has_own {
        return None;
    }
    let plugin = spec.id.namespace();
    let name = spec.id.as_str().strip_prefix(plugin)?.strip_prefix('.')?;
    Some((plugin, name))
}

/// Whether a setting with `shown` shows now: not while the setting its
/// `shown_if` names has another value (as [`Settings::shows`] for a spec's
/// own; a setting not there hides nothing).
pub(super) fn shows(settings: &Settings, shown: &Shown<'_>) -> bool {
    if shown.shown_id.is_empty() {
        return true;
    }
    let Some(condition) = settings.spec(shown.shown_id) else {
        return true;
    };
    condition
        .kind
        .parse(shown.shown_value)
        .is_some_and(|value| settings.get(shown.shown_id) == Some(&value))
}

/// Hands `f` the id of each setting a native plugin's declared `shown_if`
/// names (once per record, as `all` spells it). With `named`, only the
/// records that may name that setting are read: a declared id is spelled as
/// `all` spells it (`declarations::full_id`: no placement), so a record that
/// names the setting holds its id. Nothing while the store is being changed.
pub(super) fn each_governing<'s>(
    sources: &Sources<'_>,
    all: &'s Settings,
    named: Option<&str>,
    f: &mut dyn FnMut(&'s str),
) {
    let Some(directory) = sources.directory else {
        return;
    };
    let Ok(store) = directory.store.try_borrow() else {
        return;
    };
    let wanted = |bytes: &[u8]| {
        named.is_none_or(|id| crate::native_host::declaration_store::holds(bytes, id))
    };
    store.each_wanted(Kind::Extra, &wanted, &mut |record| {
        if let Record::SettingExtra(extra) = record
            && let Some(spec) = all.spec(extra.shown_id)
        {
            f(spec.id.as_str());
        }
    });
}

/// Whether a native plugin's declared `shown_if` names setting `id`: asked
/// for one row alone, so only the declared extras that may name it are read
/// (every extra of the device was, a row: #248).
pub(super) fn governs(sources: &Sources<'_>, all: &Settings, id: &str) -> bool {
    let mut found = false;
    each_governing(sources, all, Some(id), &mut |of| found |= of == id);
    found
}
