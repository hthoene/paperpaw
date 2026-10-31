//! Widgets in a grid, like a phone's home screen: an area cut into cells
//! (columns and rows), each item taking a rectangle of them: a widget (any
//! plugin's) or a tool (an action, shown as its icon and name). Cells left
//! empty stay empty, so everything sits where it was put.
//!
//! The layout is data: a text such as
//! `home.clock 0/0 4x1, tool:files.library 0/5` ([`parse`], [`write()`]),
//! each item its id, its column and row, and its size in cells, kept where
//! the plugin likes (a setting, a file). An item without a place (as
//! plugins add them) goes into the first free cells ([`place`]), as wide as
//! the grid and as high as it needs.
//!
//! [`GridView`] shows such a grid on a screen of the plugin's own, and edits
//! it in place: drag an item to move it, by its corner to size it, tap it
//! for its options (move or size it with the buttons, its settings, remove
//! it). With buttons alone, the focus goes from item to item.
//!
//! ```ignore
//! struct Board { grid: GridView } // made with `GridView::asking(env, CHOSEN)`
//!
//! impl Board {
//!     fn layout(&mut self, env: &Env<'_>) {
//!         let geometry = Geometry::new(env, area, 4, 0);
//!         let items = grid::parse(env.settings().read(&LAYOUT));
//!         self.grid.layout(env, geometry, &items, &[]);
//!     }
//!
//!     fn respond(&mut self, response: GridResponse, cx: &mut Cx<'_, '_>) -> Handled {
//!         match &response {
//!             GridResponse::Changed(items) => {
//!                 let value = LAYOUT.value(grid::write(items).into());
//!                 cx.command(Command::SetSetting(LAYOUT.id().clone(), value));
//!             }
//!             GridResponse::Editing(_) => { let env = *cx.env(); self.layout(&env) }
//!             _ => {}
//!         }
//!         response.handled()
//!     }
//! }
//!
//! impl Screen for Board {
//!     fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) { self.grid.draw(canvas, env) }
//!     fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
//!         let response = self.grid.on_action(action, cx);
//!         self.respond(response, cx)
//!     }
//!     fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
//!         let env = *cx.env();
//!         if self.grid.update_widgets(&env, cx) {
//!             cx.request_refresh(RefreshMode::Fast);
//!         }
//!         cx.wake_at(env.now().add_millis(60_000));
//!     }
//!     // on_pointer, on_gesture, on_event, hint, holds_edges as on_action
//! }
//! ```

mod view;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::ui::UiStyle;
use crate::{Canvas, Color, Env, Id, Point, Rect};

pub use view::{DropCheck, GridResponse, GridView};

/// Columns at most, and rows at most.
pub const MAX_COLUMNS: u8 = 6;
pub const MAX_ROWS: u8 = 12;
/// Tools are written with this before their action's id.
const TOOL: &str = "tool:";

/// What an item shows.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum What {
    /// A widget, by its id.
    Widget(String),
    /// A tool: an action, shown as its icon and name.
    Tool(Id),
}

impl What {
    fn text(&self) -> String {
        match self {
            What::Widget(id) => id.clone(),
            What::Tool(action) => format!("{TOOL}{action}"),
        }
    }

    /// Its name, translated: the widget's, or the action's.
    pub fn title(&self, env: &Env<'_>) -> String {
        let (id, key) = match self {
            What::Widget(id) => (id.as_str(), env.widgets().title(id)),
            What::Tool(action) => (action.as_str(), env.menus().title(action.as_str())),
        };
        key.map_or(id, |key| env.i18n().tr(key)).to_string()
    }
}

/// An item of a grid: what it shows and the cells it takes.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Item {
    pub what: What,
    /// Its first column and row, and its width and height in cells;
    /// `None` until it has a place (see [`place`]).
    pub at: Option<Cells>,
}

impl Item {
    /// An item without a place yet.
    pub fn new(what: What) -> Self {
        Self { what, at: None }
    }

