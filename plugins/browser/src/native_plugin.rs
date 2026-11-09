//! The interface-7 browser (slice 2): the start screen with the address, the
//! bookmarks and the pages read lately, the choices on a held entry, loading
//! an address through `core.net` and showing a web page in the reader. What
//! comes next (downloads of books, fonts and other files) is in
//! `PORTING-7.md`.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Handled;
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    callbacks::{DrawFrame, Input},
    commands::{CommandSender, RedrawMode},
    events::{AnswerOutcome, Answered, Event, Incoming, kinds},
    files::Files,
    formatting::Arg,
    library::Library,
    net::{FileRoot, Header, HttpDone, Net, Reason, Request},
    registrations,
    reply::Reply,
    storage::DataStorage,
    ui::{HandoverId, Instance, Open, Param, Question, RequestId as AskId, TextMode, Ui},
};
use paperpaw_sdk::{Storage, native::net::REPLY, native::net::REQUEST_INPUT};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Screens,
    mem::{SERVICE_BUFFER, buffer, push},
    read_event, text,
    text::{concat, copy},
    uitext,
};

use crate::declarations::{LOADED, OPEN, SCREEN};
use crate::native_save as save;
use crate::native_store::{self as store, Entry, StoreError};
use crate::native_web::{self as web, Kind};

mod image {
    include!(concat!(env!("OUT_DIR"), "/browser_declarations.rs"));
}

/// The parameter that opens the screen on an address (the reader's links).
const URL: &str = "url";
/// The longest address typed.
const TYPED_LIMIT: usize = 1_100;
/// What the plugin says it is, as interface 6 did.
const AGENT: &str = "PaperPaw/0.1 (e-reader)";
const ACCEPT: &str = "text/html,application/xhtml+xml,application/epub+zip,*/*";

/// A row of the start screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Address,
    Bookmark(usize),
    History(usize),
}

/// What a question is about.
#[derive(Debug)]
enum Asked {
    /// The address or the words to search for.
    Address,
    /// What to do with a bookmark or a page of the history.
    Held(Row, &'static [&'static str]),
    /// Whether to read the book just saved (its path in the library).
    Read(String),
}

/// The start screen: its lists and the line for messages below them.
#[derive(Debug, Default)]
struct View {
    list: ItemList,
    bookmarks: Vec<Entry>,
    /// The bookmarks were read: they may be saved. Not where they could not
    /// be, or what they held would be lost.
    bookmarks_read: bool,
    history: Vec<Entry>,
    message: String,
    /// An address to load once shown (opened for a link).
    start: Option<String>,
    /// A page was opened in the reader from here: back here means *Back*.
    showing: bool,
}

/// The request being loaded: its number and the address asked for.
#[derive(Debug)]
struct Loading {
    token: u32,
    url: String,
}

/// The interface-7 browser.
#[derive(Debug, Default)]
pub struct NativeBrowser {
    ui: Option<Ui>,
    net: Option<Net>,
    /// Opened with the first download that needs them.
    library: Option<Library>,
    files: Option<Files>,
    screens: Screens<View>,
    /// The pages shown this time, the one in the reader last: *Back* goes
    /// through them.
    pages: Vec<Entry>,
    loading: Option<Loading>,
    /// The number of the next request.
    token: u32,
    asked: Option<(AskId, Asked)>,
}

const HELD_BOOKMARK: &[&str] = &["open", "delete"];
const HELD_HISTORY: &[&str] = &["open", "bookmark", "delete"];

