//! A list of plugins (interface 6: `view.rs`): those of a group, those
//! installed (those with an update first), the newest, those a library picked
//! for its start (a section), or those a search found. A group's list shows the
//! library's fair order of the day where it has one, and can be ordered by name,
//! newest first or most recently updated first; long lists can jump to a letter
//! (from A to Z) and to a page.
//!
//! The plugins to show are picked through the index on the card once, as the
//! list opens, its order changes or the list kept is replaced, and kept there
//! (`native_index::hit`); the screen holds only how many there are, and builds
//! the rows of the page shown, one page at a time (a [`RowSource`] of a
//! [`WindowList`]).
//!
//! Plain functions do the work and are tested without a host: [`Which::of`],
//! [`tops`], [`pick`], [`first_from`], [`Source::rows`], [`mark`], [`activation`].
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::builtin::params::{ID, TEXT};
use paperpaw_sdk::native::{
    Error, Host,
    callbacks::{DrawFrame, Input},
    commands::{CommandSender, RedrawMode},
    events::Event,
    formatting::Arg,
    snapshot,
};
use paperpaw_sdk::{Handled, Storage};
use paperpaw_ui_kit::mem::push;
use paperpaw_ui_kit::window::{RowSource, WindowList};
use paperpaw_ui_kit::{Footer, ItemList, ItemStep, RowItem, text};

use crate::native_index::{Index, Order, hit, name_key};
use crate::native_model::{Entry, GROUPS, SECTIONS};
use crate::native_page::{Kind, newer};
use crate::native_screen_letters::KEY as LETTER;
use crate::native_screen_list::{FONTS, HostSay, Say, group_name, here, number, section_name};
use crate::native_screens::{Ask, Ctx, Installs, Next, Reply, Screen, Step, ids};

/// The parameter that says which list: `group:3`, `installed`, `new`,
/// `section:1` or `search` (with `text`).
pub const VIEW: &str = "view";
/// The parameter that says the order of a group's list.
pub const ORDER: &str = "order";
/// Parameter of a plugin's page: its place in the index.
pub const ENTRY: &str = "entry";
/// The newest plugins shown at most.
pub const NEWEST: usize = 30;
/// The question *Sort by*, and the one *Go to page*.
pub const SORTED: u8 = 1;
pub const JUMPED: u8 = 2;
/// Rows a page holds, taken for the first frame, before one has been drawn
/// (the X4's), to tell whether a list needs *Go to letter*.
const PER_PAGE: usize = 6;

/// Which plugins a list shows.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Which {
    Group(usize),
    Installed,
    New,
    /// A section of the library's start (of [`SECTIONS`]).
    Section(usize),
    Search(String),
}

impl Which {
    /// The value of the parameter [`VIEW`].
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn key(&self) -> Result<String, Error> {
        let mut digits = [0; 20];
        match self {
            Which::Group(group) => text::concat(&["group:", text::decimal(*group, &mut digits)]),
            Which::Installed => text::copy("installed"),
            Which::New => text::copy("new"),
            Which::Section(section) => {
                text::concat(&["section:", text::decimal(*section, &mut digits)])
            }
            Which::Search(_) => text::copy("search"),
        }
    }