    /// An item at `cells`.
    pub fn placed(what: What, cells: Cells) -> Self {
        Self {
            what,
            at: Some(cells),
        }
    }
}

/// A rectangle of cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Cells {
    pub col: u8,
    pub row: u8,
    pub w: u8,
    pub h: u8,
}

impl Cells {
    pub const fn new(col: u8, row: u8, w: u8, h: u8) -> Self {
        Self { col, row, w, h }
    }

    pub fn overlaps(&self, other: &Cells) -> bool {
        self.col < other.col + other.w
            && other.col < self.col + self.w
            && self.row < other.row + other.h
            && other.row < self.row + self.h
    }

    /// Whether it lies within a grid of `columns` by `rows`.
    pub fn fits(&self, columns: u8, rows: u8) -> bool {
        self.w > 0
            && self.h > 0
            && u16::from(self.col) + u16::from(self.w) <= u16::from(columns)
            && u16::from(self.row) + u16::from(self.h) <= u16::from(rows)
    }
}

/// The items of a layout as [`write()`] has them; what cannot be read is
/// left out.
pub fn parse(text: &str) -> Vec<Item> {
    text.split(',')
        .filter_map(|item| {
            let mut parts = item.split_whitespace();
            let id = parts.next()?;
            let what = match id.strip_prefix(TOOL) {
                Some(action) => What::Tool(Id::parse(action).ok()?),
                None => What::Widget(id.to_string()),
            };
            let at = (|| {
                let (col, row) = parts.next()?.split_once('/')?;
                let (w, h) = parts.next()?.split_once('x')?;
                Some(Cells {
                    col: col.parse().ok()?,
                    row: row.parse().ok()?,
                    w: w.parse().ok()?,
                    h: h.parse().ok()?,
                })
            })();
            Some(Item { what, at })
        })
        .collect()
}

/// The layout of `items` as text: each item's id (a tool's action after
/// `tool:`), its column and row, and its size in cells, separated by
/// commas.
pub fn write(items: &[Item]) -> String {
    let mut out = String::new();
    for item in items {
        if !out.is_empty() {
            out.push_str(", ");
        }
        out.push_str(&item.what.text());
        if let Some(c) = item.at {
            out.push_str(&format!(" {}/{} {}x{}", c.col, c.row, c.w, c.h));
        }
    }
    out
}

/// The cells of a grid on this screen, and where they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Geometry {
    pub columns: u8,
    pub rows: u8,
    /// The area the cells fill.
    pub area: Rect,
    pub cell_w: u16,
    pub cell_h: u16,
    /// Between items.
    pub gap: u16,
}

impl Default for Geometry {
    fn default() -> Self {
        Self::of(Rect::default(), 1, 1, 0)
    }
}

impl Geometry {
    /// The grid in `area` with `columns` (2 to [`MAX_COLUMNS`]) and `rows`
    /// (2 to [`MAX_ROWS`]; 0: as many as make cells about as high as wide),
    /// the style's gap between items. The gap's half is left at the area's
    /// edges, so an area half a gap wider than the content lines items up
    /// with it.
    pub fn new(env: &Env<'_>, area: Rect, columns: u8, rows: u8) -> Self {
        let columns = columns.clamp(2, MAX_COLUMNS);
        let cell_w = area.width / u16::from(columns);
        let rows = match rows {
            0 => {
                let target = (u32::from(cell_w) * 85 / 100).max(1);
                ((u32::from(area.height) + target / 2) / target).clamp(3, u32::from(MAX_ROWS)) as u8
            }
            n => n.clamp(2, MAX_ROWS),
        };
        Self::of(area, columns, rows, UiStyle::new(env).gap as u16)
    }

    /// The grid in `area` with exactly `columns` and `rows` and `gap`.
    pub fn of(area: Rect, columns: u8, rows: u8, gap: u16) -> Self {
        let (columns, rows) = (columns.max(1), rows.max(1));
        Self {
            columns,
            rows,
            area,
            cell_w: area.width / u16::from(columns),
            cell_h: area.height / u16::from(rows),
            gap,
        }
    }

