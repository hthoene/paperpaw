//! The store's screens (slice 3): what the reader sees and does of the library,
//! host-light. A screen is a [`Screen`]: it is opened from the parameters of
//! its declaration, draws itself, takes the reader's input and the events that
//! concern it, and answers with a [`Next`] for the plugin to carry out
//! (`native_plugin`, Port A's wiring): open another screen, start a fetch,
//! install, ask. What a screen reads comes through [`Ctx`]: the host (texts,
//! drawing), the data folder (the index and the files kept on the card), the
//! loader (the list and the pages fetched) and the installs (what is going on).
//!
//! Memory (#248): a screen builds its rows when it opens or comes on top, never
//! at the start; lists are read from the index on the card a page at a time;
//! nothing here is kept between screens.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::Handled;
use paperpaw_sdk::Storage;
use paperpaw_sdk::native::{
    Error, Host,
    callbacks::{DrawFrame, Input},
    events::Event,
};

use crate::native_index::Index;
use crate::native_list::Page;

/// Why the list is not there (what the loader says of it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trouble {
    /// A list is kept: nothing is wrong.
    None,
    /// A list is being fetched.
    Loading,
    /// There is no network.
    Offline,
    /// The fetch failed, or what came is no list.
    Failed,
}

/// What the loader holds: the index kept on the card and the pages fetched.
pub trait Loader {
    /// How the list stands.
    fn list_state(&self) -> Trouble;
    /// The index kept, read once (the screens read entries through it).
    fn index(&self) -> Option<&Index>;
    /// The page of plugin `id`, when it was fetched.
    fn page(&self, id: &str) -> Option<&Page>;
}

/// How the install of a plugin stands (Port E's steps).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Nothing is going on.
    Idle,
    /// Downloading: bytes done and in all (0 where not known).
    Downloading { done: u32, total: u32 },
    /// The host is installing it.
    Installing,
    /// It was installed (a restart may be waiting).
    Installed,
    /// It ended in this reason (`device.md` 5.5).
    Failed(u32),
}

/// What is going on with the installs.
pub trait Installs {
    /// The step of plugin `id`.
    fn step(&self, id: &str) -> Step;
    /// Whether the queue holds plugins to fetch and install (*Update all*).
    fn queued(&self) -> bool;
    /// Whether plugin `id` was installed and works only after a restart (a
    /// pack of letters is a data plugin and works at once). Added with the
    /// fetch screen; where the installs cannot tell, it is `false`.
    fn restart_needed(&self, id: &str) -> bool {
        let _ = id;
        false
    }
}

/// What a screen may read.
pub struct Ctx<'a> {
    /// The host: texts, drawing, commands.
    pub host: &'a Host<'a>,
    /// The plugin's data folder.
    pub data: &'a dyn Storage,
    /// The list and the pages.
    pub loader: &'a dyn Loader,
    /// The installs.
    pub installs: &'a dyn Installs,
}

impl core::fmt::Debug for Ctx<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Ctx").finish_non_exhaustive()
    }
}

/// The declared screens of the store (`declarations::screens`): the id of each
/// is `store.<name>`.
pub mod ids {
    /// The start: the library's views and the things to do.
    pub const LIST: &str = "store.list";
    /// A view: the plugins of a section, in an order, or a search.
    pub const VIEW: &str = "store.view";
    /// The letters of a view, to jump to.
    pub const LETTERS: &str = "store.letters";
    /// Where the library is read from.
    pub const SOURCE: &str = "store.source";
    /// One plugin's page.
    pub const PLUGIN: &str = "store.plugin";
    /// A plugin's pictures.
    pub const SHOTS: &str = "store.shots";
    /// The progress of a fetch of the list or of *Update all*.
    pub const FETCH: &str = "store.fetch";
}

