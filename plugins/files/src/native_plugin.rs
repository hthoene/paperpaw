//! The flat library plugin (slice 1): the library with its filters, a list
//! of books of one scope and the shelves. What is not here yet is in
//! `PORTING-7.md`.
use crate::{
    declarations::{
        BROWSER_TOKEN, FAVOURITES_TOKEN, SHELF_TOKEN, SHELVES_OPEN_TOKEN, SHELVES_TOKEN, SORT,
    },
    native_model::{self, BookLine, Filter, MOST_BOOKS, Order, Say, Scope, Selection, Window},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::CommandSender,
        events::Event,
        formatting::Arg,
        library::{BookRef, CatalogCursor, CatalogPhase, Library},
        registrations,
        reply::Reply,
        settings::{self, SettingValue},
        ui::{HandoverId, Instance, Open, Param, Ui},
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
/// Replies read at most while one window is read again (the window's bound).
const MOST_PAGES: usize = MOST_BOOKS.div_ceil(32);

/// What a row of a list of books does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    /// Steps the filter.
    Filter,
    /// Says the books are still being found.
    Note,
    /// Opens the book at this index of the window.
    Book(usize),
    /// Reads more books.
    More,
}

#[derive(Debug)]
struct Browser {
    title: String,
    selection: Selection,
    window: Window,
    rows: Vec<Row>,
    list: ItemList,
    /// What an empty list says; empty when it is not empty.
    empty: String,
}

#[derive(Debug)]
struct Shelves {
    list: ItemList,
}

#[derive(Debug)]
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
        event: Event<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
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
                        text: "",
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
            View::Books(self.read_browser(host, selection, 0, 0)?)
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
            let fresh = self.read_browser(host, selection, held, focus)?;
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
        at_least: usize,
        focus: usize,
    ) -> Result<Browser, Error> {
        let library = self.library.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let mut say = Messages::new()?;
        let title = Self::title(host, &mut say, &selection.scope)?;
        let mut window = Window::default();
        self.read_pages(
            host,
            &selection,
            &mut window,
            at_least.max(1),
            &mut input,
            &mut out,
        )?;
        // The books are still being found: the rows so far say so.
        let walking = matches!(
            library.state(host, &mut input, &mut out)?.phase(),
            CatalogPhase::Walking | CatalogPhase::Empty
        ) && selection.scope == Scope::Library;
        let mut browser = Browser {
            title,
            selection,
            window,
            rows: Vec::new(),
            list: ItemList::default(),
            empty: String::new(),
        };
        self.layout(host, &mut browser, walking)?;
        browser.list.set_focus(focus);
        Ok(browser)
    }

    /// Reads replies into `window` until it holds `want` books or the last
    /// one was read; a stale cursor starts the window over once.
    fn read_pages(
        &self,
        host: &Host<'_>,
        selection: &Selection,
        window: &mut Window,
        want: usize,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        let library = self.library.as_ref().ok_or(Error::State)?;
        let mut restarted = false;
        let mut cursor = window.next.unwrap_or_else(CatalogCursor::start);
        for _ in 0..MOST_PAGES {
            let query = selection.query(cursor)?;
            let page = match library.query(host, &query, input, out) {
                Ok(page) => page,
                // The catalog changed under the reading: start over, once.
                Err(Error::State) if !restarted => {
                    restarted = true;
                    *window = Window::default();
                    cursor = CatalogCursor::start();
                    continue;
                }
                Err(error) => return Err(error),
            };
            for entry in page.rows() {
                push(&mut window.books, BookLine::from_entry(entry?)?)?;
            }
            let end = page.end();
            window.generation = end.generation();
            window.total = end.total();
            window.next = end.next();
            match window.next {
                Some(next) if window.books.len() < want.min(MOST_BOOKS) => cursor = next,
                _ => return Ok(()),
            }
        }
        Ok(())
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
            push(&mut rows, Row::Filter)?;
            push(&mut items, Item::row(&show, &name)?)?;
        }
        if walking {
            let note = say.say("files.list.collecting")?;
            push(&mut rows, Row::Note)?;
            push(&mut items, Item::heading(&note)?)?;
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
        browser.empty = if browser.window.books.is_empty() && !walking {
            match browser.selection.filter.none_key() {
                Some(key) if browser.selection.scope == Scope::Library => say.say(key)?,
                _ if browser.selection.scope == Scope::Library => say.say("files.empty.title")?,
                _ => say.say("files.search.none")?,
            }
        } else {
            String::new()
        };
        browser.rows = rows;
        browser.list = ItemList::new(items);
        Ok(())
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
                        let fresh = self.read_browser(host, selection, 0, index)?;
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
                    Row::More => {
                        let (selection, held) = (view.selection.clone(), view.window.books.len());
                        let focus = view.list.focus();
                        let fresh = self.read_browser(host, selection, held + 32, focus)?;
                        if let Some(View::Books(old)) = self.screens.top(token) {
                            *old = fresh;
                        }
                        redraw(host)
                    }
                    Row::Book(at) => {
                        let Some(book) = view.window.books.get(at) else {
                            return Ok(());
                        };
                        let reference = BookRef::catalog(book.key, view.window.generation)?;
                        self.open_book(host, reference)
                    }
                }
            }
            None => Ok(()),
        }
    }

    /// Opens the book in the reader the user bound to `core.reader`: a
    /// hand-over with its path (`document.md` section 1).
    fn open_book(&self, host: &Host<'_>, reference: BookRef<'_>) -> Result<(), Error> {
        let library = self.library.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let path = match library.book(host, reference, &mut input, &mut out) {
            Ok(book) => copy(book.path())?,
            // The catalog changed: the list is read again when shown.
            Err(Error::State | Error::NotFound) => return Ok(()),
            Err(error) => return Err(error),
        };
        let params = [Param::new("path", &path)?];
        let handover = self
            .ui
            .as_ref()
            .ok_or(Error::State)?
            .handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).open(Open::slot("core.reader")?.handover(handover))
    }
}

fn redraw(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch)
        .queue_redraw(paperpaw_sdk::native::commands::RedrawMode::Partial)
}
