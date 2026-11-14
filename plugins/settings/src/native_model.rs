//! What the settings pages are made of, without the host: settings and
//! groups as the registry describes them, which are shown, the rows of a
//! page, stepping a number, and searching. Everything allocates fallibly.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy},
};

/// `reg_setting.flags` (registry.md section 3).
pub mod flag {
    /// Written by its own plugin and the settings editor alone.
    pub const GUARDED: u32 = 1;
    /// Kept with the secrets.
    pub const SECRET: u32 = 1 << 1;
    /// May differ per scope (a book).
    pub const SCOPED: u32 = 1 << 2;
    /// Reset leaves it.
    pub const KEPT_ON_RESET: u32 = 1 << 3;
    /// Shown when the user asks for advanced settings.
    pub const ADVANCED: u32 = 1 << 4;
    /// The value is withheld.
    pub const HIDDEN: u32 = 1 << 5;
    /// The value is a preview.
    pub const PREVIEW: u32 = 1 << 8;
    /// Its `shown_if` holds now.
    pub const SHOWN: u32 = 1 << 9;
    /// Another setting's `shown_if` names it.
    pub const GOVERNS: u32 = 1 << 10;
}

/// The group developer settings are in.
pub const DEVELOPER_GROUP: &str = "core.group.developer";
/// The group not-found groups fall into.
pub const OTHER_GROUP: &str = "core.group.other";
/// The setting that shows the developer group.
pub const DEVELOPER: &str = "core.developer";
/// "Only this book".
pub const SCOPE_ONLY: &str = "core.scope-only";
/// The language, which "reset all" leaves.
pub const LANGUAGE: &str = "core.language";
/// The advanced switch.
pub const ADVANCED: &str = "settings.advanced";
/// The menu of the pages other plugins add.
pub const PAGES_MENU: &str = "core.settings-pages";
/// Where a group that names no order is listed: after those that do.
pub const UNORDERED: i32 = 10_000;
/// The icon of a group that names none.
pub const GROUP_ICON: &str = "puzzle";

/// What kind of value a setting takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A switch.
    Bool,
    /// A number.
    Int,
    /// One of a few.
    Choice,
    /// Free text.
    Text,
    /// A UI language.
    Language,
    /// A list of ids.
    List,
    /// Something a newer host adds.
    Other,
}

/// A value, owned.
#[derive(Debug, PartialEq, Eq)]
pub enum Val {
    /// A switch.
    Bool(bool),
    /// A number.
    Int(i64),
    /// Text.
    Text(String),
}

/// A setting as the registry lists it.
#[derive(Debug, PartialEq, Eq)]
pub struct Setting {
    /// Its full id.
    pub key: String,
    /// Its group.
    pub group: String,
    /// What it takes.
    pub kind: Kind,
    /// `reg_setting.flags`.
    pub flags: u32,
    /// Whether the value is the default (source 0).
    pub is_default: bool,
    /// Whether the value is kept in the book scope.
    pub in_scope: bool,
    /// Translated name.
    pub title: String,
    /// The readable value.
    pub value: Option<Val>,
}

impl Setting {
    /// Whether `bit` is set.
    #[must_use]
    pub const fn has(&self, bit: u32) -> bool {
        self.flags & bit != 0
    }
}

/// What a setting's page row needs besides the list row (`reg_setting_extra`
/// and the help text), read for the page that shows it.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Detail {
    /// Smallest number.
    pub min: i64,
    /// Largest number.
    pub max: i64,
    /// One step.
    pub step: i64,
    /// Translated unit; may hold `{count}`.
    pub unit: String,
    /// The section key it is listed under; empty for none.
    pub section: String,
    /// 0 none, 1 screen, 2 slot.
    pub editor_kind: u32,
    /// The editor's screen or slot.
    pub editor: String,
    /// How many choices.
    pub choices: u32,
    /// Its description.
    pub help: String,
}

/// A group as the registry lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// Its id.
    pub id: String,
    /// The group it is a part of; empty for none.
    pub parent: String,
    /// A screen of its plugin shown instead of the generated page.
    pub page: String,
    /// By order; 0 names none.
    pub order: i32,
    /// Translated name.
    pub title: String,
}

