//! What a menu is made of, without the host: the rows the registry answers
//! for it, which of them are shown, which the focus passes over, what choosing
//! one does, and what a setting row does. Everything allocates fallibly.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::text::copy;

/// The menu that opens first (`core.menu`).
pub const MAIN: &str = "core.main";
/// Hand-over parameter: the menu to show.
pub const PARAM_MENU: &str = "menu";
/// Hand-over parameter: how many menus are open below this one.
pub const PARAM_DEPTH: &str = "depth";
/// Hand-over parameter: items to leave out, separated by commas.
pub const PARAM_HIDE: &str = "hide";
/// Hand-over parameter: how to show the menu (`bar`).
pub const PARAM_MODE: &str = "mode";
/// The value of [`PARAM_MODE`] for the reader's bar.
pub const MODE_BAR: &str = "bar";
/// The setting that says how menus look.
pub const STYLE: &str = "menu.style";
/// [`STYLE`]: a list.
pub const STYLE_LIST: &str = "menu.style.list";
/// [`STYLE`]: tiles.
pub const STYLE_TILES: &str = "menu.style.tiles";
/// Most menus that may lie over each other: a loop of submenus ends here.
pub const MAX_DEPTH: usize = 8;

/// `reg_menu_row.flags` (registry.md section 3).
pub mod flag {
    /// Shown as a switch, on.
    pub const CHECKED: u32 = 1;
    /// Chosen now; otherwise shown faint and passed by the focus.
    pub const ENABLED: u32 = 1 << 1;
}

/// What a row of a menu is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Runs an action.
    Action,
    /// A setting, changed in place.
    Setting,
    /// Another menu, opened over this one.
    Submenu,
    /// A heading over the rows that follow.
    Section,
}

/// A row as the registry answers it, owned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The action, setting or submenu (empty for a section).
    pub item: String,
    /// What it is.
    pub kind: Kind,
    /// [`flag`] bits.
    pub flags: u32,
    /// The label, already translated.
    pub title: String,
    /// The icon, empty for none.
    pub icon: String,
    /// What the plugin shows at the right, empty for none.
    pub badge: String,
    /// The line under it, empty for none.
    pub describe: String,
}

impl Row {
    /// A copy made with fallible allocation.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn try_clone(&self) -> Result<Self, Error> {
        Ok(Self {
            item: copy(&self.item)?,
            kind: self.kind,
            flags: self.flags,
            title: copy(&self.title)?,
            icon: copy(&self.icon)?,
            badge: copy(&self.badge)?,
            describe: copy(&self.describe)?,
        })
    }

    /// Whether it can be chosen now: sections never, faint rows not.
    #[must_use]
    pub fn choosable(&self) -> bool {
        self.kind != Kind::Section && self.flags & flag::ENABLED != 0
    }

    /// Whether its plugin shows it as a switch, on.
    #[must_use]
    pub fn checked(&self) -> bool {
        self.flags & flag::CHECKED != 0
    }
}

/// Whether `item` is named in the list `hide` of a menu parameter.
#[must_use]
pub fn hidden(hide: &str, item: &str) -> bool {
    hide.split(',').any(|h| h.trim() == item)
}

/// The depth in a parameter: digits, else none (the menu is a first one).
#[must_use]
pub fn depth_of(text: Option<&str>) -> usize {
    text.and_then(|t| t.parse().ok()).unwrap_or(0)
}

/// A copy of `rows` made with fallible allocation.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn copy_rows(rows: &[Row]) -> Result<Vec<Row>, Error> {
    let mut found = Vec::new();
    found
        .try_reserve_exact(rows.len())
        .map_err(|_| Error::NoMemory)?;
    for row in rows {
        found.push(row.try_clone()?);
    }
    Ok(found)
}

/// The rows a menu shows: those not left out by `hide`, and no heading with
/// nothing below it (a heading is followed by a row that is not a heading).
///
/// # Errors
/// [`Error::NoMemory`] when the list cannot be made; `rows` is consumed.
pub fn shown(rows: Vec<Row>, hide: &str, drop: impl Fn(&Row) -> bool) -> Result<Vec<Row>, Error> {
    let mut kept: Vec<Row> = Vec::new();
    kept.try_reserve_exact(rows.len())
        .map_err(|_| Error::NoMemory)?;
    for row in rows {
        if row.kind != Kind::Section && hidden(hide, &row.item) {
            continue;
        }
        if drop(&row) {
            continue;
        }
        kept.push(row);
    }
    let mut keep: Vec<bool> = Vec::new();
    keep.try_reserve_exact(kept.len())
        .map_err(|_| Error::NoMemory)?;
    for i in 0..kept.len() {
        keep.push(
            kept[i].kind != Kind::Section
                || kept
                    .get(i + 1)
                    .is_some_and(|next| next.kind != Kind::Section),
        );
    }
    let mut keep = keep.into_iter();
    kept.retain(|_| keep.next().unwrap_or(true));
    Ok(kept)
}