    /// Where the cells `c` are on the screen, the gap left around.
    pub fn rect(&self, c: Cells) -> Rect {
        let half = self.gap / 2;
        Rect::new(
            self.area.x + u16::from(c.col) * self.cell_w + half,
            self.area.y + u16::from(c.row) * self.cell_h + half,
            (u16::from(c.w) * self.cell_w).saturating_sub(self.gap),
            (u16::from(c.h) * self.cell_h).saturating_sub(self.gap),
        )
    }

    /// The cell (column, row) at `point`, if it is on the grid.
    pub fn cell_at(&self, point: Point) -> Option<(u8, u8)> {
        let (dx, dy) = (
            point.x - i32::from(self.area.x),
            point.y - i32::from(self.area.y),
        );
        if dx < 0 || dy < 0 {
            return None;
        }
        let col = dx / i32::from(self.cell_w.max(1));
        let row = dy / i32::from(self.cell_h.max(1));
        (col < i32::from(self.columns) && row < i32::from(self.rows))
            .then_some((col as u8, row as u8))
    }

    /// The size an item gets when it has no place yet: a tool one cell, a
    /// widget the grid's width and the rows it needs (made to measure it,
    /// what it shows read).
    pub fn size_of(&self, env: &Env<'_>, what: &What) -> (u8, u8) {
        let (w, h, _) = self.sizes_of(env, what);
        (w, h)
    }

    /// [`Geometry::size_of`], and the fewest rows it can do with
    /// ([`Widget::min_height`](crate::widgets::Widget::min_height)).
    fn sizes_of(&self, env: &Env<'_>, what: &What) -> (u8, u8, u8) {
        match what {
            What::Tool(_) => (1, 1, 1),
            What::Widget(id) => {
                // As it says, within the grid, without making it.
                if let Some(hint) = env.widgets().cells(id) {
                    let w = hint.w.clamp(1, self.columns.max(1));
                    let h = hint.h.clamp(1, self.rows.max(1));
                    let cells = hint.keep(Cells::new(0, 0, w, h), w <= h);
                    let (w, h) = (cells.w.min(self.columns), cells.h.min(self.rows));
                    return (w, h, hint.min_h.clamp(1, h));
                }
                let width = (u16::from(self.columns) * self.cell_w).saturating_sub(self.gap);
                let (height, least) = env.widgets().make(id, env).map_or((0, 0), |widget| {
                    let height = widget.height(env, width);
                    (height, widget.min_height(env, width).min(height))
                });
                let rows = |height: u16| {
                    (u32::from(height) + u32::from(self.gap))
                        .div_ceil(u32::from(self.cell_h.max(1)))
                        .clamp(1, u32::from(self.rows)) as u8
                };
                (self.columns, rows(height), rows(least))
            }
        }
    }
}

/// The cells an item at `from` takes when a finger that came down at
/// `start` is at `point`: moved by as many cells as the finger moved, or
/// with `resize` (its corner taken), its corner in the cell under the
/// finger, at least one cell (a widget's least where it says, see
/// [`CellSpan::keep`](crate::widgets::CellSpan::keep)). `None` off the
/// grid.
pub fn dragged(
    geometry: &Geometry,
    from: Cells,
    start: Point,
    point: Point,
    resize: bool,
) -> Option<Cells> {
    let (col, row) = geometry.cell_at(point)?;
    if resize {
        return Some(Cells {
            w: (col + 1).saturating_sub(from.col).max(1),
            h: (row + 1).saturating_sub(from.row).max(1),
            ..from
        });
    }
    let (start_col, start_row) = geometry.cell_at(start)?;
    let col = (i32::from(from.col) + i32::from(col) - i32::from(start_col)).max(0) as u8;
    let row = (i32::from(from.row) + i32::from(row) - i32::from(start_row)).max(0) as u8;
    Some(Cells { col, row, ..from })
}

