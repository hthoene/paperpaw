//! The flat library plugin (slice 1): the library with its filters, a list
//! of books of one scope and the shelves. What is not here yet is in
//! `PORTING-7.md`.
use crate::{
    declarations::{
        BROWSER_TOKEN, FAVOURITES_TOKEN, SHELF_TOKEN, SHELVES_OPEN_TOKEN, SHELVES_TOKEN, SORT,
    },
    native_model::{
        self, BookLine, Filter, MOST_BOOKS, Order, Replied, Say, Scope, Selection, Source, Window,
    },
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::CommandSender,
        events::{Event, Incoming},
        formatting::Arg,
        library::{BookRef, CatalogCursor, CatalogPhase, Library},
        registrations,
        reply::Reply,
        settings::{self, SettingValue},
        ui::{HandoverId, Instance, Notify, Open, Param, Ui},
    },
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Screens,
    mem::{SERVICE_BUFFER, buffer, push},
    text::copy,
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/files_declarations.rs"));
}

/// The parameters that open a list of one scope.
const KIND: &str = "kind";
const VALUE: &str = "value";

/// Where the library keeps books unless the setting says otherwise.
const DEFAULT_FOLDER: &str = "books";
/// The kernel's setting of it.
const FOLDER_SETTING: &str = "core.library-folder";

/// What a row of a list of books does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    /// Steps the filter.
    Filter,
    /// Says the books are still being found.
    Note,
    /// Opens the book at this index of the window.
    Book(usize),
    /// Slides back to the window before this one.
    Previous,
    /// Reads more books: the window grows, or slides on when it is full.
    More,
}

/// Where a list is read from: its window's first reply, and the starts of
/// the windows before it.
#[derive(Debug)]
struct Reading {
    start: CatalogCursor,
    starts: Vec<CatalogCursor>,
}

impl Reading {
    const fn from(start: CatalogCursor, starts: Vec<CatalogCursor>) -> Self {
        Self { start, starts }
    }
}

/// Where the focus goes once a window is read.
#[derive(Debug, Clone, Copy)]
enum Then {
    Keep,
    FirstBook,
    LastRow,
}

/// What to read for a list and where to leave the focus.
#[derive(Debug)]
struct Plan {
    selection: Selection,
    reading: Reading,
    at_least: usize,
    focus: usize,
    then: Then,
}

/// A copy of window starts, fallibly.
fn cursors(from: &[CatalogCursor]) -> Result<Vec<CatalogCursor>, Error> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(from.len())
        .map_err(|_| Error::NoMemory)?;
    for cursor in from {
        push(&mut copy, *cursor)?;
    }
    Ok(copy)
}

#[derive(Debug)]
struct Browser {
    title: String,
    selection: Selection,
    window: Window,
    /// Where the windows before this one began, the nearest last.
    starts: Vec<CatalogCursor>,
    rows: Vec<Row>,
    list: ItemList,
    /// What an empty list says; empty when it is not empty.
    empty: String,
    /// What it says below the title when it is empty (the folder to copy to).
    empty_text: String,
}

#[derive(Debug)]
struct Shelves {
    list: ItemList,
}

#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "boxing would allocate without a fallible path; the screen stack holds a few"
)]
enum View {
    /// The library (`Scope::Library`) or a list of one scope.
    Books(Browser),
    Shelves(Shelves),
}

/// The interface-7 library plugin.
#[derive(Debug, Default)]
pub struct NativeFiles {
    library: Option<Library>,
    ui: Option<Ui>,
    screens: Screens<View>,
}

