//! The home screen's pages without the host: each page a grid of items kept
//! in a setting of its own (`home.page-1` to `home.page-8`, and
//! `home.page-N.wide` for a screen on its side), the layout a screen on its
//! side derives from the upright one while it has none of its own, and what
//! a change writes. The same rules as the interface-6 pages, with fallible
//! allocation; what the pages read comes from a [`Source`] and what they
//! write is returned as [`Write`]s.
use crate::native_grid::{self as grid, Item, Sizes, What, copy_items};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{native::Error, widgets::grid::Geometry};
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy},
};

/// Pages at most.
pub const MAX_PAGES: usize = 8;

/// Pages of the layout for a screen on its side at most while it is derived
/// (not stored): more than the settings keep, as a grid of fewer cells than
/// the upright one may need more pages for the same items.
const MAX_DERIVED: usize = 3 * MAX_PAGES;

/// Columns of the grid at most.
const MAX_COLUMNS: i64 = 6;

/// The status line, a built-in widget.
pub const STATUS: &str = "home.status";
/// On a page: every tool not on it otherwise, each in a cell.
pub const TOOLS: &str = "home.tools";
/// A gap of a height.
pub const SPACE: &str = "core.space";
/// The room the other widgets leave.
pub const FILL: &str = "core.fill";
/// Said when the layout for a screen on its side has more pages than are kept.
pub const WIDE_FULL: &str = "home.grid.wide-full";

/// What the pages show at first: the clock, the book being read and the
/// recent ones, then the tools.
pub const DEFAULTS: [&str; MAX_PAGES] = [
    "home.clock, home.continue, home.recent",
    "home.tools",
    "",
    "",
    "",
    "",
    "",
    "",
];

/// What the pages read from the host.
pub trait Source: Sizes {
    /// The text of a setting (`home.page-1`, …); empty for none.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals.
    fn text(&self, setting: &str) -> Result<String, Error>;
    /// Whether the status line shows above the grid (`home.status-bar`).
    fn status_bar(&self) -> bool;
    /// The grid's columns upright (`home.columns`).
    fn columns(&self) -> i64;
    /// Whether a widget (a placement of it) is there to show.
    fn has_widget(&self, id: &str) -> bool;
    /// The tools a page may show: their actions, in order.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals.
    fn tools(&self) -> Result<Vec<String>, Error>;
    /// The most a widget is wide on a screen on its side, in pixels (110 mm).
    fn widest_px(&self) -> u32;
}

/// A change to the settings, or a message.
#[derive(Debug, PartialEq, Eq)]
pub enum Write {
    /// Set `setting` to `text`.
    Set {
        /// The setting.
        setting: String,
        /// Its new text.
        text: String,
    },
    /// Say the message of this catalog key.
    Notify(&'static str),
}

/// A page shown: which setting keeps it, and its items.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    /// The setting it is kept in: 0 for `home.page-1`.
    pub slot: usize,
    /// Its items.
    pub items: Vec<Item>,
}

impl Page {
    /// A copy made with fallible allocation.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn try_clone(&self) -> Result<Self, Error> {
        Ok(Self {
            slot: self.slot,
            items: copy_items(&self.items)?,
        })
    }
}

/// The setting of page `slot` (0 the first), for a screen on its side
/// (`wide`) or one upright.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn setting(slot: usize, wide: bool) -> Result<String, Error> {
    let mut digits = [0u8; 20];
    let n = paperpaw_ui_kit::text::decimal(slot + 1, &mut digits);
    concat(&["home.page-", n, if wide { ".wide" } else { "" }])
}

/// The slot of the setting `id`, if it keeps a page: 0 for `home.page-1`.
#[must_use]
pub fn slot_of(id: &str) -> Option<usize> {
    let rest = id.strip_prefix("home.page-")?;
    let n = rest.strip_suffix(".wide").unwrap_or(rest);
    let n: usize = n.parse().ok()?;
    (1..=MAX_PAGES).contains(&n).then(|| n - 1)
}

/// The widget a placement shows: `widgets.clock` for `widgets.clock@2`.
#[must_use]
pub fn base(id: &str) -> &str {
    id.split_once('@').map_or(id, |(base, _)| base)
}

/// The pages kept in the settings of one layout (`wide`: the screen on its
/// side): the first always, the others where they hold something.
fn read(src: &impl Source, wide: bool) -> Result<Vec<Page>, Error> {
    let mut pages = Vec::new();
    for slot in 0..MAX_PAGES {
        let items = grid::parse(&src.text(&setting(slot, wide)?)?)?;
        if slot == 0 || !items.is_empty() {
            push(&mut pages, Page { slot, items })?;
        }
    }
    Ok(pages)
}

