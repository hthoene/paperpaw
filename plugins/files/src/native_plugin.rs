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

mod books;
mod names;
mod open;
mod plan;
mod sources;

use plan::{Plan, Reading, Row, Then, cursors, step_back, step_on};
use sources::{NameReplies, Replies, Speaker};

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

impl NativeFiles {
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
}

fn redraw(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch)
        .queue_redraw(paperpaw_sdk::native::commands::RedrawMode::Partial)
}
