//! Arranging the items of a page while editing, without the host: which button
//! moves or sizes an item, where an item may go, and what removing one leaves.
//! The same rules as the interface-6 grid view (`GridView::move_item`,
//! `free`, `remove`), with fallible allocation. The dashboard holds the items
//! as laid out and writes them with `native_pages::store` once an item is set
//! down.
//!
//! A widget is kept at least as high as it can do with (`Sizes`: what it declared
//! as its least rows, else what it measured at the grid's width), and one cell
//! wide at the least, as in interface 6, whose resizing did not read the hint's
//! least width or square either (only the size an item gets when added or placed
//! does).
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Point,
    native::Error,
    widgets::{
        CellSpan,
        grid::{self, Cells, Geometry},
    },
};
use paperpaw_ui_kit::{mem::push, text::copy};

use crate::native_grid::{Item, What};

/// What the buttons do to the item chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Left and right go sideways, up and down a row.
    Move,
    /// Left and right make it narrower and wider, up and down lower and taller.
    Resize,
}

/// A button press while an item is moved or sized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

impl Dir {
    /// The direction an action stands for: the page buttons are up and down
    /// where there are no such buttons (the X4's side buttons).
    #[must_use]
    pub fn of(action: &str) -> Option<Self> {
        Some(match action {
            "core.left" | "core.previous" => Self::Left,
            "core.right" | "core.next" => Self::Right,
            "core.up" | "core.prev-page" => Self::Up,
            "core.down" | "core.next-page" => Self::Down,
            _ => return None,
        })
    }
}

/// The cells an item has after one step: moved a cell, or made a cell wider,
/// narrower, taller or lower (never under one cell, nor lower than `least`
/// rows).
#[must_use]
pub fn stepped(mode: Mode, at: Cells, dir: Dir, least: u8) -> Cells {
    let more = |n: u8| n.saturating_add(1);
    let less = |n: u8| n.saturating_sub(1);
    let (col, row, w, h) = (at.col, at.row, at.w, at.h);
    match (mode, dir) {
        (Mode::Move, Dir::Left) => Cells::new(less(col), row, w, h),
        (Mode::Move, Dir::Right) => Cells::new(more(col), row, w, h),
        (Mode::Move, Dir::Up) => Cells::new(col, less(row), w, h),
        (Mode::Move, Dir::Down) => Cells::new(col, more(row), w, h),
        (Mode::Resize, Dir::Left) => Cells::new(col, row, less(w).max(1), h),
        (Mode::Resize, Dir::Right) => Cells::new(col, row, more(w), h),
        (Mode::Resize, Dir::Up) => Cells::new(col, row, w, less(h).max(least.max(1))),
        (Mode::Resize, Dir::Down) => Cells::new(col, row, w, more(h)),
    }
}

/// What came of putting an item at new cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Put {
    /// It is there.
    Done,
    /// Not changed (a step that came to the same cells, or past the grid's edge).
    Same,
    /// Within the grid, but another item is in the way: said, so the press does
    /// not seem lost.
    InTheWay,
}

/// Puts item `index` at `to` where the grid has room.
pub fn put(items: &mut [Item], index: usize, to: Cells, geometry: &Geometry) -> Put {
    let Some(at) = items.get(index).and_then(|item| item.at) else {
        return Put::Same;
    };
    if to == at || !to.fits(geometry.columns, geometry.rows) {
        return Put::Same;
    }
    let blocked = items
        .iter()
        .enumerate()
        .any(|(i, item)| i != index && item.at.is_some_and(|other| other.overlaps(&to)));
    if blocked {
        return Put::InTheWay;
    }
    items[index].at = Some(to);
    Put::Done
}

/// Whether item `index` may take the cells `to`: inside the grid and clear of
/// every other item.
#[must_use]
pub fn free(items: &[Item], index: usize, to: Cells, geometry: &Geometry) -> bool {
    to.fits(geometry.columns, geometry.rows)
        && !items
            .iter()
            .enumerate()
            .any(|(i, item)| i != index && item.at.is_some_and(|other| other.overlaps(&to)))
}

/// The cells an item at `from` takes when a finger that came down at `start` is
/// at `point`: moved by as many cells as the finger moved, or with `resize` (its
/// corner taken) its corner in the cell under the finger. A widget is kept at
/// least as high as it can do with (`least` rows) and as its declared cells say
/// (`span`: its least size, square). `None` off the grid.
#[must_use]
pub fn dragged(
    geometry: &Geometry,
    from: Cells,
    (start, point): (Point, Point),
    resize: bool,
    (least, span): (u8, Option<CellSpan>),
) -> Option<Cells> {
    let to = grid::dragged(geometry, from, start, point, resize)?;
    if !resize {
        return Some(to);
    }
    let kept = match span {
        Some(span) => span.keep(to, to.w != from.w),
        None => to,
    };
    let h = kept.h.max(least.max(1)).min(geometry.rows.max(1));
    Some(Cells::new(kept.col, kept.row, kept.w, h))
}

/// `items` without item `index`, and the placement whose values go with it:
/// a widget's own placement (`id@n`) is reset when it is removed.
///
/// # Errors
/// [`Error::NoMemory`]; nothing is returned in part.
pub fn removed(items: &[Item], index: usize) -> Result<(Vec<Item>, Option<String>), Error> {
    let mut rest = Vec::new();
    rest.try_reserve_exact(items.len().saturating_sub(1))
        .map_err(|_| Error::NoMemory)?;
    let mut reset = None;
    for (i, item) in items.iter().enumerate() {
        if i == index {
            if let What::Widget(id) = &item.what
                && id.contains('@')
            {
                reset = Some(copy(id)?);
            }
        } else {
            push(&mut rest, item.try_clone()?)?;
        }
    }
    Ok((rest, reset))
}

/// What the options of an item offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Move,
    Resize,
    /// The widget's own settings (a placement of its own), where it has some.
    Settings,
    Remove,
    Cancel,
}

/// The options in the order they are offered, each with the catalog key of its
/// word (the core's: they are the grid's own words).
pub const CHOICES: [(Choice, &str); 4] = [
    (Choice::Move, "core.grid.move"),
    (Choice::Resize, "core.grid.resize"),
    (Choice::Remove, "core.grid.remove"),
    (Choice::Cancel, "home.edit.cancel"),
];

/// The options of a widget that has settings of its own: *Settings* between
/// *Change the size* and *Remove*, as interface 6 offered them.
pub const CHOICES_WITH_SETTINGS: [(Choice, &str); 5] = [
    (Choice::Move, "core.grid.move"),
    (Choice::Resize, "core.grid.resize"),
    (Choice::Settings, "core.grid.settings"),
    (Choice::Remove, "core.grid.remove"),
    (Choice::Cancel, "home.edit.cancel"),
];

/// The options an item offers: with *Settings* where it is a widget that has
/// settings.
#[must_use]
pub const fn choices(settings: bool) -> &'static [(Choice, &'static str)] {
    if settings {
        &CHOICES_WITH_SETTINGS
    } else {
        &CHOICES
    }
}

/// The help said when moving or sizing starts.
#[must_use]
pub const fn help(mode: Mode) -> &'static str {
    match mode {
        Mode::Move => "core.grid.move.help",
        Mode::Resize => "core.grid.resize.help",
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests;
