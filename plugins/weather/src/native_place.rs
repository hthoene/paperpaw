//! The place's screen: *Search for a place*, the places found, *No place*
//! while one is set, and the offer to go online (interface-6 code: `place.rs`).
//!
//! The view holds no host handle: the plugin (`native_plugin.rs`) asks the
//! keyboard, searches and writes the settings, as the [`Act`] a key or a tap
//! gives says. What the view keeps is owned data (the places found, how the
//! search went, the offer); the rows are made from it, with the catalog's
//! texts, when the screen is drawn ([`PlaceView::rows`]), and the state and
//! its decisions are tested without a host.
//!
//! Where it differs from interface 6, on purpose: the offer to go online is
//! the kit's dialog of two buttons (interface 6's own widget), and its taps
//! outside the buttons are taken and do nothing; a long press on a row does
//! nothing.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, Input},
        commands::CommandSender,
        formatting::Arg,
    },
};
use paperpaw_ui_kit::{Footer, Item, ItemList, ItemStep, Messages, text};

use crate::native_weather::Place;
use offer::Offer;

mod offer;
#[cfg(test)]
mod tests;

/// The screen's title (a catalog key).
const TITLE: &str = "weather.setting.place";

/// How the search went (what the first row's note says).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Search {
    /// None yet.
    Idle,
    /// Asked for this name.
    Searching(String),
    /// The places are listed.
    Found,
    /// This name found nothing.
    Nothing(String),
    /// The search failed.
    Failed,
    /// There is no network.
    NoNetwork,
}

/// What the plugin has to do after an input.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Act {
    /// Not for the view.
    Ignored,
    /// Taken; nothing to do but draw again.
    Taken,
    /// Ask for the name on the keyboard, this text first.
    Ask(String),
    /// The place at this index of the places found was chosen.
    Pick(usize),
    /// *No place* was chosen.
    Clear,
    /// *Go online* was chosen on the offer; search for the name it waits for.
    Online(String),
    /// *Not now* was chosen on the offer.
    NotNow,
    /// The back arrow.
    Back,
}

/// What a row of the list is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// *Search for a place*.
    Search,
    /// The place at this index of the places found.
    Found(usize),
    /// *No place*.
    None,
}

/// The catalog's texts, made fallibly, so that the rows are made without a
/// host in tests.
pub trait Words {
    /// The text of `key`.
    ///
    /// # Errors
    /// `NoMemory`, or what the host says.
    fn copy(&mut self, key: &str) -> Result<String, Error>;
    /// The text of `key` with its names filled in; `Arg::Count` selects the
    /// plural form.
    ///
    /// # Errors
    /// `NoMemory`, or what the host says.
    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error>;
}

/// The host's catalog.
struct HostWords<'h, 'a> {
    host: &'h Host<'a>,
    messages: Messages,
}

impl Words for HostWords<'_, '_> {
    fn copy(&mut self, key: &str) -> Result<String, Error> {
        self.messages.copy(self.host, key)
    }

    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error> {
        self.messages.format(self.host, key, args)
    }
}

/// The place's screen.
#[derive(Debug)]
pub struct PlaceView {
    places: Vec<Place>,
    search: Search,
    /// The place's name now (trimmed), and whether coordinates are set.
    place: String,
    has_place: bool,
    /// The keyboard was asked for at once (no place was set).
    asked: bool,
    /// The offer to go online, while it is made.
    offer: Option<Offer>,
    /// The rows as the last drawing made them (a focus and the page hits).
    list: ItemList,
    /// Where the focus goes at the next drawing.
    focus: Option<usize>,
}

impl PlaceView {
    /// A view; `place` is the name set (trimmed), `has_place` whether coordinates are set.
    ///
    /// # Errors
    /// `NoMemory`, or what the host says.
    pub fn new(host: &Host<'_>, place: &str, has_place: bool) -> Result<Self, Error> {
        let _ = host;
        Ok(Self {
            places: Vec::new(),
            search: Search::Idle,
            place: text::copy(place)?,
            has_place,
            asked: false,
            offer: None,
            list: ItemList::default(),
            focus: None,
        })
    }