/// A row of the menu of pages other plugins add.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageRow {
    /// The action it runs; empty for a section.
    pub action: String,
    /// Whether it is a section's heading.
    pub section: bool,
    /// Its order.
    pub order: i32,
    /// Its name.
    pub title: String,
    /// Its icon.
    pub icon: String,
}

/// Whether `id` is `of` or a part (at any depth) of it, by the groups' parents.
#[must_use]
pub fn within(groups: &[Group], id: &str, of: &str) -> bool {
    let mut at = id;
    for _ in 0..=groups.len() {
        if at == of {
            return true;
        }
        match groups.iter().find(|g| g.id == at) {
            Some(g) if !g.parent.is_empty() => at = &g.parent,
            _ => return false,
        }
    }
    false
}

/// Whether `setting` is shown in group `of` (a group not registered shows
/// nothing of its own: its settings fall into the other group).
#[must_use]
pub fn group_is(groups: &[Group], setting: &Setting, of: &str) -> bool {
    in_group(groups, &setting.group, of)
}

/// Whether a setting of group `own` is shown in group `of` (as [`group_is`],
/// for a row that is not copied yet).
#[must_use]
pub fn in_group(groups: &[Group], own: &str, of: &str) -> bool {
    let registered = |id: &str| groups.iter().any(|g| g.id == id);
    if own == of {
        of == OTHER_GROUP || registered(of)
    } else {
        of == OTHER_GROUP && !registered(own)
    }
}

/// What decides which settings show.
#[derive(Clone, Copy, Debug, Default)]
pub struct Context {
    /// The advanced switch.
    pub advanced: bool,
    /// The developer switch.
    pub developer: bool,
    /// Whether a book scope is open on this screen.
    pub scoped: bool,
}

/// Whether `setting` is offered here at all, advanced or not: it means
/// something now and on this device. `options` is its number of choices,
/// when it is a choice and the number is known.
#[must_use]
pub fn offered(groups: &[Group], cx: Context, setting: &Setting, options: Option<u32>) -> bool {
    if setting.key == SCOPE_ONLY && !cx.scoped {
        return false;
    }
    // Secrets are kept elsewhere (Passwords and keys).
    if setting.has(flag::SECRET) || !setting.has(flag::SHOWN) {
        return false;
    }
    if setting.kind == Kind::Other {
        return false;
    }
    // Nothing to choose.
    if setting.kind == Kind::Choice && options.is_some_and(|n| n < 2) {
        return false;
    }
    if group_is(groups, setting, DEVELOPER_GROUP) {
        return cx.developer;
    }
    true
}

/// Whether `setting` shows with the current level.
#[must_use]
pub fn visible(groups: &[Group], cx: Context, setting: &Setting, options: Option<u32>) -> bool {
    offered(groups, cx, setting, options)
        && (!setting.has(flag::ADVANCED)
            || cx.advanced
            || group_is(groups, setting, DEVELOPER_GROUP))
}

