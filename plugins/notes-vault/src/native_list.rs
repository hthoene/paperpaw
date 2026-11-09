//! The lists of notes: the folders of the vault, the latest changes, the notes
//! of a tag, the results of a search. Each is shown a window of at most 100
//! entries at a time (`native_model::page`), with *Earlier notes* and *More
//! notes* rows at its ends where the list goes on; the model reads the card
//! through `native_card`, so nothing of the vault is held but the window.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Handled;
use paperpaw_sdk::keys::InputMode;
use paperpaw_sdk::native::callbacks::{DrawFrame, Input};
use paperpaw_sdk::native::commands::{CommandSender, RedrawMode};
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::storage::DataStorage;
use paperpaw_sdk::native::{Error, Host};
use paperpaw_ui_kit::text::{concat, copy};
use paperpaw_ui_kit::{Messages, RowItem, RowSource};

use crate::native_card::with_card;
use crate::native_model::index;
use crate::native_model::note::folder_in;
use crate::native_model::page::{self, Order, Row};
use crate::native_model::source::Source;
use crate::native_paged::Paged;

/// Parameter: what the list shows (`folder`, `recent`, `tag`, `search`).
pub const MODE: &str = "mode";
/// Parameter: the folder (`folder`), the tag (`tag`) or the words searched for
/// (`search`).
pub const WHAT: &str = "text";

/// What a list shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Folder,
    Recent,
    Tag,
    Search,
}

impl Mode {
    fn of(name: Option<&str>) -> Self {
        match name {
            Some("recent") => Self::Recent,
            Some("tag") => Self::Tag,
            Some("search") => Self::Search,
            _ => Self::Folder,
        }
    }

    /// What the list says where there is nothing to list.
    const fn empty(self) -> &'static str {
        match self {
            Self::Folder => "notes-vault.empty.folder",
            Self::Recent => "notes-vault.empty.recent",
            Self::Tag => "notes-vault.empty.tag",
            Self::Search => "notes-vault.empty.search",
        }
    }
}

/// What a row of the list is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Line {
    /// *Earlier notes*: the window before.
    Earlier,
    Entry(usize),
    /// *More notes*: the window after.
    More,
    /// What the list says where it has nothing.
    Message,
}

/// What a chosen row asks the plugin to do.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Next {
    Nothing,
    /// Open the folder at this path in a list of its own.
    Folder(String),
    /// Open the note at this path in the reader.
    Note(String),
}

/// A list of notes.
#[derive(Debug)]
pub struct List {
    paged: Paged,
    mode: Mode,
    source: Source,
    order: Order,
    /// The vault's folder, to show where a note is in it.
    root: String,
    title: String,
    rows: Vec<Row>,
    more: bool,
    /// What each window before this began after, from the first.
    before: Vec<Option<Row>>,
    /// What this window began after.
    after: Option<Row>,
    /// There is an index, where the list needs one.
    indexed: bool,
}

/// The mode and what of a handover.
///
/// # Errors
/// [`Error::Invalid`] for a key given twice differently; [`Error::NoMemory`].
pub fn params<'a>(
    params: impl Iterator<Item = Result<(&'a str, &'a str), Error>>,
) -> Result<(Option<String>, String), Error> {
    let (mut mode, mut what): (Option<&str>, Option<&str>) = (None, None);
    for param in params {
        let (key, value) = param?;
        let seen = match key {
            MODE => &mut mode,
            WHAT => &mut what,
            _ => continue,
        };
        if seen.is_some_and(|seen| seen != value) {
            return Err(Error::Invalid);
        }
        *seen = Some(value);
    }
    Ok((mode.map(copy).transpose()?, copy(what.unwrap_or_default())?))
}

impl List {
    /// The list a handover names, its first window read.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals.
    pub fn new(host: &Host<'_>, mode: Option<&str>, what: String) -> Result<Self, Error> {
        let root = crate::native_home::vault(host)?;
        let mode = Mode::of(mode);
        let mut messages = Messages::new()?;
        let (source, order, title) = match mode {
            Mode::Recent => (
                Source::Notes {
                    folder: String::new(),
                    query: String::new(),
                },
                Order::Recent,
                messages.copy(host, "notes-vault.recent")?,
            ),
            Mode::Tag => {
                let title = concat(&["#", &what])?;
                (Source::Tag(what), Order::Name, title)
            }
            Mode::Search => {
                let title = messages.format(
                    host,
                    "notes-vault.search.title",
                    &[Arg::Text {
                        name: "query",
                        value: &what,
                    }],
                )?;
                (
                    Source::Notes {
                        folder: String::new(),
                        query: what,
                    },
                    Order::Name,
                    title,
                )
            }
            Mode::Folder => {
                let dir = if what.is_empty() { copy(&root)? } else { what };
                let title = if dir == root {
                    messages.copy(host, "notes-vault.folders")?
                } else {
                    copy(dir.rsplit('/').next().unwrap_or(&dir))?
                };
                (Source::Folder(dir), Order::Name, title)
            }
        };
        let indexed =
            mode == Mode::Folder || index::kept(&DataStorage::new(host), &root)?.is_some();
        let mut list = Self {
            paged: Paged::new(),
            mode,
            source,
            order,
            root,
            title,
            rows: Vec::new(),
            more: false,
            before: Vec::new(),
            after: None,
            indexed,
        };
        list.load(host)?;
        Ok(list)
    }