impl Plugin for NativeBrowser {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SCREENS
            .iter()
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
        }
        // The services are opened by the first callback: opening needs a live frame.
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(incoming; kinds::ActionRun, kinds::Answered, kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenClosed, kinds::HttpDone)
        else {
            return Ok(());
        };
        match event {
            Event::ActionRun { .. } if token == OPEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::slot("core.browser")?)
            }
            Event::Answered { answer, .. } => self.answered(host, answer),
            Event::ScreenOpened {
                instance, handover, ..
            } if token == SCREEN => self.opened(host, instance, handover),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == SCREEN => {
                self.screens.shown(token, instance)?;
                self.shown(host)
            }
            Event::ScreenClosed { instance, .. } => {
                // The last browser screen is gone: what is loading is of no use.
                if let Some(token) = self.closed(instance) {
                    self.cancel(host, token)?;
                }
                Ok(())
            }
            Event::HttpDone(done) if token == LOADED => self.loaded(host, &done),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let view = self.screens.top(token).ok_or(Error::State)?;
        let title = say.copy(host, "browser.title")?;
        let footer = if view.message.is_empty() {
            Footer::None
        } else {
            Footer::Note(&view.message)
        };
        view.list.draw_with(host, &mut frame, &title, true, footer)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        let step = view.list.input(host, &input)?;
        match step {
            ItemStep::Chosen(index) => self.activate(host, index)?,
            ItemStep::Held(index) => self.offer(host, index)?,
            _ => {}
        }
        Ok(ItemList::handled(step))
    }
}

impl NativeBrowser {
    /// Opens `core.ui` once; `core.net` too, where the device has a network
    /// (a device without says so when an address is loaded).
    fn ensure(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        if self.net.is_none() {
            self.net = match Net::open(host) {
                Ok(net) => Some(net),
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => None,
            };
        }
        Ok(())
    }

    /// A screen is closed; the number of the request to cancel when it was
    /// the last screen and a request is still loading.
    fn closed(&mut self, instance: Instance) -> Option<u32> {
        self.screens.close(instance);
        if self.screens.is_empty() {
            self.loading.take().map(|loading| loading.token)
        } else {
            None
        }
    }

    /// Whether a browser screen is open to take an answer.
    fn wanted(&self) -> bool {
        !self.screens.is_empty()
    }

