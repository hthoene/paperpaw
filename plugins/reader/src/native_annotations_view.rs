//! The annotations screen (`reader.annotations`): the bookmarks and highlights
//! of the book in a list (`core.library` op 15, in reading order), a choice that
//! tells the reading screen where to go, and *Export as Markdown* at the end of
//! the list. The model (rows, Markdown, file names, the order of the export) is
//! `native_annotations`; this is the part that meets the host.
//!
//! * **List.** At most [`MAX_ROWS`] notes are held (the rest are named in a
//!   note under the list; the export has them all), each as the one row the
//!   list draws (its text cut at [`TEXT_SHOWN`] bytes), so a note costs its
//!   row and eight bytes of place.
//! * **Export.** The Markdown is written in pieces of at most [`PIECE`] bytes
//!   into `<file>.new` (the highlights from one pass over the notes, the
//!   bookmarks from a second, so nothing but a page of notes is in memory) and
//!   moved over the old file at the end; the stage `reader.notes-exported` runs
//!   after a successful write only. A refused write says so and leaves the old
//!   file.
use crate::{
    native_annotations::{self as model, Headings, Kind, Note},
    native_notes,
    native_toc::Place,
    native_view::{Handover, read_handover},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host,
        callbacks::{DrawFrame, Input},
        commands::CommandSender,
        files::{self, Files},
        formatting::Arg,
        library::{AnnotationCursor, AnnotationKind, BookRef, Library},
        settings::{self, SettingValue},
        snapshot,
        storage::{self, MAX_DATA, Root},
        ui::{HandoverId, Notify, Open, Param, Ui},
    },
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages,
    mem::{SERVICE_BUFFER, buffer},
    text,
};

/// The notes a list holds; a book with more shows the first and says so.
pub const MAX_ROWS: usize = 100;
/// The bytes of a note's text its row shows (cut at a character).
pub const TEXT_SHOWN: usize = 160;
/// The most one write to the card carries; the Markdown goes in pieces of it.
pub const PIECE: usize = MAX_DATA;
/// How often a list is read again when the notes changed under the read.
const TRIES: usize = 3;
/// The setting that names the library folder (`Notes` goes beside it).
const LIBRARY_FOLDER: &str = "core.library-folder";

/// One open annotations screen.
#[derive(Debug)]
pub struct AnnotationsView {
    /// The book's path.
    path: String,
    /// Where each note leads, in the order of the rows (the export row follows).
    places: Vec<Place>,
    list: ItemList,
    /// More notes than [`MAX_ROWS`]: the first are shown.
    more: bool,
    /// Why there is nothing to list (the key of the text).
    message: Option<&'static str>,
}

/// What an input on the screen meant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing the screen handles.
    Ignored,
    /// Handled; the screen stays.
    Handled,
    /// Go to this place and close the screen.
    Go(Place),
}

/// `text` cut to at most `most` bytes, at a character.
fn cut(text: &str, most: usize) -> &str {
    let mut end = text.len().min(most);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or("")
}

/// The kind a row of the library is shown as: what is not a bookmark is a
/// highlight (as Beta 3 showed a kind it did not know).
fn kind_of(kind: AnnotationKind) -> Kind {
    match kind {
        AnnotationKind::Bookmark => Kind::Bookmark,
        _ => Kind::Highlight,
    }
}

/// The percent a row says (the library's 0-100).
fn percent_of(percent: u32) -> u8 {
    u8::try_from(percent.min(100)).unwrap_or(100)
}

impl AnnotationsView {
    fn none(message: &'static str) -> Self {
        Self {
            path: String::new(),
            places: Vec::new(),
            list: ItemList::default(),
            more: false,
            message: Some(message),
        }
    }

