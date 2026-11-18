//! The reader's bar (`mode=bar`) without the host, as Beta 3 showed it: a
//! title and a line about where one is (the chapter, the page at its end) at
//! the top of the screen, then the entries as icons with short labels, several
//! to a row. What it was opened with, which rows it shows, where everything
//! goes, which entry a tap or the buttons reach and what an entry does.
//! Everything allocates fallibly.
use crate::native_model::{self as model, Act, Kind, Row, SETTING_SHOWN, SettingInfo, SettingKind};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{Point, Rect, native::Error};
use paperpaw_ui_kit::text::copy;

/// Hand-over parameter: the bar's first line (the book's title).
pub const PARAM_TITLE: &str = "title";
/// Hand-over parameter: the start of its second line (the chapter).
pub const PARAM_TEXT: &str = "text";
/// Hand-over parameter: the end of its second line, always whole (the page).
pub const PARAM_PAGE: &str = "page";
/// Hand-over parameter: the action a tap on the two lines runs.
pub const PARAM_ACTION: &str = "action";
/// Icons in a bar are this much larger than body text.
pub const ICON_SCALE: f32 = 1.6;

/// What a menu was opened with (`menu.list`, slot `core.menu`): the list's
/// parameters and the bar's. Unknown keys are left out.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Handed {
    /// [`model::PARAM_MENU`].
    pub menu: String,
    /// [`model::PARAM_DEPTH`], as given.
    pub depth: Option<String>,
    /// [`model::PARAM_HIDE`].
    pub hide: String,
    /// [`model::PARAM_MODE`].
    pub mode: String,
    /// [`PARAM_TITLE`].
    pub title: String,
    /// [`PARAM_TEXT`].
    pub text: String,
    /// [`PARAM_PAGE`].
    pub page: String,
    /// [`PARAM_ACTION`].
    pub action: String,
}

impl Handed {
    /// Takes one parameter, copied to its exact size.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn take(&mut self, key: &str, value: &str) -> Result<(), Error> {
        let field = match key {
            model::PARAM_MENU => &mut self.menu,
            model::PARAM_HIDE => &mut self.hide,
            model::PARAM_MODE => &mut self.mode,
            PARAM_TITLE => &mut self.title,
            PARAM_TEXT => &mut self.text,
            PARAM_PAGE => &mut self.page,
            PARAM_ACTION => &mut self.action,
            model::PARAM_DEPTH => {
                self.depth = Some(copy(value)?);
                return Ok(());
            }
            _ => return Ok(()),
        };
        *field = copy(value)?;
        Ok(())
    }

    /// Whether the menu shows as the reader's bar.
    #[must_use]
    pub fn is_bar(&self) -> bool {
        self.mode == model::MODE_BAR
    }

    /// Whether the menu may show as tiles (see [`model::tiles`]): not as the
    /// reader's bar, and a menu of its own (no depth given: not a submenu).
    #[must_use]
    pub fn may_be_tiles(&self) -> bool {
        !self.is_bar() && self.depth.is_none()
    }

    /// The hand-over of tiles: the menu and `hide` copied; no title, chapter,
    /// page or action (Beta 3's tiles were the bar without its two lines).
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn tiles(&self) -> Result<Self, Error> {
        Ok(Self {
            menu: copy(&self.menu)?,
            hide: copy(&self.hide)?,
            ..Self::default()
        })
    }
}

/// Whether a menu opened with `handed` shows as tiles: the style read
/// (`menu.style`, `None` where it could not be read) is Tiles, it is a menu of
/// its own and every entry of `rows` (as the registry answers them) has an
/// icon, as Beta 3 decided.
#[must_use]
pub fn shows_tiles(style: Option<&str>, handed: &Handed, rows: &[Row]) -> bool {
    handed.may_be_tiles()
        && style.is_some_and(|style| model::tiles(style, handed.depth.is_some(), rows))
}