    /// The parameters that open the screen for this list.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn params(&self) -> Result<Vec<(&'static str, String)>, Error> {
        let mut params = Vec::new();
        params.try_reserve_exact(2).map_err(|_| Error::NoMemory)?;
        params.push((VIEW, self.key()?));
        if let Which::Search(typed) = self {
            params.push((TEXT, text::copy(typed)?));
        }
        Ok(params)
    }

    /// What opens the screen for this list.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn open(&self) -> Result<Next, Error> {
        Ok(Next::Open {
            screen: ids::VIEW,
            params: self.params()?,
        })
    }

    /// The list the parameters name: a number past the groups or sections is the
    /// first, anything unknown is the first group.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn of(params: &[(&str, &str)]) -> Result<Which, Error> {
        let get = |key: &str| {
            params
                .iter()
                .find(|(name, _)| *name == key)
                .map_or("", |(_, value)| *value)
        };
        let view = get(VIEW);
        Ok(match view {
            "installed" => Which::Installed,
            "new" => Which::New,
            "search" => Which::Search(text::copy(get(TEXT))?),
            view if view.starts_with("section:") => Which::Section(
                view.strip_prefix("section:")
                    .and_then(|s| s.parse().ok())
                    .filter(|&s| s < SECTIONS.len())
                    .unwrap_or(0),
            ),
            _ => Which::Group(
                view.strip_prefix("group:")
                    .and_then(|g| g.parse().ok())
                    .filter(|&g| g < GROUPS.len())
                    .unwrap_or(0),
            ),
        })
    }

    /// The title of the list.
    ///
    /// # Errors
    /// As [`Say::copy`].
    pub fn title(&self, say: &mut dyn Say) -> Result<String, Error> {
        match self {
            Which::Group(group) => group_name(say, *group),
            Which::Installed => say.copy("store.installed-list"),
            Which::New => say.copy("store.new"),
            Which::Section(section) => section_name(say, *section),
            Which::Search(typed) => say.format(
                "store.search.title",
                &[Arg::Text {
                    name: "query",
                    value: typed,
                }],
            ),
        }
    }
}

/// A row above the plugins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Top {
    /// The fonts browser, with the looks.
    Fonts,
    /// The order, to choose another.
    Sort,
    /// Go to a letter, from A to Z.
    Letter,
    /// Updates all plugins that have an update.
    UpdateAll,
}

/// The rows above the plugins of a list of `hits` plugins, `per_page` rows to a
/// page (0 where none was drawn yet): *Fonts* in the looks, the order and (for
/// more than a page, where a letter is quicker than paging) *Go to letter* in a
/// group, *Update all* in the installed where one has an update.
///
/// # Errors
/// `NoMemory`.
pub fn tops(
    which: &Which,
    hits: usize,
    per_page: usize,
    fonts: bool,
    updates: bool,
) -> Result<Vec<Top>, Error> {
    let mut top = Vec::new();
    top.try_reserve_exact(3).map_err(|_| Error::NoMemory)?;
    match which {
        Which::Group(group) => {
            if fonts && GROUPS.get(*group) == Some(&"look") {
                top.push(Top::Fonts);
            }
            if hits > 0 {
                top.push(Top::Sort);
            }
            let page = if per_page == 0 { PER_PAGE } else { per_page };
            if hits > page {
                top.push(Top::Letter);
            }
        }
        Which::Installed if updates => top.push(Top::UpdateAll),
        _ => {}
    }
    Ok(top)
}

/// What picking the plugins of a list came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Picked {
    /// How many plugins the list has (kept on the card, read with `hit`).
    pub hits: usize,
    /// One of them has an update.
    pub updates: bool,
}

/// Picks the plugins of the list `which` in `order` through `index`, keeping
/// them on the card.
///
/// # Errors
/// `NoMemory`.
pub fn pick(
    which: &Which,
    order: Order,
    index: &Index,
    storage: &dyn Storage,
    installs: &dyn Installs,
) -> Result<Picked, Error> {
    let mut updates = false;
    let hits = match which {
        Which::Group(group) => index.pick(
            storage,
            |_, e| Ok((e.group == *group).then_some(0)),
            order,
            usize::MAX,
        )?,
        // Those with an update first.
        Which::Installed => index.pick(
            storage,
            |_, e| {
                let update = here(installs, e);
                updates |= update == Some(true);
                Ok(update.map(u32::from))
            },
            Order::Rank,
            usize::MAX,
        )?,
        Which::New => index.pick(storage, |_, _| Ok(Some(0)), Order::Added, NEWEST)?,
        Which::Section(section) => {
            // In the library's order: the first the highest.
            let ids = index.section(storage, *section)?;
            index.pick(
                storage,
                |_, e| {
                    Ok(ids
                        .iter()
                        .position(|id| *id == e.id)
                        .map(|at| u32::try_from(ids.len() - at).unwrap_or(u32::MAX)))
                },
                Order::Rank,
                usize::MAX,
            )?
        }
        Which::Search(typed) => index.search(storage, typed)?,
    };
    Ok(Picked { hits, updates })
}

