//! The store's start (interface 6: `list.rs`): *Search*, the plugins installed
//! (with how many have an update), what the library picked (recommended, new,
//! popular, recently updated, to discover; the newest where it picks nothing),
//! then the groups with how many plugins each has. Each opens a list of its own
//! ([`crate::native_screen_view`]); *Fonts* is in the list of looks. The list
//! kept from the last visit shows at once, while a fresh one is fetched
//! ([`Screen::started`] asks for it).
//!
//! However long the list, the screen keeps only how many plugins each group has
//! and the rows made of them: the plugins are counted in one pass through the
//! index on the card when the screen opens or comes on top, when the list or the
//! plugins change, never at the start of the store.
//!
//! The rows are made from the counts and the words by plain functions
//! ([`count`], [`rows`], [`items`], [`choose`]), tested without a host.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error, Host,
    callbacks::{DrawFrame, Input},
    events::Event,
    formatting::Arg,
    snapshot,
};
use paperpaw_sdk::{Handled, Storage};
use paperpaw_ui_kit::mem::push;
use paperpaw_ui_kit::{Footer, Item, ItemList, ItemStep, Messages, text};

use crate::native_index::Index;
use crate::native_model::{Entry, GROUPS, SECTIONS};
use crate::native_page::{Kind, newer};
use crate::native_screen_view::Which;
use crate::native_screens::{Ask, Ctx, Installs, Next, Reply, Screen, Trouble};

/// The fonts browser's action: the *Fonts* row of the looks runs it.
pub const FONTS: &str = "fonts-browser.open";
/// The question the *Search* row asks: the text to look for.
pub const SEARCH: u8 = 1;

/// The words of the screens, said by the catalog (or by a test).
pub trait Say {
    /// Text `key` of the catalog.
    ///
    /// # Errors
    /// `NoMemory`, or what the catalog says (`NotFound`).
    fn copy(&mut self, key: &str) -> Result<String, Error>;
    /// Text `key` of the catalog with `args` (a plural by `Arg::Count`).
    ///
    /// # Errors
    /// `NoMemory`, or what the catalog says.
    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error>;
    /// `text`, a number, in the language's digits.
    ///
    /// # Errors
    /// `NoMemory`, or what the host says.
    fn digits(&mut self, text: &str) -> Result<String, Error>;
}

/// The words as the host's catalog says them.
#[derive(Debug)]
pub struct HostSay<'a> {
    host: &'a Host<'a>,
    messages: Messages,
}

impl<'a> HostSay<'a> {
    /// The catalog's words through `host`.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new(host: &'a Host<'a>) -> Result<Self, Error> {
        Ok(Self {
            host,
            messages: Messages::new()?,
        })
    }
}

impl Say for HostSay<'_> {
    fn copy(&mut self, key: &str) -> Result<String, Error> {
        self.messages.copy(self.host, key)
    }

    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error> {
        self.messages.format(self.host, key, args)
    }

    fn digits(&mut self, text: &str) -> Result<String, Error> {
        self.messages.digits(self.host, text)
    }
}

/// A number in the language's digits.
///
/// # Errors
/// As [`Say::digits`].
pub fn number(say: &mut dyn Say, value: usize) -> Result<String, Error> {
    let mut digits = [0; 20];
    say.digits(text::decimal(value, &mut digits))
}

/// The name of section `section` (of [`SECTIONS`]).
///
/// # Errors
/// As [`Say::copy`].
pub fn section_name(say: &mut dyn Say, section: usize) -> Result<String, Error> {
    let name = SECTIONS.get(section).copied().unwrap_or(SECTIONS[0]);
    say.copy(&text::concat(&["store.section.", name])?)
}

/// The name of group `group`: one past the groups (not from this store's own
/// index) is `system`.
///
/// # Errors
/// As [`Say::copy`].
pub fn group_name(say: &mut dyn Say, group: usize) -> Result<String, Error> {
    let name = GROUPS
        .get(group)
        .or_else(|| GROUPS.last())
        .copied()
        .unwrap_or("system");
    say.copy(&text::concat(&["store.group.", name])?)
}

