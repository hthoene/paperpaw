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
    native::Error,
    widgets::grid::{Cells, Geometry},
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