    /// Ends the request `token`; its end is ignored (nothing is loading).
    fn cancel(&self, host: &Host<'_>, token: u32) -> Result<(), Error> {
        let Some(net) = &self.net else {
            return Ok(());
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        net.cancel(host, token, &mut input, &mut out)
    }

    fn ui(&self) -> Result<&Ui, Error> {
        self.ui.as_ref().ok_or(Error::State)
    }

    fn redraw(host: &Host<'_>) -> Result<(), Error> {
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
    }

    /// The screen is opened (for an address, with a hand-over).
    fn opened(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        handover: Option<HandoverId>,
    ) -> Result<(), Error> {
        self.ensure(host)?;
        let mut start = None;
        if let Some(handover) = handover {
            let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
            for param in self.ui()?.params(host, handover, &mut input, &mut out)? {
                let param = param?;
                if param.key() == URL && !param.value().trim().is_empty() {
                    start = Some(copy(param.value().trim())?);
                }
            }
        }
        if start.is_none() {
            // Opened from the menu: a new start.
            self.pages.clear();
        }
        let mut view = View {
            start,
            ..View::default()
        };
        self.read_lists(host, &mut view)?;
        self.build(host, &mut view)?;
        self.screens.open(instance, view)
    }

    /// Reads the bookmarks and the history; what cannot be read is shown as
    /// none and never written over.
    fn read_lists(&self, host: &Host<'_>, view: &mut View) -> Result<(), Error> {
        let storage = DataStorage::new(host);
        let language = settings_language(host)?;
        match store::bookmarks(&storage, &language) {
            Ok(Some(list)) => {
                view.bookmarks = list;
                view.bookmarks_read = true;
            }
            Ok(None) => view.bookmarks_read = true,
            Err(StoreError::NoMemory) => return Err(Error::NoMemory),
            Err(_) => {}
        }
        view.history = match store::load(&storage, store::HISTORY_FILE) {
            Ok(Some(list)) => list,
            Ok(None) | Err(StoreError::Storage(_)) => Vec::new(),
            Err(StoreError::NoMemory) => return Err(Error::NoMemory),
        };
        Ok(())
    }

    /// The rows of the lists, in the order they are shown.
    fn rows(view: &View) -> Result<Vec<Row>, Error> {
        let mut rows = Vec::new();
        rows.try_reserve(1 + view.bookmarks.len() + view.history.len())
            .map_err(|_| Error::NoMemory)?;
        rows.push(Row::Address);
        rows.extend((0..view.bookmarks.len()).map(Row::Bookmark));
        rows.extend((0..view.history.len()).map(Row::History));
        Ok(rows)
    }

    /// Makes the items of the open lists, keeping the focus.
    fn build(&self, host: &Host<'_>, view: &mut View) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let mut items = Vec::new();
        for row in Self::rows(view)? {
            let item = match row {
                Row::Address => Item::row(&say.copy(host, "browser.address")?, "›")?,
                Row::Bookmark(i) => entry_item(&view.bookmarks[i])?,
                Row::History(i) => entry_item(&view.history[i])?,
            };
            push(&mut items, item)?;
        }
        let focus = view.list.focus();
        view.list = ItemList::new(items);
        view.list.set_focus(focus);
        Ok(())
    }

    /// Shows what the open screen holds again, and asks for a redraw.
    fn refresh(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(mut view) = self.screens.top(SCREEN).map(core::mem::take) else {
            return Ok(());
        };
        let built = self.build(host, &mut view);
        if let Some(top) = self.screens.top(SCREEN) {
            *top = view;
        }
        built?;
        Self::redraw(host)
    }

    /// Says `key` (with arguments) on the line below the lists.
    fn say(&mut self, host: &Host<'_>, key: &str, args: &[Arg<'_>]) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let message = if args.is_empty() {
            say.copy(host, key)?
        } else {
            say.format(host, key, args)?
        };
        if let Some(top) = self.screens.top(SCREEN) {
            top.message = message;
        }
        Self::redraw(host)
    }

    /// The screen is shown: an address to load, or back from a page.
    fn shown(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let (start, showing) = match self.screens.top(SCREEN) {
            Some(view) => (view.start.take(), core::mem::take(&mut view.showing)),
            None => return Ok(()),
        };
        if let Some(url) = start {
            return self.load(host, &url);
        }
        if !showing {
            return Ok(());
        }
        // Back from a page: the one before, else the start screen.
        self.pages.pop();
        let Some(previous) = self.pages.pop() else {
            let Some(mut view) = self.screens.top(SCREEN).map(core::mem::take) else {
                return Ok(());
            };
            let read = self.read_lists(host, &mut view);
            let built = read.and_then(|()| self.build(host, &mut view));
            if let Some(top) = self.screens.top(SCREEN) {
                *top = view;
            }
            built?;
            return Self::redraw(host);
        };
        let path = store::page_path(&previous.url)?;
        if DataStorage::new(host).size(&path).is_ok() {
            self.show(host, previous)
        } else {
            let url = copy(&previous.url)?;
            self.load(host, &url)
        }
    }

    /// Opens what row `index` stands for.
    fn activate(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let Some(row) = self.row_at(index)? else {
            return Ok(());
        };
        match row {
            Row::Address => self.ask_address(host),
            Row::Bookmark(_) | Row::History(_) => {
                let Some(url) = self.entry_url(row)? else {
                    return Ok(());
                };
                self.pages.clear();
                self.load(host, &url)
            }
        }
    }

    fn row_at(&mut self, index: usize) -> Result<Option<Row>, Error> {
        let Some(view) = self.screens.top(SCREEN) else {
            return Ok(None);
        };
        Ok(Self::rows(view)?.get(index).copied())
    }

    fn entry_of(view: &View, row: Row) -> Option<&Entry> {
        match row {
            Row::Address => None,
            Row::Bookmark(i) => view.bookmarks.get(i),
            Row::History(i) => view.history.get(i),
        }
    }

    fn entry_url(&mut self, row: Row) -> Result<Option<String>, Error> {
        let Some(view) = self.screens.top(SCREEN) else {
            return Ok(None);
        };
        match Self::entry_of(view, row) {
            Some(entry) => Ok(Some(copy(&entry.url)?)),
            None => Ok(None),
        }
    }

    /// Asks for an address or words to search for.
    fn ask_address(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let title = say.copy(host, "browser.address")?;
        let question = Question::text(&title, "")?.mode(TextMode::Url)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let request = self.ui()?.ask(host, question, &mut input, &mut out)?;
        self.asked = Some((request, Asked::Address));
        Ok(())
    }

    /// What can be done with row `index`.
    fn offer(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let Some(row) = self.row_at(index)? else {
            return Ok(());
        };
        let options = match row {
            Row::Bookmark(_) => HELD_BOOKMARK,
            Row::History(_) => HELD_HISTORY,
            Row::Address => return self.activate(host, index),
        };
        let Some(title) = self
            .screens
            .top(SCREEN)
            .and_then(|view| Self::entry_of(view, row))
            .map(|entry| copy(&entry.title))
            .transpose()?
        else {
            return Ok(());
        };
        let mut say = Messages::new()?;
        let mut choices = String::new();
        for option in options {
            let label = say.copy(host, &concat(&["browser.option.", option])?)?;
            choices = concat(&[&choices, if choices.is_empty() { "" } else { "\n" }, &label])?;
        }
        let question = Question::choice(&title, "", &choices)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let request = self.ui()?.ask(host, question, &mut input, &mut out)?;
        self.asked = Some((request, Asked::Held(row, options)));
        Ok(())
    }

    /// An answer to a question the plugin asked.
    fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        let Some((request, _)) = &self.asked else {
            return Ok(());
        };
        if *request != answer.request() || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        let Some((request, asked)) = self.asked.take() else {
            return Ok(());
        };
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        match asked {
            Asked::Address => {
                let typed = uitext::read_all(self.ui()?, host, request.text(), TYPED_LIMIT)?;
                if typed.trim().is_empty() {
                    return Ok(());
                }
                self.pages.clear();
                let url = web::address(typed.trim())?;
                self.load(host, &url)
            }
            Asked::Read(path) => {
                if answer.choice() == 0 {
                    self.read(host, &path)?;
                }
                Ok(())
            }
            Asked::Held(row, options) => match options.get(answer.choice() as usize) {
                Some(option) => self.chosen(host, row, option),
                None => Ok(()),
            },
        }
    }

    /// What was chosen for a bookmark or a page of the history.
    fn chosen(&mut self, host: &Host<'_>, row: Row, option: &str) -> Result<(), Error> {
        let Some(view) = self.screens.top(SCREEN) else {
            return Ok(());
        };
        let Some(entry) = Self::entry_of(view, row) else {
            return Ok(());
        };
        let entry = entry.try_clone()?;
        let storage = DataStorage::new(host);
        match (row, option) {
            (_, "open") => {
                self.pages.clear();
                self.load(host, &entry.url)
            }
            (Row::History(_), "bookmark") => {
                let Some(view) = self.screens.top(SCREEN).filter(|v| v.bookmarks_read) else {
                    return Ok(());
                };
                let mut list = Vec::new();
                list.try_reserve(view.bookmarks.len() + 1)
                    .map_err(|_| Error::NoMemory)?;
                for kept in view.bookmarks.iter().filter(|b| b.url != entry.url) {
                    list.push(kept.try_clone()?);
                }
                list.push(entry);
                self.keep(host, &storage, store::BOOKMARKS, list, true)
            }
            (Row::Bookmark(i), "delete") => {
                let Some(view) = self.screens.top(SCREEN).filter(|v| v.bookmarks_read) else {
                    return Ok(());
                };
                let list = without(&view.bookmarks, i)?;
                self.keep(host, &storage, store::BOOKMARKS, list, true)
            }
            (Row::History(i), "delete") => {
                let Some(view) = self.screens.top(SCREEN) else {
                    return Ok(());
                };
                let list = without(&view.history, i)?;
                // The page's file goes with it.
                let _ = storage.remove(&store::page_path(&entry.url)?);
                self.keep(host, &storage, store::HISTORY_FILE, list, false)
            }
            _ => Ok(()),
        }
    }

    /// Saves a list and shows it as it is now; a refusal is told, the lists
    /// stay as they were.
    fn keep(
        &mut self,
        host: &Host<'_>,
        storage: &DataStorage<'_, '_>,
        file: &str,
        list: Vec<Entry>,
        bookmarks: bool,
    ) -> Result<(), Error> {
        match store::save(storage, file, &list) {
            Ok(()) => {}
            Err(StoreError::NoMemory) => return Err(Error::NoMemory),
            Err(StoreError::Storage(_)) => return self.say(host, "browser.cannot-save", &[]),
        }
        if let Some(view) = self.screens.top(SCREEN) {
            if bookmarks {
                view.bookmarks = list;
            } else {
                view.history = list;
            }
        }
        self.refresh(host)
    }

    /// Loads `url`; what it is decides where it goes once it is there.
    fn load(&mut self, host: &Host<'_>, url: &str) -> Result<(), Error> {
        let Some(net) = &self.net else {
            return self.say(host, "browser.no-network", &[]);
        };
        self.token = self.token.wrapping_add(1).max(1);
        let token = self.token;
        let headers = [
            Header::new("User-Agent", AGENT)?,
            Header::new("Accept", ACCEPT)?,
        ];
        let request = Request::get(token, url)?
            .headers(&headers)?
            .save_to(FileRoot::Data, store::DOWNLOAD)?;
        let (mut input, mut out) = (buffer(REQUEST_INPUT)?, buffer(REPLY)?);
        match net.request(host, &request, &mut input, &mut out) {
            Ok(_) => {}
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // Four requests at once, no network at all: said, not lost.
            Err(_) => return self.say(host, "browser.no-network", &[]),
        }
        self.loading = Some(Loading {
            token,
            url: copy(url)?,
        });
        let at = text::copy(paperpaw_sdk::web::host(url))?;
        self.say(
            host,
            "browser.loading",
            &[Arg::Text {
                name: "host",
                value: &at,
            }],
        )
    }

    /// A request ended.
    fn loaded(&mut self, host: &Host<'_>, done: &HttpDone) -> Result<(), Error> {
        let Some(loading) = self.loading.take_if(|l| l.token == done.token()) else {
            return Ok(());
        };
        if !self.wanted() {
            // No browser screen is open: nothing may open the reader over what
            // the reader shows now.
            let _ = DataStorage::new(host).remove(store::DOWNLOAD);
            return Ok(());
        }
        let name = copy(paperpaw_sdk::web::host(&loading.url))?;
        let host_arg = [Arg::Text {
            name: "host",
            value: &name,
        }];
        match done.reason() {
            Reason::Ok => {}
            Reason::NoNetwork | Reason::Offline | Reason::Saving | Reason::NetworkOff => {
                return self.say(host, "browser.no-network", &[]);
            }
            Reason::TooLarge | Reason::Io | Reason::Full => {
                return self.say(host, "browser.cannot-save", &[]);
            }
            Reason::Cancelled | Reason::Replaced => return Ok(()),
            _ => return self.say(host, "browser.failed", &host_arg),
        }
        let (mut input, mut out) = (buffer(REQUEST_INPUT)?, buffer(REPLY)?);
        let net = self.net.as_ref().ok_or(Error::State)?;
        let answer = net.answer(host, done.id(), &mut input, &mut out)?;
        let status = answer.status();
        if status == 0 {
            return self.say(host, "browser.failed", &host_arg);
        }
        let url = if answer.final_url().is_empty() {
            copy(&loading.url)?
        } else {
            copy(answer.final_url())?
        };
        let content_type = (!answer.content_type().is_empty())
            .then(|| copy(answer.content_type()))
            .transpose()?;
        let storage = DataStorage::new(host);
        // An error's page comes in memory, never in the download file: kept
        // there to show (a download never holds an error page).
        if status >= 400 && done.body_len() > 0 {
            self.keep_error_body(host, done, &storage)?;
        }
        let mut head = [0u8; 512];
        let length = storage.read_at(store::DOWNLOAD, 0, &mut head).unwrap_or(0);
        let kind = web::kind(
            content_type.as_deref(),
            &url,
            head.get(..length).unwrap_or(&[]),
        )?;
        if status >= 400 && kind != Kind::Page {
            let _ = storage.remove(store::DOWNLOAD);
            let status = i64::from(status);
            return self.say(
                host,
                "browser.error",
                &[
                    Arg::Text {
                        name: "host",
                        value: &name,
                    },
                    Arg::Int {
                        name: "status",
                        value: status,
                    },
                ],
            );
        }
        match kind {
            Kind::Page => self.keep_page(host, &storage, &url, &name),
            Kind::Book | Kind::Font | Kind::Other => self.keep_download(host, kind, &url),
        }
    }

    /// A book, a font or another file: from the download file to where it
    /// belongs; told, and a book offered to be read.
    fn keep_download(&mut self, host: &Host<'_>, kind: Kind, url: &str) -> Result<(), Error> {
        let name = paperpaw_sdk::web::try_file_name(url).map_err(|_| Error::NoMemory)?;
        let result = match kind {
            Kind::Book => self.save_book(host, &name),
            Kind::Font => self.save_card(host, save::FONTS, &name),
            _ => self.save_card(host, save::DOWNLOADS, &name),
        };
        let _ = DataStorage::new(host).remove(store::DOWNLOAD);
        let saved = match result {
            Ok(saved) => saved,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // The fonts folder is where the card is the host's to give.
            Err(Error::Unsupported) if kind == Kind::Font => {
                return self.say(host, "browser.no-fonts", &[]);
            }
            Err(_) => return self.say(host, "browser.cannot-save", &[]),
        };
        let name = Arg::Text {
            name: "name",
            value: file_name(&saved),
        };
        match kind {
            Kind::Book => self.offer_read(host, saved),
            Kind::Font => self.say(host, "browser.saved-font", &[name]),
            _ => {
                let folder = Arg::Text {
                    name: "folder",
                    value: save::DOWNLOADS,
                };
                self.say(host, "browser.saved-file", &[name, folder])
            }
        }
    }

    /// Imports the download into the library; the book's path.
    fn save_book(&mut self, host: &Host<'_>, name: &str) -> Result<String, Error> {
        if self.library.is_none() {
            self.library = Some(Library::open(host)?);
        }
        let library = self.library.as_ref().ok_or(Error::State)?;
        save::book(host, library, store::DOWNLOAD, name)
    }

    /// Moves the download into `folder` of the card; the name taken.
    fn save_card(&mut self, host: &Host<'_>, folder: &str, name: &str) -> Result<String, Error> {
        if self.files.is_none() {
            self.files = Some(Files::open(host)?);
        }
        let files = self.files.as_ref().ok_or(Error::State)?;
        save::card_file(host, files, store::DOWNLOAD, folder, name)
    }

    /// Says the book is in the library and asks whether to read it now.
    fn offer_read(&mut self, host: &Host<'_>, path: String) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let name = Arg::Text {
            name: "name",
            value: file_name(&path),
        };
        let text = say.format(host, "browser.saved-book", &[name])?;
        let choices = concat(&[
            &say.copy(host, "browser.option.read")?,
            "\n",
            &say.copy(host, "browser.option.ok")?,
        ])?;
        if let Some(view) = self.screens.top(SCREEN) {
            view.message = String::new();
        }
        let question = Question::choice(&text, "", &choices)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let request = self.ui()?.ask(host, question, &mut input, &mut out)?;
        self.asked = Some((request, Asked::Read(path)));
        Self::redraw(host)
    }

    /// Opens the book at `path` in the reader.
    fn read(&self, host: &Host<'_>, path: &str) -> Result<(), Error> {
        let params = [Param::new("path", path)?];
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let handover = self.ui()?.handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).open(Open::slot("core.reader")?.handover(handover))
    }

    /// Writes the error answer's body (in memory) to the download file.
    fn keep_error_body(
        &self,
        host: &Host<'_>,
        done: &HttpDone,
        storage: &DataStorage<'_, '_>,
    ) -> Result<(), Error> {
        let net = self.net.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(REQUEST_INPUT)?, buffer(REPLY)?);
        let mut body = Vec::new();
        let mut offset = 0u32;
        while offset < done.body_len() {
            let part = net.body(host, done.id(), offset, &mut input, &mut out)?;
            if part.is_empty() {
                break;
            }
            body.try_reserve(part.len()).map_err(|_| Error::NoMemory)?;
            body.extend_from_slice(part);
            offset = offset.saturating_add(u32::try_from(part.len()).map_err(|_| Error::Invalid)?);
        }
        storage.write(store::DOWNLOAD, &body).map_err(|_| Error::Io)
    }

    /// A page: kept with its address, titled, remembered and shown in the reader.
    fn keep_page(
        &mut self,
        host: &Host<'_>,
        storage: &DataStorage<'_, '_>,
        url: &str,
        name: &str,
    ) -> Result<(), Error> {
        let path = store::page_path(url)?;
        match web::keep_page(storage, store::DOWNLOAD, &path, url) {
            Ok(()) => {}
            Err(web::KeepError::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return self.say(host, "browser.cannot-save", &[]),
        }
        let title = match web::title(storage, &path)? {
            Some(title) => title,
            None => copy(name)?,
        };
        self.show(host, Entry::new(&title, url)?)
    }

    /// Shows page `entry` (already kept) in the reader.
    fn show(&mut self, host: &Host<'_>, entry: Entry) -> Result<(), Error> {
        let path = store::page_path(&entry.url)?;
        match store::remember(&DataStorage::new(host), entry.try_clone()?) {
            Ok(Some(history)) => {
                if let Some(view) = self.screens.top(SCREEN) {
                    view.history = history;
                }
            }
            Ok(None) | Err(StoreError::Storage(_)) => {}
            Err(StoreError::NoMemory) => return Err(Error::NoMemory),
        }
        push(&mut self.pages, entry)?;
        if let Some(view) = self.screens.top(SCREEN) {
            view.showing = true;
            view.message = String::new();
        }
        let params = [Param::new("root", "0")?, Param::new("path", &path)?];
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let handover = self.ui()?.handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).open(Open::slot("core.reader")?.handover(handover))
    }
}

/// The last part of a path.
fn file_name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// An entry's row: its title and the host of its address.
fn entry_item(entry: &Entry) -> Result<Item, Error> {
    Item::row(&entry.title, paperpaw_sdk::web::host(&entry.url))
}

/// The entries without the one at `at`.
fn without(entries: &[Entry], at: usize) -> Result<Vec<Entry>, Error> {
    let mut list = Vec::new();
    list.try_reserve(entries.len())
        .map_err(|_| Error::NoMemory)?;
    for (index, entry) in entries.iter().enumerate() {
        if index != at {
            list.push(entry.try_clone()?);
        }
    }
    Ok(list)
}

/// The language of the interface (the bookmarks' three first places follow it).
fn settings_language(host: &Host<'_>) -> Result<String, Error> {
    use paperpaw_sdk::native::settings::{SettingValue, read_setting};
    let mut scratch = [0; 64];
    match read_setting(host, "core.language", &mut scratch) {
        Ok(SettingValue::Text(language)) => copy(language),
        Ok(_) | Err(Error::NotFound | Error::Denied | Error::NoSpace) => Ok(String::new()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests;