/// The ends of the settings of a widget that a widget look covers
/// (`<widget>.frame` …).
const LOOK_PARTS: [&str; 9] = [
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

/// The settings of all widgets a widget look covers, and the gap between them.
const LOOK_ALL: [&str; 8] = [
    "core.widgets.frame",
    "core.widgets.paper",
    "core.widgets.ink",
    "core.widgets.corners",
    "core.widgets.frame-line",
    "core.widgets.shadow",
    "core.widgets.padding",
    "core.widgets.gap",
];

/// Whether a widget look covers setting `key`: each registered widget's frame,
/// padding, size, alignment, colours and shape, and those of all widgets and
/// their gap; not the settings of other things that end so (the status
/// lines' size).
#[must_use]
pub fn widget_look_covers(key: &str, widgets: &[String]) -> bool {
    LOOK_ALL.contains(&key)
        || LOOK_PARTS.iter().any(|part| {
            key.strip_suffix(part)
                .is_some_and(|widget| widgets.iter().any(|w| w == widget))
        })
}

/// The widget a placement id names: `widgets.clock` for `widgets.clock@2`;
/// `None` for an id that is no placement.
#[must_use]
pub fn unplaced(id: &str) -> Option<&str> {
    let (base, number) = id.rsplit_once('@')?;
    (!base.is_empty()
        && !base.contains('@')
        && !number.is_empty()
        && number.bytes().all(|b| b.is_ascii_digit()))
    .then_some(base)
}

/// The setting a placement's setting stands for: `widgets.clock.face` for
/// `widgets.clock@2.face`; `Ok(None)` for a key that is not a placement's.
///
/// # Errors
/// [`Error::NoMemory`] when the id cannot be built.
pub fn unplaced_key(key: &str) -> Result<Option<String>, Error> {
    let Some(at) = key.find('@') else {
        return Ok(None);
    };
    let tail = &key[at + 1..];
    let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
    let rest = &tail[digits..];
    // The number ends the placement: a dot (or nothing) follows, no second one.
    let ended = rest.is_empty() || rest.starts_with('.');
    if at == 0 || digits == 0 || !ended || key[..at].contains('@') || rest.contains('@') {
        return Ok(None);
    }
    concat(&[&key[..at], rest]).map(Some)
}

/// The id of setting `id` of group `of` for its placement `placement`.
///
/// # Errors
/// [`Error::NoMemory`] when the id cannot be built; `Ok(None)` when `id` is not
/// a setting of `of`.
pub fn placed(id: &str, of: &str, placement: &str) -> Result<Option<String>, Error> {
    let Some(rest) = id.strip_prefix(of).filter(|r| r.starts_with('.')) else {
        return Ok(None);
    };
    concat(&[placement, rest]).map(Some)
}

impl Setting {
    /// A copy made with fallible allocation.
    ///
    /// # Errors
    /// [`Error::NoMemory`] when a text cannot be copied.
    pub fn try_clone(&self) -> Result<Self, Error> {
        Ok(Self {
            key: copy(&self.key)?,
            group: copy(&self.group)?,
            kind: self.kind,
            flags: self.flags,
            is_default: self.is_default,
            in_scope: self.in_scope,
            title: copy(&self.title)?,
            value: match &self.value {
                Some(Val::Text(text)) => Some(Val::Text(copy(text)?)),
                Some(Val::Bool(v)) => Some(Val::Bool(*v)),
                Some(Val::Int(v)) => Some(Val::Int(*v)),
                None => None,
            },
        })
    }
}

/// The groups a page of top-level groups lists: those with something visible
/// (`holding`: every group that holds a visible setting), not widgets' own
/// (`widget`) and not parts of another, by order, then by name.
pub fn main_groups(
    groups: &[Group],
    holding: &[String],
    widget: impl Fn(&str) -> bool,
) -> Result<Vec<usize>, Error> {
    let mut found = Vec::new();
    for (index, group) in groups.iter().enumerate() {
        if group.parent.is_empty()
            && !widget(&group.id)
            && holding.iter().any(|held| within(groups, held, &group.id))
        {
            push(&mut found, index)?;
        }
    }
    let key = |i: &usize| {
        let g = &groups[*i];
        (
            if g.order == 0 { UNORDERED } else { g.order },
            g.title.as_str(),
        )
    };
    paperpaw_ui_kit::mem::stable_sort_by(&mut found, |a, b| key(a).cmp(&key(b)));
    Ok(found)
}

/// The settings a reset of `group` (empty: all) puts back are chosen one at a
/// time by [`reset_step`], in the order they are sent, and "only this book" is
/// added last by [`reset_end`]: clearing it first would send the later resets
/// to the global settings instead of the book's values.
///
/// Left alone: what is kept on reset (accounts, folders), secrets, settings
/// already at their default outside a book scope (nothing to reload), the
/// language on a reset of all, and on a page what that page does not show.
///
/// One setting of the reset plan: its key is added where the reset takes it.
///
/// # Errors
/// [`Error::NoMemory`] when the list cannot grow.
pub fn reset_step(
    keys: &mut Vec<String>,
    groups: &[Group],
    cx: Context,
    setting: &Setting,
    group: &str,
) -> Result<(), Error> {
    if setting.has(flag::KEPT_ON_RESET)
        || setting.has(flag::SECRET)
        || setting.is_default && !setting.in_scope
        || setting.key == SCOPE_ONLY
    {
        return Ok(());
    }
    let chosen = if group.is_empty() {
        setting.key != LANGUAGE
    } else {
        group_is(groups, setting, group) && visible(groups, cx, setting, None)
    };
    if chosen {
        push(keys, copy(&setting.key)?)?;
    }
    Ok(())
}

/// The end of a reset plan: the book scope is always reset.
///
/// # Errors
/// [`Error::NoMemory`] when the list cannot grow.
pub fn reset_end(keys: &mut Vec<String>) -> Result<(), Error> {
    push(keys, copy(SCOPE_ONLY)?)
}

/// The standard order of sections on a page (after those that name none).
pub const SECTION_ORDER: &[&str] = paperpaw_sdk::builtin::group_sections::ORDER;

/// The rank of a section on a page: none first, the standard ones, the rest.
#[must_use]
pub fn section_rank(section: &str) -> usize {
    if section.is_empty() {
        0
    } else {
        SECTION_ORDER
            .iter()
            .position(|s| *s == section)
            .map_or(SECTION_ORDER.len().saturating_add(1), |p| p + 1)
    }
}

/// The distinct sections of `sections` in page order.
pub fn section_order<'a>(sections: impl Iterator<Item = &'a str>) -> Result<Vec<&'a str>, Error> {
    let mut found: Vec<&str> = Vec::new();
    for section in sections {
        if !found.contains(&section) {
            push(&mut found, section)?;
        }
    }
    paperpaw_ui_kit::mem::stable_sort_by(&mut found, |a, b| section_rank(a).cmp(&section_rank(b)));
    Ok(found)
}

