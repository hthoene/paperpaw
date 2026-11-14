//! The grid of cells a dashboard's pages (the home screen's, the picture home's) are laid out in, without the
//! host: items (a widget or a tool) with the cells they take, the text they
//! are kept as, and putting them in a grid. The same rules as the SDK's
//! interface-6 grid (`paperpaw_sdk::widgets::grid`), with fallible
//! allocation and the size of a widget given by the caller ([`Sizes`])
//! instead of read from an environment.
use crate::{
    mem::push,
    text::{concat, copy},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Id,
    native::Error,
    widgets::grid::{Cells, Geometry, free_place},
};

/// Tools are written with this before their action's id.
const TOOL: &str = "tool:";

/// What an item shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum What {
    /// A widget, by its id.
    Widget(String),
    /// A tool: an action, shown as its icon and name.
    Tool(String),
}

impl What {
    /// A copy made with fallible allocation.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn try_clone(&self) -> Result<Self, Error> {
        Ok(match self {
            Self::Widget(id) => Self::Widget(copy(id)?),
            Self::Tool(id) => Self::Tool(copy(id)?),
        })
    }

    /// How it is written in a layout.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn text(&self) -> Result<String, Error> {
        match self {
            Self::Widget(id) => copy(id),
            Self::Tool(id) => concat(&[TOOL, id]),
        }
    }
}

/// An item of a grid: what it shows and the cells it takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// What it shows.
    pub what: What,
    /// Its first column and row, and its width and height in cells; `None`
    /// until it has a place.
    pub at: Option<Cells>,
}

impl Item {
    /// An item without a place yet.
    #[must_use]
    pub fn new(what: What) -> Self {
        Self { what, at: None }
    }

    /// An item at `cells`.
    #[must_use]
    pub fn placed(what: What, cells: Cells) -> Self {
        Self {
            what,
            at: Some(cells),
        }
    }

    /// A copy made with fallible allocation.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn try_clone(&self) -> Result<Self, Error> {
        Ok(Self {
            what: self.what.try_clone()?,
            at: self.at,
        })
    }
}

/// A copy of `items` made with fallible allocation.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn copy_items(items: &[Item]) -> Result<Vec<Item>, Error> {
    let mut found = Vec::new();
    found
        .try_reserve_exact(items.len())
        .map_err(|_| Error::NoMemory)?;
    for item in items {
        found.push(item.try_clone()?);
    }
    Ok(found)
}

/// The items of a layout as [`write()`] has them; what cannot be read is left
/// out (a tool whose action is no id, an empty entry).
///
/// # Errors
/// [`Error::NoMemory`]; nothing is returned in part.
pub fn parse(text: &str) -> Result<Vec<Item>, Error> {
    let mut found = Vec::new();
    for entry in text.split(',') {
        let mut parts = entry.split_whitespace();
        let Some(id) = parts.next() else {
            continue;
        };
        let what = match id.strip_prefix(TOOL) {
            Some(action) => match Id::try_parse(action) {
                Ok(action) => What::Tool(copy(action.as_str())?),
                Err(paperpaw_sdk::IdError::NoMemory) => return Err(Error::NoMemory),
                Err(_) => continue,
            },
            None => What::Widget(copy(id)?),
        };
        let at = (|| {
            let (col, row) = parts.next()?.split_once('/')?;
            let (w, h) = parts.next()?.split_once('x')?;
            Some(Cells::new(
                col.parse().ok()?,
                row.parse().ok()?,
                w.parse().ok()?,
                h.parse().ok()?,
            ))
        })();
        push(&mut found, Item { what, at })?;
    }
    Ok(found)
}

/// The layout of `items` as text: each item's id (a tool's action after
/// `tool:`), its column and row, and its size in cells, separated by commas.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn write(items: &[Item]) -> Result<String, Error> {
    let mut out = String::new();
    let mut digits = [0u8; 20];
    for item in items {
        let one = item.what.text()?;
        out.try_reserve(one.len() + 24)
            .map_err(|_| Error::NoMemory)?;
        if !out.is_empty() {
            out.push_str(", ");
        }
        out.push_str(&one);
        if let Some(c) = item.at {
            for (sep, value) in [(" ", c.col), ("/", c.row), (" ", c.w), ("x", c.h)] {
                out.push_str(sep);
                out.push_str(crate::text::decimal(usize::from(value), &mut digits));
            }
        }
    }
    Ok(out)
}