/// What the plugin carries out for a screen.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Next {
    /// Nothing.
    None,
    /// Open the screen `screen` with these parameters (name, value).
    Open {
        screen: &'static str,
        params: Vec<(&'static str, String)>,
    },
    /// Close this screen.
    Close,
    /// Fetch the list of plugins.
    FetchList,
    /// Fetch the page of plugin `id`.
    FetchPage(String),
    /// Install plugin `id`.
    Install(String),
    /// Update plugin `id`.
    Update(String),
    /// Remove plugin `id`.
    Remove(String),
    /// Stop the install of plugin `id`.
    Stop(String),
    /// Update every plugin that has a newer version.
    UpdateAll,
    /// Say text `key` of the catalog, once.
    Notify(&'static str),
    /// Close this screen, then say text `key` of the catalog, once.
    CloseNotify(&'static str),
    /// Fetch again the plugins `ids` that plugin `by` asked for, after a
    /// download that did not end well: the request is kept anew
    /// (`native_request::Request::again`) and its plugins are queued as
    /// asked for (`native_queue::Mark::Requested`), then the queue goes on.
    Again { by: String, ids: Vec<String> },
    /// Restart PaperPaw.
    Restart,
    /// Redraw this screen.
    Redraw,
}

/// A screen of the store.
pub trait Screen: Sized {
    /// Opens the screen from the parameters of its declaration (name, value).
    ///
    /// # Errors
    /// `NoMemory`, or `Invalid` where a parameter is none.
    fn open(ctx: &Ctx<'_>, params: &[(&str, &str)]) -> Result<Self, Error>;

    /// The screen is on top again: what it shows may have changed on the card.
    ///
    /// # Errors
    /// `NoMemory`.
    fn shown(&mut self, ctx: &Ctx<'_>) -> Result<(), Error> {
        let _ = ctx;
        Ok(())
    }

    /// The screen is on top again, and what to do about it: where what it
    /// shows ended while it was covered, it closes. The default is
    /// [`shown`](Self::shown) and nothing to do; the plugin calls this where
    /// it called `shown`.
    ///
    /// # Errors
    /// `NoMemory`.
    fn resume(&mut self, ctx: &Ctx<'_>) -> Result<Next, Error> {
        self.shown(ctx)?;
        Ok(Next::None)
    }

    /// Another screen covers this one: events that end its job wait for
    /// [`resume`](Self::resume). The default does nothing.
    fn hidden(&mut self) {}

    /// Draws the screen.
    ///
    /// # Errors
    /// `NoMemory`, or what the host refuses.
    fn draw(&mut self, ctx: &Ctx<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error>;

    /// Takes an input; says whether it was used, and what to do.
    ///
    /// # Errors
    /// `NoMemory`, or what the host refuses.
    fn input(&mut self, ctx: &Ctx<'_>, input: &Input<'_>) -> Result<(Handled, Next), Error>;

    /// An event that may concern the screen (an answer, a download's end, an
    /// install's step).
    ///
    /// # Errors
    /// `NoMemory`, or what the host refuses.
    fn event(&mut self, ctx: &Ctx<'_>, event: &Event<'_>) -> Result<Next, Error> {
        let _ = (ctx, event);
        Ok(Next::None)
    }
}

/// An open screen of the store.
#[derive(Debug)]
#[non_exhaustive]
pub enum StoreView {
    /// The start.
    List(crate::native_screen_list::View),
    /// A view.
    View(crate::native_screen_view::View),
    /// The letters.
    Letters(crate::native_screen_letters::View),
    /// Where the library is read from.
    Source(crate::native_screen_source::View),
    /// A plugin's page.
    Plugin(crate::native_screen_page::View),
    /// A plugin's pictures.
    Shots(crate::native_screen_shots::View),
    /// A fetch's progress.
    Fetch(crate::native_screen_fetch::View),
}

impl StoreView {
    /// Opens the declared screen `screen`.
    ///
    /// # Errors
    /// `NotFound` for a screen that is none of the store's, else what the
    /// screen's own `open` says.
    pub fn open(ctx: &Ctx<'_>, screen: &str, params: &[(&str, &str)]) -> Result<Self, Error> {
        Ok(match screen {
            ids::LIST => Self::List(Screen::open(ctx, params)?),
            ids::VIEW => Self::View(Screen::open(ctx, params)?),
            ids::LETTERS => Self::Letters(Screen::open(ctx, params)?),
            ids::SOURCE => Self::Source(Screen::open(ctx, params)?),
            ids::PLUGIN => Self::Plugin(Screen::open(ctx, params)?),
            ids::SHOTS => Self::Shots(Screen::open(ctx, params)?),
            ids::FETCH => Self::Fetch(Screen::open(ctx, params)?),
            _ => return Err(Error::NotFound),
        })
    }

    /// The screen is on top again.
    ///
    /// # Errors
    /// What the screen says.
    pub fn shown(&mut self, ctx: &Ctx<'_>) -> Result<(), Error> {
        match self {
            Self::List(v) => v.shown(ctx),
            Self::View(v) => v.shown(ctx),
            Self::Letters(v) => v.shown(ctx),
            Self::Source(v) => v.shown(ctx),
            Self::Plugin(v) => v.shown(ctx),
            Self::Shots(v) => v.shown(ctx),
            Self::Fetch(v) => v.shown(ctx),
        }
    }

    /// The screen is on top again, and what to do about it.
    ///
    /// # Errors
    /// What the screen says.
    pub fn resume(&mut self, ctx: &Ctx<'_>) -> Result<Next, Error> {
        match self {
            Self::List(v) => v.resume(ctx),
            Self::View(v) => v.resume(ctx),
            Self::Letters(v) => v.resume(ctx),
            Self::Source(v) => v.resume(ctx),
            Self::Plugin(v) => v.resume(ctx),
            Self::Shots(v) => v.resume(ctx),
            Self::Fetch(v) => v.resume(ctx),
        }
    }

    /// Another screen covers this one.
    pub fn hidden(&mut self) {
        match self {
            Self::List(v) => v.hidden(),
            Self::View(v) => v.hidden(),
            Self::Letters(v) => v.hidden(),
            Self::Source(v) => v.hidden(),
            Self::Plugin(v) => v.hidden(),
            Self::Shots(v) => v.hidden(),
            Self::Fetch(v) => v.hidden(),
        }
    }

    /// Draws the screen.
    ///
    /// # Errors
    /// What the screen says.
    pub fn draw(&mut self, ctx: &Ctx<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        match self {
            Self::List(v) => v.draw(ctx, frame),
            Self::View(v) => v.draw(ctx, frame),
            Self::Letters(v) => v.draw(ctx, frame),
            Self::Source(v) => v.draw(ctx, frame),
            Self::Plugin(v) => v.draw(ctx, frame),
            Self::Shots(v) => v.draw(ctx, frame),
            Self::Fetch(v) => v.draw(ctx, frame),
        }
    }

    /// Takes an input.
    ///
    /// # Errors
    /// What the screen says.
    pub fn input(&mut self, ctx: &Ctx<'_>, input: &Input<'_>) -> Result<(Handled, Next), Error> {
        match self {
            Self::List(v) => v.input(ctx, input),
            Self::View(v) => v.input(ctx, input),
            Self::Letters(v) => v.input(ctx, input),
            Self::Source(v) => v.input(ctx, input),
            Self::Plugin(v) => v.input(ctx, input),
            Self::Shots(v) => v.input(ctx, input),
            Self::Fetch(v) => v.input(ctx, input),
        }
    }

    /// An event for the screen.
    ///
    /// # Errors
    /// What the screen says.
    pub fn event(&mut self, ctx: &Ctx<'_>, event: &Event<'_>) -> Result<Next, Error> {
        match self {
            Self::List(v) => v.event(ctx, event),
            Self::View(v) => v.event(ctx, event),
            Self::Letters(v) => v.event(ctx, event),
            Self::Source(v) => v.event(ctx, event),
            Self::Plugin(v) => v.event(ctx, event),
            Self::Shots(v) => v.event(ctx, event),
            Self::Fetch(v) => v.event(ctx, event),
        }
    }
}