/// Whether `action` names an action (an id): Beta 3 left the tap on the two
/// lines without one otherwise.
///
/// # Errors
/// [`Error::NoMemory`] when the id cannot be checked.
pub fn valid_action(action: &str) -> Result<bool, Error> {
    match paperpaw_sdk::Id::try_parse(action) {
        Ok(_) => Ok(true),
        Err(paperpaw_sdk::IdError::NoMemory) => Err(Error::NoMemory),
        Err(_) => Ok(false),
    }
}

/// The rows the bar shows: no headings, none named by `hide`, no setting
/// shown only while another has a value (`infos` has the menu's settings).
///
/// # Errors
/// [`Error::NoMemory`]; `rows` is consumed.
pub fn entries(rows: Vec<Row>, hide: &str, infos: &[SettingInfo]) -> Result<Vec<Row>, Error> {
    model::shown(rows, hide, |row| match row.kind {
        Kind::Section => true,
        Kind::Setting => infos
            .iter()
            .find(|i| i.key == row.item)
            .is_some_and(|i| i.flags & SETTING_SHOWN == 0),
        _ => false,
    })
}

/// What choosing an entry does.
#[derive(Debug, PartialEq, Eq)]
pub enum Does {
    /// Nothing: shown faint by its plugin.
    Nothing,
    /// Open this menu as a list over the bar (depth 1).
    OpenMenu(String),
    /// Close the bar, then run this action.
    Run(String),
    /// What a setting's entry does ([`Act::Step`]: changed in place).
    Setting(Act),
}

/// What choosing `row` does; `info` is its setting, if the registry knows it.
/// As Beta 3: switches and choices of two change in place, longer choices
/// open their list, a setting's own editor opens, numbers, text and lists
/// open their settings page; an entry that is no setting runs.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn does(row: &Row, info: Option<&SettingInfo>) -> Result<Does, Error> {
    if !row.choosable() {
        return Ok(Does::Nothing);
    }
    Ok(match (row.kind, info) {
        (Kind::Section, _) => Does::Nothing,
        (Kind::Submenu, _) => Does::OpenMenu(copy(&row.item)?),
        (Kind::Action, _) | (Kind::Setting, None) => Does::Run(copy(&row.item)?),
        (Kind::Setting, Some(info)) => match model::act(info, true, false)? {
            // Numbers have no room here: their settings page.
            Act::Nothing if info.kind == SettingKind::Int => Does::Setting(Act::Page),
            act => Does::Setting(act),
        },
    })
}

/// Whether `row` shows as on: a switch that is on, or an entry its plugin
/// shows as one.
#[must_use]
pub fn on(row: &Row, info: Option<&SettingInfo>) -> bool {
    match info.and_then(|i| i.value.as_ref()) {
        Some(model::Value::Bool(on)) => *on,
        _ => row.checked(),
    }
}

/// What a button does to the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// The focus one entry on (1) or back (-1); the first press only shows it.
    Move(isize),
    /// Choose the entry with the focus.
    Confirm,
    /// Close the bar.
    Close,
    /// Taken, nothing done (turning pages while the focus is not shown).
    Swallow,
    /// Not the bar's.
    Other,
}

/// What `action` does with the focus shown or not (`keys`) in an interface
/// that runs right to left or not: the entries run from the right there, so
/// the left button goes on and the right one back.
#[must_use]
pub fn key(action: &str, keys: bool, rtl: bool) -> Key {
    let sideways = if rtl { -1 } else { 1 };
    match action {
        // Turning pages (also by swiping) does not show the focus on touch
        // screens; directions do.
        "core.next-page" | "core.prev-page" if !keys => Key::Swallow,
        "core.up" | "core.previous" | "core.prev-page" => Key::Move(-1),
        "core.down" | "core.next" | "core.next-page" => Key::Move(1),
        "core.left" => Key::Move(-sideways),
        "core.right" => Key::Move(sideways),
        "core.confirm" => Key::Confirm,
        "core.menu" | "core.back" => Key::Close,
        _ => Key::Other,
    }
}

