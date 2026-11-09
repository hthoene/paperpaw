//! The flat library plugin: the library with its filters, a list of books
//! of one scope (the favourites, an author, a series, a tag, a folder, a
//! search, the next books of the series begun), the shelves with their
//! lists of names, and book information. What is not here yet is in
//! `PORTING-7.md`.
use crate::{
    declarations::{
        BROWSER_TOKEN, BY_AUTHOR_TOKEN, BY_SERIES_TOKEN, BY_TAG_TOKEN, FAVOURITES_TOKEN,
        INFO_TOKEN, NAMES_TOKEN, SEARCH_TOKEN, SHELF_TOKEN, SHELVES_OPEN_TOKEN, SHELVES_TOKEN,
        SORT,
    },
    native_info,
    native_model::{
        self, BookLine, Filter, MOST_BOOKS, Order, Replied, Say, Scope, Selection, Source, Window,
    },
    native_shelves::{
        self, Begun, MOST_SERIES, NameLine, SeriesNames, SeriesSource, Shelf, collect_next,
    },
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::CommandSender,
        events::{AnswerOutcome, Answered, Event, Incoming, kinds},
        formatting::Arg,
        library::{BookRef, CatalogCursor, CatalogPhase, Facet, Library, Query, Show},
        registrations,
        reply::Reply,
        settings::{self, SettingValue},
        ui::{HandoverId, Instance, Notify, Open, Param, Question, RequestId, Ui},
    },
};
use paperpaw_ui_kit::read_event;
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Screens,
    mem::{SERVICE_BUFFER, buffer, push},
    text::copy,
    uitext,
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/files_declarations.rs"));
}

/// The parameters that open a list of one scope.
const KIND: &str = "kind";
const VALUE: &str = "value";
/// The parameter that names a book's path (`core.book-info`).
const PATH: &str = "path";

/// Where the library keeps books unless the setting says otherwise.
const DEFAULT_FOLDER: &str = "books";
/// The kernel's setting of it.
const FOLDER_SETTING: &str = "core.library-folder";
/// The most bytes of words a search takes (the host's cap, never cut).
const SEARCH_LIMIT: usize = 256;

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

/// Where to read a window of a list next, and where the focus goes.
#[derive(Debug)]
struct Step {
    reading: Reading,
    at_least: usize,
    focus: usize,
    then: Then,
}

/// "More": a window with room grows, a full one slides on to what follows
/// it (the starts of the ones before are kept for "Previous").
fn step_on<T>(
    window: &Window<CatalogCursor, T>,
    starts: &[CatalogCursor],
    focus: usize,
) -> Result<Step, Error> {
    let mut starts = cursors(starts)?;
    if window.can_grow() {
        return Ok(Step {
            reading: Reading::from(window.start, starts),
            at_least: window.books.len() + 32,
            focus,
            then: Then::Keep,
        });
    }
    let next = window.next.ok_or(Error::State)?;
    push(&mut starts, window.start)?;
    Ok(Step {
        reading: Reading::from(next, starts),
        at_least: 0,
        focus: 0,
        then: Then::FirstBook,
    })
}

/// "Previous": the window before, read in full, the focus on the row that
/// goes on again; none when there is none.
fn step_back(starts: &[CatalogCursor]) -> Result<Option<Step>, Error> {
    let mut starts = cursors(starts)?;
    let Some(start) = starts.pop() else {
        return Ok(None);
    };
    Ok(Some(Step {
        reading: Reading::from(start, starts),
        at_least: MOST_BOOKS,
        focus: 0,
        then: Then::LastRow,
    }))
}

impl Plan {
    fn of(selection: Selection, step: Step) -> Self {
        Self {
            selection,
            reading: step.reading,
            at_least: step.at_least,
            focus: step.focus,
            then: step.then,
        }
    }
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

/// A row of the shelves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShelfRow {
    /// The next book of each series begun.
    Next,
    Favourites,
    /// The names of one list.
    By(Shelf),
}

#[derive(Debug)]
struct Shelves {
    rows: Vec<ShelfRow>,
    list: ItemList,
}

/// The names of one shelf, a window of them.
#[derive(Debug)]
struct Names {
    shelf: Shelf,
    window: Window<CatalogCursor, NameLine>,
    starts: Vec<CatalogCursor>,
    /// `Row::Book(at)` is the name at `at` of the window.
    rows: Vec<Row>,
    list: ItemList,
    /// What an empty list says; empty when it is not empty.
    empty: String,
}