/// Whether the menu opens as tiles: the user wants them, it is a menu of its
/// own (not a submenu) and every entry has an icon.
#[must_use]
pub fn tiles(style: &str, depth_given: bool, rows: &[Row]) -> bool {
    if style != STYLE_TILES || depth_given {
        return false;
    }
    let mut entries = rows.iter().filter(|r| r.kind != Kind::Section).peekable();
    entries.peek().is_some() && entries.all(|r| !r.icon.is_empty())
}

/// The row the focus starts on: the first that can be chosen (the first row
/// if none can).
#[must_use]
pub fn first_choosable(rows: &[Row]) -> usize {
    rows.iter().position(Row::choosable).unwrap_or(0)
}

/// The next row the focus rests on from `from`, forward or back; rows that
/// cannot be chosen are passed. `None` at the end (the focus stays).
#[must_use]
pub fn step_focus(rows: &[Row], from: usize, forward: bool) -> Option<usize> {
    if forward {
        (from + 1..rows.len()).find(|i| rows[*i].choosable())
    } else {
        (0..from.min(rows.len()))
            .rev()
            .find(|i| rows[*i].choosable())
    }
}

/// What choosing a row does.
#[derive(Debug, PartialEq, Eq)]
pub enum Choice {
    /// Nothing: a section, a faint row, or too deep.
    Nothing,
    /// Open this menu over the current one, one deeper.
    OpenMenu {
        /// The submenu.
        menu: String,
        /// Its depth.
        depth: usize,
    },
    /// Close this menu and those below it, then run the action: `back`
    /// closes in all.
    Run {
        /// The action.
        action: String,
        /// How many menus to close first.
        back: usize,
    },
    /// Change the setting in place (or open its editor).
    Setting(String),
}

/// What choosing `row` in a menu `depth` deep does.
///
/// # Errors
/// [`Error::NoMemory`] when the answer's text cannot be copied.
pub fn choose(row: &Row, depth: usize) -> Result<Choice, Error> {
    if !row.choosable() {
        return Ok(Choice::Nothing);
    }
    Ok(match row.kind {
        Kind::Section => Choice::Nothing,
        Kind::Submenu if depth + 1 >= MAX_DEPTH => Choice::Nothing,
        Kind::Submenu => Choice::OpenMenu {
            menu: copy(&row.item)?,
            depth: depth + 1,
        },
        Kind::Setting => Choice::Setting(copy(&row.item)?),
        Kind::Action => Choice::Run {
            action: copy(&row.item)?,
            back: depth + 1,
        },
    })
}

/// A value of a setting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// A switch.
    Bool(bool),
    /// A number.
    Int(i64),
    /// A text or the id of a choice.
    Text(String),
}

/// What kind of value a setting takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingKind {
    /// A switch.
    Bool,
    /// A number.
    Int,
    /// One of a few.
    Choice,
    /// A text.
    Text,
    /// A language.
    Language,
    /// A list.
    List,
    /// A kind a newer host adds.
    Other,
}

/// `reg_setting.flags` bit: its `shown_if` holds now.
pub const SETTING_SHOWN: u32 = 1 << 9;
/// `reg_setting.flags` bit: the value is withheld.
pub const SETTING_HIDDEN: u32 = 1 << 5;
/// `reg_setting.flags` bit: an int setting that asks for a slider
/// (`docs/abi/registry.md`).
pub const SETTING_SLIDER: u32 = 1 << 11;

/// A setting in a menu with what its row needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingInfo {
    /// Its id.
    pub key: String,
    /// What it takes.
    pub kind: SettingKind,
    /// `reg_setting.flags`.
    pub flags: u32,
    /// Its value, if readable.
    pub value: Option<Value>,
    /// Smallest number.
    pub min: i64,
    /// Largest number.
    pub max: i64,
    /// One step.
    pub step: i64,
    /// Translated unit; may hold `{count}`.
    pub unit: String,
    /// 0 none, 1 screen, 2 slot.
    pub editor_kind: u32,
    /// The editor's screen or slot.
    pub editor: String,
    /// How many choices.
    pub choices: u32,
}