/// What an entry of the start is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Row {
    Search,
    Installed,
    New,
    /// What the library picked (of [`SECTIONS`]).
    Section(usize),
    Group(usize),
}

/// How many plugins the start counts: per group, installed, with an update.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Counts {
    pub groups: [usize; GROUPS.len()],
    pub installed: usize,
    pub updates: usize,
    /// The plugins of each section this device can install.
    pub sections: [usize; SECTIONS.len()],
}

/// Whether the device has `entry` for the lists, and then whether an update
/// waits for it: `None` where it is not installed or is a built-in switched
/// off (the store switches it on, it is not *installed* by the store).
#[must_use]
pub fn here(installs: &dyn Installs, entry: &Entry) -> Option<bool> {
    let have = installs.have(&entry.id)?;
    if have.kind == Kind::BuiltIn && !have.enabled {
        return None;
    }
    Some(newer(entry, &have, installs.takes_betas()))
}

/// Counts the plugins of `index`: per group (from the runs), those installed
/// and with an update, and those each section of the library picks that this
/// device lists. One pass through the index; the ids of the sections are held
/// for it (a few dozen at most).
///
/// # Errors
/// `NoMemory`.
pub fn count(
    index: &Index,
    storage: &dyn Storage,
    installs: &dyn Installs,
) -> Result<Counts, Error> {
    let mut counts = Counts::default();
    for &(group, count) in index.runs() {
        if let Some(total) = counts.groups.get_mut(group) {
            *total = total.saturating_add(count);
        }
    }
    // A few dozen ids at most: those the library picked.
    let mut picked: Vec<Vec<String>> = Vec::new();
    picked
        .try_reserve_exact(SECTIONS.len())
        .map_err(|_| Error::NoMemory)?;
    for section in 0..SECTIONS.len() {
        let ids = if index.fair() && index.section_len(section) > 0 {
            index.section(storage, section)?
        } else {
            Vec::new()
        };
        picked.push(ids);
    }
    index.scan(storage, &mut |_, entry| {
        if let Some(update) = here(installs, &entry) {
            counts.installed = counts.installed.saturating_add(1);
            counts.updates = counts.updates.saturating_add(usize::from(update));
        }
        for (count, ids) in counts.sections.iter_mut().zip(&picked) {
            *count = count.saturating_add(usize::from(ids.contains(&entry.id)));
        }
        Ok(())
    })?;
    Ok(counts)
}

/// The entries of the start, in order: *Search*, the installed (where there is
/// one), what the library picked (else *New*), then the groups with plugins (the
/// looks also without, where `fonts` says *Fonts* is there).
///
/// # Errors
/// `NoMemory`.
pub fn rows(counts: &Counts, fonts: bool) -> Result<Vec<Row>, Error> {
    let mut rows = Vec::new();
    rows.try_reserve_exact(2 + SECTIONS.len() + GROUPS.len())
        .map_err(|_| Error::NoMemory)?;
    rows.push(Row::Search);
    if counts.installed > 0 {
        rows.push(Row::Installed);
    }
    let picked = counts.sections.iter().filter(|&&n| n > 0).count();
    if picked == 0 {
        rows.push(Row::New);
    }
    for (section, _) in counts.sections.iter().enumerate().filter(|(_, n)| **n > 0) {
        rows.push(Row::Section(section));
    }
    for (group, &count) in counts.groups.iter().enumerate() {
        if count > 0 || (fonts && GROUPS.get(group) == Some(&"look")) {
            rows.push(Row::Group(group));
        }
    }
    Ok(rows)
}

