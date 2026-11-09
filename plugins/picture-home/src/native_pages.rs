//! The picture home screen's pages without the host: each page a grid of
//! items kept in a setting of its own (`picture-home.page-1` to `-4`), what a
//! screen on its side shows of them, and what a change writes. The same rules
//! as the interface-6 `pages` (the first page always shows, the others where
//! they have items, an emptied page keeps a space so that it shows until it
//! is removed), with fallible allocation. What the pages read comes from a
//! [`Source`] and what they write is returned as [`Write`]s.
//!
//! One layout serves the screen upright and on its side: on its side the items
//! of a page are placed anew in the order they have (the grid puts each into
//! the first free cells), nothing of that is kept, so that turning the screen
//! back and forth changes no page ([`placed_anew`]). There is no layout of its
//! own for a screen on its side, and so no `.wide` setting as the built-in home
//! screen has.
use crate::native_grid::{self as grid, Item, Sizes, What, copy_items};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy},
};

/// Pages at most.
pub const MAX_PAGES: usize = 4;

/// On a page: every tool not on it otherwise, each in a cell.
pub const TOOLS: &str = "home.tools";
/// A gap of a height.
pub const SPACE: &str = "core.space";
/// The room the other widgets leave.
pub const FILL: &str = "core.fill";

/// What the pages show at first: the clock, the book being read and the recent
/// ones, then the tools.
pub const DEFAULTS: [&str; MAX_PAGES] = ["home.clock, home.continue, home.recent", "home.tools", "", ""];

/// What the pages read from the host.
pub trait Source: Sizes {
    /// The text of a setting (`picture-home.page-1`, …); empty for none.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals.
    fn text(&self, setting: &str) -> Result<String, Error>;
    /// Whether a widget (a placement of it) is there to show.
    fn has_widget(&self, id: &str) -> bool;
    /// The tools a page may show: their actions, in order.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals.
    fn tools(&self) -> Result<Vec<String>, Error>;
}

/// A change to the settings.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Write {
    /// The setting to set.
    pub setting: String,
    /// Its new text.
    pub text: String,
}

/// A page shown: which setting keeps it, and its items.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Page {
    /// The setting it is kept in: 0 for `picture-home.page-1`.
    pub slot: usize,
    /// Its items.
    pub items: Vec<Item>,
}

/// The widget a placement shows: `home.clock` for `home.clock@2`.
#[must_use]
pub fn base(id: &str) -> &str {
    id.split_once('@').map_or(id, |(base, _)| base)
}

/// The setting of page `slot` (0 the first).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn setting(slot: usize) -> Result<String, Error> {
    let mut digits = [0u8; 20];
    let n = paperpaw_ui_kit::text::decimal(slot + 1, &mut digits);
    concat(&["picture-home.page-", n])
}

/// The slot of the setting `id`, if it keeps a page: 0 for `picture-home.page-1`.
#[must_use]
pub fn slot_of(id: &str) -> Option<usize> {
    let n: usize = id.strip_prefix("picture-home.page-")?.parse().ok()?;
    (1..=MAX_PAGES).contains(&n).then(|| n - 1)
}

/// Every page shown, in the order of their settings: the first always, the
/// others where they hold something.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn pages(src: &impl Source) -> Result<Vec<Page>, Error> {
    let mut pages = Vec::new();
    for slot in 0..MAX_PAGES {
        let items = grid::parse(&src.text(&setting(slot)?)?)?;
        if slot == 0 || !items.is_empty() {
            push(&mut pages, Page { slot, items })?;
        }
    }
    Ok(pages)
}

/// A page's items as the grid shows them: the tools for `home.tools`, without
/// what the grid has no use for (spaces: empty cells are, fills) or cannot show
/// (a widget of a plugin not loaded now).
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn expand(src: &impl Source, items: &[Item]) -> Result<Vec<Item>, Error> {
    let mut out: Vec<Item> = Vec::new();
    for item in items {
        match &item.what {
            What::Widget(id) if id == TOOLS => {
                for action in src.tools()? {
                    let shown = items
                        .iter()
                        .any(|i| matches!(&i.what, What::Tool(t) if *t == action));
                    if !shown {
                        push(&mut out, Item::new(What::Tool(action)))?;
                    }
                }
            }
            What::Widget(id) if id == SPACE || id == FILL => {}
            What::Widget(id) if !src.has_widget(id) => {}
            _ => push(&mut out, item.try_clone()?)?,
        }
    }
    Ok(out)
}

/// `items` as a screen on its side shows them (`wide`): placed anew in the
/// order they have, none kept where it was; upright as they are.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn placed_anew(items: &[Item], wide: bool) -> Result<Vec<Item>, Error> {
    if !wide {
        return copy_items(items);
    }
    let mut out = Vec::new();
    out.try_reserve_exact(items.len())
        .map_err(|_| Error::NoMemory)?;
    for item in items {
        out.push(Item::new(item.what.try_clone()?));
    }
    Ok(out)
}

/// Whether the page keeps `item` without showing it ([`expand`] leaves it out):
/// a widget of a plugin not loaded now, a fill.
fn hidden(src: &impl Source, item: &Item) -> bool {
    match &item.what {
        What::Widget(id) if id == TOOLS || id == SPACE => false,
        What::Widget(id) => id == FILL || !src.has_widget(id),
        What::Tool(_) => false,
    }
}

/// The text a page keeps: a page other than the first keeps a space when
/// emptied, so that it still shows.
fn page_text(slot: usize, items: &[Item]) -> Result<String, Error> {
    if items.is_empty() && slot > 0 {
        copy(SPACE)
    } else {
        grid::write(items)
    }
}

/// What keeps `items` (as the page shows them) as the page in `slot`, with what
/// the page keeps there without showing it. One write.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn store(src: &impl Source, slot: usize, items: &[Item]) -> Result<Vec<Write>, Error> {
    let mut all = copy_items(items)?;
    for item in grid::parse(&src.text(&setting(slot)?)?)? {
        if hidden(src, &item) && !all.iter().any(|i| i.what == item.what) {
            push(&mut all, item)?;
        }
    }
    let mut one = Vec::new();
    push(
        &mut one,
        Write {
            setting: setting(slot)?,
            text: page_text(slot, &all)?,
        },
    )?;
    Ok(one)
}

/// What keeps `pages` as the pages, in their order, and empties the others: once
/// a page was added or removed. Each page keeps what it hides as [`store`] does,
/// and only the settings whose text changes are written (the budget of a
/// callback's commands is small).
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn store_all(src: &impl Source, pages: &[Page]) -> Result<Vec<Write>, Error> {
    let mut writes = Vec::new();
    writes
        .try_reserve_exact(MAX_PAGES)
        .map_err(|_| Error::NoMemory)?;
    for slot in 0..MAX_PAGES {
        // The pages move up when one went: each into the setting of its place.
        let write = match pages.get(slot) {
            Some(page) => store(src, slot, &page.items)?.pop(),
            None => Some(Write {
                setting: setting(slot)?,
                text: String::new(),
            }),
        };
        if let Some(write) = write
            && src.text(&write.setting)? != write.text
        {
            writes.push(write);
        }
    }
    Ok(writes)
}

/// Whether any page holds the widget `id` (a placement `id@n`).
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn holds(src: &impl Source, id: &str) -> Result<bool, Error> {
    Ok(pages(src)?
        .iter()
        .flat_map(|p| &p.items)
        .any(|i| matches!(&i.what, What::Widget(w) if w == id)))
}

#[cfg(test)]
mod tests;
