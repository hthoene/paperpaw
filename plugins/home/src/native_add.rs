//! Adding to a page of the home screen, without the host: what there is to
//! add (the widgets not hidden here, the tools not on the page yet), the kinds
//! they are shown under, the rows of the two steps (the kinds, then the things
//! of one kind) and what a choice writes. The same rules as the interface-6
//! `add` screen, with fallible allocation; what is read comes from a
//! [`Source`] and the pages, what is written is returned as [`Write`]s.
//!
//! Not here (ignored acceptance cases, `PORTING-7.md`): the gallery of
//! widgets drawn as they would show, and the ready-made layouts.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    native::Error,
    widgets::{categories, grid::Geometry},
};
use paperpaw_ui_kit::{mem::push, text::copy};

use crate::native_grid::{self as grid, Item, What};
use crate::native_pages::{
    self as pages, FILL, MAX_PAGES, Page, SPACE, STATUS, Source, TOOLS, Write,
};

/// The kind standing for the tools.
pub const TOOLS_KIND: &str = "home.edit.tools";

/// A widget the registry lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetRow {
    /// Its id.
    pub id: String,
    /// Its name, as the reader reads it.
    pub title: String,
    /// Its kind (category).
    pub kind: String,
}

/// One thing that can be added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The kind it is shown under (the tools': [`TOOLS_KIND`]).
    pub kind: String,
    /// What is added.
    pub what: What,
    /// Its name, as the reader reads it.
    pub title: String,
}

/// A row of the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// A kind, and how many things there are of it.
    Kind(String, usize),
    /// The thing of this number among the entries.
    Add(usize),
    /// A page added after the others.
    AddPage,
    /// This page removed.
    RemovePage,
    /// The layout for a screen on its side emptied.
    ResetWide,
}

/// What a choice comes to.
#[derive(Debug, PartialEq, Eq)]
pub enum Plan {
    /// Write these, then go back to the page.
    Writes(Vec<Write>),
    /// There is no room on the page: nothing is written, the screen stays.
    NoRoom,
    /// Nothing to do.
    Nothing,
}

/// Every widget, and every tool not on `shown` (the page as it shows), with
/// its kind and name. A widget may be added more than once (each with
/// settings of its own); empty cells and the tools block are the grid's own,
/// and the status line is above it where shown.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn entries(
    widgets: &[WidgetRow],
    tools: &[(String, String)],
    status_bar: bool,
    shown: &[Item],
) -> Result<Vec<Entry>, Error> {
    let mut found = Vec::new();
    for widget in widgets {
        let id = widget.id.as_str();
        let own = id == SPACE || id == FILL || id == TOOLS || (id == STATUS && status_bar);
        if !own {
            push(
                &mut found,
                Entry {
                    kind: copy(&widget.kind)?,
                    what: What::Widget(copy(id)?),
                    title: copy(&widget.title)?,
                },
            )?;
        }
    }
    for (action, title) in tools {
        let what = What::Tool(copy(action)?);
        if !shown.iter().any(|item| item.what == what) {
            push(
                &mut found,
                Entry {
                    kind: copy(TOOLS_KIND)?,
                    what,
                    title: copy(title)?,
                },
            )?;
        }
    }
    Ok(found)
}

/// Where a kind comes in the screen: the kernel's kinds in the order of
/// [`categories::ALL`], then a plugin's own, the tools last.
fn rank(kind: &str) -> usize {
    if kind == TOOLS_KIND {
        return categories::ALL.len() + 1;
    }
    categories::ALL
        .iter()
        .position(|known| *known == kind)
        .unwrap_or(categories::ALL.len())
}

/// The kinds that have something to add and how many things each has, in the
/// order screens show kinds on interface 6 (the kernel's, then plugins' own in
/// the order found, the tools last).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn kinds(entries: &[Entry]) -> Result<Vec<(String, usize)>, Error> {
    let mut found: Vec<(String, usize)> = Vec::new();
    for entry in entries {
        match found.iter_mut().find(|(kind, _)| *kind == entry.kind) {
            Some((_, count)) => *count += 1,
            None => push(&mut found, (copy(&entry.kind)?, 1))?,
        }
    }
    // A stable order without sorting in place (a sort may allocate): by rank,
    // ties in the order found.
    let mut ordered = Vec::new();
    ordered
        .try_reserve_exact(found.len())
        .map_err(|_| Error::NoMemory)?;
    for wanted in 0..=categories::ALL.len() + 1 {
        for (kind, count) in found.iter().filter(|(kind, _)| rank(kind) == wanted) {
            ordered.push((copy(kind)?, *count));
        }
    }
    Ok(ordered)
}

/// The rows of the screen: with no `kind` chosen the kinds, *Add a page* (where
/// there is room for one), *Remove this page* (where there is more than one)
/// and *Reset landscape layout* (where one is kept); with a kind its things.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn rows(
    kind: Option<&str>,
    entries: &[Entry],
    pages: usize,
    wide_kept: bool,
) -> Result<Vec<Row>, Error> {
    let mut rows = Vec::new();
    match kind {
        Some(kind) => {
            for (index, entry) in entries.iter().enumerate() {
                if entry.kind == kind {
                    push(&mut rows, Row::Add(index))?;
                }
            }
        }
        None => {
            for (kind, count) in kinds(entries)? {
                push(&mut rows, Row::Kind(kind, count))?;
            }
            if pages < MAX_PAGES {
                push(&mut rows, Row::AddPage)?;
            }
            if pages > 1 {
                push(&mut rows, Row::RemovePage)?;
            }
            if wide_kept {
                push(&mut rows, Row::ResetWide)?;
            }
        }
    }
    Ok(rows)
}