    /// Shown again after `last_input`.
    pub fn shown(&mut self, last_input: Option<InputMode>) {
        self.paged.shown(last_input);
    }

    /// Reads the window that follows `self.after`.
    fn load(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if !self.indexed {
            return Ok(());
        }
        let data = DataStorage::new(host);
        let (order, after, source) = (self.order, self.after.as_ref(), &self.source);
        let window = with_card(host, |vault| {
            page::window(order, after, &mut |emit| source.feed(vault, &data, emit))
        })?;
        self.rows = window.rows;
        self.more = window.more;
        Ok(())
    }

    /// The rows the list shows, in order.
    fn lines(&self) -> impl Iterator<Item = Line> + '_ {
        let message = !self.indexed || (self.rows.is_empty() && self.before.is_empty());
        let earlier = !message && !self.before.is_empty();
        let more = !message && self.more;
        message
            .then_some(Line::Message)
            .into_iter()
            .chain(earlier.then_some(Line::Earlier))
            .chain(
                (0..self.rows.len())
                    .filter(move |_| !message)
                    .map(Line::Entry),
            )
            .chain(more.then_some(Line::More))
    }

    fn len(&self) -> usize {
        self.lines().count()
    }

    /// Draws the list.
    ///
    /// # Errors
    /// Allocation, font, canvas and host errors.
    pub fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let mut paged = core::mem::take(&mut self.paged);
        let title = copy(&self.title)?;
        let drawn = paged.draw(host, frame, &title, &mut Rows { list: &*self });
        self.paged = paged;
        drawn
    }

    /// Takes an input; a chosen note or folder is for the plugin to open.
    ///
    /// # Errors
    /// Allocation and host errors; nothing is staged then.
    pub fn input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<(Handled, Next), Error> {
        let (handled, chosen) = self.paged.input(host, self.len(), input)?;
        let Some(line) = chosen.and_then(|index| self.lines().nth(index)) else {
            return Ok((handled, Next::Nothing));
        };
        let next = match line {
            Line::Entry(at) => match self.rows.get(at) {
                Some(row) if row.folder => Next::Folder(copy(&row.path)?),
                Some(row) => Next::Note(copy(&row.path)?),
                None => Next::Nothing,
            },
            Line::More => {
                self.turn(host, true)?;
                Next::Nothing
            }
            Line::Earlier => {
                self.turn(host, false)?;
                Next::Nothing
            }
            Line::Message => Next::Nothing,
        };
        Ok((handled, next))
    }

    /// Goes to the window after (or before) this one and draws anew; where
    /// the new window cannot be read, the one shown stays.
    fn turn(&mut self, host: &Host<'_>, forward: bool) -> Result<(), Error> {
        if forward {
            let Some(last) = self.rows.last() else {
                return Ok(());
            };
            let last = Row::of(&last.as_ref())?;
            self.before.try_reserve(1).map_err(|_| Error::NoMemory)?;
            let previous = self.after.replace(last);
            if let Err(error) = self.load(host) {
                self.after = previous;
                return Err(error);
            }
            self.before.push(previous);
        } else {
            let Some(previous) = self.before.pop() else {
                return Ok(());
            };
            let current = core::mem::replace(&mut self.after, previous);
            if let Err(error) = self.load(host) {
                // The room is there: the entry was just taken out.
                let previous = core::mem::replace(&mut self.after, current);
                self.before.push(previous);
                return Err(error);
            }
        }
        self.paged.set_focus(0);
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
    }
}

/// The rows of the list for a draw.
struct Rows<'a> {
    list: &'a List,
}

impl RowSource for Rows<'_> {
    fn len(&self) -> usize {
        self.list.len()
    }

    fn fill(
        &mut self,
        host: &Host<'_>,
        first: usize,
        max: usize,
        out: &mut Vec<RowItem>,
    ) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        for line in self.list.lines().skip(first).take(max) {
            let row = match line {
                Line::Message => {
                    let key = if self.list.indexed {
                        self.list.mode.empty()
                    } else {
                        "notes-vault.no-index"
                    };
                    plain(messages.copy(host, key)?, String::new(), String::new())
                }
                Line::Earlier => plain(
                    messages.copy(host, "notes-vault.earlier")?,
                    copy("‹")?,
                    String::new(),
                ),
                Line::More => plain(
                    messages.copy(host, "notes-vault.more")?,
                    copy("›")?,
                    String::new(),
                ),
                Line::Entry(at) => {
                    let Some(row) = self.list.rows.get(at) else {
                        continue;
                    };
                    let note = self.list.note_line(host, &mut messages, row)?;
                    plain(
                        copy(&row.title)?,
                        copy(if row.folder { "›" } else { "" })?,
                        note,
                    )
                }
            };
            out.try_reserve(1).map_err(|_| Error::NoMemory)?;
            out.push(row);
        }
        Ok(())
    }
}

