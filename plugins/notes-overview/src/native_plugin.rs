//! The interface-7 All notes: the list of books with notes and the notes of
//! one book, over the library service. The export and the opening of a book
//! at a note need host pieces that are not there yet: those rows refuse
//! plainly (`PORTING-7.md`).
use crate::declarations::{BOOK_TOKEN, LIST_TOKEN, OPEN_TOKEN};
use crate::model::{self, Counted};
use crate::native_library::{BUFFER, HostSource};
use crate::native_scan::scan;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::CommandSender,
        events::{Event, Incoming, kinds},
        formatting::Arg,
        library::{AnnotationCursor, AnnotationKind, BookRef, Library},
        registrations,
        reply::Reply,
        ui::{HandoverId, Instance, Open, Param, Ui},
    },
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Screens,
    mem::{buffer, push},
    read_event,
    text::copy,
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/notes_overview_declarations.rs"));
}

/// Notes of one book shown at most. A book has more only by the thousand;
/// interface 6 showed them all, from a file of up to 1 MiB.
pub const MOST_NOTES: usize = 300;
/// A pass over a book's notes ends after this many pages (a book with 300
/// notes has a dozen), so a host that never ends cannot hold the screen.
const MOST_PAGES: usize = 64;

/// What a screen's rows stand for.
#[derive(Debug)]
enum Rows {
    /// The books of the list by path, then the *Export* row.
    Books(Vec<String>),
    /// The notes of a book.
    Notes,
}

/// One open screen.
#[derive(Debug)]
struct View {
    title: String,
    rows: Rows,
    list: ItemList,
    /// What the screen says with no rows: the heading and the text.
    empty: (String, String),
}

/// The interface-7 All notes.
#[derive(Debug, Default)]
pub struct NativeNotesOverview {
    ui: Option<Ui>,
    library: Option<Library>,
    screens: Screens<View>,
}

impl Plugin for NativeNotesOverview {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::UI {
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
        let Some(event) = read_event!(incoming; kinds::ActionRun, kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenClosed)
        else {
            return Ok(());
        };
        match event {
            Event::ActionRun {
                action: "notes-overview.open",
                ..
            } if token == OPEN_TOKEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("notes-overview.screen")?)
            }
            Event::ScreenOpened { instance, .. } if token == LIST_TOKEN => {
                self.opened_list(host, instance)
            }
            Event::ScreenOpened {
                instance, handover, ..
            } if token == BOOK_TOKEN => self.opened_book(host, instance, handover),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == LIST_TOKEN || token == BOOK_TOKEN => self.screens.shown(token, instance),
            Event::ScreenClosed { instance, .. } => {
                self.screens.close(instance);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let view = self.screens.top(token).ok_or(Error::State)?;
        let View {
            title, list, empty, ..
        } = view;
        let footer = if list.items().is_empty() {
            Footer::Empty {
                title: &empty.0,
                text: &empty.1,
            }
        } else {
            Footer::None
        };
        list.draw_with(host, &mut frame, title, true, footer)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        let step = view.list.input(host, &input)?;
        let ItemStep::Chosen(index) = step else {
            return Ok(ItemList::handled(step));
        };
        match &view.rows {
            Rows::Books(paths) => match paths.get(index) {
                Some(path) => {
                    let path = copy(path)?;
                    self.open_book(host, &path)?;
                    Ok(Handled::Yes)
                }
                // BLOCKED (#228): the export writes through `core.files`
                // (PORTING-7.md); the row refuses plainly until it is wired.
                None => Err(Error::Unsupported),
            },
            // BLOCKED (#228): the book opens at the note with `command.open`
            // into `core.reader` and the goto event (draft 22 `doc_open`).
            Rows::Notes => Err(Error::Unsupported),
        }
    }
}

impl NativeNotesOverview {
    fn ensure(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        if self.library.is_none() {
            self.library = Some(Library::open(host)?);
        }
        Ok(())
    }

    fn library(&self) -> Result<&Library, Error> {
        self.library.as_ref().ok_or(Error::State)
    }