/// The first of `hits` plugins whose key is at or after `target`, found by
/// halving (the plugins are in order of their keys).
///
/// # Errors
/// What `key_of` says.
pub fn first_from(
    hits: usize,
    target: u64,
    mut key_of: impl FnMut(usize) -> Result<u64, Error>,
) -> Result<usize, Error> {
    let (mut low, mut high) = (0, hits);
    while low < high {
        let mid = low + (high - low) / 2;
        if key_of(mid)? < target {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    Ok(low)
}

/// The plugin of hit `n` of the list, with its place in the index.
///
/// # Errors
/// `NoMemory`.
pub fn hit_entry(
    index: &Index,
    storage: &dyn Storage,
    n: usize,
) -> Result<Option<(usize, Entry)>, Error> {
    let Some(place) = hit(storage, n) else {
        return Ok(None);
    };
    Ok(index.entry(storage, place)?.map(|entry| (place, entry)))
}

/// What choosing row `row` of a list with the rows `top` above its plugins
/// means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Act {
    /// A row above the plugins.
    Top(Top),
    /// The plugin of this hit.
    Plugin(usize),
}

/// What row `row` is: one of `top`, or a plugin (counted from the first below
/// them).
#[must_use]
pub fn activation(top: &[Top], row: usize) -> Act {
    match top.get(row) {
        Some(&top) => Act::Top(top),
        None => Act::Plugin(row - top.len()),
    }
}

/// What opens the page of plugin `id`, at place `n` of the index.
///
/// # Errors
/// `NoMemory`.
pub fn open_page(n: usize, id: &str) -> Result<Next, Error> {
    let mut digits = [0; 20];
    let mut params = Vec::new();
    params.try_reserve_exact(2).map_err(|_| Error::NoMemory)?;
    params.push((ID, text::copy(id)?));
    params.push((ENTRY, text::copy(text::decimal(n, &mut digits))?));
    Ok(Next::Open {
        screen: ids::PLUGIN,
        params,
    })
}

/// A size as people read it: `820 B`, `14 KB`, `2,3 MB`, with the decimal mark
/// given (interface 6's `files::size`; whole numbers all the way, no float).
///
/// # Errors
/// `NoMemory`.
pub fn size_text(bytes: u64, mark: &str) -> Result<String, Error> {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let (mut unit, mut unit_bytes) = (0usize, 1u128);
    let bytes = u128::from(bytes);
    // Up to 1,000 of a unit, then the next.
    while bytes >= 1000 * unit_bytes && unit + 1 < UNITS.len() {
        unit += 1;
        unit_bytes *= 1024;
    }
    let name = UNITS.get(unit).copied().unwrap_or("B");
    let mut digits = [0; 20];
    let mut more = [0; 20];
    // The value, rounded half up, in whole units; in tenths below ten.
    let whole = (bytes * 2 + unit_bytes) / (2 * unit_bytes);
    if unit == 0 || bytes >= 10 * unit_bytes {
        let whole = u64::try_from(whole).unwrap_or(u64::MAX);
        return text::concat(&[text::decimal_u64(whole, &mut digits), " ", name]);
    }
    let tenths = u64::try_from((bytes * 20 + unit_bytes) / (2 * unit_bytes)).unwrap_or(u64::MAX);
    text::concat(&[
        text::decimal_u64(tenths / 10, &mut digits),
        mark,
        text::decimal_u64(tenths % 10, &mut more),
        " ",
        name,
    ])
}

/// The decimal mark of the language (`core.decimal`), `.` where it has none.
fn decimal_mark(say: &mut dyn Say) -> Result<String, Error> {
    match say.copy("core.decimal") {
        Ok(mark) if !mark.is_empty() => Ok(mark),
        Err(Error::NoMemory) => Err(Error::NoMemory),
        _ => text::copy("."),
    }
}

/// How far a download is: `done / total`, or that it waits for the reader's
/// word once it is all here.
fn progress(say: &mut dyn Say, done: u32, total: u32) -> Result<String, Error> {
    if total == 0 {
        return number(say, done as usize);
    }
    if done >= total {
        return say.copy("store.state.waiting");
    }
    let (mut a, mut b) = ([0; 20], [0; 20]);
    let both = text::concat(&[
        text::decimal_u64(u64::from(done), &mut a),
        " / ",
        text::decimal_u64(u64::from(total), &mut b),
    ])?;
    say.digits(&both)
}

/// What a plugin's row says at its end: how far its download is, that it waits
/// (for the reader, a turn in the queue, a restart), that it is off, that its
/// build is not there yet, that an update waits or that it is installed; else
/// its size.
///
/// # Errors
/// `NoMemory`, or what the catalog says.
pub fn mark(say: &mut dyn Say, installs: &dyn Installs, entry: &Entry) -> Result<String, Error> {
    match installs.step(&entry.id) {
        Step::Downloading { done, total } => return progress(say, done, total),
        Step::Installing => return say.copy("store.state.waiting"),
        _ => {}
    }
    if installs.waiting(&entry.id) {
        return say.copy("store.state.queued");
    }
    let have = installs.have(&entry.id);
    let key = match &have {
        Some(have) if have.restart_needed => "store.state.restart",
        Some(have) if have.kind == Kind::BuiltIn && !have.enabled => "store.state.off",
        None if entry.waits_author => "store.no-build",
        None if entry.needs_newer => "store.needs-newer",
        Some(have) if newer(entry, have, installs.takes_betas()) => "store.state.update",
        Some(_) => "store.state.installed",
        None => {
            let mark = decimal_mark(say)?;
            return say.digits(&size_text(entry.size, &mark)?);
        }
    };
    say.copy(key)
}

/// What a plugin does, on one line: PaperPaw's own say so first, and those
/// nobody reviewed.
///
/// # Errors
/// `NoMemory`, or what the catalog says.
pub fn note(say: &mut dyn Say, entry: &Entry) -> Result<String, Error> {
    if entry.official {
        text::concat(&[&say.copy("store.official")?, " · ", &entry.summary])
    } else if entry.unreviewed {
        text::concat(&[&say.copy("store.unreviewed")?, " · ", &entry.summary])
    } else {
        text::copy(&entry.summary)
    }
}

/// The row of a plugin: its name, its mark, what it does.
///
/// # Errors
/// `NoMemory`, or what the catalog says.
pub fn plugin_row(
    say: &mut dyn Say,
    installs: &dyn Installs,
    entry: &Entry,
) -> Result<RowItem, Error> {
    Ok(RowItem {
        title: text::copy(&entry.name)?,
        value: mark(say, installs, entry)?,
        note: note(say, entry)?,
        zone: None,
        stepper: false,
        disabled: false,
        on: None,
        field: false,
    })
}

/// The row above the plugins: with `help` below the order (how to go to a
/// page, where the list has more than one).
///
/// # Errors
/// `NoMemory`, or what the catalog says.
pub fn top_row(say: &mut dyn Say, top: Top, order: Order, help: &str) -> Result<RowItem, Error> {
    let more = "›";
    let (title, value, note) = match top {
        Top::Fonts => (say.copy("store.fonts")?, text::copy(more)?, String::new()),
        Top::Sort => (
            say.copy("store.sort")?,
            say.copy(&text::concat(&["store.order.", order.key()])?)?,
            text::copy(help)?,
        ),
        Top::UpdateAll => (say.copy("store.update-all")?, String::new(), String::new()),
        Top::Letter => (say.copy("store.letter")?, text::copy(more)?, String::new()),
    };
    Ok(RowItem {
        title,
        value,
        note,
        zone: None,
        stepper: false,
        disabled: false,
        on: None,
        field: false,
    })
}

/// Where the rows of a list come from: the rows above the plugins, then the
/// plugins picked, read from the index a page at a time.
pub struct Source<'a> {
    pub index: &'a Index,
    pub storage: &'a dyn Storage,
    pub installs: &'a dyn Installs,
    pub top: &'a [Top],
    pub hits: usize,
    pub order: Order,
}