impl Plugin for NativeFiles {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::SCREENS)
            .chain(image::NAVIGATION)
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
        let event = incoming.decode()?;
        match event {
            Event::ActionRun { .. } if token == SHELVES_OPEN_TOKEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("files.shelves")?)
            }
            Event::ActionRun { .. } if token == FAVOURITES_TOKEN => {
                self.open_shelf(host, &Scope::Favourites)
            }
            Event::ScreenOpened {
                instance, handover, ..
            } if is_screen(token) => self.opened(host, token, instance, handover),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if is_screen(token) => {
                self.screens.shown(token, instance)?;
                self.reread(host)
            }
            Event::ScreenClosed { instance, .. } => {
                self.screens.close(instance);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        match self.screens.top(token).ok_or(Error::State)? {
            View::Books(view) => {
                let footer = if view.empty.is_empty() {
                    Footer::None
                } else {
                    Footer::Empty {
                        title: &view.empty,
                        text: &view.empty_text,
                    }
                };
                view.list
                    .draw_with(host, &mut frame, &view.title, true, footer)
            }
            View::Shelves(view) => {
                let title = messages.copy(host, "files.shelves")?;
                view.list
                    .draw_with(host, &mut frame, &title, true, Footer::None)
            }
        }
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        let list = match view {
            View::Books(view) => &mut view.list,
            View::Shelves(view) => &mut view.list,
        };
        let step = list.input(host, &input)?;
        if let ItemStep::Chosen(index) = step {
            self.chosen(host, token, index)?;
        }
        Ok(ItemList::handled(step))
    }
}

const fn is_screen(token: u32) -> bool {
    token == BROWSER_TOKEN || token == SHELF_TOKEN || token == SHELVES_TOKEN
}

/// The replies of the host's library query for one selection.
struct Replies<'a, 'h> {
    host: &'a Host<'h>,
    library: &'a Library,
    selection: &'a Selection,
    input: &'a mut [u8],
    out: &'a mut [u8],
}

impl Source<CatalogCursor> for Replies<'_, '_> {
    fn read(&mut self, from: CatalogCursor) -> Result<Replied<CatalogCursor>, Error> {
        let query = self.selection.query(from)?;
        let page = self
            .library
            .query(self.host, &query, self.input, self.out)?;
        let mut books = Vec::new();
        for entry in page.rows() {
            push(&mut books, BookLine::from_entry(entry?)?)?;
        }
        let end = page.end();
        Ok(Replied {
            books,
            generation: end.generation(),
            total: end.total(),
            next: end.next(),
        })
    }
}

/// The catalog's words for the model, in the reader's language.
struct Speaker<'h, 'a> {
    host: &'h Host<'a>,
    messages: Messages,
}

impl Say for Speaker<'_, '_> {
    fn say(&mut self, key: &str) -> Result<String, Error> {
        self.messages.copy(self.host, key)
    }
}

impl NativeFiles {
    fn ensure(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.library.is_none() {
            self.library = Some(Library::open(host)?);
        }
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        Ok(())
    }

    /// The order the user chose (`files.sort`).
    fn order(host: &Host<'_>) -> Result<Order, Error> {
        let mut scratch = [0; 64];
        Ok(match settings::read_setting(host, SORT, &mut scratch)? {
            SettingValue::Text(key) => Order::from_key(key),
            _ => Order::default(),
        })
    }

    /// The library folder the user chose (`core.library-folder`), without
    /// slashes around it; the default when the setting says nothing.
    fn folder(host: &Host<'_>) -> Result<String, Error> {
        let mut scratch = [0; 512];
        match settings::read_setting(host, FOLDER_SETTING, &mut scratch) {
            Ok(SettingValue::Text(text)) if !text.trim_matches('/').is_empty() => {
                copy(text.trim_matches('/'))
            }
            Ok(_) | Err(Error::Denied | Error::NotFound | Error::NoSpace) => copy(DEFAULT_FOLDER),
            Err(error) => Err(error),
        }
    }