    /// The notes of the book the hand-over names, read.
    ///
    /// # Errors
    /// `NoMemory`; the host's other refusals are a screen that says so.
    pub fn open(
        host: &Host<'_>,
        ui: &Ui,
        library: Option<&Library>,
        handover: Option<HandoverId>,
    ) -> Result<Self, Error> {
        let found = match handover {
            Some(handover) => read_handover(host, ui, handover).unwrap_or_default(),
            None => Handover::default(),
        };
        if found.path.is_empty() {
            return Ok(Self::none("reader.no-book"));
        }
        let Some(library) = library else {
            return Ok(Self::none("reader.error"));
        };
        let mut view = Self::none("reader.error");
        view.path = found.path;
        match view.read(host, library) {
            Ok(()) => Ok(view),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => {
                view.places.clear();
                view.list = ItemList::default();
                view.message = Some("reader.error");
                Ok(view)
            }
        }
    }

    /// Reads the notes into rows, again where they changed under the read.
    fn read(&mut self, host: &Host<'_>, library: &Library) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        let bookmark = messages.copy(host, "reader.bookmark")?;
        let highlight = messages.copy(host, "reader.highlight")?;
        let mut tries = 0;
        let mut items = loop {
            self.places.clear();
            match self.read_pages(host, library, &mut messages, (&bookmark, &highlight)) {
                Ok(items) => break items,
                Err(Error::State) if tries < TRIES => tries += 1,
                Err(error) => return Err(error),
            }
        };
        if items.is_empty() {
            self.message = Some("reader.annotations.empty.title");
            return Ok(());
        }
        self.message = None;
        items.try_reserve(1).map_err(|_| Error::NoMemory)?;
        let export = messages.copy(host, "reader.export")?;
        let help = messages.copy(host, "reader.export.help")?;
        items.push(Item::noted(&export, "›", &help)?);
        self.list = ItemList::new(items);
        Ok(())
    }

    /// One pass over the pages of notes: a row for each (at most
    /// [`MAX_ROWS`]), the places filled in.
    fn read_pages(
        &mut self,
        host: &Host<'_>,
        library: &Library,
        messages: &mut Messages,
        (bookmark, highlight): (&str, &str),
    ) -> Result<Vec<Item>, Error> {
        let book = BookRef::path(&self.path)?;
        let (mut input, mut out) = (buffer(256)?, buffer(SERVICE_BUFFER)?);
        let mut items = Vec::new();
        let mut cursor = AnnotationCursor::start();
        self.more = false;
        loop {
            let page = library.annotations(host, book, cursor, &mut input, &mut out)?;
            for row in page.rows() {
                let row = row?;
                if self.places.len() >= MAX_ROWS {
                    self.more = true;
                    break;
                }
                let note = Note::new(
                    kind_of(row.kind()),
                    percent_of(row.percent()),
                    cut(row.text(), TEXT_SHOWN),
                    cut(row.note(), TEXT_SHOWN),
                );
                let (title, line) = model::row(&note, bookmark, highlight, |quoted| {
                    messages.format(
                        host,
                        "reader.quoted",
                        &[Arg::Text {
                            name: "text",
                            value: quoted,
                        }],
                    )
                })?;
                items.try_reserve(1).map_err(|_| Error::NoMemory)?;
                self.places.try_reserve(1).map_err(|_| Error::NoMemory)?;
                items.push(note_row(&title, &line)?);
                self.places
                    .push(Place::new(row.start_section(), row.start_offset()));
            }
            match page.end().next() {
                Some(next) if !self.more => cursor = next,
                _ => break,
            }
        }
        Ok(items)
    }

    /// What an input did: a choice of a note goes there; the last row exports.
    ///
    /// # Errors
    /// The host's refusals.
    pub fn input(
        &mut self,
        host: &Host<'_>,
        library: Option<&Library>,
        input: &Input<'_>,
    ) -> Result<(Handled, Outcome), Error> {
        let step = self.list.input(host, input)?;
        let outcome = match step {
            ItemStep::Chosen(index) => match self.places.get(index) {
                Some(place) => Outcome::Go(*place),
                None => {
                    if index == self.places.len() && !self.places.is_empty() {
                        self.run_export(host, library);
                    }
                    Outcome::Handled
                }
            },
            ItemStep::Ignored => Outcome::Ignored,
            _ => Outcome::Handled,
        };
        Ok((ItemList::handled(step), outcome))
    }