fn plain(title: String, value: String, note: String) -> RowItem {
    RowItem {
        title,
        value,
        note,
        ..RowItem::default()
    }
}

impl List {
    /// The line under a note's title: where it is in the vault.
    fn note_line(
        &self,
        host: &Host<'_>,
        messages: &mut Messages,
        row: &Row,
    ) -> Result<String, Error> {
        if row.folder {
            return messages.copy(host, "notes-vault.folder");
        }
        let folder = folder_in(&row.path, &self.root);
        if folder.is_empty() {
            messages.copy(host, "notes-vault.in-vault")
        } else {
            copy(folder)
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::fail_alloc::sweep;
    use alloc::vec;

    fn pairs<'a>(
        list: &'a [(&'a str, &'a str)],
    ) -> impl Iterator<Item = Result<(&'a str, &'a str), Error>> {
        list.iter().map(|pair| Ok(*pair))
    }

    fn row(title: &str) -> Row {
        Row {
            title: title.into(),
            path: alloc::format!("v/{title}.md"),
            folder: false,
            modified: 0,
        }
    }

    /// A list as it is after a window was read.
    fn list(indexed: bool, rows: usize, before: usize, more: bool) -> List {
        List {
            paged: Paged::new(),
            mode: Mode::Folder,
            source: Source::Folder("v".into()),
            order: Order::Name,
            root: "v".into(),
            title: "t".into(),
            rows: (0..rows).map(|n| row(&alloc::format!("n{n}"))).collect(),
            more,
            before: vec![None; before],
            after: None,
            indexed,
        }
    }

    #[test]
    fn the_mode_and_what_are_read_from_the_handover() {
        let named = [("mode", "tag"), ("text", "idea"), ("other", "x")];
        assert_eq!(
            params(pairs(&named)).unwrap(),
            (Some("tag".into()), "idea".into())
        );
        assert_eq!(params(pairs(&[])).unwrap(), (None, String::new()));
        let twice = [("mode", "tag"), ("mode", "recent")];
        assert_eq!(params(pairs(&twice)), Err(Error::Invalid));
        let same = [("text", "a"), ("text", "a")];
        assert_eq!(params(pairs(&same)).unwrap().1, "a");
        let refused = [Ok(("mode", "tag")), Err(Error::Busy)];
        assert_eq!(params(refused.into_iter()), Err(Error::Busy));
    }

    #[test]
    fn reading_the_handover_survives_refused_allocations() {
        let named = [("mode", "search"), ("text", "the words")];
        sweep(|| params(pairs(&named)).map(drop));
    }

    #[test]
    fn a_mode_that_is_unknown_lists_a_folder() {
        assert_eq!(Mode::of(Some("recent")), Mode::Recent);
        assert_eq!(Mode::of(Some("tag")), Mode::Tag);
        assert_eq!(Mode::of(Some("search")), Mode::Search);
        assert_eq!(Mode::of(Some("folder")), Mode::Folder);
        assert_eq!(Mode::of(Some("x")), Mode::Folder);
        assert_eq!(Mode::of(None), Mode::Folder);
    }

    #[test]
    fn the_rows_are_the_entries_with_earlier_and_more_where_the_list_goes_on() {
        let lines = |l: &List| l.lines().collect::<Vec<_>>();
        assert_eq!(
            lines(&list(true, 2, 0, false)),
            [Line::Entry(0), Line::Entry(1)]
        );
        assert_eq!(
            lines(&list(true, 2, 0, true)),
            [Line::Entry(0), Line::Entry(1), Line::More]
        );
        assert_eq!(
            lines(&list(true, 2, 1, true)),
            [Line::Earlier, Line::Entry(0), Line::Entry(1), Line::More]
        );
        assert_eq!(list(true, 2, 1, true).len(), 4);
    }

    #[test]
    fn a_list_with_nothing_or_no_index_says_so_in_one_row() {
        // Nothing in the first window: the message, however the flags stand.
        assert_eq!(
            list(true, 0, 0, false).lines().collect::<Vec<_>>(),
            [Line::Message]
        );
        // No index: the message, whatever is held.
        assert_eq!(
            list(false, 3, 0, true).lines().collect::<Vec<_>>(),
            [Line::Message]
        );
        // Nothing in a later window still lets the way back show.
        assert_eq!(
            list(true, 0, 1, false).lines().collect::<Vec<_>>(),
            [Line::Earlier]
        );
    }
}