    /// Opens a list of books of `scope`.
    fn open_shelf(&mut self, host: &Host<'_>, scope: &Scope) -> Result<(), Error> {
        self.ensure(host)?;
        let (kind, value) = scope.params();
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let params = [Param::new(KIND, kind)?, Param::new(VALUE, value)?];
        let handover = self
            .ui
            .as_ref()
            .ok_or(Error::State)?
            .handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).open(Open::screen("files.shelf")?.handover(handover))
    }

    /// The scope a list was opened for; the library for none.
    fn scope_of(&self, host: &Host<'_>, handover: Option<HandoverId>) -> Result<Scope, Error> {
        let Some(handover) = handover else {
            return Ok(Scope::Library);
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let (mut kind, mut value) = (String::new(), String::new());
        for param in ui.params(host, handover, &mut input, &mut out)? {
            let param = param?;
            match param.key() {
                KIND => kind = copy(param.value())?,
                VALUE => value = copy(param.value())?,
                _ => {}
            }
        }
        Scope::from_params(&kind, &value)
    }

    fn opened(
        &mut self,
        host: &Host<'_>,
        token: u32,
        instance: Instance,
        handover: Option<HandoverId>,
    ) -> Result<(), Error> {
        self.ensure(host)?;
        let view = if token == SHELVES_TOKEN {
            View::Shelves(self.read_shelves(host)?)
        } else {
            let scope = if token == BROWSER_TOKEN {
                Scope::Library
            } else {
                self.scope_of(host, handover)?
            };
            let selection = Selection {
                filter: Filter::All,
                order: Self::order(host)?,
                scope,
            };
            View::Books(self.read_browser(
                host,
                selection,
                Reading::from(CatalogCursor::start(), Vec::new()),
                0,
                0,
            )?)
        };
        self.screens.open(instance, view)
    }

    /// Every top screen read again, its focus kept: books come and go.
    fn reread(&mut self, host: &Host<'_>) -> Result<(), Error> {
        for token in [BROWSER_TOKEN, SHELF_TOKEN] {
            let Some(View::Books(old)) = self.screens.top(token) else {
                continue;
            };
            let selection = Selection {
                order: Self::order(host)?,
                ..old.selection.clone()
            };
            let (held, focus) = (old.window.books.len(), old.list.focus());
            let reading = Reading::from(old.window.start, cursors(&old.starts)?);
            let fresh = self.read_browser(host, selection, reading, held, focus)?;
            if let Some(View::Books(old)) = self.screens.top(token) {
                *old = fresh;
            }
        }
        redraw(host)
    }

    fn read_shelves(&self, host: &Host<'_>) -> Result<Shelves, Error> {
        let mut say = Messages::new()?;
        let mut items = Vec::new();
        let favourites = say.copy(host, "files.favourites")?;
        push(&mut items, Item::noted(&favourites, "›", "")?)?;
        // BLOCKED (#228): authors, series, tags and folders list their names
        // with the host's `facets` (LIB op 5), which has no SDK facade yet
        // (`PORTING-7.md`, `sdk-library-facets`).
        Ok(Shelves {
            list: ItemList::new(items),
        })
    }

    /// The books of `selection`, `at_least` of them at once (a window read
    /// again keeps its size) and the focus on row `focus`.
    fn read_browser(
        &self,
        host: &Host<'_>,
        selection: Selection,
        reading: Reading,
        at_least: usize,
        focus: usize,
    ) -> Result<Browser, Error> {
        let library = self.library.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let mut say = Messages::new()?;
        let title = Self::title(host, &mut say, &selection.scope)?;
        let Reading { start, mut starts } = reading;
        let (window, restarted) = {
            let mut source = Replies {
                host,
                library,
                selection: &selection,
                input: &mut input,
                out: &mut out,
            };
            native_model::read_window(&mut source, start, CatalogCursor::start(), at_least.max(1))?
        };
        if restarted {
            // The windows before were read in another generation.
            starts.clear();
        }
        // The books are still being found: the rows so far say so.
        let walking = matches!(
            library.state(host, &mut input, &mut out)?.phase(),
            CatalogPhase::Walking | CatalogPhase::Empty
        ) && selection.scope == Scope::Library;
        let mut browser = Browser {
            title,
            selection,
            window,
            starts,
            rows: Vec::new(),
            list: ItemList::default(),
            empty: String::new(),
            empty_text: String::new(),
        };
        self.layout(host, &mut browser, walking)?;
        browser.list.set_focus(focus);
        Ok(browser)
    }

    fn title(host: &Host<'_>, say: &mut Messages, scope: &Scope) -> Result<String, Error> {
        match scope {
            Scope::Library | Scope::Folder(_) => say.copy(host, "files.title"),
            Scope::Favourites => say.copy(host, "files.favourites"),
            Scope::Author(name) | Scope::Series(name) | Scope::Tag(name) => copy(name),
            Scope::Search(text) => say.format(
                host,
                "files.search.title",
                &[Arg::Text {
                    name: "text",
                    value: text,
                }],
            ),
        }
    }

    /// The rows and items of `browser`'s books.
    fn layout(&self, host: &Host<'_>, browser: &mut Browser, walking: bool) -> Result<(), Error> {
        let mut say = Speaker {
            host,
            messages: Messages::new()?,
        };
        let mut rows = Vec::new();
        let mut items = Vec::new();
        if browser.selection.scope == Scope::Library {
            let show = say.say("files.list.show")?;
            let name = say.say(browser.selection.filter.key())?;
            let count = if browser.selection.filter == Filter::Shelves {
                String::new()
            } else {
                say.messages.format(
                    host,
                    "files.books",
                    &[Arg::Count(i64::from(browser.window.total))],
                )?
            };
            push(&mut rows, Row::Filter)?;
            push(&mut items, Item::noted(&show, &name, &count)?)?;
        }
        if walking {
            let note = say.say("files.list.collecting")?;
            push(&mut rows, Row::Note)?;
            push(&mut items, Item::heading(&note)?)?;
        }
        if !browser.starts.is_empty() {
            let previous = say.say("files.list.previous")?;
            push(&mut rows, Row::Previous)?;
            push(&mut items, Item::noted(&previous, "‹", "")?)?;
        }
        for (at, book) in browser.window.books.iter().enumerate() {
            let line = native_model::line(&mut say, book)?;
            push(&mut rows, Row::Book(at))?;
            push(
                &mut items,
                Item::noted(&line.title, &line.value, &line.note)?,
            )?;
        }
        if browser.window.has_more() {
            let more = say.say("files.list.more")?;
            push(&mut rows, Row::More)?;
            push(&mut items, Item::noted(&more, "›", "")?)?;
        }
        browser.empty = String::new();
        browser.empty_text = String::new();
        if browser.window.books.is_empty() && !walking {
            let library = browser.selection.scope == Scope::Library;
            match browser.selection.filter.none_key() {
                Some(key) if library => browser.empty = say.say(key)?,
                _ if library => {
                    browser.empty = say.say("files.empty.title")?;
                    let folder = Self::folder(host)?;
                    browser.empty_text = say.messages.format(
                        host,
                        "files.empty",
                        &[Arg::Text {
                            name: "dir",
                            value: &folder,
                        }],
                    )?;
                }
                _ => browser.empty = say.say("files.search.none")?,
            }
        }
        browser.rows = rows;
        browser.list = ItemList::new(items);
        Ok(())
    }

    /// Reads the books of `token`'s screen as `plan` says, and shows them.
    fn replace_books(
        &mut self,
        host: &Host<'_>,
        token: u32,
        plan: impl FnOnce(&Browser) -> Result<Option<Plan>, Error>,
    ) -> Result<(), Error> {
        let Some(View::Books(view)) = self.screens.top(token) else {
            return Ok(());
        };
        let Some(plan) = plan(view)? else {
            return Ok(());
        };
        let mut fresh = self.read_browser(
            host,
            plan.selection,
            plan.reading,
            plan.at_least,
            plan.focus,
        )?;
        match plan.then {
            Then::Keep => {}
            Then::FirstBook => fresh.focus_first_book(),
            Then::LastRow => fresh.list.set_focus(fresh.rows.len().saturating_sub(1)),
        }
        if let Some(View::Books(old)) = self.screens.top(token) {
            *old = fresh;
        }
        redraw(host)
    }

    /// "More books": a window with room grows, a full one slides on to the
    /// books after it (the starts of the ones before are kept for "Previous").
    fn slide_on(&mut self, host: &Host<'_>, token: u32) -> Result<(), Error> {
        self.replace_books(host, token, |view| {
            let selection = view.selection.clone();
            let (held, focus) = (view.window.books.len(), view.list.focus());
            let mut starts = cursors(&view.starts)?;
            if view.window.can_grow() {
                return Ok(Some(Plan {
                    selection,
                    reading: Reading::from(view.window.start, starts),
                    at_least: held + 32,
                    focus,
                    then: Then::Keep,
                }));
            }
            let next = view.window.next.ok_or(Error::State)?;
            push(&mut starts, view.window.start)?;
            Ok(Some(Plan {
                selection,
                reading: Reading::from(next, starts),
                at_least: 0,
                focus: 0,
                then: Then::FirstBook,
            }))
        })
    }

    /// "Previous books": the window before, read in full, on the row that
    /// goes on again.
    fn slide_back(&mut self, host: &Host<'_>, token: u32) -> Result<(), Error> {
        self.replace_books(host, token, |view| {
            let selection = view.selection.clone();
            let mut starts = cursors(&view.starts)?;
            let Some(start) = starts.pop() else {
                return Ok(None);
            };
            Ok(Some(Plan {
                selection,
                reading: Reading::from(start, starts),
                at_least: MOST_BOOKS,
                focus: 0,
                then: Then::LastRow,
            }))
        })
    }

    fn chosen(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        self.ensure(host)?;
        match self.screens.top(token) {
            Some(View::Shelves(_)) => {
                // Only the favourites have a list until the host's facets have an SDK.
                if index == 0 {
                    return self.open_shelf(host, &Scope::Favourites);
                }
                Ok(())
            }
            Some(View::Books(view)) => {
                let Some(row) = view.rows.get(index).copied() else {
                    return Ok(());
                };
                match row {
                    Row::Note => Ok(()),
                    Row::Filter => {
                        let next = view.selection.filter.next();
                        let mut selection = view.selection.clone();
                        selection.filter = if next == Filter::Shelves {
                            Filter::All
                        } else {
                            next
                        };
                        let fresh = self.read_browser(
                            host,
                            selection,
                            Reading::from(CatalogCursor::start(), Vec::new()),
                            0,
                            index,
                        )?;
                        if let Some(View::Books(old)) = self.screens.top(token) {
                            *old = fresh;
                        }
                        if next == Filter::Shelves {
                            let mut scratch = [0; 128];
                            CommandSender::new(host, &mut scratch)
                                .open(Open::screen("files.shelves")?)?;
                        }
                        redraw(host)
                    }
                    Row::More => self.slide_on(host, token),
                    Row::Previous => self.slide_back(host, token),
                    Row::Book(at) => {
                        let Some(book) = view.window.books.get(at) else {
                            return Ok(());
                        };
                        let reference = BookRef::catalog(book.key, view.window.generation)?;
                        if self.open_book(host, reference)? {
                            return Ok(());
                        }
                        // The book is gone (deleted on a computer, say): the
                        // list is read again and the user is told.
                        self.reread(host)?;
                        let mut scratch = [0; 256];
                        let mut say = Messages::new()?;
                        let text = say.copy(host, "files.list.gone")?;
                        // A notice is best effort: the list is already right.
                        CommandSender::new(host, &mut scratch)
                            .notify(Notify::new(&text)?)
                            .ok();
                        Ok(())
                    }
                }
            }
            None => Ok(()),
        }
    }

    /// Opens the book in the reader the user bound to `core.reader`: a
    /// hand-over with its path (`document.md` section 1).
    /// `false` when the book is no longer in the catalog.
    fn open_book(&self, host: &Host<'_>, reference: BookRef<'_>) -> Result<bool, Error> {
        let library = self.library.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let path = match library.book(host, reference, &mut input, &mut out) {
            Ok(book) => copy(book.path())?,
            Err(Error::State | Error::NotFound) => return Ok(false),
            Err(error) => return Err(error),
        };
        let params = [Param::new("path", &path)?];
        let handover = self
            .ui
            .as_ref()
            .ok_or(Error::State)?
            .handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch)
            .open(Open::slot("core.reader")?.handover(handover))?;
        Ok(true)
    }
}

impl Browser {
    /// Focus on the first book (after the filter row and the way back).
    fn focus_first_book(&mut self) {
        let at = self
            .rows
            .iter()
            .position(|row| matches!(row, Row::Book(_)))
            .unwrap_or(0);
        self.list.set_focus(at);
    }
}

fn redraw(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch)
        .queue_redraw(paperpaw_sdk::native::commands::RedrawMode::Partial)
}