    /// Opens the notes of the book at `path` with the path handed over.
    fn open_book(&mut self, host: &Host<'_>, path: &str) -> Result<(), Error> {
        self.ensure(host)?;
        let (mut input, mut out) = (buffer(BUFFER)?, buffer(BUFFER)?);
        let params = [Param::new("path", path)?];
        let handover = self
            .ui
            .as_ref()
            .ok_or(Error::State)?
            .handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch)
            .open(Open::screen("notes-overview.book")?.handover(handover))
    }

    fn opened_list(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        self.ensure(host)?;
        let listing = {
            let mut source = HostSource::new(host, self.library()?)?;
            scan(&mut source)?
        };
        let mut messages = Messages::new()?;
        let mut items = Vec::new();
        let mut paths = Vec::new();
        for book in listing.books() {
            push(&mut paths, copy(&book.path)?)?;
            let note = counts(host, &mut messages, book)?;
            push(&mut items, Item::noted(&book.title, "", &note)?)?;
        }
        if !listing.books().is_empty() {
            let help = if listing.more() {
                "notes-overview.export.help.more"
            } else {
                "notes-overview.export.help"
            };
            let (title, note) = (
                messages.copy(host, "notes-overview.export")?,
                messages.copy(host, help)?,
            );
            push(&mut items, Item::noted(&title, "›", &note)?)?;
        }
        let view = View {
            title: messages.copy(host, "notes-overview.title")?,
            rows: Rows::Books(paths),
            list: ItemList::new(items),
            empty: empty(host, &mut messages)?,
        };
        self.screens.open(instance, view)
    }

    fn opened_book(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        handover: Option<HandoverId>,
    ) -> Result<(), Error> {
        self.ensure(host)?;
        let (mut input, mut out) = (buffer(BUFFER)?, buffer(BUFFER)?);
        let mut path = String::new();
        if let Some(handover) = handover {
            for param in self
                .ui
                .as_ref()
                .ok_or(Error::State)?
                .params(host, handover, &mut input, &mut out)?
            {
                let param = param?;
                if param.key() == "path" {
                    path = copy(param.value())?;
                }
            }
        }
        let mut messages = Messages::new()?;
        let title = match self
            .library()?
            .book(host, BookRef::path(&path)?, &mut input, &mut out)
        {
            Ok(book) => copy(book.title())?,
            Err(Error::NotFound) => copy(paperpaw_sdk::files::name(&path))?,
            Err(error) => return Err(error),
        };
        let items = self.notes(host, &mut messages, &path, &mut input, &mut out)?;
        let view = View {
            title,
            rows: Rows::Notes,
            list: ItemList::new(items),
            empty: empty(host, &mut messages)?,
        };
        self.screens.open(instance, view)
    }

    /// The rows of a book's notes, in reading order: bookmarks as their
    /// label, highlights quoted, each with its own note (else its kind) and
    /// how far into the book it lies. Kinds a newer reader keeps are not
    /// listed.
    fn notes(
        &self,
        host: &Host<'_>,
        messages: &mut Messages,
        path: &str,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<Vec<Item>, Error> {
        let library = self.library()?;
        let mut items = Vec::new();
        let mut cursor = AnnotationCursor::start();
        for _ in 0..MOST_PAGES {
            let page = library.annotations(host, BookRef::path(path)?, cursor, input, out)?;
            for row in page.rows() {
                let row = row?;
                let (kind, text) = match row.kind() {
                    AnnotationKind::Bookmark => ("notes-overview.bookmark", copy(row.text())?),
                    AnnotationKind::Highlight => (
                        "notes-overview.highlight",
                        messages.format(
                            host,
                            "notes-overview.quoted",
                            &[Arg::Text {
                                name: "text",
                                value: row.text(),
                            }],
                        )?,
                    ),
                    _ => continue,
                };
                if items.len() >= MOST_NOTES {
                    return Ok(items);
                }
                let what = if row.note().is_empty() {
                    messages.copy(host, kind)?
                } else {
                    copy(row.note())?
                };
                let mut detail = String::new();
                model::put_detail(
                    &mut detail,
                    &what,
                    u8::try_from(row.percent()).unwrap_or(100),
                )?;
                push(&mut items, Item::noted(&text, "", &detail)?)?;
            }
            match page.end().next() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        Ok(items)
    }
}

/// `3 highlights · 1 bookmark`.
fn counts(host: &Host<'_>, messages: &mut Messages, book: &Counted) -> Result<String, Error> {
    let mut out = String::new();
    for (key, n) in model::count_parts(book).into_iter().flatten() {
        let part = messages.format(host, key, &[Arg::Count(i64::from(n))])?;
        if !out.is_empty() {
            model::Sink::put(&mut out, model::COUNT_SEPARATOR)?;
        }
        model::Sink::put(&mut out, &part)?;
    }
    Ok(out)
}

/// What an empty screen says.
fn empty(host: &Host<'_>, messages: &mut Messages) -> Result<(String, String), Error> {
    Ok((
        messages.copy(host, "notes-overview.empty.title")?,
        messages.copy(host, "notes-overview.empty")?,
    ))
}