/// The row of entry `row`: its name, and what it says at its end (how many
/// plugins, how many have an update, or `›`).
///
/// # Errors
/// `NoMemory`, or what the catalog says.
pub fn item(say: &mut dyn Say, row: Row, counts: &Counts) -> Result<Item, Error> {
    let more = "›";
    match row {
        Row::Search => Item::row(&say.copy("store.search")?, more),
        Row::Installed => {
            let name = say.copy("store.installed-list")?;
            // How many have an update, marked as in their rows.
            let value = if counts.updates > 0 {
                let count = i64::try_from(counts.updates).unwrap_or(i64::MAX);
                say.format("store.updates-count", &[Arg::Count(count)])?
            } else {
                number(say, counts.installed)?
            };
            Item::row(&name, &value)
        }
        Row::New => Item::row(&say.copy("store.new")?, more),
        Row::Section(section) => Item::row(&section_name(say, section)?, more),
        Row::Group(group) => {
            let name = group_name(say, group)?;
            // The looks without plugins: only the fonts are there.
            match counts.groups.get(group).copied().unwrap_or(0) {
                0 => Item::row(&name, more),
                n => Item::row(&name, &number(say, n)?),
            }
        }
    }
}

/// The rows of the screen, one for each entry.
///
/// # Errors
/// As [`item`].
pub fn items(say: &mut dyn Say, rows: &[Row], counts: &Counts) -> Result<Vec<Item>, Error> {
    let mut items = Vec::new();
    items
        .try_reserve_exact(rows.len())
        .map_err(|_| Error::NoMemory)?;
    for &row in rows {
        push(&mut items, item(say, row, counts)?)?;
    }
    Ok(items)
}

/// What choosing entry `row` does: *Search* asks what to look for, the others
/// open a list of their own.
///
/// # Errors
/// `NoMemory`, or what the catalog says.
pub fn choose(say: &mut dyn Say, row: Row) -> Result<Next, Error> {
    let which = match row {
        Row::Search => {
            return Ok(Next::Ask {
                tag: SEARCH,
                ask: Ask::Text {
                    title: say.copy("store.search")?,
                    number: false,
                },
            });
        }
        Row::Installed => Which::Installed,
        Row::New => Which::New,
        Row::Section(section) => Which::Section(section),
        Row::Group(group) => Which::Group(group),
    };
    which.open()
}

/// What the text typed for *Search* does: a list of what it finds, nothing for
/// no text.
///
/// # Errors
/// `NoMemory`.
pub fn searched(typed: &str) -> Result<Next, Error> {
    let typed = typed.trim();
    if typed.is_empty() {
        return Ok(Next::None);
    }
    Which::Search(text::copy(typed)?).open()
}

/// What the screen says when it has no rows: its title, its text.
#[must_use]
pub fn empty_words(trouble: Trouble, touch: bool) -> (&'static str, &'static str) {
    match trouble {
        Trouble::Offline if touch => ("store.offline", "store.offline.help"),
        Trouble::Offline => ("store.offline", "store.offline.help.buttons"),
        Trouble::Failed if touch => ("store.unreachable", "store.unreachable.help"),
        Trouble::Failed => ("store.unreachable", "store.unreachable.help.buttons"),
        _ => ("store.loading", ""),
    }
}

/// Whether *Select* (or a tap) tries again: nothing to show for want of the
/// list.
#[must_use]
pub fn retry(empty: bool, trouble: Trouble) -> bool {
    empty && matches!(trouble, Trouble::Offline | Trouble::Failed)
}

/// The screen.
#[derive(Debug, Default)]
pub struct View {
    list: ItemList,
    rows: Vec<Row>,
    counts: Counts,
    /// The lists taken when the rows were made (`Loader::generation`).
    generation: u32,
    /// Whether the list kept was there when they were.
    had_index: bool,
}

impl View {
    /// Counts the plugins anew and makes the rows of them; the focus stays
    /// where it was.
    fn refresh(&mut self, ctx: &Ctx<'_>) -> Result<(), Error> {
        let index = ctx.loader.index();
        let counts = match index {
            Some(index) => count(index, ctx.data, ctx.installs)?,
            None => Counts::default(),
        };
        let rows = if index.is_some() {
            rows(&counts, ctx.installs.has_action(FONTS))?
        } else {
            Vec::new()
        };
        let items = items(&mut HostSay::new(ctx.host)?, &rows, &counts)?;
        self.list.replace(items);
        self.rows = rows;
        self.counts = counts;
        self.generation = ctx.loader.generation();
        self.had_index = index.is_some();
        Ok(())
    }

