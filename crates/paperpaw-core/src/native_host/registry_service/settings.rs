//! Ops 1 `settings`, 2 `setting` and 3 `choices` (`registry.md` 2.2, 2.3, 3).
//! Specs, groups and values are read from the environment's settings; what a
//! value shows is decided here, by the same rule as slot 5
//! ([`visibility_of`](crate::abi7::settings_visibility::visibility_of)).
use super::{
    Asker, Row,
    fixed::{Fixed, caps, clip, translated},
    menu_native::Native,
    native_extra::{self, ExtraLookup, Shown},
    schema,
    sources::{Sources, scope_hash},
};
use crate::native_host::declaration_store::{DeclKey, DeclarationStore};
use crate::{
    abi7::{
        settings_visibility::{Visibility, visibility_of},
        settings_wire::{SettingValueRef, settings_encode},
    },
    native_host::registrations::{self as wire, Record},
};
use alloc::{rc::Rc, vec::Vec};
use core::{cell::OnceCell, mem::MaybeUninit, ops::ControlFlow};
use paperpaw_digest::fnv1a64;
use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED};
use paperpaw_sdk::Pick;
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

/// The hash [`governing`] keeps of a setting id.
pub(super) fn id_hash(id: &str) -> u64 {
    fnv1a64(id.as_bytes())
}

/// The ids of the settings whose value another setting's showing depends on
/// ([`Settings::governs`], and a native plugin's declared `shown_if`), as
/// [`id_hash`]es, sorted and each once: one pass over the specs for every
/// list of the generation instead of one per row (a list of every setting
/// was quadratic, #248) or one per page (#422). A hash that two ids share
/// marks the other one as governing too, which only has its plugin read the
/// list again after a change. `None` where there is no room for it, and each
/// row then asks `governs` itself.
pub(super) fn governing(sources: &Sources<'_>, all: &Settings) -> Option<Rc<[u64]>> {
    if let Some(kept) = sources
        .generation
        .memo()
        .and_then(|memo| memo.governing.clone())
    {
        return Some(kept);
    }
    let mut declared = 0_usize;
    native_extra::each_governing(sources, all, None, &mut |_| declared += 1);
    // An extra is of a spec kept as it is: none a plugin describes is made.
    let count = all
        .kept_specs()
        .filter(|spec| spec.extra().shown_if.is_some())
        .count();
    let mut ids = Vec::new();
    ids.try_reserve_exact(count.checked_add(declared)?).ok()?;
    ids.extend(
        all.kept_specs()
            .filter_map(|spec| spec.extra().shown_if.as_ref())
            .map(|(of, _)| id_hash(of.as_str())),
    );
    // Within the room counted: the store is changed by the kernel alone,
    // never during a call.
    native_extra::each_governing(sources, all, None, &mut |of| {
        if ids.len() < ids.capacity() {
            ids.push(id_hash(of));
        }
    });
    paperpaw_sort::sort_unstable(&mut ids);
    ids.dedup();
    let ids: Rc<[u64]> = Rc::from(ids);
    if let Some(mut memo) = sources.generation.memo() {
        memo.governing = Some(Rc::clone(&ids));
    }
    Some(ids)
}

/// What the rows of one op call look up in the declaration store (#422:
/// looked up afresh for every row, opening the settings screen read the
/// whole store hundreds of times): whether the device meets the `menu.need`
/// rows a native plugin declared for a setting, from one store borrow and
/// with the items that have such rows kept for the generation (a store that
/// is busy or absent asks nothing), and their declared extras.
struct Lookups<'s> {
    native: Option<Native<'s>>,
    /// [`Native::need_keys`], from the memo; `None`: no room, each row then
    /// reads them as [`Native::need_met`] does.
    needs: Option<Rc<[(u32, DeclKey)]>>,
    extras: ExtraLookup,
}

impl<'s> Lookups<'s> {
    /// `fill`: read what the memo lacks (a list); else only what a list of
    /// the round read is used, and one row reads no more than its own.
    fn new(sources: &Sources<'_>, store: Option<&'s DeclarationStore>, fill: bool) -> Self {
        let native = store.map(|store| Native::new(store, sources.env.caps()));
        let needs = native.as_ref().and_then(|native| {
            let mut memo = sources.generation.memo()?;
            if memo.needs.is_none() && fill {
                memo.needs = native.need_keys().map(Rc::from);
            }
            memo.needs.clone()
        });
        let extras = ExtraLookup::new(sources, native.as_ref(), fill);
        Self {
            native,
            needs,
            extras,
        }
    }

    fn met(&self, id: &str) -> bool {
        match (&self.native, &self.needs) {
            (None, _) => true,
            (Some(native), Some(needs)) => native.need_met_keyed(needs, id, false),
            (Some(native), None) => native.need_met(id, false),
        }
    }
}