/// Book information: the book's title and author above the rows.
#[derive(Debug)]
struct Info {
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
    Names(Names),
    Info(Info),
}

/// The interface-7 library plugin.
#[derive(Debug, Default)]
pub struct NativeFiles {
    library: Option<Library>,
    ui: Option<Ui>,
    screens: Screens<View>,
    /// The question for the words of a search, while it is open.
    asked: Option<RequestId>,
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
        let Some(event) = read_event!(incoming; kinds::ActionRun, kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenClosed, kinds::Answered)
        else {
            return Ok(());
        };
        match event {
            Event::ActionRun { .. } if token == SHELVES_OPEN_TOKEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("files.shelves")?)
            }
            Event::ActionRun { .. } if token == FAVOURITES_TOKEN => {
                self.open_shelf(host, &Scope::Favourites)
            }
            Event::ActionRun { .. } if token == BY_AUTHOR_TOKEN => {
                self.open_names(host, Shelf::Authors)
            }
            Event::ActionRun { .. } if token == BY_SERIES_TOKEN => {
                self.open_names(host, Shelf::Series)
            }
            Event::ActionRun { .. } if token == BY_TAG_TOKEN => self.open_names(host, Shelf::Tags),
            Event::ActionRun { .. } if token == SEARCH_TOKEN => self.ask_search(host),
            Event::Answered { answer, .. } => self.answered(host, answer),
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
            View::Names(view) => {
                let title = messages.copy(host, view.shelf.title_key())?;
                let footer = if view.empty.is_empty() {
                    Footer::None
                } else {
                    Footer::Empty {
                        title: &view.empty,
                        text: "",
                    }
                };
                view.list.draw_with(host, &mut frame, &title, true, footer)
            }
            View::Info(view) => {
                let title = messages.copy(host, "files.info")?;
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
            View::Names(view) => &mut view.list,
            View::Info(view) => &mut view.list,
        };
        let step = list.input(host, &input)?;
        if let ItemStep::Chosen(index) = step {
            self.chosen(host, token, index)?;
        }
        Ok(ItemList::handled(step))
    }
}