impl core::fmt::Debug for Source<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Source")
            .field("top", &self.top)
            .field("hits", &self.hits)
            .field("order", &self.order)
            .finish_non_exhaustive()
    }
}

impl Source<'_> {
    /// How many rows there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.top.len().saturating_add(self.hits)
    }

    /// Whether there is no row.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The rows `first..first + max` (fewer at the end). A plugin the card does
    /// not give any more is an empty row, so that the rows keep their places.
    ///
    /// # Errors
    /// `NoMemory`, or what the catalog says.
    pub fn rows(
        &self,
        say: &mut dyn Say,
        first: usize,
        max: usize,
        help: &str,
    ) -> Result<Vec<RowItem>, Error> {
        let end = first.saturating_add(max).min(self.len());
        let mut rows = Vec::new();
        rows.try_reserve_exact(end.saturating_sub(first))
            .map_err(|_| Error::NoMemory)?;
        for row in first..end {
            let item = match activation(self.top, row) {
                Act::Top(top) => top_row(say, top, self.order, help)?,
                Act::Plugin(n) => match hit_entry(self.index, self.storage, n)? {
                    Some((_, entry)) => plugin_row(say, self.installs, &entry)?,
                    None => RowItem::default(),
                },
            };
            push(&mut rows, item)?;
        }
        Ok(rows)
    }
}