    /// Writes the notes as Markdown and says where (or that it failed); runs
    /// the stage after a write that worked.
    fn run_export(&self, host: &Host<'_>, library: Option<&Library>) {
        let result = library
            .ok_or(Error::State)
            .and_then(|library| export(host, library, &self.path));
        let Ok(mut messages) = Messages::new() else {
            return;
        };
        let said = match &result {
            Ok(file) => messages.format(
                host,
                "reader.exported",
                &[Arg::Text {
                    name: "path",
                    value: file,
                }],
            ),
            Err(_) => messages.copy(host, "reader.export-failed"),
        };
        let Ok(said) = said else {
            return;
        };
        let Ok(note) = Notify::new(&said) else {
            return;
        };
        let mut scratch = [0; 768];
        CommandSender::new(host, &mut scratch).notify(note).ok();
    }

    /// Draws the list, or what says why there is none.
    ///
    /// # Errors
    /// Allocation, font and drawing errors.
    pub fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        let title = messages.copy(host, "reader.annotations")?;
        if let Some(key) = self.message {
            let heading = messages.copy(host, key)?;
            let touch = snapshot::capture_snapshot(host)?.has_touch();
            let help = if key == "reader.annotations.empty.title" {
                messages.copy(
                    host,
                    if touch {
                        "reader.annotations.empty"
                    } else {
                        "reader.annotations.empty.buttons"
                    },
                )?
            } else {
                String::new()
            };
            return self.list.draw_with(
                host,
                frame,
                &title,
                true,
                Footer::Empty {
                    title: &heading,
                    text: &help,
                },
            );
        }
        if self.more {
            let note = messages.format(
                host,
                "reader.annotations.more",
                &[Arg::Int {
                    name: "count",
                    value: MAX_ROWS as i64,
                }],
            )?;
            return self
                .list
                .draw_with(host, frame, &title, true, Footer::Note(&note));
        }
        self.list.draw_with(host, frame, &title, true, Footer::None)
    }
}

/// Opens the annotations screen of the book `params` name (the reading
/// screen's `toc_params`: path, root, section, offset).
///
/// # Errors
/// `NoMemory` and the host's refusals.
pub fn show(host: &Host<'_>, ui: &Ui, params: &[(&'static str, String); 4]) -> Result<(), Error> {
    open_screen(host, ui, "reader.annotations", params)
}