const fn is_screen(token: u32) -> bool {
    token == BROWSER_TOKEN
        || token == SHELF_TOKEN
        || token == SHELVES_TOKEN
        || token == NAMES_TOKEN
        || token == INFO_TOKEN
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
        if self.selection.scope == Scope::Next {
            return self.read_next();
        }
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

impl Replies<'_, '_> {
    /// The next book of each series begun: all of them in one reply.
    fn read_next(&mut self) -> Result<Replied<CatalogCursor>, Error> {
        let mut source = NextHost {
            host: self.host,
            library: self.library,
            input: &mut *self.input,
            out: &mut *self.out,
            generation: 0,
        };
        let books = collect_next(&mut source, CatalogCursor::start(), MOST_SERIES)?;
        let total = u32::try_from(books.len()).unwrap_or(u32::MAX);
        Ok(Replied {
            books,
            generation: source.generation,
            total,
            next: None,
        })
    }
}

/// The series begun and their next books, from the host's library service.
/// A catalog that changes while they are read is `Error::State`: read again.
struct NextHost<'a, 'h> {
    host: &'a Host<'h>,
    library: &'a Library,
    input: &'a mut [u8],
    out: &'a mut [u8],
    generation: u32,
}

impl NextHost<'_, '_> {
    /// Notes the generation a reply was read in; a second one is a change.
    fn note(&mut self, generation: u32) -> Result<(), Error> {
        match self.generation {
            0 => self.generation = generation,
            seen if seen != generation => return Err(Error::State),
            _ => {}
        }
        Ok(())
    }
}

const fn show(begun: Begun) -> Show {
    match begun {
        Begun::Reading => Show::Reading,
        Begun::Finished => Show::Finished,
    }
}

impl SeriesSource<CatalogCursor> for NextHost<'_, '_> {
    fn names(
        &mut self,
        begun: Begun,
        from: CatalogCursor,
    ) -> Result<SeriesNames<CatalogCursor>, Error> {
        let query = Query::new().show(show(begun)).cursor(from);
        let page = self
            .library
            .facets(self.host, &query, Facet::Series, self.input, self.out)?;
        let end = page.end();
        let mut names = Vec::new();
        for row in page.rows() {
            push(&mut names, copy(row?.text())?)?;
        }
        self.note(end.generation())?;
        Ok(SeriesNames {
            names,
            next: end.next(),
        })
    }

    fn next_of(&mut self, begun: Begun, series: &str) -> Result<Option<BookLine>, Error> {
        // Any begun book of the series: the host answers with the series'
        // lowest unread book (`library.md`, op 6).
        let query = Query::new().show(show(begun)).series(series)?;
        let page = self
            .library
            .query(self.host, &query, self.input, self.out)?;
        let generation = page.end().generation();
        let key = match page.rows().next() {
            Some(row) => row?.key(),
            None => return Ok(None),
        };
        self.note(generation)?;
        let last = BookRef::catalog(key, generation)?;
        match self
            .library
            .next_in_series(self.host, last, self.input, self.out)
        {
            Ok(entry) => Ok(Some(BookLine::from_entry(entry)?)),
            Err(Error::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

/// The names of one shelf from the host's `facets`.
struct NameReplies<'a, 'h> {
    host: &'a Host<'h>,
    library: &'a Library,
    shelf: Shelf,
    input: &'a mut [u8],
    out: &'a mut [u8],
}

impl Source<CatalogCursor, NameLine> for NameReplies<'_, '_> {
    fn read(&mut self, from: CatalogCursor) -> Result<Replied<CatalogCursor, NameLine>, Error> {
        let query = Query::new().cursor(from);
        let page =
            self.library
                .facets(self.host, &query, self.shelf.facet(), self.input, self.out)?;
        let mut books = Vec::new();
        for row in page.rows() {
            let row = row?;
            push(&mut books, {
                let (text, cut) = native_shelves::short(row.text(), native_shelves::MOST_NAME);
                NameLine {
                    text: copy(text)?,
                    cut,
                    count: row.count(),
                }
            })?;
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

    fn count(&mut self, key: &str, count: i64) -> Result<String, Error> {
        self.messages.format(self.host, key, &[Arg::Count(count)])
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

    /// The parameters (`first`, `second`) a screen was opened with; empty
    /// for one that was not given.
    fn handover_params(
        &self,
        host: &Host<'_>,
        handover: Option<HandoverId>,
        keys: [&str; 2],
    ) -> Result<[String; 2], Error> {
        let mut found = [String::new(), String::new()];
        let Some(handover) = handover else {
            return Ok(found);
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        for param in ui.params(host, handover, &mut input, &mut out)? {
            let param = param?;
            if let Some(at) = keys.iter().position(|key| *key == param.key())
                && let Some(slot) = found.get_mut(at)
            {
                *slot = copy(param.value())?;
            }
        }
        Ok(found)
    }

    /// The scope a list was opened for; the library for none.
    fn scope_of(&self, host: &Host<'_>, handover: Option<HandoverId>) -> Result<Scope, Error> {
        let [kind, value] = self.handover_params(host, handover, [KIND, VALUE])?;
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
        } else if token == NAMES_TOKEN {
            let [kind, _] = self.handover_params(host, handover, [KIND, VALUE])?;
            let shelf = Shelf::from_kind(&kind).ok_or(Error::Invalid)?;
            View::Names(self.read_names(
                host,
                shelf,
                Reading::from(CatalogCursor::start(), Vec::new()),
                0,
                0,
            )?)
        } else if token == INFO_TOKEN {
            let [path, _] = self.handover_params(host, handover, [PATH, VALUE])?;
            let library = self.library.as_ref().ok_or(Error::State)?;
            let info = native_info::read(host, library, &path)?;
            View::Info(Info {
                list: ItemList::new(native_info::items(&info)?),
            })
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
        if let Some(View::Names(old)) = self.screens.top(NAMES_TOKEN) {
            let (shelf, held, focus) = (old.shelf, old.window.books.len(), old.list.focus());
            let reading = Reading::from(old.window.start, cursors(&old.starts)?);
            let fresh = self.read_names(host, shelf, reading, held, focus)?;
            if let Some(View::Names(old)) = self.screens.top(NAMES_TOKEN) {
                *old = fresh;
            }
        }
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
        let mut rows = Vec::new();
        let mut items = Vec::new();
        // Always offered: looking for a next book costs host calls for each
        // series, so the list says when there is none (`files.list.none.next`).
        let next = say.copy(host, "files.list.show.next")?;
        push(&mut rows, ShelfRow::Next)?;
        push(&mut items, Item::noted(&next, "›", "")?)?;
        let favourites = say.copy(host, "files.favourites")?;
        push(&mut rows, ShelfRow::Favourites)?;
        push(&mut items, Item::noted(&favourites, "›", "")?)?;
        for shelf in Shelf::ALL {
            let title = say.copy(host, shelf.title_key())?;
            push(&mut rows, ShelfRow::By(shelf))?;
            push(&mut items, Item::noted(&title, "›", "")?)?;
        }
        Ok(Shelves {
            rows,
            list: ItemList::new(items),
        })
    }

    /// The whole name at row `at` of the window that was read from
    /// `start`: the pages are read again, none kept.
    fn whole_name(
        &self,
        host: &Host<'_>,
        shelf: Shelf,
        start: CatalogCursor,
        at: usize,
    ) -> Result<String, Error> {
        let library = self.library.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let (mut cursor, mut before) = (start, 0usize);
        for _ in 0..=native_model::MOST_BOOKS {
            let query = Query::new().cursor(cursor);
            let page = library.facets(host, &query, shelf.facet(), &mut input, &mut out)?;
            if let Some(row) = page.rows().nth(at - before) {
                return copy(row?.text());
            }
            before += page.len();
            match page.end().next() {
                Some(next) if !page.is_empty() => cursor = next,
                _ => break,
            }
        }
        // The catalog changed under the list: it is read again.
        Err(Error::State)
    }

    /// Opens the list of the names of `shelf`.
    fn open_names(&mut self, host: &Host<'_>, shelf: Shelf) -> Result<(), Error> {
        self.ensure(host)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let params = [Param::new(KIND, shelf.kind())?];
        let handover = self
            .ui
            .as_ref()
            .ok_or(Error::State)?
            .handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).open(Open::screen("files.names")?.handover(handover))
    }

    /// The names of `shelf`, `at_least` of them at once and the focus on
    /// row `focus`.
    fn read_names(
        &self,
        host: &Host<'_>,
        shelf: Shelf,
        reading: Reading,
        at_least: usize,
        focus: usize,
    ) -> Result<Names, Error> {
        let library = self.library.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let Reading { start, mut starts } = reading;
        let (window, restarted) = {
            let mut source = NameReplies {
                host,
                library,
                shelf,
                input: &mut input,
                out: &mut out,
            };
            native_model::read_window(&mut source, start, CatalogCursor::start(), at_least.max(1))?
        };
        if restarted {
            starts.clear();
        }
        let mut names = Names {
            shelf,
            window,
            starts,
            rows: Vec::new(),
            list: ItemList::default(),
            empty: String::new(),
        };
        Self::layout_names(host, &mut names)?;
        names.list.set_focus(focus);
        Ok(names)
    }

    /// The rows and items of `names`.
    fn layout_names(host: &Host<'_>, names: &mut Names) -> Result<(), Error> {
        let mut say = Speaker {
            host,
            messages: Messages::new()?,
        };
        let mut rows = Vec::new();
        let mut items = Vec::new();
        if !names.starts.is_empty() {
            let previous = say.say("files.list.previous")?;
            push(&mut rows, Row::Previous)?;
            push(&mut items, Item::noted(&previous, "‹", "")?)?;
        }
        for (at, name) in names.window.books.iter().enumerate() {
            let (title, note) = native_shelves::name_row(&mut say, names.shelf, name)?;
            push(&mut rows, Row::Book(at))?;
            push(&mut items, Item::noted(&title, "›", &note)?)?;
        }
        if names.window.has_more() {
            let more = say.say("files.list.more")?;
            push(&mut rows, Row::More)?;
            push(&mut items, Item::noted(&more, "›", "")?)?;
        }
        names.empty = if names.window.books.is_empty() {
            say.say(names.shelf.none_key())?
        } else {
            String::new()
        };
        names.rows = rows;
        names.list = ItemList::new(items);
        Ok(())
    }

    /// "More" and "Previous" of a list of names (as of books).
    fn slide_names(&mut self, host: &Host<'_>, token: u32, back: bool) -> Result<(), Error> {
        let Some(View::Names(view)) = self.screens.top(token) else {
            return Ok(());
        };
        let shelf = view.shelf;
        let step = if back {
            match step_back(&view.starts)? {
                Some(step) => step,
                None => return Ok(()),
            }
        } else {
            step_on(&view.window, &view.starts, view.list.focus())?
        };
        let mut fresh = self.read_names(host, shelf, step.reading, step.at_least, step.focus)?;
        match step.then {
            Then::Keep => {}
            Then::FirstBook => {
                let at = fresh
                    .rows
                    .iter()
                    .position(|row| matches!(row, Row::Book(_)))
                    .unwrap_or(0);
                fresh.list.set_focus(at);
            }
            Then::LastRow => fresh.list.set_focus(fresh.rows.len().saturating_sub(1)),
        }
        if let Some(View::Names(old)) = self.screens.top(token) {
            *old = fresh;
        }
        redraw(host)
    }

    /// Asks for the words of a search.
    fn ask_search(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.ensure(host)?;
        let mut say = Messages::new()?;
        let title = say.copy(host, "files.search")?;
        let prompt = say.copy(host, "files.search.prompt")?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let request = self.ui.as_ref().ok_or(Error::State)?.ask(
            host,
            Question::text(&title, &prompt)?,
            &mut input,
            &mut out,
        )?;
        self.asked = Some(request);
        Ok(())
    }

    /// The words typed: the books that have them, as a list of their own.
    fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        if self.asked != Some(answer.request()) || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        let Some(request) = self.asked.take() else {
            return Ok(());
        };
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let typed = match uitext::read_all(ui, host, request.text(), SEARCH_LIMIT) {
            Ok(typed) => typed,
            // More than the host searches for: nothing is searched, never a cut word.
            Err(Error::NoSpace) => return self.notify(host, "files.search.long"),
            Err(error) => return Err(error),
        };
        let words = typed.trim();
        if words.is_empty() {
            return Ok(());
        }
        self.open_shelf(host, &Scope::Search(copy(words)?))
    }

    /// A notice, best effort.
    fn notify(&self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        let text = Messages::new()?.copy(host, key)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch)
            .notify(Notify::new(&text)?)
            .ok();
        Ok(())
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
            Scope::Next => say.copy(host, "files.list.show.next"),
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
                _ if browser.selection.scope == Scope::Next => {
                    browser.empty = say.say("files.list.none.next")?;
                }
                _ if browser.selection.scope == Scope::Favourites => {
                    browser.empty = say.say("files.no-favourites.title")?;
                    browser.empty_text = say.say("files.no-favourites")?;
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
            let step = step_on(&view.window, &view.starts, view.list.focus())?;
            Ok(Some(Plan::of(view.selection.clone(), step)))
        })
    }

    /// "Previous books": the window before, read in full, on the row that
    /// goes on again.
    fn slide_back(&mut self, host: &Host<'_>, token: u32) -> Result<(), Error> {
        self.replace_books(host, token, |view| {
            Ok(step_back(&view.starts)?.map(|step| Plan::of(view.selection.clone(), step)))
        })
    }

    fn chosen(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        self.ensure(host)?;
        match self.screens.top(token) {
            Some(View::Shelves(view)) => match view.rows.get(index).copied() {
                Some(ShelfRow::Next) => self.open_shelf(host, &Scope::Next),
                Some(ShelfRow::Favourites) => self.open_shelf(host, &Scope::Favourites),
                Some(ShelfRow::By(shelf)) => self.open_names(host, shelf),
                None => Ok(()),
            },
            Some(View::Names(view)) => {
                let Some(row) = view.rows.get(index).copied() else {
                    return Ok(());
                };
                match row {
                    Row::More => self.slide_names(host, token, false),
                    Row::Previous => self.slide_names(host, token, true),
                    Row::Book(at) => {
                        let Some(name) = view.window.books.get(at) else {
                            return Ok(());
                        };
                        let (shelf, start) = (view.shelf, view.window.start);
                        let scope = if name.cut {
                            // Held cut: the host has the whole name.
                            let whole = self.whole_name(host, shelf, start, at)?;
                            shelf.scope(&whole)?
                        } else {
                            shelf.scope(&name.text)?
                        };
                        self.open_shelf(host, &scope)
                    }
                    Row::Filter | Row::Note => Ok(()),
                }
            }
            Some(View::Info(_)) => Ok(()),
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