/// The layout a change is kept in: the pages shown, and the grids of the
/// screen as it is and of one on its side.
#[derive(Debug)]
pub struct Layout<'a> {
    /// Whether the screen is on its side.
    pub wide: bool,
    /// The grid it shows now.
    pub grid: &'a Geometry,
    /// The grid of a screen on its side.
    pub wide_grid: &'a Geometry,
    /// Every page shown.
    pub pages: &'a [Page],
}

/// Adds `what` to page number `page` (0 the first): in the first free cells,
/// kept in the layout shown, and in the other layout too.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn add_item(
    src: &impl Source,
    layout: &Layout<'_>,
    page: usize,
    what: What,
) -> Result<Plan, Error> {
    let Some(on) = layout.pages.get(page) else {
        return Ok(Plan::Nothing);
    };
    let shown = pages::expand(src, &on.items)?;
    let Some(added) = grid::add(src, layout.grid, &shown, what.try_clone()?)? else {
        return Ok(Plan::NoRoom);
    };
    // The layout shown is kept; for a screen on its side that is the whole
    // derived layout, from the first change on.
    let in_layout = layout.wide.then_some(layout.grid);
    let mut writes = pages::store(src, on.slot, &added, in_layout)?;
    let other = pages::add_to_other(src, layout.wide, on.slot, what, layout.wide_grid)?;
    writes
        .try_reserve(other.len())
        .map_err(|_| Error::NoMemory)?;
    writes.extend(other);
    Ok(Plan::Writes(writes))
}

/// Adds a page after the others, kept as a space until it is filled.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn add_page(src: &impl Source, layout: &Layout<'_>) -> Result<Plan, Error> {
    if layout.pages.len() >= MAX_PAGES {
        return Ok(Plan::Nothing);
    }
    let mut all = copy_pages(layout.pages, 1)?;
    all.push(Page {
        slot: layout.pages.len(),
        items: Vec::new(),
    });
    Ok(Plan::Writes(pages::store_all(src, &all, layout.wide)?))
}

/// Removes page number `page`; the last page is never removed.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn remove_page(src: &impl Source, layout: &Layout<'_>, page: usize) -> Result<Plan, Error> {
    if page >= layout.pages.len() || layout.pages.len() < 2 {
        return Ok(Plan::Nothing);
    }
    let mut all = copy_pages(layout.pages, 0)?;
    all.remove(page);
    Ok(Plan::Writes(pages::store_all(src, &all, layout.wide)?))
}

/// The widgets of page number `page` whose placements' values are reset when
/// the page is removed: those no page left (of either layout) still shows.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn removed_placements(
    src: &impl Source,
    layout: &Layout<'_>,
    page: usize,
) -> Result<Vec<String>, Error> {
    let Some(gone) = layout.pages.get(page) else {
        return Ok(Vec::new());
    };
    let other = pages::other_layer(src, layout.wide)?;
    let mut found = Vec::new();
    for item in &gone.items {
        let What::Widget(id) = &item.what else {
            continue;
        };
        let held = layout
            .pages
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != page)
            .map(|(_, p)| p)
            .chain(&other)
            .flat_map(|p| &p.items)
            .any(|i| matches!(&i.what, What::Widget(w) if w == id));
        if !held && !found.iter().any(|f| f == id) {
            push(&mut found, copy(id)?)?;
        }
    }
    Ok(found)
}

/// The most the keys and texts staged in one callback may be: the host's
/// budget for the stage (`docs/abi/commands.md`).
pub const STAGE_BUDGET: usize = 4096;

/// Whether what a choice stages (the settings written and the placements
/// reset) is within [`STAGE_BUDGET`]; where it is not the choice is not made
/// and the screen says there is no room (a full home of 8 pages that shifts
/// every page on a removal is the case).
#[must_use]
pub fn fits(writes: &[Write], resets: &[String]) -> bool {
    let sets = writes.iter().map(|write| match write {
        Write::Set { setting, text } => setting.len() + text.len(),
        Write::Notify(_) => 0,
    });
    let bytes = sets
        .chain(resets.iter().map(String::len))
        .fold(0usize, usize::saturating_add);
    bytes <= STAGE_BUDGET
}

/// Empties the layout for a screen on its side.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn reset_wide(src: &impl Source) -> Result<Plan, Error> {
    Ok(Plan::Writes(pages::reset_wide(src)?))
}

fn copy_pages(pages: &[Page], extra: usize) -> Result<Vec<Page>, Error> {
    let mut all = Vec::new();
    all.try_reserve_exact(pages.len() + extra)
        .map_err(|_| Error::NoMemory)?;
    for page in pages {
        all.push(page.try_clone()?);
    }
    Ok(all)
}

/// The placement of a widget to add: `id@n` for the first number no page of
/// either layout holds and that `has_values` says has no values set, so that
/// each widget added has settings of its own; `id` itself where the first 256
/// all are.
///
/// # Errors
/// [`Error::NoMemory`] and what `has_values` returns.
pub fn placement(
    src: &impl Source,
    id: &str,
    mut has_values: impl FnMut(&str) -> Result<bool, Error>,
) -> Result<String, Error> {
    let plain = pages::base(id);
    let mut digits = [0u8; 20];
    for n in 1..=256usize {
        let n = paperpaw_ui_kit::text::decimal(n, &mut digits);
        let placed = paperpaw_ui_kit::text::concat(&[plain, "@", n])?;
        if !pages::holds(src, &placed)? && !has_values(&placed)? {
            return Ok(placed);
        }
    }
    copy(id)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests;