/// Every page shown in the layout for a screen upright, in the order of
/// their settings.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn pages(src: &impl Source) -> Result<Vec<Page>, Error> {
    read(src, false)
}

/// Whether any page has a layout of its own for a screen on its side.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn wide_kept(src: &impl Source) -> Result<bool, Error> {
    for slot in 0..MAX_PAGES {
        if !src.text(&setting(slot, true)?)?.is_empty() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The columns the grid has upright.
#[must_use]
pub fn columns_upright(src: &impl Source) -> u8 {
    src.columns().clamp(2, MAX_COLUMNS) as u8
}

/// `items` as a page reads: row by row, what has no place last; stable.
fn reading_order(items: &mut [Item]) {
    let key = |item: &Item| item.at.map_or((true, 0, 0), |c| (false, c.row, c.col));
    for i in 1..items.len() {
        let mut j = i;
        while j > 0 && key(&items[j - 1]) > key(&items[j]) {
            items.swap(j - 1, j);
            j -= 1;
        }
    }
}

/// The most columns of `geometry` a widget takes on a screen on its side:
/// those that make 110 millimetres, rounded up, all where the screen is not
/// wider.
fn widest_widget(src: &impl Source, geometry: &Geometry) -> u8 {
    let column = u32::from(geometry.cell_w) + u32::from(geometry.gap);
    src.widest_px()
        .div_ceil(column.max(1))
        .clamp(1, u32::from(geometry.columns)) as u8
}

/// A page's items as the grid shows them: the tools for `home.tools`,
/// without what the grid has no use for (the status line above it, spaces:
/// empty cells are) and widgets that are not there.
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
            What::Widget(id)
                if id == SPACE || id == FILL || (base(id) == STATUS && src.status_bar()) => {}
            What::Widget(id) if !src.has_widget(id) => {}
            _ => push(&mut out, item.try_clone()?)?,
        }
    }
    Ok(out)
}

/// Whether the page keeps `item` without showing it ([`expand`] leaves it
/// out): a widget of a plugin not loaded now, the status line while it shows
/// above the grid, a fill.
fn hidden(src: &impl Source, item: &Item) -> bool {
    match &item.what {
        What::Widget(id) if id == TOOLS || id == SPACE => false,
        What::Widget(id) => {
            id == FILL || (base(id) == STATUS && src.status_bar()) || !src.has_widget(id)
        }
        What::Tool(_) => false,
    }
}

/// What a page keeps without showing it among the items of the settings
/// `from`, added to `items`.
fn with_hidden(
    src: &impl Source,
    mut items: Vec<Item>,
    from: &[String],
) -> Result<Vec<Item>, Error> {
    for setting in from {
        for item in grid::parse(&src.text(setting)?)? {
            if hidden(src, &item) && !items.iter().any(|i| i.what == item.what) {
                push(&mut items, item)?;
            }
        }
    }
    Ok(items)
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

fn set(slot: usize, wide: bool, text: String) -> Result<Write, Error> {
    Ok(Write::Set {
        setting: setting(slot, wide)?,
        text,
    })
}

/// Every page shown for a screen on its side, whose grid is `geometry`: the
/// pages kept for it where there are any, else the items of the upright
/// layout laid out anew in it, page after page in reading order, each page
/// filled before the next is begun. Nothing is stored: where the pages are
/// more than [`MAX_PAGES`] they all show, and the layout cannot be kept (see
/// [`store_all`]) until there is less.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn pages_wide(src: &impl Source, geometry: &Geometry) -> Result<Vec<Page>, Error> {
    if wide_kept(src)? {
        return read(src, true);
    }
    let from = columns_upright(src);
    let widest = widest_widget(src, geometry);
    let mut carried: Vec<Item> = Vec::new();
    for page in pages(src)? {
        let mut items = expand(src, &page.items)?;
        reading_order(&mut items);
        carried
            .try_reserve(items.len())
            .map_err(|_| Error::NoMemory)?;
        carried.append(&mut items);
    }
    let mut out: Vec<Page> = Vec::new();
    while out.len() < MAX_DERIVED && (out.is_empty() || !carried.is_empty()) {
        let (placed, left) = grid::refit(src, geometry, from, widest, &carried)?;
        // Nothing fits even an empty page: what remains is not shown.
        if placed.is_empty() && !out.is_empty() {
            break;
        }
        let slot = out.len();
        push(
            &mut out,
            Page {
                slot,
                items: placed,
            },
        )?;
        carried = left;
    }
    Ok(out)
}

