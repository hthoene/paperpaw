//! The vault's page: where to go (folders, latest changes, tags, search), and
//! the index, made in the background a piece at a time while the page is open
//! (the work service calls `work` again and again; see `PORTING-7.md`).
//!
//! The model of what is on the page and what the index is doing is here; the
//! screens around it (opening a list, asking for the search words, choosing the
//! folder) are the plugin's (`native_plugin.rs`), which `input` tells by
//! [`Next`].
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::keys::InputMode;
use paperpaw_sdk::native::callbacks::{DrawFrame, Input};
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::storage::DataStorage;
use paperpaw_sdk::native::{Error, Host, clock, settings, snapshot};
use paperpaw_sdk::{Handled, Storage};
use paperpaw_ui_kit::{Messages, RowItem, RowSource, text::copy};

use crate::native_card::with_card;
use crate::native_model::index::{self, Indexer, State, Step, Tagger};
use crate::native_paged::Paged;

/// The setting that names the vault's folder.
pub const FOLDER: &str = "notes-vault.folder";
/// An index older than this, in seconds, is made anew when the page opens.
const REFRESH: i64 = 6 * 3600;
/// How long one call of `work` reads the card (microseconds): the index is made
/// in such slices, between which the device answers its buttons.
const SLICE_US: u64 = 200_000;
/// Room for the vault's folder as the setting holds it.
const FOLDER_BYTES: usize = 1100;

/// A row of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    /// No vault is chosen: choose its folder.
    Choose,
    Folders,
    Recent,
    Tags,
    Search,
    Update,
}

const CHOOSE: [Item; 1] = [Item::Choose];
const ALL: [Item; 5] = [
    Item::Folders,
    Item::Recent,
    Item::Tags,
    Item::Search,
    Item::Update,
];

/// What a chosen row asks the plugin to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Next {
    Nothing,
    /// Choose the vault's folder.
    Choose,
    /// The vault's folders.
    Folders,
    /// The latest changes.
    Recent,
    /// The tags.
    Tags,
    /// Ask for the words to search for.
    Search,
    /// The index is not there yet: say so.
    Wait,
    /// The index is made anew: the work is to be declared.
    Work,
}

/// What the background work is doing.
#[derive(Debug)]
enum Run {
    Idle,
    Indexing(Indexer),
    Tagging(Tagger),
}

/// Whether the work has more to do after a slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slice {
    /// Call `work` again.
    More,
    /// Nothing left to do.
    Over,
}

/// The vault's folder as the setting holds it, without slashes at its ends;
/// empty where none is chosen.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn vault(host: &Host<'_>) -> Result<String, Error> {
    let mut scratch = [0u8; FOLDER_BYTES];
    match settings::read_setting(host, FOLDER, &mut scratch)? {
        settings::SettingValue::Text(text) => copy(text.trim().trim_matches('/')),
        _ => Ok(String::new()),
    }
}

/// The page.
#[derive(Debug)]
pub struct Home {
    paged: Paged,
    vault: String,
    /// The index kept for the vault.
    state: Option<State>,
    run: Run,
    failed: bool,
}

impl Home {
    /// The page for the vault now chosen, with the index it has; the index is
    /// begun where there is none or it is old.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals.
    pub fn new(host: &Host<'_>) -> Result<Self, Error> {
        let mut home = Self {
            paged: Paged::new(),
            vault: String::new(),
            state: None,
            run: Run::Idle,
            failed: false,
        };
        home.refresh(host)?;
        Ok(home)
    }

    /// Whether `work` has something to do.
    #[must_use]
    pub const fn running(&self) -> bool {
        !matches!(self.run, Run::Idle)
    }

    /// Shown again after `last_input`: the vault's folder may have been changed
    /// in the settings, and the index may be due. Returns whether the work is to
    /// be declared.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals.
    pub fn shown(&mut self, host: &Host<'_>, last_input: Option<InputMode>) -> Result<bool, Error> {
        self.paged.shown(last_input);
        self.refresh(host)?;
        Ok(self.running())
    }

    fn refresh(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let now = vault(host)?;
        let data = DataStorage::new(host);
        if now != self.vault {
            // Another vault: its own index.
            self.state = index::kept(&data, &now)?;
            self.vault = now;
            self.run = Run::Idle;
            self.failed = false;
        }
        if matches!(self.run, Run::Idle) && !self.vault.is_empty() {
            if self.due(host)? {
                self.index_now()?;
            } else if self.state.as_ref().is_some_and(|state| !state.tagged) {
                self.run = Run::Tagging(Tagger::new());
            }
        }
        Ok(())
    }

    /// Whether the index is to be made anew now: there is none, or it is old.
    fn due(&self, host: &Host<'_>) -> Result<bool, Error> {
        let Some(state) = &self.state else {
            return Ok(true);
        };
        Ok(match (unix_seconds(host)?, state.built) {
            (Some(now), built) if built > 0 => now.saturating_sub(built) > REFRESH,
            _ => false,
        })
    }

