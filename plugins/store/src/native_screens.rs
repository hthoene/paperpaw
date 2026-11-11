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
use crate::native_page::Installed;

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
    /// The libraries have nothing made for this device.
    Unsupported,
}

/// What the loader holds: the index kept on the card and the pages fetched.
pub trait Loader {
    /// How the list stands.
    fn list_state(&self) -> Trouble;
    /// The index kept, read once (the screens read entries through it).
    fn index(&self) -> Option<&Index>;
    /// The page of plugin `id`, when it was fetched.
    fn page(&self, id: &str) -> Option<&Page>;
    /// Counts the lists taken: it changes each time a fresh list takes the
    /// place of the one kept, so a screen that picked its plugins from the
    /// list before picks them again. (Addition of the screens' port; the
    /// default says the list never changes.)
    fn generation(&self) -> u32 {
        0
    }
    /// How the page of plugin `id` stands (a screen that waits for it says so):
    /// here, still coming (the default where it is not here), offline, or
    /// failed (the library no longer offers it).
    fn page_state(&self, id: &str) -> Trouble {
        if self.page(id).is_some() {
            Trouble::None
        } else {
            Trouble::Loading
        }
    }
    /// Whether the device has little memory (the pictures of a page are then
    /// kept small). Unknown reads as small.
    fn small_memory(&self) -> bool {
        true
    }
}

/// The store's loader (`native_load::Loader`, slice 1) as the screens read it.
/// The plugin makes it for each call: `generation` is its count of the lists
/// taken (it goes up when `native_load::Ended::List` came), `small_memory`
/// what the wire says.
#[derive(Debug, Clone, Copy)]
pub struct LoaderView<'a> {
    /// The loader.
    pub loader: &'a crate::native_load::Loader,
    /// Counts the lists taken.
    pub generation: u32,
    /// Whether the device has little memory.
    pub small_memory: bool,
}

impl Loader for LoaderView<'_> {
    fn list_state(&self) -> Trouble {
        use crate::native_load::Trouble as Load;
        match self.loader.trouble() {
            Load::Loading => Trouble::Loading,
            Load::Offline => Trouble::Offline,
            Load::Failed => Trouble::Failed,
            Load::Unsupported => Trouble::Unsupported,
            _ => Trouble::None,
        }
    }

    fn index(&self) -> Option<&Index> {
        self.loader.index()
    }

    fn page(&self, id: &str) -> Option<&Page> {
        self.loader.page(id)
    }

    fn generation(&self) -> u32 {
        self.generation
    }

    fn page_state(&self, id: &str) -> Trouble {
        use crate::native_load::Details;
        match self.loader.details() {
            (Some(of), Details::Offline) if of == id => Trouble::Offline,
            (Some(of), Details::Failed) if of == id => Trouble::Failed,
            (Some(of), Details::Here) if of == id => Trouble::None,
            _ if self.loader.page(id).is_some() => Trouble::None,
            _ => Trouble::Loading,
        }
    }

    fn small_memory(&self) -> bool {
        self.small_memory
    }
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
    /// What the device has of plugin `id`, if anything: its kind, whether it
    /// runs, its version and who put it there (interface 6's
    /// `env.plugins().get(id)` with the origins). The lists mark the plugins
    /// installed or with an update by it. The default says nothing is
    /// installed. (Addition of the screens' port.)
    fn have(&self, id: &str) -> Option<Installed<'_>> {
        let _ = id;
        None
    }
    /// Whether the device takes betas as updates. (Addition of the screens'
    /// port.)
    fn takes_betas(&self) -> bool {
        false
    }
    /// Whether action `action` is registered (interface 6's
    /// `env.actions().contains`): the *Fonts* row of the looks shows where the
    /// fonts browser's is. (Addition of the screens' port.)
    fn has_action(&self, action: &str) -> bool {
        let _ = action;
        false
    }
    /// Whether plugin `id` waits its turn in the queue. (Addition of the
    /// screens' port.)
    fn waiting(&self, id: &str) -> bool {
        let _ = id;
        false
    }
    /// Whether the kernel stopped plugin `id`: the key its author signed it
    /// with was withdrawn.
    fn revoked(&self, id: &str) -> bool {
        let _ = id;
        false
    }
    /// Whether a check (`check_file`) said there is no room for plugin `id`.
    fn no_room(&self, id: &str) -> bool {
        let _ = id;
        false
    }
    /// The name of the plugin whose install goes on, when it is not `id`.
    fn other_job(&self, id: &str) -> Option<&str> {
        let _ = id;
        None
    }
    /// The device runs the stock layout of the code flash (*no room* says the
    /// full layout gives much more).
    fn flash_hint(&self) -> bool {
        false
    }
    /// Whether the device loads code plugins and there is a network, so a
    /// plugin of the library can be added (the first-run offer reads it). The
    /// default says no. (Addition of the screens' port.)
    fn can_install(&self) -> bool {
        false
    }
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
    /// The first-run offer of a few plugins (the setup's page).
    pub const OFFER: &str = "store.offer";
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
    /// Ask the reader something; the answer comes back to this screen through
    /// [`Screen::answered`] with `tag`. (Addition of the screens' port.)
    Ask { tag: u8, ask: Ask },
    /// Close this screen and hand `value` (named `key`) to the screen below
    /// through [`Screen::returned`]. (Addition of the screens' port.)
    CloseWith { key: &'static str, value: String },
    /// Show the text file `path` of the data folder in the reader.
    Read(String),
    /// Switch on the built-in plugin `id`.
    SwitchOn(String),
    /// Say text `key` of the catalog with `name` for its `{name}`, once.
    Said { key: &'static str, name: String },
    /// The plugins chosen on the first-run offer: switch on the built-in ones
    /// `switch_on` (`set_plugin_enabled`), start the queue where `queued`
    /// (they were put in it quietly, `native_ask::keep_chosen`), say text
    /// `notify` once, and close the screen where `close` (it is not where the
    /// page is left through *Back*). (Addition of the offer.)
    Offered {
        switch_on: Vec<&'static str>,
        queued: bool,
        notify: &'static str,
        close: bool,
    },
}

/// What a screen asks the reader. The texts are already in the reader's
/// language.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Ask {
    /// A text typed (`number`: digits only).
    Text { title: String, number: bool },
    /// One of `options` chosen, by its place.
    Choice { title: String, options: Vec<String> },
}

