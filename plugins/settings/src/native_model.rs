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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Val {
    /// A switch.
    Bool(bool),
    /// A number.
    Int(i64),
    /// Text.
    Text(String),
}

/// A setting as the registry lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
    let registered = |id: &str| groups.iter().any(|g| g.id == id);
    if setting.group == of {
        of == OTHER_GROUP || registered(of)
    } else {
        of == OTHER_GROUP && !registered(&setting.group)
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

/// The id of setting `id` of group `of` for its placement `placement`.
#[must_use]
pub fn placed(id: &str, of: &str, placement: &str) -> Option<String> {
    let rest = id.strip_prefix(of).filter(|r| r.starts_with('.'))?;
    concat(&[placement, rest]).ok()
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

/// `value` with its unit: a unit that holds `{count}` has the number put in,
/// any other follows the number.
pub fn with_unit(value: i64, unit: &str) -> Result<String, Error> {
    let mut digits = [0; 20];
    let number = if value < 0 {
        let positive = paperpaw_ui_kit::text::decimal_u64(value.unsigned_abs(), &mut digits);
        concat(&["-", positive])?
    } else {
        copy(paperpaw_ui_kit::text::decimal_u64(
            value as u64,
            &mut digits,
        ))?
    };
    if unit.is_empty() {
        return Ok(number);
    }
    match unit.split_once("{count}") {
        Some((before, after)) => concat(&[before, &number, after]),
        None => concat(&[&number, " ", unit]),
    }
}

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