/// The rows of a list for the window of a screen.
struct Rows<'a> {
    source: Source<'a>,
}

impl RowSource for Rows<'_> {
    fn len(&self) -> usize {
        self.source.len()
    }

    fn fill(
        &mut self,
        host: &Host<'_>,
        first: usize,
        max: usize,
        out: &mut Vec<RowItem>,
    ) -> Result<(), Error> {
        let mut say = HostSay::new(host)?;
        // Below the order of a list of more than a page: how to go to a page.
        let jump = self.source.len() > max && self.source.top.contains(&Top::Sort);
        let help = if jump && first == 0 {
            let touch = snapshot::capture_snapshot(host)?.has_touch();
            say.copy(if touch {
                "store.jump.help"
            } else {
                "store.jump.help.buttons"
            })?
        } else {
            String::new()
        };
        *out = self.source.rows(&mut say, first, max, &help)?;
        Ok(())
    }
}

/// What the lists of the index are, to tell when a fresh list took the place of
/// the one the plugins were picked from: its size and its runs.
fn stamp(index: &Index) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut mix = |value: usize| {
        hash = (hash ^ value as u64).wrapping_mul(0x100_0000_01b3);
    };
    mix(index.len());
    for &(group, count) in index.runs() {
        mix(group);
        mix(count);
    }
    for section in 0..SECTIONS.len() {
        mix(index.section_len(section));
    }
    hash
}

fn redraw(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
}

/// The screen.
#[derive(Debug)]
pub struct View {
    which: Which,
    order: Order,
    top: Vec<Top>,
    /// How many plugins the list has.
    hits: usize,
    /// One of them has an update.
    updates: bool,
    list: WindowList,
    /// What a list with nothing says, and where its header's back arrow is.
    empty: ItemList,
    /// The lists taken and the index's stamp when the plugins were picked.
    generation: u32,
    stamp: Option<u64>,
}

impl View {
    fn len(&self) -> usize {
        self.top.len().saturating_add(self.hits)
    }

    fn per_page(&self) -> usize {
        self.list.per_page().max(1)
    }

    fn pages(&self) -> usize {
        self.len().div_ceil(self.per_page()).max(1)
    }

    /// Picks the plugins of the list anew.
    fn pick(&mut self, ctx: &Ctx<'_>) -> Result<(), Error> {
        let Some(index) = ctx.loader.index() else {
            self.hits = 0;
            self.updates = false;
            self.stamp = None;
            self.generation = ctx.loader.generation();
            self.top.clear();
            return Ok(());
        };
        let picked = pick(&self.which, self.order, index, ctx.data, ctx.installs)?;
        self.hits = picked.hits;
        self.updates = picked.updates;
        self.stamp = Some(stamp(index));
        self.generation = ctx.loader.generation();
        self.retop(ctx)?;
        Ok(())
    }

    /// The rows above the plugins again (the rows a page holds are known once
    /// a page was drawn); whether they changed. The focus stays on its plugin.
    fn retop(&mut self, ctx: &Ctx<'_>) -> Result<bool, Error> {
        let top = tops(
            &self.which,
            self.hits,
            self.list.per_page(),
            ctx.installs.has_action(FONTS),
            self.updates,
        )?;
        if top == self.top {
            return Ok(false);
        }
        let (before, after) = (self.top.len(), top.len());
        let focus = self.list.focus();
        self.top = top;
        self.list
            .set_focus(focus.saturating_add(after).saturating_sub(before));
        Ok(true)
    }