/// What keeps `items` (as the page shows them) as the page in `slot`, with
/// what the page keeps there without showing it: in the layout for a screen
/// upright (`wide` is `None`), or in that for a screen on its side, whose
/// grid is `wide`. The first change in the latter keeps every page of the
/// layout derived from the upright one in the `.wide` settings, which are
/// the layout from then on.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn store(
    src: &impl Source,
    slot: usize,
    items: &[Item],
    wide: Option<&Geometry>,
) -> Result<Vec<Write>, Error> {
    let Some(geometry) = wide else {
        let mut one = Vec::new();
        let mut from = Vec::new();
        push(&mut from, setting(slot, false)?)?;
        let items = with_hidden(src, copy_items(items)?, &from)?;
        push(&mut one, set(slot, false, page_text(slot, &items)?)?)?;
        return Ok(one);
    };
    let mut pages = pages_wide(src, geometry)?;
    if let Some(page) = pages.iter_mut().find(|p| p.slot == slot) {
        page.items = copy_items(items)?;
    }
    store_all(src, &pages, true)
}

/// What keeps `pages` of a layout in the first settings, in their order, and
/// empties the others: once a page was added or removed. For the screen on
/// its side what the upright pages keep without showing is added on its last
/// page. Where that layout has more than [`MAX_PAGES`] pages it is not kept,
/// so that nothing is cut off: the message says to make room upright.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn store_all(src: &impl Source, pages: &[Page], wide: bool) -> Result<Vec<Write>, Error> {
    if !wide {
        return write_all(src, pages, false);
    }
    if pages.len() > MAX_PAGES {
        let mut one = Vec::new();
        push(&mut one, Write::Notify(WIDE_FULL))?;
        return Ok(one);
    }
    let mut with: Vec<Page> = Vec::new();
    with.try_reserve_exact(pages.len())
        .map_err(|_| Error::NoMemory)?;
    for page in pages {
        let mut from = Vec::new();
        push(&mut from, setting(page.slot, true)?)?;
        let items = with_hidden(src, copy_items(&page.items)?, &from)?;
        with.push(Page {
            slot: page.slot,
            items,
        });
    }
    // What the upright pages keep without showing (their places are theirs
    // and no use here): on the last page, where it is not on a page yet.
    let mut upright: Vec<String> = Vec::new();
    for slot in 0..MAX_PAGES {
        push(&mut upright, setting(slot, false)?)?;
    }
    let mut hidden_upright = with_hidden(src, Vec::new(), &upright)?;
    hidden_upright.retain(|item| {
        !with
            .iter()
            .flat_map(|p| &p.items)
            .any(|i| i.what == item.what)
    });
    if let Some(last) = with.last_mut() {
        last.items
            .try_reserve(hidden_upright.len())
            .map_err(|_| Error::NoMemory)?;
        last.items
            .extend(hidden_upright.into_iter().map(|item| Item::new(item.what)));
    }
    write_all(src, &with, true)
}

/// [`store_all`] for the pages as they are: only the settings whose text
/// changes are written (the budget of a callback's commands is small).
fn write_all(src: &impl Source, pages: &[Page], wide: bool) -> Result<Vec<Write>, Error> {
    let mut writes = Vec::new();
    writes
        .try_reserve_exact(MAX_PAGES)
        .map_err(|_| Error::NoMemory)?;
    for slot in 0..MAX_PAGES {
        let text = match pages.get(slot) {
            Some(page) => page_text(slot, &page.items)?,
            None => String::new(),
        };
        if src.text(&setting(slot, wide)?)? != text {
            writes.push(set(slot, wide, text)?);
        }
    }
    Ok(writes)
}

/// Whether any page of either layout holds the widget `id`.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn holds(src: &impl Source, id: &str) -> Result<bool, Error> {
    let has = |pages: &[Page]| {
        pages
            .iter()
            .flat_map(|p| &p.items)
            .any(|i| matches!(&i.what, What::Widget(w) if w == id))
    };
    Ok(has(&pages(src)?) || (wide_kept(src)? && has(&read(src, true)?)))
}

/// The pages of the layout other than the one shown (`wide`: the screen is
/// on its side): the pages kept for that, or none where the layout for the
/// screen on its side is derived and so has none of its own.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn other_layer(src: &impl Source, wide: bool) -> Result<Vec<Page>, Error> {
    if wide {
        pages(src)
    } else if wide_kept(src)? {
        read(src, true)
    } else {
        Ok(Vec::new())
    }
}