/// The focus one step (`delta` 1 or -1) on from `from` among `len` entries,
/// round the ends, past those `enabled` refuses; it stays where every one is.
#[must_use]
pub fn step_focus(len: usize, from: usize, delta: isize, enabled: impl Fn(usize) -> bool) -> usize {
    let Ok(count) = isize::try_from(len) else {
        return from;
    };
    if count == 0 {
        return from;
    }
    let next = |at: usize| -> usize {
        isize::try_from(at)
            .unwrap_or(0)
            .saturating_add(delta)
            .rem_euclid(count)
            .unsigned_abs()
    };
    let mut focus = next(from);
    for _ in 0..len {
        if enabled(focus) {
            return focus;
        }
        focus = next(focus);
    }
    from
}

/// The measures the bar is laid out with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// The screen.
    pub width: u16,
    pub height: u16,
    /// The line height of the title (text size) and of the second line and
    /// the labels (small size).
    pub line: f32,
    pub small: f32,
    /// The icons' size.
    pub icon: f32,
    pub gap: u16,
    pub margin: u16,
    /// The line under the bar.
    pub border: u16,
    /// The interface runs right to left.
    pub rtl: bool,
}

/// Where the bar's parts are.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The whole bar, from the top of the screen.
    pub region: Rect,
    /// The title and the second line (no height without them).
    pub info: Rect,
    /// Where each entry is drawn (as wide as its icon or label).
    pub cells: Vec<Rect>,
    /// Taps count this far to the sides of a cell: halfway to the next one.
    pub half: u16,
    /// Laid out right to left.
    pub rtl: bool,
}

/// What a tap at a point reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// Outside the bar: it closes.
    Outside,
    /// The entry at this index.
    Entry(usize),
    /// The title and the second line.
    Info,
    /// The bar, but nothing in it.
    Inside,
}

impl Layout {
    /// Where entry `index` is tapped.
    #[must_use]
    pub fn target(&self, index: usize) -> Option<Rect> {
        let cell = self.cells.get(index)?;
        Some(Rect::new(
            cell.x.saturating_sub(self.half),
            cell.y,
            cell.width.saturating_add(self.half.saturating_mul(2)),
            cell.height,
        ))
    }

    /// What a tap at `at` reaches: entries first, then the two lines.
    #[must_use]
    pub fn hit(&self, at: Point) -> Hit {
        if !self.region.contains(at) {
            return Hit::Outside;
        }
        if let Some(index) =
            (0..self.cells.len()).find(|i| self.target(*i).is_some_and(|t| t.contains(at)))
        {
            return Hit::Entry(index);
        }
        if self.info.contains(at) {
            Hit::Info
        } else {
            Hit::Inside
        }
    }
}

/// The width an entry takes: its label's or its icon's, whichever is wider,
/// in whole pixels.
#[must_use]
pub fn entry_width(label: f32, icon: f32) -> f32 {
    let wide = label.max(icon).max(0.0) + 0.99;
    // Whole pixels, as Beta 3 rounded them (a cast saturates).
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a saturating float-to-integer cast is wanted"
    )]
    let whole = wide as u32;
    whole as f32
}

/// The widest entry in each of `columns` columns (entry `i` in column
/// `i % columns`), summed; nothing is allocated.
fn columns_width(widths: &[f32], columns: usize) -> f32 {
    (0..columns).map(|c| column_width(widths, c, columns)).sum()
}

fn column_width(widths: &[f32], column: usize, columns: usize) -> f32 {
    widths
        .iter()
        .skip(column)
        .step_by(columns.max(1))
        .fold(0.0f32, |a, &w| a.max(w))
}