/// Opens the reader's screen `screen` with a hand-over of `params`.
///
/// # Errors
/// `NoMemory` and the host's refusals.
pub fn open_screen(
    host: &Host<'_>,
    ui: &Ui,
    screen: &str,
    params: &[(&'static str, String)],
) -> Result<(), Error> {
    let mut list = Vec::new();
    list.try_reserve_exact(params.len())
        .map_err(|_| Error::NoMemory)?;
    for (key, value) in params {
        list.push(Param::new(key, value)?);
    }
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let handover = ui.handover(host, &list, &mut input, &mut out)?;
    let mut scratch = [0; 256];
    CommandSender::new(host, &mut scratch).open(Open::screen(screen)?.handover(handover))?;
    Ok(())
}

/// Writes the Markdown of the notes of the book at `path` to its file in
/// `Notes`, and runs the stage `reader.notes-exported` after it. Returns the
/// file's path.
///
/// # Errors
/// `NoMemory`, and the host's refusal to write (the old file is intact then).
pub fn export(host: &Host<'_>, library: &Library, path: &str) -> Result<String, Error> {
    let (mut input, mut out) = (buffer(256)?, buffer(SERVICE_BUFFER)?);
    let book = BookRef::path(path)?;
    let (title, author) = {
        let found = library.book(host, book, &mut input, &mut out)?;
        (
            model::title_of(path, found.title())?,
            text::copy(found.author())?,
        )
    };
    let dir = model::notes_dir(&library_folder(host)?)?;
    let name = model::file_name(path, &title, &author)?;
    let file = text::concat(&[&dir, "/", &name])?;
    let temp = text::concat(&[&file, ".new"])?;
    make_dir(host, &dir);
    let mut messages = Messages::new()?;
    let highlights = messages.copy(host, "reader.highlights")?;
    let bookmarks = messages.copy(host, "reader.bookmarks")?;
    let headings = Headings::new(&highlights, &bookmarks);
    let mut sink = Sink::new(host, &temp)?;
    let written = stream(host, library, path, (&title, &author), &headings, &mut sink)
        .and_then(|()| sink.finish())
        .and_then(|()| storage::rename(host, Root::Files, &temp, &file));
    if written.is_err() {
        storage::remove(host, Root::Files, &temp).ok();
    }
    let refused = written.as_ref().err().copied();
    let done = model::export(
        |_, _| written,
        &file,
        "",
        || native_notes::run(host, native_notes::ROOT_FILES, &file, &title),
    );
    match (done, refused) {
        (true, _) => Ok(file),
        (false, Some(Error::NoMemory)) => Err(Error::NoMemory),
        (false, _) => Err(Error::Io),
    }
}

/// The library folder, without the slashes at its ends.
fn library_folder(host: &Host<'_>) -> Result<String, Error> {
    let mut scratch = [0; 256];
    match settings::read_setting(host, LIBRARY_FOLDER, &mut scratch)? {
        SettingValue::Text(folder) => text::copy(folder.trim_matches('/')),
        _ => Ok(String::new()),
    }
}

/// Makes the notes folder where it is missing; writing the file says if it
/// is not there.
fn make_dir(host: &Host<'_>, dir: &str) {
    let (Ok(service), Ok(mut input), Ok(mut out)) = (
        Files::open(host),
        buffer(files::INPUT_BYTES),
        buffer(files::OUTPUT_BYTES),
    ) else {
        return;
    };
    service
        .make_dir(host, Root::Files, dir, &mut input, &mut out)
        .ok();
    service.release(host).ok();
}

/// Where the Markdown pieces go: the first replaces the file, the rest are
/// appended; every piece is at most [`PIECE`] bytes.
struct Sink<'a, 'h> {
    host: &'a Host<'h>,
    file: &'a str,
    piece: String,
    started: bool,
}

impl<'a, 'h> Sink<'a, 'h> {
    fn new(host: &'a Host<'h>, file: &'a str) -> Result<Self, Error> {
        let mut piece = String::new();
        piece
            .try_reserve_exact(PIECE)
            .map_err(|_| Error::NoMemory)?;
        Ok(Self {
            host,
            file,
            piece,
            started: false,
        })
    }

    /// Adds `text`, writing the piece first when it would not fit.
    fn put(&mut self, text: &str) -> Result<(), Error> {
        if self.piece.len() + text.len() > PIECE {
            self.flush()?;
        }
        if text.len() > PIECE {
            return Err(Error::NoSpace);
        }
        self.piece.push_str(text);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Error> {
        if self.piece.is_empty() && self.started {
            return Ok(());
        }
        if self.started {
            storage::append(self.host, Root::Files, self.file, self.piece.as_bytes())?;
        } else {
            storage::write(self.host, Root::Files, self.file, self.piece.as_bytes())?;
            self.started = true;
        }
        self.piece.clear();
        Ok(())
    }

    /// Writes what is left (an empty file where nothing was put).
    fn finish(&mut self) -> Result<(), Error> {
        self.flush()
    }
}

/// The Markdown of the book's notes, in the order of `native_annotations::markdown`,
/// from one pass over the notes for the highlights and one for the bookmarks.
fn stream(
    host: &Host<'_>,
    library: &Library,
    path: &str,
    (title, author): (&str, &str),
    headings: &Headings<'_>,
    sink: &mut Sink<'_, '_>,
) -> Result<(), Error> {
    let mut put = |text: &str| sink.put(text);
    let mut markdown = Markdown::new(title, author, headings, &mut put);
    markdown.begin()?;
    for kind in [Kind::Highlight, Kind::Bookmark] {
        let (mut input, mut out) = (buffer(256)?, buffer(SERVICE_BUFFER)?);
        let book = BookRef::path(path)?;
        let mut cursor = AnnotationCursor::start();
        loop {
            let page = library.annotations(host, book, cursor, &mut input, &mut out)?;
            for row in page.rows() {
                let row = row?;
                if kind_of(row.kind()) != kind {
                    continue;
                }
                let note = Note::new(kind, percent_of(row.percent()), row.text(), row.note());
                markdown.note(&note)?;
            }
            match page.end().next() {
                Some(next) => cursor = next,
                None => break,
            }
        }
    }
    markdown.end()
}

/// The Markdown, one piece at a time (what `native_annotations::markdown`
/// makes in one string): the title and author, the highlights as quotes with
/// their note, the bookmarks as a list, a heading over each kind at its first.
struct Markdown<'a, 'p> {
    title: &'a str,
    author: &'a str,
    headings: &'a Headings<'a>,
    put: &'p mut dyn FnMut(&str) -> Result<(), Error>,
    /// The heading of the kind in hand is written.
    headed: Option<Kind>,
    /// A bookmark was written (the list ends with a blank line).
    bookmarks: bool,
}

impl<'a, 'p> Markdown<'a, 'p> {
    fn new(
        title: &'a str,
        author: &'a str,
        headings: &'a Headings<'a>,
        put: &'p mut dyn FnMut(&str) -> Result<(), Error>,
    ) -> Self {
        Self {
            title,
            author,
            headings,
            put,
            headed: None,
            bookmarks: false,
        }
    }

    fn piece(&mut self, args: core::fmt::Arguments<'_>) -> Result<(), Error> {
        let piece = text::format(args)?;
        (self.put)(&piece)
    }

    fn begin(&mut self) -> Result<(), Error> {
        let (title, author) = (self.title, self.author);
        self.piece(format_args!("# {title}\n\n"))?;
        if !author.is_empty() {
            self.piece(format_args!("{author}\n\n"))?;
        }
        Ok(())
    }

    fn note(&mut self, note: &Note<'_>) -> Result<(), Error> {
        if self.headed != Some(note.kind) {
            self.headed = Some(note.kind);
            let heading = match note.kind {
                Kind::Highlight => self.headings.highlights,
                _ => self.headings.bookmarks,
            };
            self.piece(format_args!("## {heading}\n\n"))?;
        }
        match note.kind {
            Kind::Highlight => {
                self.piece(format_args!("> {}\n>\n> {} %\n\n", note.text, note.percent))?;
                if !note.note.is_empty() {
                    self.piece(format_args!("{}\n\n", note.note))?;
                }
            }
            _ => {
                self.bookmarks = true;
                self.piece(format_args!("- {} %: {}\n", note.percent, note.text))?;
            }
        }
        Ok(())
    }

    fn end(&mut self) -> Result<(), Error> {
        if self.bookmarks {
            self.piece(format_args!("\n"))?;
        }
        Ok(())
    }
}

/// A bookmark or highlight as a row: its text shown whole, wrapped as in
/// Beta 3, not cut with an ellipsis.
fn note_row(title: &str, line: &str) -> Result<Item, Error> {
    Item::whole(title, "", line)
}

#[cfg(test)]
mod tests;