/// What adds `what` to the page the layout other than the one shown has in
/// `slot` (the last page where it has fewer) without a place, where the grid
/// puts it in the first free cells: a widget or tool added on a screen upright
/// is on its side too, and the other way. Nothing where that layout is the
/// derived one, which has it already. `wide_grid` is the grid of a screen on
/// its side.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn add_to_other(
    src: &impl Source,
    wide: bool,
    slot: usize,
    what: What,
    wide_grid: &Geometry,
) -> Result<Vec<Write>, Error> {
    let other = other_layer(src, wide)?;
    let Some(page) = other.get(slot).or(other.last()) else {
        return Ok(Vec::new());
    };
    if page.items.iter().any(|i| i.what == what) {
        return Ok(Vec::new());
    }
    let mut items = copy_items(&page.items)?;
    push(&mut items, Item::new(what))?;
    // The other layout's own grid: only the stored layer is written.
    if wide {
        store(src, page.slot, &items, None)
    } else {
        store(src, page.slot, &items, Some(wide_grid))
    }
}

/// What empties the layout for a screen on its side (the settings that hold
/// something): the derived one shows again.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn reset_wide(src: &impl Source) -> Result<Vec<Write>, Error> {
    let mut writes = Vec::new();
    writes
        .try_reserve_exact(MAX_PAGES)
        .map_err(|_| Error::NoMemory)?;
    for slot in 0..MAX_PAGES {
        if !src.text(&setting(slot, true)?)?.is_empty() {
            writes.push(set(slot, true, String::new())?);
        }
    }
    Ok(writes)
}

/// The pages fitted to `geometry` once its columns or rows changed: every
/// item where the grid now shows it (moved inside it as little as can be),
/// and what a page has no room for on the next page (a page added where there
/// is none), so that nothing is lost or hidden; `None` where nothing moved.
/// The tools and the spaces stay as they are.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn reflow(
    src: &impl Source,
    geometry: &Geometry,
    wide: bool,
) -> Result<Option<Vec<Page>>, Error> {
    let kept = |item: &Item| match &item.what {
        What::Widget(id) => id == TOOLS || id == SPACE || id == FILL,
        What::Tool(_) => false,
    };
    // The pages kept for a screen on its side where it has its own.
    if wide && !wide_kept(src)? {
        return Ok(None);
    }
    let mut pages = read(src, wide)?;
    let mut carried: Vec<Item> = Vec::new();
    let mut changed = false;
    let mut index = 0;
    while index < pages.len() || (!carried.is_empty() && pages.len() < MAX_PAGES) {
        if index == pages.len() {
            push(
                &mut pages,
                Page {
                    slot: index,
                    items: Vec::new(),
                },
            )?;
        }
        let (special, mut movable): (Vec<Item>, Vec<Item>) = {
            let (mut special, mut movable) = (Vec::new(), Vec::new());
            for item in &pages[index].items {
                if kept(item) {
                    push(&mut special, item.try_clone()?)?;
                } else {
                    push(&mut movable, item.try_clone()?)?;
                }
            }
            (special, movable)
        };
        // What did not fit before comes first, so it finds room here.
        let mut first = Vec::new();
        first
            .try_reserve_exact(carried.len() + movable.len())
            .map_err(|_| Error::NoMemory)?;
        first.extend(carried.drain(..).map(|item| Item::new(item.what)));
        first.append(&mut movable);
        let (mut placed, left) = grid::fit(src, geometry, &first)?;
        placed
            .try_reserve(special.len())
            .map_err(|_| Error::NoMemory)?;
        placed.extend(special);
        if placed != pages[index].items {
            pages[index].items = placed;
            changed = true;
        }
        carried = left;
        index += 1;
    }
    Ok(changed.then_some(pages))
}

/// What a change of the grid's columns or rows writes: each layout fitted to
/// its own grid (`upright` for a screen upright, `wide` for one on its side),
/// whatever way the screen is now. Empty where nothing moved.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn refit(src: &impl Source, upright: &Geometry, wide: &Geometry) -> Result<Vec<Write>, Error> {
    let mut writes = Vec::new();
    for (is_wide, geometry) in [(false, upright), (true, wide)] {
        let Some(fitted) = reflow(src, geometry, is_wide)? else {
            continue;
        };
        let mut stored = store_all(src, &fitted, is_wide)?;
        writes
            .try_reserve(stored.len())
            .map_err(|_| Error::NoMemory)?;
        writes.append(&mut stored);
    }
    Ok(writes)
}

#[cfg(test)]
mod tests;
