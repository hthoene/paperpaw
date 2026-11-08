//! The place's screen: *Search for a place*, the places found, *No place*
//! while one is set, and the offer to go online. Owner: the place-screen
//! port. The interface-6 code is `place.rs`. The view holds no host handle:
//! the plugin (`native_plugin.rs`) asks the keyboard, searches, and writes the
//! settings, as the [`Act`] a key or a tap gives says.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host,
        callbacks::{DrawFrame, Input},
    },
};

use crate::native_weather::Place;

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
    /// Taken; nothing to do but draw again (the view asked for it).
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

/// The place's screen.
#[derive(Debug)]
pub struct PlaceView {
    places: Vec<Place>,
    search: Search,
}

impl PlaceView {
    /// A view; `place` is the name set (trimmed), `has_place` whether coordinates are set.
    ///
    /// # Errors
    /// `NoMemory`, or what the host says.
    pub fn new(host: &Host<'_>, place: &str, has_place: bool) -> Result<Self, Error> {
        let _ = (host, place, has_place);
        Ok(Self {
            places: Vec::new(),
            search: Search::Idle,
        })
    }

    /// The place's name or its being set changed.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn set_place(&mut self, place: &str, has_place: bool) -> Result<(), Error> {
        let _ = (place, has_place);
        Ok(())
    }

    /// Whether the keyboard should open at once (no place is set, and it was not yet).
    pub fn asks_at_once(&mut self) -> bool {
        false
    }

    /// A search for `name` is under way.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn searching(&mut self, name: String) -> Result<(), Error> {
        self.places.clear();
        self.search = Search::Searching(name);
        Ok(())
    }

    /// The offer to go online is shown, the search for `name` waiting for it.
    pub fn offer(&mut self, name: String) {
        let _ = name;
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
        self.places = places;
        Ok(())
    }

    /// The search failed or there is no network.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn ended(&mut self, search: Search) -> Result<(), Error> {
        self.places.clear();
        self.search = search;
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

    /// Draws the screen.
    ///
    /// # Errors
    /// What the host says; `NoMemory`.
    pub fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let _ = (host, frame);
        Err(Error::Unsupported)
    }

    /// A key or a tap.
    ///
    /// # Errors
    /// What the host says; `NoMemory`.
    pub fn input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Act, Error> {
        let _ = (host, input);
        Ok(Act::Ignored)
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
