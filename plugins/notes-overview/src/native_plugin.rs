//! The interface-7 All notes: the list of books with notes and the notes of
//! one book, over the library service, and the export of every note to
//! `My Clippings.txt`. The opening of a book at a note needs a host piece
//! that is not there yet: those rows refuse plainly (`PORTING-7.md`).
use crate::declarations::{BOOK_TOKEN, LIST_TOKEN, OPEN_TOKEN};
use crate::model::{self, Counted, Handle};
use crate::native_library::{BUFFER, HostSource};
use crate::native_run::run;
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
        ui::{HandoverId, Instance, Notify, Open, Param, Ui},
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

/// The handover parameters that name a book: its catalog handle and the
/// generation of the catalog it came from, as decimal numbers.
const KEY: &str = "key";
const GENERATION: &str = "generation";
/// Digits of a `u32`.
const DIGITS: usize = 10;

/// `n` in decimal, written into `buf`.
fn digits(mut n: u32, buf: &mut [u8; DIGITS]) -> &str {
    let mut at = DIGITS;
    loop {
        at -= 1;
        buf[at] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    core::str::from_utf8(&buf[at..]).unwrap_or("0")
}

/// Notes of one book shown at most. A book has more only by the thousand;
/// interface 6 showed them all, from a file of up to 1 MiB. With the texts
/// cut at [`model::TEXT_MAX`] and [`model::NOTE_MAX`] a row costs at most
/// 350 bytes (`ROW_WORST` in the tests), so the screen holds 100 x that, about 44 KiB.
pub const MOST_NOTES: usize = 100;
/// A pass over a book's notes ends after this many pages (a book with 300
/// notes has a dozen), so a host that never ends cannot hold the screen.
const MOST_PAGES: usize = 64;

/// The most a row of a note takes: the item (three strings and flags), the
/// quoted text (two curly quotes, 6 bytes), the detail (the note or the kind,
/// then `· 100 %`, at most 16 bytes more) and 8 bytes of allocator header for
/// each of the three strings.
#[cfg(test)]
const ROW_WORST: usize =
    core::mem::size_of::<Item>() + model::TEXT_MAX + 6 + model::NOTE_MAX + 16 + 3 * 8;

/// What a screen's rows stand for.
#[derive(Debug)]
enum Rows {
    /// The books of the list by catalog handle, then the *Export* row.
    Books(Vec<Handle>),
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
    /// The book of a notes screen, to read its rows again.
    book: Option<Handle>,
    /// Whether the rows are held. A notes screen hidden under another one has
    /// none ([`free_notes`]) and reads them again when it is shown.
    loaded: bool,
    /// The focused row when the rows were freed.
    focus: usize,
}

/// Frees the rows of every notes screen that holds them. Rows of a book take
/// up to 45 KiB each, and the stack can hold several books (book A, the Tools
/// menu, All notes again, book B), so only the book on top keeps its rows.
fn free_notes<'a>(views: impl Iterator<Item = &'a mut View>) {
    for view in views {
        if matches!(view.rows, Rows::Notes) && view.loaded {
            view.focus = view.list.focus();
            view.list.replace(Vec::new());
            view.loaded = false;
        }
    }
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
            } if token == LIST_TOKEN || token == BOOK_TOKEN => {
                self.screens.shown(token, instance)?;
                if token == BOOK_TOKEN {
                    self.reload(host, instance)?;
                }
                Ok(())
            }
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
        let book = match &view.rows {
            Rows::Books(handles) => index < handles.len(),
            Rows::Notes => false,
        };
        if book {
            self.open_chosen(host, token, index)?;
            return Ok(Handled::Yes);
        }
        match view.rows {
            Rows::Books(_) => {
                self.export(host)?;
                Ok(Handled::Yes)
            }
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

    /// Writes every note to `My Clippings.txt` and says what came of it.
    fn export(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.ensure(host)?;
        let mut messages = Messages::new()?;
        let text = match run(host, self.library()?) {
            Ok((done, path)) if done.notes > 0 => messages.format(
                host,
                "notes-overview.exported",
                &[
                    Arg::Count(i64::from(done.notes)),
                    Arg::Text {
                        name: "path",
                        value: &path,
                    },
                ],
            )?,
            Ok(_) => messages.copy(host, "notes-overview.none")?,
            Err(_) => messages.copy(host, "notes-overview.export-failed")?,
        };
        let mut scratch = [0; 512];
        CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?)
    }

    /// Opens the notes of the book the row `index` of the list names. A
    /// handle is good in one generation of the catalog: when the catalog has
    /// changed (`State`) or the book is gone (`NotFound`), the list is made
    /// again once, and the book is opened if it is still there (found by its
    /// title, among equal titles by place); else the list shows what is there
    /// now and says that it changed.
    fn open_chosen(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        self.ensure(host)?;
        let Some(Rows::Books(handles)) = self.screens.top(token).map(|view| &view.rows) else {
            return Err(Error::State);
        };
        let handle = *handles.get(index).ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(BUFFER)?, buffer(BUFFER)?);
        let found = BookRef::catalog(handle.key, handle.generation).and_then(|book| {
            self.library()?
                .book(host, book, &mut input, &mut out)
                .map(|_| ())
        });
        match found {
            Ok(()) => self.open_book(host, handle),
            Err(Error::State | Error::NotFound) => self.refresh(host, token, index),
            Err(error) => Err(error),
        }
    }

    /// Makes the list again under the screen of `token`; opens the book that
    /// row `index` of the old list was, or says the list changed.
    fn refresh(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        let (title, place) = {
            let view = self.screens.top(token).ok_or(Error::State)?;
            let (title, place) = place_of(view.list.items(), index).ok_or(Error::State)?;
            (copy(title)?, place)
        };
        let mut messages = Messages::new()?;
        let (handles, items) = self.list_rows(host, &mut messages)?;
        let again = find_again(&items, &handles, &title, place);
        let hint = messages.copy(host, "notes-overview.changed")?;
        let view = self.screens.top(token).ok_or(Error::State)?;
        view.rows = Rows::Books(handles);
        view.list.replace(items);
        match again {
            Some(handle) => self.open_book(host, handle),
            None => {
                let mut scratch = [0; 256];
                CommandSender::new(host, &mut scratch).notify(Notify::new(&hint)?)
            }
        }
    }

    /// Opens the notes of the book named by `handle`, which is handed over as
    /// two numbers; the book's path is never fetched.
    fn open_book(&mut self, host: &Host<'_>, handle: Handle) -> Result<(), Error> {
        let (mut input, mut out) = (buffer(BUFFER)?, buffer(BUFFER)?);
        let (mut key, mut generation) = ([0; DIGITS], [0; DIGITS]);
        let params = [
            Param::new(KEY, digits(handle.key, &mut key))?,
            Param::new(GENERATION, digits(handle.generation, &mut generation))?,
        ];
        let handover = self
            .ui
            .as_ref()
            .ok_or(Error::State)?
            .handover(host, &params, &mut input, &mut out)?;
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch)
            .open(Open::screen("notes-overview.book")?.handover(handover))
    }

    /// The books with notes as handles and rows: each book, then the
    /// *Export* row.
    fn list_rows(
        &self,
        host: &Host<'_>,
        messages: &mut Messages,
    ) -> Result<(Vec<Handle>, Vec<Item>), Error> {
        let listing = {
            let mut source = HostSource::new(host, self.library()?)?;
            scan(&mut source)?
        };
        let mut items = Vec::new();
        let mut handles = Vec::new();
        for book in listing.books() {
            push(&mut handles, book.handle)?;
            let note = counts(host, messages, book)?;
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
        Ok((handles, items))
    }

    fn opened_list(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        self.ensure(host)?;
        let mut messages = Messages::new()?;
        let (handles, items) = self.list_rows(host, &mut messages)?;
        let view = View {
            title: messages.copy(host, "notes-overview.title")?,
            rows: Rows::Books(handles),
            list: ItemList::new(items),
            empty: empty(host, &mut messages)?,
            book: None,
            loaded: true,
            focus: 0,
        };
        self.screens.open(instance, view)
    }

    /// Reads the rows again of a notes screen that was shown after it had
    /// freed them. A book that changed under it leaves it with nothing to
    /// show but what to do.
    fn reload(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        let Some(view) = self.screens.get(instance) else {
            return Ok(());
        };
        let (Rows::Notes, false, Some(handle)) = (&view.rows, view.loaded, view.book) else {
            return Ok(());
        };
        let focus = view.focus;
        // Only one notes screen holds rows: the others are freed first.
        free_notes(self.screens.all());
        let (mut input, mut out) = (buffer(BUFFER)?, buffer(BUFFER)?);
        let mut messages = Messages::new()?;
        let book = BookRef::catalog(handle.key, handle.generation)?;
        let items = match self.notes(host, &mut messages, book, &mut input, &mut out) {
            Ok(items) => Some(items),
            Err(Error::State | Error::NotFound) => None,
            Err(error) => return Err(error),
        };
        let gone = if items.is_none() {
            Some((
                messages.copy(host, "notes-overview.empty.title")?,
                messages.copy(host, "notes-overview.gone")?,
            ))
        } else {
            None
        };
        let view = self.screens.get(instance).ok_or(Error::State)?;
        if let Some(items) = items {
            view.list.replace(items);
            view.list.set_focus(focus);
        }
        if let Some(gone) = gone {
            view.empty = gone;
        }
        view.loaded = true;
        Ok(())
    }

    fn opened_book(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        handover: Option<HandoverId>,
    ) -> Result<(), Error> {
        self.ensure(host)?;
        let (mut input, mut out) = (buffer(BUFFER)?, buffer(BUFFER)?);
        let (mut key, mut generation) = (None, None);
        if let Some(handover) = handover {
            for param in self
                .ui
                .as_ref()
                .ok_or(Error::State)?
                .params(host, handover, &mut input, &mut out)?
            {
                let param = param?;
                match param.key() {
                    KEY => key = param.value().parse().ok(),
                    GENERATION => generation = param.value().parse().ok(),
                    _ => {}
                }
            }
        }
        // A handover without the two numbers names no book.
        let (Some(key), Some(generation)) = (key, generation) else {
            return Err(Error::Invalid);
        };
        let book = BookRef::catalog(key, generation)?;
        // The book on top is the only one with rows: the others are freed
        // before this one is read.
        free_notes(self.screens.all());
        let mut messages = Messages::new()?;
        // A list older than a change to the catalog is `State` here, as is a
        // book that is gone (`NotFound`): the screen is not opened.
        let title = copy(
            self.library()?
                .book(host, book, &mut input, &mut out)?
                .title(),
        )?;
        let items = self.notes(host, &mut messages, book, &mut input, &mut out)?;
        let view = View {
            title,
            rows: Rows::Notes,
            list: ItemList::new(items),
            empty: empty(host, &mut messages)?,
            book: Some(Handle { key, generation }),
            loaded: true,
            focus: 0,
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
        book: BookRef<'_>,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<Vec<Item>, Error> {
        let library = self.library()?;
        let mut items = Vec::new();
        let mut cursor = AnnotationCursor::start();
        for _ in 0..MOST_PAGES {
            let page = library.annotations(host, book, cursor, input, out)?;
            for row in page.rows() {
                let row = row?;
                let (kind, text) = match row.kind() {
                    AnnotationKind::Bookmark => (
                        "notes-overview.bookmark",
                        copy(model::cut(row.text(), model::TEXT_MAX))?,
                    ),
                    AnnotationKind::Highlight => (
                        "notes-overview.highlight",
                        messages.format(
                            host,
                            "notes-overview.quoted",
                            &[Arg::Text {
                                name: "text",
                                value: model::cut(row.text(), model::TEXT_MAX),
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
                    copy(model::cut(row.note(), model::NOTE_MAX))?
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

/// The title of row `index` and how many rows before it have the same title.
fn place_of(items: &[Item], index: usize) -> Option<(&str, usize)> {
    let Some(Item::Row(row)) = items.get(index) else {
        return None;
    };
    let place = items[..index]
        .iter()
        .filter(|item| matches!(item, Item::Row(other) if other.title == row.title))
        .count();
    Some((&row.title, place))
}

/// The book of the rows `items` (the first `handles.len()` are books) that is
/// the `place`th with `title`.
fn find_again(items: &[Item], handles: &[Handle], title: &str, place: usize) -> Option<Handle> {
    items
        .iter()
        .zip(handles)
        .filter(|(item, _)| matches!(item, Item::Row(row) if row.title == title))
        .nth(place)
        .map(|(_, handle)| *handle)
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

#[cfg(test)]
mod tests {
    use super::{
        DIGITS, Handle, Item, ItemList, Rows, View, digits, find_again, free_notes, place_of,
    };
    use alloc::string::String;
    use alloc::vec::Vec;

    fn rows(titles: &[&str]) -> (Vec<Item>, Vec<Handle>) {
        let items = titles
            .iter()
            .map(|title| Item::noted(title, "", "").unwrap())
            .collect();
        let handles = (0..titles.len() as u32)
            .map(|key| Handle {
                key: key + 50,
                generation: 2,
            })
            .collect();
        (items, handles)
    }

    #[test]
    fn a_book_is_found_again_by_its_title_and_its_place_among_equal_titles() {
        // The old list: two books of one title; the second was chosen.
        let (old, _) = rows(&["Apple", "Apple", "Pear", "Export"]);
        assert_eq!(place_of(&old, 1), Some(("Apple", 1)));
        assert_eq!(place_of(&old, 2), Some(("Pear", 0)));
        assert_eq!(place_of(&old, 9), None);
        // The new list has a book added before them: the handles are new, the
        // book is the same.
        let (new, handles) = rows(&["Aaa", "Apple", "Apple", "Pear", "Export"]);
        let handles = &handles[..4];
        assert_eq!(find_again(&new, handles, "Apple", 1), Some(handles[2]));
        assert_eq!(find_again(&new, handles, "Pear", 0), Some(handles[3]));
        // The second Apple is gone: not found, and so is a title never listed.
        let (new, handles) = rows(&["Apple", "Pear", "Export"]);
        assert_eq!(find_again(&new, &handles[..2], "Apple", 1), None);
        // The *Export* row is not a book, even under the title of a book.
        assert_eq!(find_again(&new, &handles[..2], "Export", 0), None);
        assert_eq!(find_again(&new, &handles[..2], "Quince", 0), None);
    }

    #[test]
    fn the_notes_of_a_book_fit_in_48_kib_at_worst() {
        // The rows of a screen, and the vector holding them grown to the next
        // power of two (an item is 80 bytes or so).
        let rows = super::MOST_NOTES * super::ROW_WORST;
        let vector = super::MOST_NOTES.next_power_of_two() * core::mem::size_of::<Item>();
        assert!(rows + vector <= 48 * 1024, "{}", rows + vector);
    }

    fn notes_view(rows: &[&str], focus: usize) -> View {
        let items: Vec<Item> = rows
            .iter()
            .map(|t| Item::noted(t, "", "").unwrap())
            .collect();
        let mut list = ItemList::new(items);
        list.set_focus(focus);
        View {
            title: String::new(),
            rows: Rows::Notes,
            list,
            empty: (String::new(), String::new()),
            book: Some(Handle {
                key: 1,
                generation: 1,
            }),
            loaded: true,
            focus: 0,
        }
    }

    #[test]
    fn opening_a_second_book_frees_the_rows_of_the_first() {
        // Book A under the list, then book B opens: only B keeps rows.
        let mut a = notes_view(&["one", "two", "three"], 2);
        let mut list = notes_view(&["Book A"], 0);
        list.rows = Rows::Books(Vec::new());
        list.book = None;
        let mut b = notes_view(&["x", "y"], 1);
        free_notes([&mut a, &mut list].into_iter());
        assert!(!a.loaded);
        assert!(a.list.items().is_empty());
        assert_eq!(a.focus, 2, "the focus is kept for the reload");
        assert!(a.book.is_some(), "the book is kept to read it again");
        // The list of books is not a notes screen: its rows stay.
        assert!(list.loaded);
        assert_eq!(list.list.items().len(), 1);
        // B is loaded after A was freed; freeing again changes nothing of A,
        // and frees B when a third book opens.
        free_notes([&mut a, &mut b].into_iter());
        assert!(a.list.items().is_empty());
        assert!(!b.loaded);
        assert!(b.list.items().is_empty());
        assert_eq!(b.focus, 1);
    }

    #[test]
    fn numbers_are_written_in_decimal_without_allocating() {
        let mut buf = [0; DIGITS];
        assert_eq!(digits(0, &mut buf), "0");
        assert_eq!(digits(7, &mut buf), "7");
        assert_eq!(digits(1_000, &mut buf), "1000");
        assert_eq!(digits(u32::MAX, &mut buf), "4294967295");
        assert_eq!("4294967295".parse::<u32>().ok(), Some(u32::MAX));
    }
}