    /// Whether a fresh list took the place of the one the plugins were picked
    /// from.
    fn stale(&self, ctx: &Ctx<'_>) -> bool {
        self.generation != ctx.loader.generation() || self.stamp != ctx.loader.index().map(stamp)
    }

    /// Shows the plugins from `letter` on, from A to Z: the first whose name
    /// sorts at or after it.
    fn go_to_letter(&mut self, ctx: &Ctx<'_>, letter: &str) -> Result<(), Error> {
        if self.order != Order::Name {
            self.order = Order::Name;
            self.pick(ctx)?;
        }
        let Some(index) = ctx.loader.index() else {
            return Ok(());
        };
        let target = name_key(letter);
        let low = first_from(self.hits, target, |n| {
            Ok(hit_entry(index, ctx.data, n)?.map_or(u64::MAX, |(_, entry)| name_key(&entry.name)))
        })?;
        let row = self
            .top
            .len()
            .saturating_add(low)
            .min(self.len().saturating_sub(1));
        self.list.set_focus(row);
        Ok(())
    }

    /// Asks which page to go to.
    fn ask_page(&self, ctx: &Ctx<'_>) -> Result<Next, Error> {
        let pages = i64::try_from(self.pages()).unwrap_or(i64::MAX);
        let title = HostSay::new(ctx.host)?.format(
            "store.go-to-page",
            &[Arg::Int {
                name: "pages",
                value: pages,
            }],
        )?;
        Ok(Next::Ask {
            tag: JUMPED,
            ask: Ask::Text {
                title,
                number: true,
            },
        })
    }

    /// Asks which order to show the list in.
    fn ask_order(&self, ctx: &Ctx<'_>) -> Result<Next, Error> {
        let mut say = HostSay::new(ctx.host)?;
        let orders = ctx.loader.index().map_or(&[][..], Index::orders);
        let mut options = Vec::new();
        options
            .try_reserve_exact(orders.len())
            .map_err(|_| Error::NoMemory)?;
        for order in orders {
            push(
                &mut options,
                say.copy(&text::concat(&["store.order.", order.key()])?)?,
            )?;
        }
        Ok(Next::Ask {
            tag: SORTED,
            ask: Ask::Choice {
                title: say.copy("store.sort")?,
                options,
            },
        })
    }

    fn activate(&mut self, ctx: &Ctx<'_>, row: usize) -> Result<Next, Error> {
        match activation(&self.top, row) {
            Act::Top(Top::Fonts) => {
                let mut scratch = [0; 128];
                CommandSender::new(ctx.host, &mut scratch).run_action(FONTS)?;
                Ok(Next::None)
            }
            Act::Top(Top::Letter) => {
                let mut params = Vec::new();
                params.try_reserve_exact(1).map_err(|_| Error::NoMemory)?;
                params.push((VIEW, self.which.key()?));
                Ok(Next::Open {
                    screen: ids::LETTERS,
                    params,
                })
            }
            Act::Top(Top::Sort) => self.ask_order(ctx),
            Act::Top(Top::UpdateAll) => Ok(Next::UpdateAll),
            Act::Plugin(n) => {
                let Some(index) = ctx.loader.index() else {
                    return Ok(Next::None);
                };
                match hit_entry(index, ctx.data, n)? {
                    Some((place, entry)) => open_page(place, &entry.id),
                    None => Ok(Next::None),
                }
            }
        }
    }

    /// The page typed goes to its first row.
    fn jump(&mut self, typed: &str) {
        if let Ok(page) = typed.trim().parse::<usize>() {
            let page = page.clamp(1, self.pages()) - 1;
            self.list.set_focus(page.saturating_mul(self.per_page()));
        }
    }
}

impl Screen for View {
    fn open(ctx: &Ctx<'_>, params: &[(&str, &str)]) -> Result<Self, Error> {
        let which = Which::of(params)?;
        let wanted = params
            .iter()
            .find(|(name, _)| *name == ORDER)
            .map_or("", |(_, value)| *value);
        let order = ctx
            .loader
            .index()
            .map_or(Order::Name, |index| index.order(wanted));
        let mut view = Self {
            which,
            order,
            top: Vec::new(),
            hits: 0,
            updates: false,
            list: WindowList::new(true),
            empty: ItemList::new(Vec::new()),
            generation: 0,
            stamp: None,
        };
        view.pick(ctx)?;
        Ok(view)
    }