/// The bar laid out for entries `widths` wide (see [`entry_width`]), with the
/// two lines (`info`) or without: columns as wide as their entries justified
/// between the margins (the first starts under the title, the last ends with
/// the page), as many to a row as fit with a gap between them, rows filled
/// evenly (four and three rather than five and two).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn layout(widths: &[f32], info: bool, m: &Metrics) -> Result<Layout, Error> {
    let gap = m.gap;
    let info_height = if info {
        px(m.line + m.small).saturating_add(gap)
    } else {
        0
    };
    let info = Rect::new(0, gap, m.width, info_height);
    let room = f32::from(m.width.saturating_sub(m.margin.saturating_mul(2)));
    let least_gap = f32::from(gap) * 2.0;
    let count = widths.len().max(1);
    let columns = (1..=count)
        .rev()
        .find(|&n| columns_width(widths, n) + least_gap * (n - 1) as f32 <= room)
        .unwrap_or(1);
    let rows = count.div_ceil(columns);
    let columns = count.div_ceil(rows);
    let mut lefts: Vec<(f32, f32)> = Vec::new();
    lefts
        .try_reserve_exact(columns)
        .map_err(|_| Error::NoMemory)?;
    let spare = (room - columns_width(widths, columns)).max(0.0);
    let between = if columns > 1 {
        spare / (columns - 1) as f32
    } else {
        0.0
    };
    let mut x = f32::from(m.margin);
    for c in 0..columns {
        let width = column_width(widths, c, columns);
        lefts.push((x, width));
        x += width + between;
    }
    let cell_height = px(m.icon + m.small).saturating_add(gap.saturating_mul(2));
    let top = info.y.saturating_add(info.height).saturating_add(gap);
    // The first entry at the start side: the right where the UI runs right
    // to left.
    let across = Rect::new(0, top, m.width, cell_height);
    let mut cells = Vec::new();
    cells
        .try_reserve_exact(widths.len())
        .map_err(|_| Error::NoMemory)?;
    for i in 0..widths.len() {
        let (row, column) = (i / columns, i % columns);
        let (left, width) = lefts.get(column).copied().unwrap_or((0.0, 0.0));
        let y = top.saturating_add(cell_height.saturating_mul(clamp(row)));
        let cell = Rect::new(px(left), y, px(width), cell_height);
        cells.push(paperpaw_ui_kit::style::flip_rect(m.rtl, across, cell));
    }
    let height = top
        .saturating_add(cell_height.saturating_mul(clamp(rows)))
        .saturating_add(gap)
        .saturating_add(m.border);
    Ok(Layout {
        region: Rect::new(0, 0, m.width, height.min(m.height)),
        info,
        cells,
        half: px(between / 2.0),
        rtl: m.rtl,
    })
}

/// Pixels as a `u16`: a cast saturates.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a saturating float-to-integer cast is wanted"
)]
fn px(value: f32) -> u16 {
    value as u16
}

fn clamp(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

/// The chapter and the page fitted in `width`, as Beta 3's `fit_pair`: the
/// chapter takes at most 60 % of the room before the page is fitted, then
/// is shortened to what the page leaves. `fit(text, room)` shortens a text to
/// `room` pixels, `measure` gives its width. The texts and the page's width.
///
/// # Errors
/// What `fit` and `measure` return.
pub fn fit_pair(
    chapter: &str,
    page: &str,
    (width, gap): (f32, f32),
    fit: impl Fn(&str, f32) -> Result<String, Error>,
    measure: impl Fn(&str) -> Result<f32, Error>,
) -> Result<(String, String, f32), Error> {
    let room = (width - gap).max(0.0);
    let chapter_width = measure(chapter)?.min(room * 0.6);
    // A pixel of slack for rounding, as Beta 3.
    let page = fit(page, room - chapter_width + 1.0)?;
    let page_width = measure(&page)?;
    let chapter = fit(chapter, (room - page_width).max(0.0) + 1.0)?;
    Ok((chapter, page, page_width))
}