/// The first free place of `w` by `h` cells among `taken`, row by row.
pub fn free_place(geometry: &Geometry, taken: &[Cells], w: u8, h: u8) -> Option<Cells> {
    for row in 0..geometry.rows {
        for col in 0..geometry.columns {
            let c = Cells { col, row, w, h };
            if c.fits(geometry.columns, geometry.rows) && !taken.iter().any(|t| t.overlaps(&c)) {
                return Some(c);
            }
        }
    }
    None
}

/// The first free place for `what` among `taken`: its full size, or where
/// that has no room, narrower, then lower (down to what it can do with).
fn room_for(env: &Env<'_>, geometry: &Geometry, taken: &[Cells], what: &What) -> Option<Cells> {
    let (w, h, least) = geometry.sizes_of(env, what);
    room_of(geometry, taken, w, h, least)
}

/// The first free place of `w` × `h` cells among `taken`, or where that
/// has no room, narrower, then lower down to `least` rows.
fn room_of(geometry: &Geometry, taken: &[Cells], w: u8, h: u8, least: u8) -> Option<Cells> {
    (least.min(h)..=h).rev().find_map(|h| {
        (1..=w)
            .rev()
            .find_map(|w| free_place(geometry, taken, w, h))
    })
}

/// Every item with its place: those placed where they are (if they fit
/// the grid and nothing before took the cells); those whose place lies
/// outside the grid (fewer columns or rows than when they were placed)
/// as near it as the grid allows, narrower or lower where they must be,
/// else in the first free cells; the others in the first free cells.
/// Items without any room are left out.
pub fn place(env: &Env<'_>, geometry: &Geometry, items: &[Item]) -> Vec<(Item, Cells)> {
    places(env, geometry, items)
        .into_iter()
        .map(|(index, c)| (items[index].clone(), c))
        .collect()
}

/// `items` fitted to `geometry` (see [`place`]): each placed where it
/// shows, and those without room, as they were. A screen keeps the first
/// and moves the others on (to its next page) once its grid changed, so
/// that nothing is lost or hidden.
pub fn fit(env: &Env<'_>, geometry: &Geometry, items: &[Item]) -> (Vec<Item>, Vec<Item>) {
    let placed = places(env, geometry, items);
    let left = items
        .iter()
        .enumerate()
        .filter(|(index, _)| !placed.iter().any(|(p, _)| p == index))
        .map(|(_, item)| item.clone())
        .collect();
    let placed = placed
        .into_iter()
        .map(|(index, cells)| Item::placed(items[index].what.clone(), cells))
        .collect();
    (placed, left)
}

/// [`place`], each item by its index in `items` (equal items apart).
fn places(env: &Env<'_>, geometry: &Geometry, items: &[Item]) -> Vec<(usize, Cells)> {
    let mut placed: Vec<(usize, Cells)> = Vec::new();
    let fits = |c: &Cells, placed: &[(usize, Cells)]| {
        c.fits(geometry.columns, geometry.rows) && !placed.iter().any(|(_, p)| p.overlaps(c))
    };
    // Those with a place first, so the others do not take it.
    for (index, item) in items.iter().enumerate() {
        if let Some(c) = item.at
            && fits(&c, &placed)
        {
            placed.push((index, c));
        }
    }
    // Those whose place the grid no longer holds (it has fewer columns or
    // rows now): moved inside it as little as can be, narrower or lower
    // where they are wider or higher than it, else where there is room.
    for (index, item) in items.iter().enumerate() {
        let Some(c) = item.at.filter(|c| !c.fits(geometry.columns, geometry.rows)) else {
            continue;
        };
        let (w, h) = (
            c.w.min(geometry.columns).max(1),
            c.h.min(geometry.rows.max(1)).max(1),
        );
        let near = Cells {
            col: c.col.min(geometry.columns.saturating_sub(w)),
            row: c.row.min(geometry.rows.saturating_sub(h)),
            w,
            h,
        };
        let taken: Vec<Cells> = placed.iter().map(|(_, c)| *c).collect();
        let found = if fits(&near, &placed) {
            Some(near)
        } else {
            let (_, _, least) = geometry.sizes_of(env, &item.what);
            room_of(geometry, &taken, w, h, least)
        };
        if let Some(c) = found {
            placed.push((index, c));
        }
    }
    for (index, item) in items.iter().enumerate().filter(|(_, i)| i.at.is_none()) {
        let taken: Vec<Cells> = placed.iter().map(|(_, c)| *c).collect();
        if let Some(c) = room_for(env, geometry, &taken, &item.what) {
            placed.push((index, c));
        }
    }
    placed
}