    fn shown(&mut self, ctx: &Ctx<'_>) -> Result<(), Error> {
        // What is installed and has updates may have changed.
        if self.which == Which::Installed || self.stale(ctx) {
            self.pick(ctx)?;
        }
        Ok(())
    }

    fn draw(&mut self, ctx: &Ctx<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        if self.stale(ctx) {
            self.pick(ctx)?;
        }
        // The words are made and put away before the page is drawn: the draw
        // has a catalog buffer of its own.
        let title = self.which.title(&mut HostSay::new(ctx.host)?)?;
        let Some(index) = ctx.loader.index().filter(|_| self.len() > 0) else {
            let (head, help) = match self.which {
                Which::Search(_) => ("store.search.none", "store.search.none.help"),
                _ => ("store.empty", ""),
            };
            let mut say = HostSay::new(ctx.host)?;
            let head = say.copy(head)?;
            let help = if help.is_empty() {
                String::new()
            } else {
                say.copy(help)?
            };
            drop(say);
            return self.empty.draw_with(
                ctx.host,
                frame,
                &title,
                true,
                Footer::Empty {
                    title: &head,
                    text: &help,
                },
            );
        };
        let mut rows = Rows {
            source: Source {
                index,
                storage: ctx.data,
                installs: ctx.installs,
                top: &self.top,
                hits: self.hits,
                order: self.order,
            },
        };
        self.list.draw(ctx.host, frame, &title, &mut rows, true)?;
        // Now the rows a page holds are known: *Go to letter* is for more
        // than one page.
        if self.retop(ctx)? {
            redraw(ctx.host)?;
        }
        Ok(())
    }

    fn input(&mut self, ctx: &Ctx<'_>, input: &Input<'_>) -> Result<(Handled, Next), Error> {
        let len = self.len();
        if len == 0 {
            let step = self.empty.input(ctx.host, input)?;
            return Ok((ItemList::handled(step), Next::None));
        }
        let more = self.pages() > 1;
        // A long press of Select: go to a page.
        if more
            && matches!(
                *input,
                Input::Action {
                    action: "core.menu",
                    ..
                }
            )
        {
            return Ok((Handled::Yes, self.ask_page(ctx)?));
        }
        let step = self.list.input(ctx.host, len, input)?;
        let next = match step {
            // The list may have shrunk since the frame: only a row that is there.
            ItemStep::Chosen(row) if row < self.len() => self.activate(ctx, row)?,
            ItemStep::Held(_) if more => self.ask_page(ctx)?,
            _ => Next::None,
        };
        Ok((ItemList::handled(step), next))
    }

    fn event(&mut self, ctx: &Ctx<'_>, event: &Event<'_>) -> Result<Next, Error> {
        let changed = matches!(event, Event::PluginsChanged { .. } | Event::InstallDone(_));
        if self.stale(ctx) || (changed && self.which == Which::Installed) {
            self.pick(ctx)?;
            return Ok(Next::Redraw);
        }
        Ok(if changed { Next::Redraw } else { Next::None })
    }

    fn answered(&mut self, ctx: &Ctx<'_>, tag: u8, reply: Reply<'_>) -> Result<Next, Error> {
        match (tag, reply) {
            (SORTED, Reply::Choice(chosen)) => {
                let order = ctx
                    .loader
                    .index()
                    .and_then(|index| index.orders().get(chosen).copied());
                let Some(order) = order else {
                    return Ok(Next::None);
                };
                self.order = order;
                self.list.set_focus(0);
                self.pick(ctx)?;
                Ok(Next::Redraw)
            }
            (JUMPED, Reply::Text(typed)) => {
                self.jump(typed);
                Ok(Next::Redraw)
            }
            _ => Ok(Next::None),
        }
    }

    fn returned(&mut self, ctx: &Ctx<'_>, key: &str, value: &str) -> Result<Next, Error> {
        if key != LETTER {
            return Ok(Next::None);
        }
        self.go_to_letter(ctx, value)?;
        Ok(Next::Redraw)
    }
}
