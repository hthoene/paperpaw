//! The flat library plugin: the library with its filters, a list of books
//! of one scope (the favourites, an author, a series, a tag, a folder, a
//! search, the next books of the series begun), the shelves with their
//! lists of names, and book information. What is not here yet is in
//! `PORTING-7.md`.
use crate::{
    declarations::{
        BROWSER_TOKEN, BY_AUTHOR_TOKEN, BY_SERIES_TOKEN, BY_TAG_TOKEN, CHOOSE_TOKEN, EVENTS_TOKEN,
        FAVOURITES_TOKEN, INFO_TOKEN, NAMES_TOKEN, SEARCH_TOKEN, SHELF_TOKEN, SHELVES_OPEN_TOKEN,
        SHELVES_TOKEN, SORT, VIEW, VIEW_COVERS,
    },
    native_covers::{Covers, MOST, MOST_SMALL},
    native_draw::Said,
    native_info,
    native_list::{BookList, ListStep, Row},
    native_model::{
        self, BookLine, Filter, MOST_BOOKS, Order, Replied, Say, Scope, Selection, Source, Window,
    },
    native_shelves::{
        self, Begun, MOST_SERIES, NameLine, SeriesNames, SeriesSource, Shelf, collect_next,
    },
    native_states::Condition,
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::CommandSender,
        device::{Device, DeviceClass},
        events::{AnswerOutcome, Answered, Event, Incoming, kinds},
        files::Files,
        formatting::Arg,
        image::ImageId,
        library::{BookRef, CatalogCursor, Facet, Library, Query, Show},
        registrations,
        reply::Reply,
        settings::{self, SettingValue},
        storage::Root,
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
/// The archive folder's setting.
const ARCHIVE_SETTING: &str = "files.archive";
/// The most bytes of words a search takes (the host's cap, never cut).
const SEARCH_LIMIT: usize = 256;

mod books;
mod choose;
mod names;
mod open;
mod plan;
mod sheet;
mod sources;

use plan::{Plan, Reading, Then, cursors, step_back, step_on};
use sources::{NameReplies, Replies, Speaker};

/// What a question that was asked is for.
#[derive(Debug)]
enum Asked {
    /// The words of a search.
    Search,
    /// What to do with a book held: its path and what the sheet offered.
    Sheet {
        path: String,
        actions: Vec<crate::native_sheet::Action>,
    },
    /// Whether to delete the book at this path.
    Delete { path: String },
    /// The name of a new folder in the chooser.
    NewFolder,
}

/// The folder chooser of a setting: the folder on show, its folders and rows.
#[derive(Debug)]
struct Chooser {
    /// The setting the folder is written to; empty for none.
    setting: String,
    /// The folder on show; empty for the top of the card.
    dir: String,
    names: Vec<String>,
    picks: Vec<crate::native_choose::Pick>,
    list: ItemList,
}

#[derive(Debug)]
struct Browser {
    title: String,
    selection: Selection,
    window: Window,
    /// Where the windows before this one began, the nearest last.
    starts: Vec<CatalogCursor>,
    list: BookList,
    /// The covers of the window's books.
    covers: Covers,
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

/// Book information: the book as a card with its cover above the rows.
#[derive(Debug)]
struct Info {
    list: BookList,
    covers: Covers,
    /// The book's catalog key and generation, for its cover.
    cover: Option<(u32, u32)>,
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
    Chooser(Chooser),
}

/// The interface-7 library plugin.
#[derive(Debug, Default)]
pub struct NativeFiles {
    library: Option<Library>,
    ui: Option<Ui>,
    screens: Screens<View>,
    /// The question that is open, and what it is for.
    asked: Option<(RequestId, Asked)>,
    /// Covers held at once (fewer on a device of the small class); 0 until learned.
    covers_most: usize,
}

impl Plugin for NativeFiles {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::SCREENS)
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
        }
        // Help hints are a help: a host that takes none (yet) must not stop the
        // library from loading.
        for batch in image::HELP {
            registrations::submit(host, batch).ok();
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
        let Some(event) = read_event!(incoming; kinds::ActionRun, kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenClosed, kinds::Answered, kinds::LibraryChanged)
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
                self.close(host, instance);
                Ok(())
            }
            Event::LibraryChanged { what, .. } if token == EVENTS_TOKEN => {
                self.library_changed(host, what)
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        match self.screens.top(token).ok_or(Error::State)? {
            View::Books(view) => {
                let said = Said {
                    title: &view.empty,
                    text: &view.empty_text,
                };
                let mut asker = sources::Asker::new(
                    host,
                    self.library.as_ref(),
                    sources::Keys::Books(&view.window.books),
                    view.window.generation,
                );
                view.list.draw(
                    host,
                    &mut frame,
                    &view.title,
                    &mut view.covers,
                    &mut asker,
                    said,
                )
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
            View::Chooser(view) => {
                let title = if view.dir.is_empty() {
                    messages.copy(host, "files.choose.top")?
                } else {
                    copy(&view.dir)?
                };
                view.list
                    .draw_with(host, &mut frame, &title, true, Footer::None)
            }
            View::Info(view) => {
                let (key, generation) = view.cover.unwrap_or((0, 0));
                let mut asker = sources::Asker::new(
                    host,
                    self.library.as_ref(),
                    // A book the catalog does not know has no cover to ask for.
                    if view.cover.is_some() {
                        sources::Keys::One(key)
                    } else {
                        sources::Keys::Books(&[])
                    },
                    generation,
                );
                view.list.draw(
                    host,
                    &mut frame,
                    "files.info",
                    &mut view.covers,
                    &mut asker,
                    Said::default(),
                )
            }
        }
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        let list = match view {
            View::Books(view) => {
                let step = view.list.input(host, &input)?;
                self.books_step(host, token, step)?;
                return Ok(if step == ListStep::Ignored {
                    Handled::No
                } else {
                    Handled::Yes
                });
            }
            View::Info(view) => {
                // A card and rows: the focus moves, nothing is chosen.
                let step = view.list.input(host, &input)?;
                return Ok(if step == ListStep::Ignored {
                    Handled::No
                } else {
                    Handled::Yes
                });
            }
            View::Chooser(view) => {
                // Back goes up a folder before it leaves.
                if matches!(
                    input,
                    Input::Action {
                        action: "core.back",
                        ..
                    }
                ) && !view.dir.is_empty()
                {
                    return self
                        .chooser_back(host, token)
                        .map(|up| if up { Handled::Yes } else { Handled::No });
                }
                &mut view.list
            }
            View::Shelves(view) => &mut view.list,
            View::Names(view) => &mut view.list,
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
        || token == CHOOSE_TOKEN
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
            Self::put_browser(host, self.screens.top(token), fresh);
        }
        redraw(host)
    }

    /// The host says the library changed: books, their status and the walk
    /// read the lists again; a cover that is ready is asked for again.
    fn library_changed(
        &mut self,
        host: &Host<'_>,
        what: paperpaw_sdk::native::events::LibraryChanges,
    ) -> Result<(), Error> {
        let mut redraw_now = false;
        if what.cover() {
            for token in [BROWSER_TOKEN, SHELF_TOKEN] {
                if let Some(View::Books(view)) = self.screens.top(token) {
                    redraw_now |= view.covers.ready_somewhere();
                }
            }
        }
        if what.books() || what.status() || what.fields() || what.walk() {
            // A list that cannot be read again now stays as it is: the next
            // change or the next show reads it.
            return match self.reread(host) {
                Err(Error::NoMemory | Error::State) => Ok(()),
                other => other,
            };
        }
        if redraw_now { redraw(host) } else { Ok(()) }
    }

    /// Gives the host back the images of covers (best effort: an id that is
    /// already gone is no error to the host).
    fn give_back(host: &Host<'_>, covers: &mut Covers, keep: &[usize]) {
        let mut scratch = [0; 64];
        let mut sender = CommandSender::new(host, &mut scratch);
        covers.drop_outside(keep, |id: ImageId| {
            sender.image_release(id).ok();
        });
    }

    /// `fresh` takes the place of the list on show; the old one's covers go back.
    fn put_browser(host: &Host<'_>, old: Option<&mut View>, fresh: Browser) {
        if let Some(View::Books(old)) = old {
            Self::give_back(host, &mut old.covers, &[]);
            *old = fresh;
        }
    }

    /// A screen left: its covers go back to the host.
    fn close(&mut self, host: &Host<'_>, instance: Instance) {
        match self.screens.get(instance) {
            Some(View::Books(view)) => Self::give_back(host, &mut view.covers, &[]),
            Some(View::Info(view)) => Self::give_back(host, &mut view.covers, &[]),
            _ => {}
        }
        self.screens.close(instance);
    }

    /// What a step on the list of books does besides moving the focus.
    fn books_step(&mut self, host: &Host<'_>, token: u32, step: ListStep) -> Result<(), Error> {
        match step {
            ListStep::Focus(_) => {
                // The page may have changed: covers of rows that left it go back.
                if let Some(View::Books(view)) = self.screens.top(token) {
                    let visible = view.list.visible_books()?;
                    Self::give_back(host, &mut view.covers, &visible);
                }
                Ok(())
            }
            ListStep::Chosen(index) => self.chosen(host, token, index),
            ListStep::Held(index) => self.offer(host, token, index),
            ListStep::Menu => self.open_menu(host, "files.menu"),
            ListStep::Header(_) | ListStep::Ignored => Ok(()),
        }
    }
}

fn redraw(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch)
        .queue_redraw(paperpaw_sdk::native::commands::RedrawMode::Partial)
}