/// How big an item is when it has no place: the width and height in cells it
/// asks for, and the fewest rows it can do with.
pub trait Sizes {
    /// The size of a widget in `geometry`. A tool is always one cell.
    fn widget(&self, geometry: &Geometry, id: &str) -> (u8, u8, u8);
}

fn sizes_of(sizes: &impl Sizes, geometry: &Geometry, what: &What) -> (u8, u8, u8) {
    match what {
        What::Tool(_) => (1, 1, 1),
        What::Widget(id) => sizes.widget(geometry, id),
    }
}

/// The first free place of `w` × `h` cells among `taken`, or where that has
/// no room, narrower, then lower down to `least` rows.
fn room_of(geometry: &Geometry, taken: &[Cells], w: u8, h: u8, least: u8) -> Option<Cells> {
    (least.min(h)..=h).rev().find_map(|h| {
        (1..=w)
            .rev()
            .find_map(|w| free_place(geometry, taken, w, h))
    })
}

fn room_for(
    sizes: &impl Sizes,
    geometry: &Geometry,
    taken: &[Cells],
    what: &What,
) -> Option<Cells> {
    let (w, h, least) = sizes_of(sizes, geometry, what);
    room_of(geometry, taken, w, h, least)
}

/// Every item with its index and place: those placed where they are (if they
/// fit the grid and nothing before took the cells); those whose place lies
/// outside the grid, or whose cells an item before took, as near it as the
/// grid allows, narrower or lower where they must be, else in the first free
/// cells; the others in the first free cells. Items without any room are left
/// out.
fn places(
    sizes: &impl Sizes,
    geometry: &Geometry,
    items: &[Item],
) -> Result<Vec<(usize, Cells)>, Error> {
    let mut placed: Vec<(usize, Cells)> = Vec::new();
    placed
        .try_reserve(items.len())
        .map_err(|_| Error::NoMemory)?;
    let fits = |c: &Cells, placed: &[(usize, Cells)]| {
        c.fits(geometry.columns, geometry.rows) && !placed.iter().any(|(_, p)| p.overlaps(c))
    };
    let taken_of = |placed: &[(usize, Cells)]| -> Result<Vec<Cells>, Error> {
        let mut taken = Vec::new();
        taken
            .try_reserve_exact(placed.len())
            .map_err(|_| Error::NoMemory)?;
        taken.extend(placed.iter().map(|(_, c)| *c));
        Ok(taken)
    };
    // Those with a place first, so the others do not take it.
    for (index, item) in items.iter().enumerate() {
        if let Some(c) = item.at
            && fits(&c, &placed)
        {
            placed.push((index, c));
        }
    }
    // Those whose place the grid no longer holds, or an item before took:
    // moved inside it as little as can be, narrower or lower where they are
    // wider or higher than it.
    for (index, item) in items.iter().enumerate() {
        let Some(c) = item
            .at
            .filter(|_| !placed.iter().any(|(placed, _)| *placed == index))
        else {
            continue;
        };
        let (w, h) = (
            c.w.min(geometry.columns).max(1),
            c.h.min(geometry.rows.max(1)).max(1),
        );
        let near = Cells::new(
            c.col.min(geometry.columns.saturating_sub(w)),
            c.row.min(geometry.rows.saturating_sub(h)),
            w,
            h,
        );
        let found = if fits(&near, &placed) {
            Some(near)
        } else {
            let taken = taken_of(&placed)?;
            let (_, _, least) = sizes_of(sizes, geometry, &item.what);
            room_of(geometry, &taken, w, h, least)
        };
        if let Some(c) = found {
            placed.push((index, c));
        }
    }
    for (index, item) in items.iter().enumerate().filter(|(_, i)| i.at.is_none()) {
        let taken = taken_of(&placed)?;
        if let Some(c) = room_for(sizes, geometry, &taken, &item.what) {
            placed.push((index, c));
        }
    }
    Ok(placed)
}