/// `items` with `what` added in the first free cells (a widget as wide as
/// the grid and as high as it needs, a tool one cell), every item with
/// its place as shown; `None` where there is no room.
pub fn add(env: &Env<'_>, geometry: &Geometry, items: &[Item], what: What) -> Option<Vec<Item>> {
    let placed = place(env, geometry, items);
    let taken: Vec<Cells> = placed.iter().map(|(_, c)| *c).collect();
    let at = room_for(env, geometry, &taken, &what)?;
    let mut out: Vec<Item> = placed
        .into_iter()
        .map(|(item, cells)| Item::placed(item.what, cells))
        .collect();
    out.push(Item::placed(what, at));
    Some(out)
}

/// Draws a tool: its action's icon, and below it the action's name (its
/// translation `<title>.short` where there is one) if `label`. Icon and
/// name are placed by their ink, as one group in the middle of `area`,
/// half a gap apart: the name sits clearly nearer its own icon than the
/// tool below it, which the room the cells leave keeps apart.
pub fn draw_tool(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect, action: &Id, label: bool) {
    draw_tool_lines(canvas, env, area, action, label, 0);
}

/// The focus of a cell of the home screen's grid at `area`: round the
/// icon and name of a tool there (`tool_ink`, see [`draw_tool_focus`]),
/// else round the cell, as the style's corners, a little rounder outside.
pub(crate) fn draw_cell_focus(
    canvas: &mut Canvas<'_>,
    style: &UiStyle,
    area: Rect,
    tool_ink: Option<Rect>,
) {
    if let Some(ink) = tool_ink {
        return draw_tool_focus(canvas, style, ink);
    }
    let out = style.line * 2;
    let frame = area.grow(out as u16);
    canvas.stroke_round(frame, style.line * 3, style.corner + out, Color::BLACK);
}

/// The focus of a tool whose icon and name cover `ink` (as
/// [`draw_tool_lines`] returns it): a frame round them, as far from
/// their ink on every side, never through it.
pub fn draw_tool_focus(canvas: &mut Canvas<'_>, style: &UiStyle, ink: Rect) {
    let thick = style.line * 3;
    let frame = ink.grow((style.gap / 2 + thick) as u16);
    let radius = style.corner + style.line * 2;
    canvas.stroke_round(frame, thick, radius, Color::BLACK);
}

/// How many lines the name of the tool of `action` takes in `area` (see
/// [`draw_tool`]): one, two where it does not fit on one and the tool is
/// high enough for its icon and two lines; none without `label`.
pub fn tool_name_lines(env: &Env<'_>, area: Rect, action: &Id, label: bool) -> usize {
    let style = UiStyle::new(env);
    match style.font(env) {
        Some(font) if label => {
            let state = env.menus().state(env, action.as_str());
            tool_name(env, &style, &font, area, action, &state).2.len()
        }
        _ => 0,
    }
}