/// The `reg_setting` row of `spec`, handed to `f` (its strings live on this
/// stack). `governing` is what [`governing`] found, `None` where there was
/// no room for it (the row then asks `governs` itself).
fn with_row<R>(
    sources: &Sources<'_>,
    asker: &Asker<'_>,
    spec: &SettingSpec,
    governing: Option<&[u64]>,
    lookups: &Lookups<'_>,
    f: impl FnOnce(&Record<'_>) -> R,
) -> R {
    #[cfg(test)]
    built::count();
    let env = sources.env;
    let settings = env.settings();
    let id = spec.id.as_str();
    let extra = spec.extra();
    // A native plugin's declared extra is read from the store, never busy
    // here: the op checked it first ([`native_extra::ready`]).
    let (slider, shown) = native_extra::with_extra(sources, spec, &lookups.extras, |shown| {
        (shown.slider, native_extra::shows(settings, shown))
    })
    .unwrap_or_else(|_| (extra.slider, settings.shows(spec)));
    // Not for this device (a front light it lacks): as the settings screen of
    // interface 6 left it out, so does this row say it is not shown.
    let shown = shown && env.menus().available(id) && lookups.met(id);
    // A reading app to choose where only one reads the books (#260).
    let shown = shown && !crate::reading_apps::nothing_to_choose(spec);
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
                |ids| ids.binary_search(&id_hash(id)).is_ok(),
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

/// A setting's row in a list, built where it is framed and not before: its
/// title is looked up, its value read and what its plugin declares of it
/// found in the plugin's image, which a list that counts its rows or passes
/// those before its cursor has no use for (#248: a page of 32 rows built
/// every setting twice).
struct Listed<'a, 's> {
    sources: &'a Sources<'s>,
    asker: &'a Asker<'a>,
    all: &'s Settings,
    spec: &'a SettingSpec,
    /// What [`governing`] finds, asked for by the first row built.
    governing: &'a OnceCell<Option<Rc<[u64]>>>,
    lookups: &'a Lookups<'a>,
}

impl Listed<'_, '_> {
    fn built<R>(&self, f: impl FnOnce(&Record<'_>) -> R) -> R {
        let governing = self
            .governing
            .get_or_init(|| governing(self.sources, self.all));
        with_row(
            self.sources,
            self.asker,
            self.spec,
            governing.as_deref(),
            self.lookups,
            f,
        )
    }
}

impl Row for Listed<'_, '_> {
    fn framed_len(&self) -> Result<usize, i32> {
        self.built(|row| row.framed_len())
    }

    fn encode(&self, out: &mut [u8]) -> Result<usize, i32> {
        self.built(|row| row.encode(out))
    }

    /// Its key and its group alone can be too long for their fields: the
    /// title is cut to its own, a value too long for its own is left out,
    /// the rest are numbers.
    fn frames(&self) -> bool {
        let bare = Record::ServiceRegSetting(wire::ServiceRegSetting {
            key: self.spec.id.as_str(),
            group: group_of(self.sources, self.spec),
            kind: 0,
            flags: 0,
            source: 0,
            title: "",
            value: &[],
        });
        bare.framed_len().is_ok()
    }

    fn encode_within(&self, out: &mut [u8]) -> Result<Option<usize>, i32> {
        self.built(|row| {
            // `frames` says the framing rule again by hand: the built row
            // must agree with it.
            debug_assert_eq!(row.framed_len().is_ok(), self.frames());
            row.encode_within(out)
        })
    }
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
    settings_walk(sources, scope, asker, (0, 0), visit)
}

/// Where row `from` of op 1 for `scope` is in the settings' order, from the
/// last row a walk of the round visited: that row or the one after it (#422:
/// each page walked every row before its cursor again).
pub(super) fn resume_place(sources: &Sources<'_>, scope: &str, from: u32) -> Option<usize> {
    let (of, at, place) = sources.generation.memo()?.last_row?;
    if of != scope_hash(scope) {
        None
    } else if from == at {
        Some(place)
    } else if Some(from) == at.checked_add(1) {
        place.checked_add(1)
    } else {
        None
    }
}

/// Op 1 from row `from` on, which [`resume_place`] finds (`ERR_STATE` where
/// it does not: the caller asked it first).
pub(super) fn settings_from(
    sources: &Sources<'_>,
    scope: &str,
    asker: &Asker<'_>,
    from: u32,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    let place = resume_place(sources, scope, from).ok_or(ERR_STATE)?;
    settings_walk(sources, scope, asker, (from, place), visit)
}

/// Op 1's rows from `start` (the row's index and its place in the settings'
/// order) on; the last row visited is kept for the next page.
fn settings_walk(
    sources: &Sources<'_>,
    scope: &str,
    asker: &Asker<'_>,
    start: (u32, usize),
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    native_extra::ready(sources)?;
    let all = sources.env.settings();
    let is_group = scope == OTHER.static_str() || all.groups().any(|g| g.id.as_str() == scope);
    let governing = OnceCell::new();
    // Changed by the kernel alone, never during a call.
    let store = sources
        .directory
        .and_then(|directory| directory.store.try_borrow().ok());
    let lookups = Lookups::new(sources, store.as_deref(), true);
    // One spec at a time (#248): what a plugin describes of its settings is
    // made for its row and let go, and only that of the plugin asked for.
    let pick = if scope.is_empty() || is_group {
        Pick::All
    } else {
        Pick::Of(scope)
    };
    let (mut index, from) = start;
    let mut last = None;
    let walked = all.walk_from(pick, from, &mut |place, spec| {
        let hit = !is_group || scope.is_empty() || group_of(sources, spec) == scope;
        if hit {
            last = Some((index, place));
            index = index.saturating_add(1);
            visit(&Listed {
                sources,
                asker,
                all,
                spec,
                governing: &governing,
                lookups: &lookups,
            })
        } else {
            ControlFlow::Continue(())
        }
    });
    if let (Some((at, place)), Some(mut memo)) = (last, sources.generation.memo()) {
        memo.last_row = Some((scope_hash(scope), at, place));
    }
    // No room for a spec: the rows given are not all, and the caller is told.
    walked.map_err(|_| ERR_NO_MEMORY)
}

/// Op 3: the values of choice setting `scope`, the setting's own first and
/// then those other plugins added (the setting keeps them in that order);
/// of a language setting, the languages there are ([`languages`]).
/// Nothing for a setting that is not there or is neither.
fn choices(
    sources: &Sources<'_>,
    scope: &str,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    let Some(spec) = exact(sources, scope)? else {
        return Ok(());
    };
    let i18n = sources.env.i18n();
    let mut row = |value: &str, title: &str| {
        visit(&Record::ServiceRegChoice(wire::ServiceRegChoice {
            value,
            title,
        }))
    };
    match &spec.kind {
        SettingKind::Choice(values) => {
            for value in values.iter() {
                if row(value, translated(i18n, value, caps::TITLE)).is_break() {
                    break;
                }
            }
        }
        SettingKind::Language => languages(i18n, &mut row),
        _ => {}
    }
    Ok(())
}

/// The languages a `reg_choice` row can carry, in the translator's order
/// (English first, then by tag), until `visit` breaks: the tag, which is
/// the setting's value, and the language's name in that language as its
/// catalog gives it (`Deutsch`; a language pack's own), or the tag where the
/// catalog gives none. Nothing is allocated.
fn languages(
    i18n: &paperpaw_i18n::Translator,
    visit: &mut dyn FnMut(&str, &str) -> ControlFlow<()>,
) {
    i18n.each_language(&mut |tag, name| {
        if tag.len() > CHOICE_VALUE {
            return ControlFlow::Continue(());
        }
        let name = if name.is_empty() { tag } else { name };
        visit(tag, clip(name, caps::TITLE))
    });
}

/// The setting `id` itself (a placement's id names no setting here).
///
/// # Errors
/// `ERR_NO_MEMORY`: no room to read what its plugin describes of it (#248).
pub(super) fn exact<'a>(sources: &Sources<'a>, id: &str) -> Result<Option<&'a SettingSpec>, i32> {
    let spec = sources.env.settings().try_spec(id);
    let spec = spec.map_err(|_| ERR_NO_MEMORY)?;
    Ok(spec.filter(|spec| spec.id.as_str() == id))
}

/// How many values of `spec` a `reg_choice` row can carry.
fn choice_count(spec: &SettingSpec, i18n: &paperpaw_i18n::Translator) -> u32 {
    match &spec.kind {
        SettingKind::Choice(values) => {
            let count = values.iter().filter(|v| v.len() <= CHOICE_VALUE).count();
            u32::try_from(count).unwrap_or(u32::MAX)
        }
        SettingKind::Language => {
            let mut count = 0_u32;
            languages(i18n, &mut |_, _| {
                count = count.saturating_add(1);
                ControlFlow::Continue(())
            });
            count
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
    let spec = exact(sources, key)?.ok_or(ERR_NOT_FOUND)?;
    native_extra::ready(sources)?;
    let store = sources
        .directory
        .and_then(|directory| directory.store.try_borrow().ok());
    let lookups = Lookups::new(sources, store.as_deref(), false);
    // What a list of the round found, else the row asks itself.
    let governing = sources
        .generation
        .memo()
        .and_then(|memo| memo.governing.clone());
    with_row(
        sources,
        asker,
        spec,
        governing.as_deref(),
        &lookups,
        |row| visit(row),
    )?;
    let i18n = sources.env.i18n();
    native_extra::with_extra(sources, spec, &lookups.extras, |shown| {
        extra_row(spec, shown, i18n, visit)
    })??;
    drop(lookups);
    drop(store);
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
    i18n: &paperpaw_i18n::Translator,
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
            choices: choice_count(spec, i18n),
        },
    ))
}

/// How many rows [`with_row`] built on this thread, for the tests that hold
/// a list to the rows it answers.
#[cfg(test)]
pub(super) mod built {
    extern crate std as test_std;
    use core::cell::Cell;

    test_std::thread_local! {
        static BUILT: Cell<usize> = const { Cell::new(0) };
    }

    pub(super) fn count() {
        BUILT.with(|built| built.set(built.get() + 1));
    }

    /// The rows built since the last call.
    pub(in crate::native_host::registry_service) fn take() -> usize {
        BUILT.with(|built| built.replace(0))
    }
}