/// The value after one step of `step` (`+1` / `-1` units of the setting's step),
/// kept in range. A switch toggles; a choice of two takes the other; a number
/// stops at its ends. `None` where nothing changes.
#[must_use]
pub fn stepped(kind: Kind, value: &Val, detail: &Detail, step: i64) -> Option<Val> {
    match (kind, value) {
        (Kind::Bool, Val::Bool(on)) => Some(Val::Bool(!on)),
        (Kind::Int, Val::Int(now)) => {
            let unit = detail.step.max(1);
            let next = now
                .saturating_add(step.saturating_mul(unit))
                .clamp(detail.min, detail.max.max(detail.min));
            (next != *now).then_some(Val::Int(next))
        }
        _ => None,
    }
}

pub use paperpaw_ui_kit::text::with_unit;

/// `text` in lower case without accents, so "rander" finds "Ränder".
pub fn fold(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
    for c in text.chars().flat_map(char::to_lowercase) {
        let plain = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' => "a",
            'ç' | 'č' | 'ć' => "c",
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ě' => "e",
            'ì' | 'í' | 'î' | 'ï' => "i",
            'ñ' | 'ń' | 'ň' => "n",
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ő' => "o",
            'ù' | 'ú' | 'û' | 'ü' | 'ů' | 'ű' => "u",
            'ý' | 'ÿ' => "y",
            'š' | 'ś' => "s",
            'ž' | 'ź' | 'ż' => "z",
            'ß' => "ss",
            'æ' => "ae",
            'œ' => "oe",
            _ => {
                out.try_reserve(c.len_utf8()).map_err(|_| Error::NoMemory)?;
                out.push(c);
                continue;
            }
        };
        out.try_reserve(plain.len()).map_err(|_| Error::NoMemory)?;
        out.push_str(plain);
    }
    Ok(out)
}

/// The words of a query, folded; empty for a query of nothing.
pub fn words(query: &str) -> Result<Vec<String>, Error> {
    let mut found = Vec::new();
    for word in query.split_whitespace() {
        push(&mut found, fold(word)?)?;
    }
    Ok(found)
}

/// Whether every word is in `text` (both folded).
pub fn matches(text: &str, words: &[String]) -> Result<bool, Error> {
    let text = fold(text)?;
    Ok(words.iter().all(|w| text.contains(w.as_str())))
}

/// Whether the settings have the values of a preset (those it names inside
/// `covered`), the others their defaults.
pub fn applied(all: &[Setting], covered: &[String], values: &[(String, Option<Val>)]) -> bool {
    all.iter().filter(|s| covered.contains(&s.key)).all(|s| {
        match values.iter().find(|(key, _)| *key == s.key) {
            Some((_, Some(value))) => s.value.as_ref() == Some(value),
            Some((_, None)) | None => s.is_default,
        }
    })
}

#[cfg(test)]
mod tests;