/// [`draw_tool`] with room kept for a name of `lines` lines (where more
/// than its own): tools side by side given the most any of them takes
/// ([`tool_name_lines`]) keep their icons and names on one line. Returns
/// where its icon and name have ink, for its focus
/// ([`draw_tool_focus`]).
pub fn draw_tool_lines(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    area: Rect,
    action: &Id,
    label: bool,
    lines: usize,
) -> Rect {
    let style = UiStyle::new(env);
    let Some(font) = style.font(env) else {
        return area;
    };
    let icon = env.menus().icon(action.as_str()).and_then(|name| {
        let font = env.fonts().bound(crate::builtin::fonts::ICONS.as_str())?;
        let ch = crate::icons::char(name)?;
        font.has_char(ch).then_some((font, ch))
    });
    // As its plugin shows it now: a label, a badge, faint, a switch on.
    let state = env.menus().state(env, action.as_str());
    let on = state.checked == Some(true);
    let ink = match (on, state.enabled) {
        (true, _) => Color::WHITE,
        (false, true) => Color::BLACK,
        (false, false) => style.faint(crate::ui::Ink::NOTE),
    };
    if on {
        let back = if state.enabled {
            Color::BLACK
        } else {
            style.faint(crate::ui::Ink::NOTE)
        };
        canvas.fill_round(area, style.corner, back);
    }
    let name = label.then(|| tool_name(env, &style, &font, area, action, &state));
    let inner = (style.gap / 2) as i32;
    // The name's capitals above its baseline, its descenders below
    // (by the ink of letters that reach as far, so that names side by
    // side line up).
    let (cap, descent) = match &name {
        Some(_) => (
            font.ink(style.small, "H")
                .map_or((style.small * 0.7) as i32, |(top, _)| -top),
            font.ink(style.small, "g")
                .map_or((style.small * 0.25) as i32, |(_, bottom)| bottom),
        ),
        None => (0, 0),
    };
    // A second line a line lower; as much room where a tool beside it
    // has one.
    let more = name.as_ref().map_or(0, |(size, _, own)| {
        own.len().max(lines).saturating_sub(1) as i32
            * i32::from(crate::widgets::line_height(env, *size))
    });
    let text_room = if name.is_some() {
        inner + cap + descent + more
    } else {
        0
    };
    let room = (i32::from(area.height) - text_room).max(1) as f32;
    let (size, top, bottom) = icon_size(&style, icon.as_ref(), room);
    let group = (bottom - top) + text_room;
    let y = i32::from(area.y) + (i32::from(area.height) - group).div_euclid(2);
    let middle = f32::from(area.x) + f32::from(area.width) / 2.0;
    let mut covered: Option<Rect> = None;
    if let Some((icon_font, ch)) = &icon {
        let mut text = [0u8; 4];
        let ch = ch.encode_utf8(&mut text);
        // Centred by its ink, as icons are everywhere.
        let (first, last) = icon_font
            .ink_across(size, ch)
            .unwrap_or((0, icon_font.measure(size, ch) as i32));
        let x = middle - (first + last) as f32 / 2.0;
        icon_font.draw(canvas, size, x, y - top, ch, ink);
        let left = (x as i32 + first).max(0) as u16;
        covered = Some(Rect::new(
            left,
            y.max(0) as u16,
            (last - first).max(0) as u16,
            (bottom - top).max(0) as u16,
        ));
    }
    let Some((size, name, lines)) = name else {
        return covered.unwrap_or(area);
    };
    let baseline = y + (bottom - top) + inner + cap;
    let step = i32::from(crate::widgets::line_height(env, size));
    let widest = lines
        .iter()
        .map(|line| font.measure(size, line))
        .fold(0.0, f32::max);
    let at = Rect::new(
        (f32::from(area.x) + (f32::from(area.width) - widest) / 2.0).max(0.0) as u16,
        (baseline - cap).max(0) as u16,
        widest as u16,
        (cap + descent + more) as u16,
    );
    canvas.describe(crate::semantics::Role::Text, &name, at);
    // Each line centred by its ink, as the icon above it.
    for (n, line) in lines.iter().enumerate() {
        let (first, last) = font
            .ink_across(size, line)
            .unwrap_or((0, font.measure(size, line) as i32));
        let x = middle - (first + last) as f32 / 2.0;
        let line_y = baseline + n as i32 * step;
        font.draw(canvas, size, x, line_y, line, ink);
        let ink = Rect::new(
            (x as i32 + first).max(0) as u16,
            (line_y - cap).max(0) as u16,
            (last - first).max(0) as u16,
            (cap + descent).max(0) as u16,
        );
        covered = Some(covered.map_or(ink, |c| c.union(&ink)));
    }
    covered.unwrap_or(area)
}