/// `items` fitted to `geometry`: each placed where it shows, and those
/// without room, as they were. A screen keeps the first and moves the others
/// on (to its next page) once its grid changed, so that nothing is lost or
/// hidden.
///
/// # Errors
/// [`Error::NoMemory`]; nothing is returned in part.
pub fn fit(
    sizes: &impl Sizes,
    geometry: &Geometry,
    items: &[Item],
) -> Result<(Vec<Item>, Vec<Item>), Error> {
    let placed = places(sizes, geometry, items)?;
    let mut shown = Vec::new();
    let mut left = Vec::new();
    for (index, item) in items.iter().enumerate() {
        match placed.iter().find(|(p, _)| *p == index) {
            Some((_, cells)) => push(&mut shown, Item::placed(item.what.try_clone()?, *cells))?,
            None => push(&mut left, item.try_clone()?)?,
        }
    }
    Ok((shown, left))
}

/// `items` laid out anew in `geometry` for a grid made from one of
/// `from_columns` columns (a landscape home from the portrait one) rather
/// than [`fit`], which keeps each item where it was. The items are placed in
/// the order given: what is to find room first goes first. A widget keeps its
/// share of the width (at least one column, at most `widest` of the new
/// grid), a tool is one cell, an item without a place gets the size
/// [`Sizes`] says. Heights are kept, and no more than the grid has. Where a
/// size has no room, it is narrower, then lower. Returns the items placed and
/// second those with no room, unchanged, in the order given.
///
/// # Errors
/// [`Error::NoMemory`]; nothing is returned in part.
pub fn refit(
    sizes: &impl Sizes,
    geometry: &Geometry,
    from_columns: u8,
    widest: u8,
    items: &[Item],
) -> Result<(Vec<Item>, Vec<Item>), Error> {
    let (to, from) = (
        u32::from(geometry.columns).max(1),
        u32::from(from_columns.max(1)),
    );
    let mut taken: Vec<Cells> = Vec::new();
    taken
        .try_reserve_exact(items.len())
        .map_err(|_| Error::NoMemory)?;
    let (mut placed, mut left) = (Vec::new(), Vec::new());
    for item in items {
        let found = match item.at {
            Some(c) => {
                let w = match item.what {
                    What::Widget(_) => {
                        ((u32::from(c.w) * to + from / 2) / from).clamp(1, u32::from(widest).max(1))
                    }
                    What::Tool(_) => u32::from(c.w),
                };
                let (w, h) = (w.clamp(1, to) as u8, c.h.clamp(1, geometry.rows.max(1)));
                free_place(geometry, &taken, w, h).or_else(|| {
                    let (_, _, least) = sizes_of(sizes, geometry, &item.what);
                    room_of(geometry, &taken, w, h, least)
                })
            }
            None => {
                let (w, h, least) = sizes_of(sizes, geometry, &item.what);
                let w = match item.what {
                    What::Widget(_) => w.min(widest.max(1)),
                    What::Tool(_) => w,
                };
                room_of(geometry, &taken, w, h, least)
            }
        };
        match found {
            Some(cells) => {
                taken.push(cells);
                push(&mut placed, Item::placed(item.what.try_clone()?, cells))?;
            }
            None => push(&mut left, item.try_clone()?)?,
        }
    }
    Ok((placed, left))
}

/// `items` with `what` added in the first free cells (a widget as wide as the
/// grid and as high as it needs, a tool one cell), every item with its place
/// as shown; `None` where there is no room.
///
/// # Errors
/// [`Error::NoMemory`]; nothing is returned in part.
pub fn add(
    sizes: &impl Sizes,
    geometry: &Geometry,
    items: &[Item],
    what: What,
) -> Result<Option<Vec<Item>>, Error> {
    let placed = places(sizes, geometry, items)?;
    let mut taken: Vec<Cells> = Vec::new();
    taken
        .try_reserve_exact(placed.len() + 1)
        .map_err(|_| Error::NoMemory)?;
    taken.extend(placed.iter().map(|(_, c)| *c));
    let Some(at) = room_for(sizes, geometry, &taken, &what) else {
        return Ok(None);
    };
    let mut out = Vec::new();
    out.try_reserve_exact(placed.len() + 1)
        .map_err(|_| Error::NoMemory)?;
    for (index, cells) in &placed {
        out.push(Item::placed(items[*index].what.try_clone()?, *cells));
    }
    out.push(Item::placed(what, at));
    Ok(Some(out))
}

#[cfg(test)]
mod tests;