impl SettingInfo {
    /// A copy made with fallible allocation.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn try_clone(&self) -> Result<Self, Error> {
        Ok(Self {
            key: copy(&self.key)?,
            kind: self.kind,
            flags: self.flags,
            value: match &self.value {
                Some(Value::Text(t)) => Some(Value::Text(copy(t)?)),
                other => other.clone(),
            },
            min: self.min,
            max: self.max,
            step: self.step,
            unit: copy(&self.unit)?,
            editor_kind: self.editor_kind,
            editor: copy(&self.editor)?,
            choices: self.choices,
        })
    }

    /// Whether it is a number that asks for a slider over a range of more
    /// than one value.
    #[must_use]
    pub fn slider(&self) -> bool {
        self.kind == SettingKind::Int && self.flags & SETTING_SLIDER != 0 && self.max > self.min
    }

    /// Whether the menu lists it: a setting shown only while another has a
    /// value is left out, and so is one with fewer than two choices.
    #[must_use]
    pub fn listed(&self) -> bool {
        self.flags & SETTING_SHOWN != 0
            && !(matches!(self.kind, SettingKind::Choice | SettingKind::Language)
                && self.choices < 2
                && self.editor_kind == 0)
    }
}

/// What choosing a setting's row does.
#[derive(Debug, PartialEq, Eq)]
pub enum Act {
    /// Nothing.
    Nothing,
    /// Open the setting's own editor screen.
    Screen(String),
    /// Open the setting's own editor in its slot.
    Slot(String),
    /// Change it by one step at once (a switch, a choice of two).
    Step,
    /// Start changing a number with the buttons.
    StartNumber,
    /// Open the list of its values.
    Values,
    /// Open its settings page (text and the rest).
    Page,
}

/// What choosing the row of `info` does. `touch`: numbers have their − and +
/// on the row, so a tap changes nothing there and only confirm starts.
///
/// # Errors
/// [`Error::NoMemory`] when the editor's name cannot be copied.
pub fn act(info: &SettingInfo, touch: bool, confirm: bool) -> Result<Act, Error> {
    match info.editor_kind {
        1 => return Ok(Act::Screen(copy(&info.editor)?)),
        2 => return Ok(Act::Slot(copy(&info.editor)?)),
        _ => {}
    }
    Ok(match info.kind {
        SettingKind::Int if !touch || confirm => Act::StartNumber,
        SettingKind::Int => Act::Nothing,
        SettingKind::Bool => Act::Step,
        SettingKind::Choice | SettingKind::Language if info.choices == 2 => Act::Step,
        SettingKind::Choice | SettingKind::Language => Act::Values,
        SettingKind::Text | SettingKind::List => Act::Page,
        SettingKind::Other => Act::Nothing,
    })
}

/// The value after `step` steps (a switch toggles, a number stops at its
/// ends); `None` where nothing changes. A choice of two is changed by the
/// caller, which knows the other option.
#[must_use]
pub fn stepped(info: &SettingInfo, step: i64) -> Option<Value> {
    match (info.kind, &info.value) {
        (SettingKind::Bool, Some(Value::Bool(on))) => Some(Value::Bool(!on)),
        (SettingKind::Int, Some(Value::Int(now))) => {
            let unit = info.step.max(1);
            let next = now
                .saturating_add(step.saturating_mul(unit))
                .clamp(info.min, info.max.max(info.min));
            (next != *now).then_some(Value::Int(next))
        }
        _ => None,
    }
}

/// `value` with its unit as the catalogs say it: a unit that is a catalog key
/// (`core.unit.pt`, with plural forms) is formatted by `plural` with the
/// number as `{count}`; a key no catalog has leaves the number alone, never
/// the key on the screen. Any other unit is taken as written ([`with_unit`]).
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals other than "no such key".
pub fn said_with_unit(
    value: i64,
    unit: &str,
    plural: &mut dyn FnMut(&str, i64) -> Result<String, Error>,
) -> Result<String, Error> {
    let is_key = unit.contains('.') && !unit.contains(' ') && !unit.contains("{count}");
    if !is_key {
        return with_unit(value, unit);
    }
    match plural(unit, value) {
        Ok(said) if said != unit && !said.is_empty() => Ok(said),
        Ok(_) | Err(Error::NotFound | Error::Invalid) => with_unit(value, ""),
        Err(error) => Err(error),
    }
}

/// `value` with its unit: a unit that holds `{count}` has the number put in,
/// any other follows the number.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn with_unit(value: i64, unit: &str) -> Result<String, Error> {
    use paperpaw_ui_kit::text::{concat, decimal_u64};
    let mut digits = [0; 20];
    let number = if value < 0 {
        let positive = decimal_u64(value.unsigned_abs(), &mut digits);
        concat(&["-", positive])?
    } else {
        copy(decimal_u64(value as u64, &mut digits))?
    };
    if unit.is_empty() {
        return Ok(number);
    }
    match unit.split_once("{count}") {
        Some((before, after)) => concat(&[before, &number, after]),
        None => concat(&[&number, " ", unit]),
    }
}

#[cfg(test)]
mod tests;