    /// Begins the index anew.
    fn index_now(&mut self) -> Result<(), Error> {
        self.run = Run::Indexing(Indexer::new(&self.vault)?);
        self.failed = false;
        Ok(())
    }

    fn items(&self) -> &'static [Item] {
        if self.vault.is_empty() { &CHOOSE } else { &ALL }
    }

    /// What choosing the row `index` asks for.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn chosen(&mut self, index: usize) -> Result<Next, Error> {
        let Some(item) = self.items().get(index).copied() else {
            return Ok(Next::Nothing);
        };
        Ok(match item {
            Item::Choose => Next::Choose,
            Item::Folders => Next::Folders,
            Item::Recent | Item::Tags | Item::Search if self.state.is_none() => Next::Wait,
            Item::Recent => Next::Recent,
            Item::Tags => Next::Tags,
            Item::Search => Next::Search,
            Item::Update => {
                self.index_now()?;
                Next::Work
            }
        })
    }

    /// Draws the page.
    ///
    /// # Errors
    /// Allocation, font, canvas and host errors.
    pub fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let mut paged = core::mem::take(&mut self.paged);
        let drawn = paged.draw(host, frame, "notes-vault.title", &mut Rows { home: &*self });
        self.paged = paged;
        drawn
    }

    /// Takes an input; what a chosen row asks for comes with it.
    ///
    /// # Errors
    /// Allocation and host errors; nothing is staged then.
    pub fn input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<(Handled, Next), Error> {
        let (handled, chosen) = self.paged.input(host, self.items().len(), input)?;
        match chosen {
            Some(index) => Ok((handled, self.chosen(index)?)),
            None => Ok((handled, Next::Nothing)),
        }
    }

    /// Does one slice of the index: reads the card for [`SLICE_US`], or until
    /// the index is made. The tags follow the index; a slice of either may end
    /// the work.
    ///
    /// # Errors
    /// The host's refusal of the clock or the snapshot; a refusal while the
    /// card is read (or a refused allocation) ends the work and is told by the
    /// page instead.
    pub fn work(&mut self, host: &Host<'_>) -> Result<Slice, Error> {
        if matches!(self.run, Run::Idle) {
            return Ok(Slice::Over);
        }
        let started = clock::micros(host)?;
        let now = unix_seconds(host)?;
        let data = DataStorage::new(host);
        let stepped = with_card(host, |vault| {
            loop {
                if !self.step(vault, &data, now)? {
                    return Ok(Slice::Over);
                }
                if clock::micros(host)?.saturating_sub(started) >= SLICE_US {
                    return Ok(Slice::More);
                }
            }
        });
        // A refusal ends the work: what was being made is dropped, the index
        // kept stays, and the page says the index could not be made.
        Ok(stepped.unwrap_or_else(|_| {
            self.run = Run::Idle;
            self.failed = true;
            Slice::Over
        }))
    }

    /// One step; whether there are more.
    fn step(
        &mut self,
        vault: &dyn Storage,
        data: &DataStorage<'_, '_>,
        now: Option<i64>,
    ) -> Result<bool, Error> {
        match &mut self.run {
            Run::Idle => Ok(false),
            Run::Indexing(indexer) => match indexer.step(vault, data)? {
                Step::More => Ok(true),
                Step::Done(state) => {
                    self.indexed(state, data, now)?;
                    Ok(self.running())
                }
                Step::Failed => self.fail(),
            },
            Run::Tagging(tagger) => match tagger.step(vault, data)? {
                Some(false) => Ok(true),
                Some(true) => {
                    self.run = Run::Idle;
                    if let Some(state) = &mut self.state {
                        state.tagged = true;
                        state.write(data)?;
                    }
                    Ok(false)
                }
                None => self.fail(),
            },
        }
    }

    fn fail(&mut self) -> Result<bool, Error> {
        self.run = Run::Idle;
        self.failed = true;
        Ok(false)
    }

    /// The index is made: kept, and its tags next where they are not.
    fn indexed(
        &mut self,
        mut state: State,
        data: &DataStorage<'_, '_>,
        now: Option<i64>,
    ) -> Result<(), Error> {
        state.built = now.unwrap_or(0);
        // The notes are as they were: the tags made are still right.
        let same = self
            .state
            .as_ref()
            .is_some_and(|old| old.stamp == state.stamp && old.folder == state.folder)
            && data.exists(index::TAGS);
        state.tagged = same && self.state.as_ref().is_some_and(|old| old.tagged);
        state.write(data)?;
        self.run = if state.tagged {
            Run::Idle
        } else {
            Run::Tagging(Tagger::new())
        };
        self.state = Some(state);
        Ok(())
    }

    /// What the page says of the index: how far it is, or what it has.
    fn status(&self, host: &Host<'_>, messages: &mut Messages) -> Result<String, Error> {
        match (&self.run, &self.state) {
            (Run::Indexing(indexer), _) => messages.format(
                host,
                "notes-vault.status.indexing",
                &[Arg::Int {
                    name: "notes",
                    value: i64::from(indexer.notes),
                }],
            ),
            (Run::Tagging(tagger), Some(state)) => messages.format(
                host,
                "notes-vault.status.tags",
                &[
                    Arg::Int {
                        name: "done",
                        value: i64::from(tagger.done),
                    },
                    Arg::Int {
                        name: "notes",
                        value: i64::from(state.notes),
                    },
                ],
            ),
            _ if self.failed => messages.copy(host, "notes-vault.status.failed"),
            (_, None) => messages.copy(host, "notes-vault.status.none"),
            (_, Some(state)) => self.ready(host, messages, state),
        }
    }

    fn ready(
        &self,
        host: &Host<'_>,
        messages: &mut Messages,
        state: &State,
    ) -> Result<String, Error> {
        let notes = Arg::Int {
            name: "notes",
            value: i64::from(state.notes),
        };
        let Some(now) = unix_seconds(host)?.filter(|_| state.built > 0) else {
            return messages.format(host, "notes-vault.status.ready", &[notes]);
        };
        let hours = (now.saturating_sub(state.built) / 3600).max(0);
        let age = match hours {
            0 => messages.copy(host, "notes-vault.age.now")?,
            1..48 => messages.format(host, "notes-vault.age.hours", &[Arg::Count(hours)])?,
            _ => messages.format(host, "notes-vault.age.days", &[Arg::Count(hours / 24)])?,
        };
        messages.format(
            host,
            "notes-vault.status.ready-age",
            &[
                notes,
                Arg::Text {
                    name: "age",
                    value: &age,
                },
            ],
        )
    }
}

