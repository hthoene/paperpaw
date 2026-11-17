//! Widgets in a grid, like a phone's home screen: an area cut into cells
//! (columns and rows), each item taking a rectangle of them: a widget (any
//! plugin's) or a tool (an action, shown as its icon and name). Cells left
//! empty stay empty, so everything sits where it was put.
//!
//! The layout is data: a text such as
//! `home.clock 0/0 4x1, tool:files.library 0/5` ([`parse`], [`write()`]),
//! each item its id, its column and row, and its size in cells, kept where
//! the plugin likes (a setting, a file). An item without a place (as
//! plugins add them) goes into the first free cells (`place`), as wide as
//! the grid and as high as it needs.
//!
//! `GridView` shows such a grid on a screen of the plugin's own, and edits
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
//! impl Layer for Board {
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

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::ui::UiStyle;
use crate::{ Id, Point, Rect};

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
}

/// An item of a grid: what it shows and the cells it takes.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Item {
    pub what: What,
    /// Its first column and row, and its width and height in cells;
    /// `None` until it has a place (see `place`).
    pub at: Option<Cells>,
}

impl Item {
    /// An item at `cells`.
    pub fn placed(what: What, cells: Cells) -> Self {
        Self {
            what,
            at: Some(cells),
        }
    }

    /// An item without a place yet.
    pub fn new(what: What) -> Self {
        Self { what, at: None }
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

/// The columns a grid made for `portrait_columns` on a screen higher than
/// wide has in `area`: those, where `area` is not wider than high; else
/// as many as keep a cell about as wide as it was (the columns times the
/// area's width over its height, rounded), at most [`MAX_COLUMNS`] and
/// never fewer than `portrait_columns`. A phone's home of 4 columns in
/// landscape: 6.
pub fn columns_for(area: Rect, portrait_columns: u8) -> u8 {
    let portrait = portrait_columns.clamp(1, MAX_COLUMNS);
    if area.width <= area.height {
        return portrait;
    }
    let (w, h) = (u32::from(area.width), u32::from(area.height.max(1)));
    let wide = (u32::from(portrait) * w + h / 2) / h;
    wide.clamp(u32::from(portrait), u32::from(MAX_COLUMNS)) as u8
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