    /// Whether what the rows were made from has changed.
    fn stale(&self, ctx: &Ctx<'_>) -> bool {
        self.generation != ctx.loader.generation() || self.had_index != ctx.loader.index().is_some()
    }

    fn activate(&self, ctx: &Ctx<'_>, entry: usize) -> Result<Next, Error> {
        match self.rows.get(entry).copied() {
            Some(row) => choose(&mut HostSay::new(ctx.host)?, row),
            None => Ok(Next::None),
        }
    }
}

impl Screen for View {
    fn open(ctx: &Ctx<'_>, _: &[(&str, &str)]) -> Result<Self, Error> {
        let mut screen = Self::default();
        screen.refresh(ctx)?;
        Ok(screen)
    }

    fn shown(&mut self, ctx: &Ctx<'_>) -> Result<(), Error> {
        self.refresh(ctx)
    }

    /// A fresh list is fetched while the one kept shows.
    fn started(&mut self, _: &Ctx<'_>) -> Result<Next, Error> {
        Ok(Next::FetchList)
    }

    fn draw(&mut self, ctx: &Ctx<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        if self.stale(ctx) {
            // A fresh list took the place of the one the rows were made from;
            // counted once, here, if no event said so first.
            self.refresh(ctx)?;
        }
        let trouble = ctx.loader.list_state();
        let mut say = HostSay::new(ctx.host)?;
        // The title says a fresh list is loading while the one kept serves.
        let title = if trouble == Trouble::Loading && self.had_index {
            say.copy("store.loading")?
        } else {
            say.copy("store.title")?
        };
        if !self.rows.is_empty() {
            drop(say);
            return self
                .list
                .draw_with(ctx.host, frame, &title, true, Footer::None);
        }
        let touch = snapshot::capture_snapshot(ctx.host)?.has_touch();
        let (head, help) = empty_words(trouble, touch);
        let head = say.copy(head)?;
        let help = if help.is_empty() {
            String::new()
        } else {
            say.copy(help)?
        };
        drop(say);
        self.list.draw_with(
            ctx.host,
            frame,
            &title,
            true,
            Footer::Empty {
                title: &head,
                text: &help,
            },
        )
    }

    fn input(&mut self, ctx: &Ctx<'_>, input: &Input<'_>) -> Result<(Handled, Next), Error> {
        let step = self.list.input(ctx.host, input)?;
        let next = match step {
            ItemStep::Chosen(entry) => self.activate(ctx, entry)?,
            ItemStep::Ignored if self.rows.is_empty() => {
                // *Select* or a tap where there is nothing: try again.
                let wanted = matches!(
                    *input,
                    Input::Action {
                        action: "core.confirm",
                        ..
                    } | Input::Gesture {
                        kind: paperpaw_sdk::native::callbacks::InputGestureKind::Tap,
                        ..
                    }
                );
                if wanted && retry(true, ctx.loader.list_state()) {
                    return Ok((Handled::Yes, Next::FetchList));
                }
                Next::None
            }
            _ => Next::None,
        };
        Ok((ItemList::handled(step), next))
    }

    fn event(&mut self, ctx: &Ctx<'_>, event: &Event<'_>) -> Result<Next, Error> {
        // A plugin installed, updated or removed, or a fresh list: counted anew.
        if matches!(event, Event::PluginsChanged { .. }) || self.stale(ctx) {
            self.refresh(ctx)?;
            return Ok(Next::Redraw);
        }
        Ok(Next::None)
    }

    fn answered(&mut self, _: &Ctx<'_>, tag: u8, reply: Reply<'_>) -> Result<Next, Error> {
        match (tag, reply) {
            (SEARCH, Reply::Text(typed)) => searched(typed),
            _ => Ok(Next::None),
        }
    }
}