/// The answer to an [`Ask`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Reply<'a> {
    /// The text typed.
    Text(&'a str),
    /// The option chosen, counted from 0.
    Choice(usize),
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

    /// The screen is on top for the first time, after `open`: what it asks of
    /// the plugin at once (the list's fresh fetch). (Addition of the screens'
    /// port.)
    ///
    /// # Errors
    /// `NoMemory`.
    fn started(&mut self, ctx: &Ctx<'_>) -> Result<Next, Error> {
        let _ = ctx;
        Ok(Next::None)
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

    /// The reader leaves the screen through *Back* or the header's arrow, and
    /// it may still do something first (the first-run offer adds the plugins
    /// chosen). The default does nothing; the plugin calls this before it
    /// closes the screen. (Addition of the offer.)
    ///
    /// # Errors
    /// `NoMemory`, or what the card refuses.
    fn closing(&mut self, ctx: &Ctx<'_>) -> Result<Next, Error> {
        let _ = ctx;
        Ok(Next::None)
    }

    /// The screen is closed: it gives back what it holds (a picture the host
    /// keeps, a request going on, a file it wrote).
    ///
    /// # Errors
    /// `NoMemory`.
    fn closed(&mut self, ctx: &Ctx<'_>) -> Result<(), Error> {
        let _ = ctx;
        Ok(())
    }

    /// An event that may concern the screen (an answer, a download's end, an
    /// install's step).
    ///
    /// # Errors
    /// `NoMemory`, or what the host refuses.
    fn event(&mut self, ctx: &Ctx<'_>, event: &Event<'_>) -> Result<Next, Error> {
        let _ = (ctx, event);
        Ok(Next::None)
    }

    /// The reader answered the question `tag` of [`Next::Ask`]. (Addition of
    /// the screens' port.)
    ///
    /// # Errors
    /// `NoMemory`, or what the host refuses.
    fn answered(&mut self, ctx: &Ctx<'_>, tag: u8, reply: Reply<'_>) -> Result<Next, Error> {
        let _ = (ctx, tag, reply);
        Ok(Next::None)
    }

    /// A screen above this one closed with [`Next::CloseWith`]. (Addition of
    /// the screens' port.)
    ///
    /// # Errors
    /// `NoMemory`, or what the host refuses.
    fn returned(&mut self, ctx: &Ctx<'_>, key: &str, value: &str) -> Result<Next, Error> {
        let _ = (ctx, key, value);
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
    /// The first-run offer.
    Offer(crate::native_screen_offer::View),
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
            ids::OFFER => Self::Offer(Screen::open(ctx, params)?),
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
            Self::Offer(v) => v.shown(ctx),
        }
    }

    /// The screen is left through *Back*: what it does first.
    ///
    /// # Errors
    /// What the screen says.
    pub fn closing(&mut self, ctx: &Ctx<'_>) -> Result<Next, Error> {
        match self {
            Self::List(v) => v.closing(ctx),
            Self::View(v) => v.closing(ctx),
            Self::Letters(v) => v.closing(ctx),
            Self::Source(v) => v.closing(ctx),
            Self::Plugin(v) => v.closing(ctx),
            Self::Shots(v) => v.closing(ctx),
            Self::Fetch(v) => v.closing(ctx),
            Self::Offer(v) => v.closing(ctx),
        }
    }

    /// The screen is closed.
    ///
    /// # Errors
    /// What the screen says.
    pub fn closed(&mut self, ctx: &Ctx<'_>) -> Result<(), Error> {
        match self {
            Self::List(v) => v.closed(ctx),
            Self::View(v) => v.closed(ctx),
            Self::Letters(v) => v.closed(ctx),
            Self::Source(v) => v.closed(ctx),
            Self::Plugin(v) => v.closed(ctx),
            Self::Shots(v) => v.closed(ctx),
            Self::Fetch(v) => v.closed(ctx),
            Self::Offer(v) => v.closed(ctx),
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
            Self::Offer(v) => v.resume(ctx),
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
            Self::Offer(v) => v.hidden(),
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
            Self::Offer(v) => v.draw(ctx, frame),
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
            Self::Offer(v) => v.input(ctx, input),
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
            Self::Offer(v) => v.event(ctx, event),
        }
    }

    /// The screen is on top for the first time.
    ///
    /// # Errors
    /// What the screen says.
    pub fn started(&mut self, ctx: &Ctx<'_>) -> Result<Next, Error> {
        match self {
            Self::List(v) => v.started(ctx),
            Self::View(v) => v.started(ctx),
            Self::Letters(v) => v.started(ctx),
            Self::Source(v) => v.started(ctx),
            Self::Plugin(v) => v.started(ctx),
            Self::Shots(v) => v.started(ctx),
            Self::Fetch(v) => v.started(ctx),
            Self::Offer(v) => v.started(ctx),
        }
    }

    /// The reader answered a question of the screen.
    ///
    /// # Errors
    /// What the screen says.
    pub fn answered(&mut self, ctx: &Ctx<'_>, tag: u8, reply: Reply<'_>) -> Result<Next, Error> {
        match self {
            Self::List(v) => v.answered(ctx, tag, reply),
            Self::View(v) => v.answered(ctx, tag, reply),
            Self::Letters(v) => v.answered(ctx, tag, reply),
            Self::Source(v) => v.answered(ctx, tag, reply),
            Self::Plugin(v) => v.answered(ctx, tag, reply),
            Self::Shots(v) => v.answered(ctx, tag, reply),
            Self::Fetch(v) => v.answered(ctx, tag, reply),
            Self::Offer(v) => v.answered(ctx, tag, reply),
        }
    }

    /// A screen above closed with a value for this one.
    ///
    /// # Errors
    /// What the screen says.
    pub fn returned(&mut self, ctx: &Ctx<'_>, key: &str, value: &str) -> Result<Next, Error> {
        match self {
            Self::List(v) => v.returned(ctx, key, value),
            Self::View(v) => v.returned(ctx, key, value),
            Self::Letters(v) => v.returned(ctx, key, value),
            Self::Source(v) => v.returned(ctx, key, value),
            Self::Plugin(v) => v.returned(ctx, key, value),
            Self::Shots(v) => v.returned(ctx, key, value),
            Self::Fetch(v) => v.returned(ctx, key, value),
            Self::Offer(v) => v.returned(ctx, key, value),
        }
    }
}