    /// The place's name or its being set changed.
    ///
    /// # Errors
    /// `NoMemory`.
    #[cfg(test)]
    pub fn set_place(&mut self, place: &str, has_place: bool) -> Result<(), Error> {
        if self.place != place {
            self.place = text::copy(place)?;
        }
        self.has_place = has_place;
        Ok(())
    }

    /// Whether the keyboard should open at once (no place is set, and it was not yet).
    pub fn asks_at_once(&mut self) -> bool {
        if self.has_place || self.asked {
            return false;
        }
        self.asked = true;
        true
    }

    /// A search for `name` is under way.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn searching(&mut self, name: String) -> Result<(), Error> {
        self.places.clear();
        self.search = Search::Searching(name);
        self.offer = None;
        self.focus = Some(0);
        Ok(())
    }

    /// The offer to go online is shown, the search for `name` waiting for it.
    pub fn offer(&mut self, name: String) {
        self.offer = Some(Offer::new(name));
    }

    /// The places a search found (none: it found nothing), and how it went.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn found(&mut self, places: Vec<Place>, name: String) -> Result<(), Error> {
        self.search = if places.is_empty() {
            Search::Nothing(name)
        } else {
            Search::Found
        };
        self.focus = Some(usize::from(!places.is_empty()));
        self.places = places;
        self.offer = None;
        Ok(())
    }

    /// The search failed or there is no network.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn ended(&mut self, search: Search) -> Result<(), Error> {
        self.places.clear();
        self.search = search;
        self.offer = None;
        self.focus = Some(0);
        Ok(())
    }

    /// The place at `index` of the places found.
    #[must_use]
    pub fn place(&self, index: usize) -> Option<&Place> {
        self.places.get(index)
    }

    /// The name the keyboard starts with.
    #[must_use]
    pub fn initial(&self) -> &str {
        match &self.search {
            Search::Searching(name) | Search::Nothing(name) => name,
            _ => "",
        }
    }

    /// Whether the offer to go online is shown instead of the list.
    #[cfg(test)]
    #[must_use]
    pub const fn offering(&self) -> bool {
        self.offer.is_some()
    }

    /// The name the offer waits to search for, while it is made.
    #[cfg(test)]
    #[must_use]
    pub fn offer_name(&self) -> Option<&str> {
        self.offer.as_ref().map(Offer::name)
    }

    /// How many rows the list has.
    #[must_use]
    pub fn count(&self) -> usize {
        1 + self.places.len() + usize::from(self.has_place)
    }

    /// What row `index` is.
    #[must_use]
    pub fn row(&self, index: usize) -> Option<Row> {
        if index == 0 {
            Some(Row::Search)
        } else if index <= self.places.len() {
            Some(Row::Found(index - 1))
        } else if index == self.places.len() + 1 && self.has_place {
            Some(Row::None)
        } else {
            None
        }
    }

    /// What the first row tells: the place set, or how the search went.
    ///
    /// # Errors
    /// `NoMemory`, or what the catalog says.
    pub fn note(&self, words: &mut impl Words) -> Result<String, Error> {
        match &self.search {
            Search::Idle if self.place.is_empty() => words.copy("weather.place.help"),
            Search::Idle => words.format(
                "weather.place.now",
                &[Arg::Text {
                    name: "place",
                    value: &self.place,
                }],
            ),
            Search::Searching(_) => words.copy("weather.place.searching"),
            Search::Found => words.format(
                "weather.place.found",
                &[Arg::Count(
                    i64::try_from(self.places.len()).unwrap_or(i64::MAX),
                )],
            ),
            Search::Nothing(name) => words.format(
                "weather.place.nothing",
                &[Arg::Text {
                    name: "name",
                    value: name,
                }],
            ),
            Search::Failed => words.copy("weather.place.failed"),
            Search::NoNetwork => words.copy("weather.place.no-network"),
        }
    }

    /// The rows, with the catalog's texts.
    ///
    /// # Errors
    /// `NoMemory`, or what the catalog says.
    pub fn rows(&self, words: &mut impl Words) -> Result<Vec<Item>, Error> {
        let mut items = Vec::new();
        items
            .try_reserve_exact(self.count())
            .map_err(|_| Error::NoMemory)?;
        let title = words.copy("weather.place.search")?;
        let note = self.note(words)?;
        items.push(Item::noted(&title, "›", &note)?);
        for place in &self.places {
            items.push(Item::noted(&place.name, "", &place.whereabouts()?)?);
        }
        if self.has_place {
            let title = words.copy("weather.place.none")?;
            let note = words.copy("weather.place.none.help")?;
            items.push(Item::noted(&title, "", &note)?);
        }
        Ok(items)
    }

    /// Makes the rows of the list anew, the focus where it was (or where a
    /// search put it).
    ///
    /// # Errors
    /// As [`Self::rows`].
    pub fn refresh(&mut self, words: &mut impl Words) -> Result<(), Error> {
        let items = self.rows(words)?;
        self.list.replace(items);
        if let Some(to) = self.focus.take() {
            self.list.set_focus(to);
        }
        Ok(())
    }

    /// The focused row.
    #[cfg(test)]
    #[must_use]
    pub const fn focus(&self) -> usize {
        self.list.focus()
    }

    /// Draws the screen.
    ///
    /// # Errors
    /// What the host says; `NoMemory`.
    pub fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        if let Some(offer) = self.offer.as_mut() {
            return offer.draw(host, frame, TITLE);
        }
        let mut words = HostWords {
            host,
            messages: Messages::new()?,
        };
        self.refresh(&mut words)?;
        self.list.draw_with(host, frame, TITLE, true, Footer::None)
    }

    /// A key or a tap.
    ///
    /// # Errors
    /// What the host says; `NoMemory`.
    pub fn input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Act, Error> {
        let (act, run) = self.decide(input);
        if let Some(action) = run {
            let mut scratch = [0; 128];
            CommandSender::new(host, &mut scratch).run_action(action)?;
        }
        Ok(act)
    }

    /// What `input` means, and an action of the header (a page) for the host to
    /// run. The list was drawn when its rows are what the reader sees; a row
    /// chosen is read from the state, so that a row of an older drawing is
    /// never taken for another.
    pub fn decide(&mut self, input: &Input<'_>) -> (Act, Option<&'static str>) {
        if let Some(offer) = self.offer.as_mut() {
            let act = offer.decide(input);
            if matches!(act, Act::Online(_) | Act::NotNow) {
                self.offer = None;
            }
            return (act, None);
        }
        match self.list.decide(input) {
            ItemStep::Focus(to) => {
                self.list.set_focus(to);
                (Act::Taken, None)
            }
            ItemStep::Chosen(index) => {
                self.list.set_focus(index);
                (self.open(index), None)
            }
            ItemStep::Header("core.back") => (Act::Back, None),
            ItemStep::Header(action) => (Act::Taken, Some(action)),
            _ => (Act::Ignored, None),
        }
    }

    /// What choosing row `index` does.
    fn open(&self, index: usize) -> Act {
        match self.row(index) {
            Some(Row::Search) => Act::Ask(String::from(self.initial())),
            Some(Row::Found(at)) => Act::Pick(at),
            Some(Row::None) => Act::Clear,
            None => Act::Ignored,
        }
    }

    /// Whether `act` was taken.
    #[must_use]
    pub const fn handled(act: &Act) -> Handled {
        if matches!(act, Act::Ignored) {
            Handled::No
        } else {
            Handled::Yes
        }
    }
}