/// The wall clock in unix seconds, where the device knows it.
fn unix_seconds(host: &Host<'_>) -> Result<Option<i64>, Error> {
    Ok(snapshot::capture_snapshot(host)?.unix_seconds())
}

/// The rows of the page for a draw.
struct Rows<'a> {
    home: &'a Home,
}

impl RowSource for Rows<'_> {
    fn len(&self) -> usize {
        self.home.items().len()
    }

    fn fill(
        &mut self,
        host: &Host<'_>,
        first: usize,
        max: usize,
        out: &mut Vec<RowItem>,
    ) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        for item in self.home.items().iter().skip(first).take(max) {
            let (key, note, value) = match item {
                Item::Choose => (
                    "notes-vault.choose",
                    messages.copy(host, "notes-vault.choose.help")?,
                    "›",
                ),
                Item::Folders => (
                    "notes-vault.folders",
                    copy(self.home.vault.rsplit('/').next().unwrap_or_default())?,
                    "›",
                ),
                Item::Recent => ("notes-vault.recent", String::new(), "›"),
                Item::Tags => ("notes-vault.tags", String::new(), "›"),
                Item::Search => ("notes-vault.search", String::new(), "›"),
                Item::Update => (
                    "notes-vault.update",
                    self.home.status(host, &mut messages)?,
                    "",
                ),
            };
            out.try_reserve(1).map_err(|_| Error::NoMemory)?;
            out.push(RowItem {
                title: messages.copy(host, key)?,
                value: copy(value)?,
                note,
                ..RowItem::default()
            });
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::fail_alloc::sweep;

    fn home(vault: &str, indexed: bool) -> Home {
        Home {
            paged: Paged::new(),
            vault: vault.into(),
            state: indexed.then(|| State {
                folder: vault.into(),
                built: 0,
                notes: 3,
                stamp: 1,
                tagged: true,
            }),
            run: Run::Idle,
            failed: false,
        }
    }

    #[test]
    fn without_a_vault_the_page_asks_for_its_folder() {
        let mut page = home("", false);
        assert_eq!(page.items(), [Item::Choose]);
        assert_eq!(page.chosen(0).unwrap(), Next::Choose);
        assert_eq!(page.chosen(1).unwrap(), Next::Nothing);
    }

    #[test]
    fn folders_open_at_once_the_index_lists_wait_for_it() {
        let mut page = home("v", false);
        assert_eq!(page.items().len(), 5);
        assert_eq!(page.chosen(0).unwrap(), Next::Folders);
        for index in 1..=3 {
            assert_eq!(page.chosen(index).unwrap(), Next::Wait, "{index}");
        }
        let mut page = home("v", true);
        assert_eq!(page.chosen(1).unwrap(), Next::Recent);
        assert_eq!(page.chosen(2).unwrap(), Next::Tags);
        assert_eq!(page.chosen(3).unwrap(), Next::Search);
    }

    #[test]
    fn updating_begins_the_index_anew_and_asks_for_the_work() {
        let mut page = home("v", true);
        page.failed = true;
        assert!(!page.running());
        assert_eq!(page.chosen(4).unwrap(), Next::Work);
        assert!(page.running());
        assert!(!page.failed);
        assert!(matches!(page.run, Run::Indexing(_)));
    }

    #[test]
    fn starting_the_index_survives_refused_allocations() {
        // The page is made outside: only the plugin's own allocations count.
        let mut page = home("vault/notes", true);
        sweep(|| page.index_now());
    }
}