/// The name of the tool of `action` in `area`: its size, the whole name
/// and the lines it is drawn on. A little smaller where it does not fit
/// whole, then on two lines where the tool is high enough for them and
/// its icon, then shortened.
fn tool_name(
    env: &Env<'_>,
    style: &UiStyle,
    font: &crate::fonts::FontHandle<'_>,
    area: Rect,
    action: &Id,
    state: &crate::menus::ItemState,
) -> (f32, String, Vec<String>) {
    let title = env.menus().title(action.as_str()).unwrap_or_default();
    let name = env
        .i18n
        .get(&format!("{title}.short"))
        .unwrap_or(env.i18n().tr(title));
    let name = state.text(name);
    let name = name.as_str();
    let width = area.width.saturating_sub(4);
    let least = style.small * 0.85;
    let (size, line) = crate::widgets::fit_line(font, style.small, least, name, width.into());
    let two = 2 * crate::widgets::line_height(env, least);
    // Room for the icon too, at the least it is drawn (half its largest).
    let icon = (style.title * 0.8) as u16;
    if line != name && area.height >= two + icon {
        let wrapped = crate::widgets::Wrapped::fitting(env, name, style.small, least, width, two);
        if wrapped.whole() {
            return (wrapped.size(), String::from(name), wrapped.lines().to_vec());
        }
        // A word too long for a line: split where its language lets it be.
        for size in [style.small, least] {
            if let Some(lines) = hyphenated(env, font, size, name, f32::from(width)) {
                return (size, String::from(name), lines);
            }
        }
    }
    (size, String::from(name), alloc::vec![line])
}

/// `name` on two lines `width` wide in `size`, split where a word of it
/// may be: after a hyphen it holds, else where the hyphenation of the
/// interface's language lets it (a hyphen added); the split that leaves
/// the lines most even. `None` where no split fits.
fn hyphenated(
    env: &Env<'_>,
    font: &crate::fonts::FontHandle<'_>,
    size: f32,
    name: &str,
    width: f32,
) -> Option<Vec<String>> {
    let mut points: Vec<(usize, bool)> = name
        .char_indices()
        .filter(|(_, c)| *c == '-')
        .map(|(at, _)| (at + 1, false))
        .collect();
    if let Some(patterns) = env.hyphenation().get(env.i18n().language()) {
        let mut start = None;
        for (at, c) in name
            .char_indices()
            .chain(core::iter::once((name.len(), ' ')))
        {
            match (c.is_alphabetic(), start) {
                (true, None) => start = Some(at),
                (false, Some(from)) => {
                    patterns.hyphenate(&name[from..at], &mut |point| {
                        points.push((from + point, true))
                    });
                    start = None;
                }
                _ => {}
            }
        }
    }
    points
        .into_iter()
        .filter_map(|(at, mark)| {
            let first = alloc::format!("{}{}", name[..at].trim_end(), if mark { "-" } else { "" });
            let rest = name[at..].trim_start();
            let (a, b) = (font.measure(size, &first), font.measure(size, rest));
            (a <= width && b <= width && !rest.is_empty())
                .then(|| ((a - b).abs(), alloc::vec![first, String::from(rest)]))
        })
        .min_by(|x, y| x.0.total_cmp(&y.0))
        .map(|(_, lines)| lines)
}

/// The size of a tool's icon in `room` of height, and its frame's ink:
/// every icon in the same frame, the ink of a circle (which fills the
/// icons' grid), so that the names of tools side by side line up.
fn icon_size(
    style: &UiStyle,
    icon: Option<&(crate::fonts::FontHandle<'_>, char)>,
    room: f32,
) -> (f32, i32, i32) {
    let frame = |size: f32| {
        let (font, ch) = icon?;
        let circle = crate::icons::char("circle")
            .filter(|c| font.has_char(*c))
            .unwrap_or(*ch);
        font.ink(size, circle.encode_utf8(&mut [0u8; 4]))
    };
    let largest = (style.title * 1.6).min(room);
    let size = crate::widgets::largest_size(largest, largest * 0.5, |s| {
        frame(s).is_none_or(|(top, bottom)| (bottom - top) as f32 <= room)
    })
    .unwrap_or(largest * 0.5);
    let (top, bottom) = frame(size).unwrap_or((-(size as i32), 0));
    (size, top, bottom)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> Geometry {
        Geometry::of(Rect::new(0, 0, 400, 300), 4, 3, 10)
    }

    #[test]
    fn layouts_are_read_and_written() {
        let text = "home.clock 0/0 4x1, tool:core.library 1/5 1x1, stats.today";
        let items = parse(text);
        assert_eq!(items.len(), 3);
        assert_eq!(items[1].what, What::Tool(Id::new_static("core.library")));
        assert_eq!(items[0].at, Some(Cells::new(0, 0, 4, 1)));
        assert_eq!(items[2].at, None);
        assert_eq!(write(&items), text);
        // Layouts written without places: places to come.
        assert!(
            parse("home.status, home.date")
                .iter()
                .all(|i| i.at.is_none())
        );
        // What cannot be read is left out.
        assert_eq!(parse("tool:Not An Id, , home.date").len(), 1);
    }

    #[test]
    fn cells_overlap_and_fit() {
        let a = Cells::new(0, 0, 2, 2);
        assert!(a.overlaps(&Cells::new(1, 1, 1, 1)));
        assert!(!a.overlaps(&Cells::new(2, 0, 2, 1)));
        assert!(a.fits(4, 6));
        assert!(!Cells::new(3, 0, 2, 1).fits(4, 6));
        assert!(!Cells::new(250, 0, 10, 1).fits(4, 6));
    }

    #[test]
    fn widgets_shrink_to_the_room_left_down_to_their_least() {
        let g = grid();
        // One row of three left: a widget of three rows that can do with
        // one gets it; one that needs two does not.
        let taken = [Cells::new(0, 0, 4, 2)];
        assert_eq!(room_of(&g, &taken, 4, 3, 1), Some(Cells::new(0, 2, 4, 1)));
        assert_eq!(room_of(&g, &taken, 4, 3, 2), None);
        // With room, its full height.
        assert_eq!(room_of(&g, &[], 4, 2, 1), Some(Cells::new(0, 0, 4, 2)));
    }

    #[test]
    fn free_places_are_found_row_by_row() {
        let g = grid();
        let taken = [Cells::new(0, 0, 4, 1), Cells::new(0, 1, 1, 1)];
        assert_eq!(free_place(&g, &taken, 1, 1), Some(Cells::new(1, 1, 1, 1)));
        assert_eq!(free_place(&g, &taken, 4, 1), Some(Cells::new(0, 2, 4, 1)));
        assert_eq!(free_place(&g, &taken, 4, 2), None);
        assert_eq!(g.rect(Cells::new(1, 1, 2, 1)), Rect::new(105, 105, 190, 90));
        assert_eq!(g.cell_at(Point::new(250, 150)), Some((2, 1)));
        assert_eq!(g.cell_at(Point::new(450, 150)), None);
    }

    #[test]
    fn items_are_dragged_and_sized_by_cells() {
        let g = grid();
        let item = Cells::new(1, 0, 2, 1);
        let at = |x, y| Point::new(x, y);
        // Taken in its second cell, moved a cell right and down.
        let moved = dragged(&g, item, at(250, 50), at(350, 150), false);
        assert_eq!(moved, Some(Cells::new(2, 1, 2, 1)));
        // Not left of the grid's edge.
        let left = dragged(&g, item, at(250, 50), at(50, 50), false);
        assert_eq!(left, Some(Cells { col: 0, ..item }));
        // Its corner pulled to the third row: as high as that.
        let sized = dragged(&g, item, at(290, 90), at(150, 250), true);
        assert_eq!(sized, Some(Cells { w: 1, h: 3, ..item }));
        // Off the grid: nowhere.
        assert_eq!(dragged(&g, item, at(250, 50), at(450, 50), false), None);
    }
}
