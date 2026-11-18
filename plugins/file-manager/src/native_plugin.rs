//! The interface-7 file manager: one screen that browses the card, with
//! questions of the host for what to do with an entry, a new name, a new
//! folder and deleting. A book opens in the reader the user chose (or in
//! another one), and is deleted through the library with what is kept for it;
//! the host follows moves and renames on the card itself (`PORTING-7.md`).

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Handled;
use paperpaw_sdk::native::callbacks::{DrawFrame, Input, InputGestureKind};
use paperpaw_sdk::native::commands::{CommandSender, RedrawMode};
use paperpaw_sdk::native::document::{Documents, ReaderCursor};
use paperpaw_sdk::native::events::{AnswerOutcome, Answered, Event, Incoming};
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::library::{BookRef, Library};
use paperpaw_sdk::native::settings::{self, SettingValue};
use paperpaw_sdk::native::ui::{Open, Param, Question, RequestId, TextWrite, Ui};
use paperpaw_sdk::native::{Error, Host, Plugin, registrations, reply::Reply};
use paperpaw_sdk::{Point, Storage};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer, push};
use paperpaw_ui_kit::text::{concat, copy};
use paperpaw_ui_kit::{Messages, Screens, uitext};

use crate::browser::{Browser, Button, is_own_data};
use crate::card::{with_card, with_card_noted};
use crate::model::folder::Entry;
use crate::native_draw::{self, Hits};

mod image {
    include!(concat!(env!("OUT_DIR"), "/file_manager_declarations.rs"));
}

const OPEN: u32 = crate::declarations::OPEN_TOKEN;
const BROWSER: u32 = crate::declarations::BROWSER_TOKEN;
const HIDDEN: &str = "file-manager.hidden";
/// The longest name the keyboard takes.
const NAME_LIMIT: usize = 256;
/// Every option an entry can have (the labels are `file-manager.option.*`).
const OPTIONS: [&str; 6] = ["open", "open-with", "rename", "move", "copy", "delete"];
/// Reading apps listed for "Open with" at most.
const MOST_READERS: usize = 12;
/// The reading apps' list, a page of it: the request and the reply.
const READERS_INPUT: usize = 256;

/// What an open question is for.
#[derive(Debug, PartialEq, Eq)]
enum Asked {
    /// What to do with the entry at `path`: one of `options`, as listed;
    /// `book` where the library takes it for a book.
    Options {
        path: String,
        options: Vec<&'static str>,
        book: bool,
    },
    /// Which reading app opens the book at `path`: the screens, as listed.
    With { path: String, screens: Vec<String> },
    /// A new name for the entry at `path`.
    Rename { path: String },
    /// The name of a new folder.
    NewFolder,
    /// Whether to delete the entry at `path`, a book with what is kept for it.
    Delete { path: String, book: bool },
}

/// The screen: the folder on show and where it was drawn.
#[derive(Debug)]
struct View {
    browser: Browser,
    hits: Hits,
}

/// The interface-7 file manager.
#[derive(Debug, Default)]
pub struct NativeFileManager {
    ui: Option<Ui>,
    /// Opened at the first entry offered; where the host has none, entries
    /// are plain files (no book is opened from here, a file is deleted on
    /// the card).
    library: Option<Library>,
    documents: Option<Documents>,
    screens: Screens<View>,
    asked: Option<(RequestId, Asked)>,
}

fn redraw(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
}

/// Whether hidden files show.
fn hidden(host: &Host<'_>) -> bool {
    let mut scratch = [0u8; 16];
    matches!(
        settings::read_setting(host, HIDDEN, &mut scratch),
        Ok(SettingValue::Bool(true))
    )
}

impl Plugin for NativeFileManager {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS.iter().chain(image::UI) {
            registrations::submit(host, batch)?;
        }
        // `core.ui` is opened by the first callback: opening needs a live frame.
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
        self.ensure_ui(host)?;
        match event {
            Event::ActionRun {
                action: "file-manager.open",
                ..
            } if token == OPEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("file-manager.browser")?)
            }
            Event::ScreenOpened { instance, .. } => {
                let rows = native_draw::rows_now(host)?;
                let mut browser = Browser::new(String::new(), hidden(host), rows);
                with_card(host, |card| {
                    browser.reload(card, None);
                    Ok(())
                })?;
                let view = View {
                    browser,
                    hits: Hits::default(),
                };
                self.screens.open(instance, view)
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } => {
                self.screens.shown(BROWSER, instance)?;
                // The setting may have changed in the settings.
                let show = hidden(host);
                self.browse(host, |browser, card| {
                    if browser.set_hidden(show) {
                        browser.reload(card, None);
                    }
                })?;
                Ok(())
            }
            Event::ScreenClosed { instance, .. } => {
                self.screens.close(instance);
                Ok(())
            }
            Event::Answered { answer, .. } => self.answered(host, answer),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if !native_draw::is_screen(&frame) {
            return Ok(());
        }
        let rows = native_draw::rows_now(host)?;
        let Some(view) = self.screens.top(token) else {
            return Err(Error::State);
        };
        // A change of the text size changes the page.
        if rows != view.browser.rows() {
            with_card(host, |card| {
                view.browser.set_rows(card, rows);
                Ok(())
            })?;
        }
        view.hits = native_draw::draw(host, &mut frame, &view.browser, true)?;
        Ok(())
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        self.ensure_ui(host)?;
        if token != BROWSER || self.screens.top(BROWSER).is_none() {
            return Ok(Handled::No);
        }
        match input {
            Input::Action { action, .. } => self.action(host, action),
            Input::Gesture {
                position,
                kind: kind @ (InputGestureKind::Tap | InputGestureKind::LongPress),
                ..
            } => self.touch(host, position, kind),
            _ => Ok(Handled::No),
        }
    }
}

impl NativeFileManager {
    /// Opens `core.ui` in the first live callback; the handle serves later ones.
    fn ensure_ui(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        Ok(())
    }

    fn ui(&self) -> Result<&Ui, Error> {
        self.ui.as_ref().ok_or(Error::State)
    }

    /// Works on the folder on show with the card at hand, then draws it anew.
    fn browse<T>(
        &mut self,
        host: &Host<'_>,
        work: impl FnOnce(&mut Browser, &dyn Storage) -> T,
    ) -> Result<Option<T>, Error> {
        self.browse_if(host, work, |_| true)
    }

    /// As [`Self::browse`], drawn anew only where `changed` says the work
    /// changed what is shown.
    fn browse_if<T>(
        &mut self,
        host: &Host<'_>,
        work: impl FnOnce(&mut Browser, &dyn Storage) -> T,
        changed: impl FnOnce(&T) -> bool,
    ) -> Result<Option<T>, Error> {
        let Some(view) = self.screens.top(BROWSER) else {
            return Ok(None);
        };
        let (done, ran_out) = with_card_noted(host, |card| Ok(work(&mut view.browser, card)))?;
        if ran_out {
            // The browser kept it as a storage error ("that did not work").
            view.browser.lacked_memory();
        }
        if ran_out || changed(&done) {
            redraw(host)?;
        }
        Ok(Some(done))
    }

    fn action(&mut self, host: &Host<'_>, action: &str) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(BROWSER) else {
            return Ok(Handled::No);
        };
        let (cursor, bar) = (view.browser.cursor(), view.browser.bar());
        let button = bar.and_then(|i| view.browser.buttons().get(i).copied());
        match action {
            "core.up" | "core.left" | "core.previous" => {
                self.browse(host, |browser, card| browser.step(card, -1))?;
            }
            "core.down" | "core.right" | "core.next" => {
                self.browse(host, |browser, card| browser.step(card, 1))?;
            }
            "core.prev-page" | "core.next-page" => {
                let forward = action == "core.next-page";
                self.browse(host, |browser, card| browser.turn(card, forward))?;
            }
            "core.confirm" => match button {
                Some(button) => self.press(host, button)?,
                None if bar.is_none() => self.activate(host, cursor)?,
                None => {}
            },
            "core.menu" if bar.is_none() => self.offer(host, cursor)?,
            "core.back" => self.back(host)?,
            _ => return Ok(Handled::No),
        }
        Ok(Handled::Yes)
    }

    fn touch(
        &mut self,
        host: &Host<'_>,
        at: Point,
        kind: InputGestureKind,
    ) -> Result<Handled, Error> {
        let Some(view) = self.screens.top(BROWSER) else {
            return Ok(Handled::No);
        };
        let header = view.hits.header.action(at);
        let button = view.hits.button_at(at).map(|i| {
            let button = view.browser.buttons().get(i).copied();
            (i, button)
        });
        let held = view.browser.page().len();
        let entry = view.hits.entry_at(at, view.browser.first(), held);
        if kind == InputGestureKind::Tap
            && let Some(action) = header
        {
            return self.action(host, action);
        }
        if let Some((i, button)) = button {
            if kind == InputGestureKind::Tap
                && let Some(button) = button
            {
                view.browser.focus_button(i);
                self.press(host, button)?;
            }
            return Ok(Handled::Yes);
        }
        let Some(index) = entry else {
            return Ok(Handled::No);
        };
        view.browser.focus_entry(index);
        match kind {
            InputGestureKind::Tap => self.activate(host, index)?,
            _ => self.offer(host, index)?,
        }
        Ok(Handled::Yes)
    }

    /// Back goes up a folder first, then leaves.
    fn back(&mut self, host: &Host<'_>) -> Result<(), Error> {
        // Past the top folder the screen closes: nothing is drawn for it.
        let up = self
            .browse_if(host, |browser, card| browser.up(card), |up| *up)?
            .unwrap_or(false);
        if !up {
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).close_screen()?;
        }
        Ok(())
    }

    /// Tapped or confirmed: a folder opens, a file offers its actions.
    fn activate(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let Some(view) = self.screens.top(BROWSER) else {
            return Ok(());
        };
        let Some(entry) = view.browser.entry(index) else {
            return Ok(());
        };
        if entry.is_dir {
            self.browse(host, |browser, card| browser.enter(card, index))?;
            Ok(())
        } else {
            self.offer(host, index)
        }
    }

    fn press(&mut self, host: &Host<'_>, button: Button) -> Result<(), Error> {
        match button {
            Button::NewFolder => {
                self.ensure_ui(host)?;
                let title = Messages::new()?.copy(host, "file-manager.new-folder")?;
                self.ask_name(host, &title, "", Asked::NewFolder)
            }
            Button::Cancel => {
                self.browse(host, |browser, _| browser.cancel())?;
                Ok(())
            }
            Button::Paste => {
                self.browse(host, |browser, card| browser.paste(card))?;
                Ok(())
            }
        }
    }

    /// The actions for entry `index`.
    fn offer(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        self.ensure_ui(host)?;
        let Some(view) = self.screens.top(BROWSER) else {
            return Ok(());
        };
        let Some((path, is_dir)) = view.browser.path_of(index)? else {
            return Ok(());
        };
        let name = copy(crate::model::path::name(&path))?;
        // A file the library takes for a book opens; where more than one
        // reading app reads it, it also opens with another.
        let book = !is_dir && self.is_book(host, &path)?;
        let with = book && self.readers_for(host, &path)?.len() > 1;
        let mut options: Vec<&'static str> = Vec::new();
        for option in OPTIONS {
            let wanted = match option {
                "open" => book,
                "open-with" => with,
                _ => true,
            };
            if wanted {
                push(&mut options, option)?;
            }
        }
        let mut messages = Messages::new()?;
        let mut choices = String::new();
        for (n, option) in options.iter().enumerate() {
            let label = messages.copy(host, &concat(&["file-manager.option.", option])?)?;
            let separator = if n > 0 { "\n" } else { "" };
            choices = concat(&[&choices, separator, &label])?;
        }
        // The question borrows the path; the answer needs it owned.
        let owned = copy(&path)?;
        let question = Question::choice(&name, &path, &choices)?;
        self.ask(
            host,
            question,
            Asked::Options {
                path: owned,
                options,
                book,
            },
        )
    }

    /// Opens the services of the library and the documents, where the host
    /// has them; an entry is a plain file where it has not.
    fn services(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.library.is_none() {
            match Library::open(host) {
                Ok(library) => self.library = Some(library),
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => {}
            }
        }
        if self.documents.is_none() {
            match Documents::open(host) {
                Ok(documents) => self.documents = Some(documents),
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => {}
            }
        }
        Ok(())
    }

    /// Whether the library takes the file at `path` for a book (a format
    /// reads it): the preflight of a delete answers `OK` for a book and
    /// `Invalid` for any other file. Anything it cannot answer is a plain file.
    fn is_book(&mut self, host: &Host<'_>, path: &str) -> Result<bool, Error> {
        self.services(host)?;
        let Some(library) = &self.library else {
            return Ok(false);
        };
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        match library.can_delete(host, &[path], &mut input, &mut out) {
            Ok(report) => Ok(report.rows().next().is_some_and(|row| row.status().is_ok())),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(false),
        }
    }

    /// The reading apps that read the book at `path` (those of its format
    /// where the library knows it, else all): their screens and titles, at
    /// most `MOST_READERS`.
    fn readers_for(&mut self, host: &Host<'_>, path: &str) -> Result<Vec<(String, String)>, Error> {
        self.services(host)?;
        let Some(documents) = &self.documents else {
            return Ok(Vec::new());
        };
        let mut format = String::new();
        if let Some(library) = &self.library {
            let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
            match BookRef::path(path)
                .and_then(|book| library.book(host, book, &mut input, &mut out))
            {
                Ok(book) => format = copy(book.format())?,
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => {}
            }
        }
        let (mut input, mut out) = (buffer(READERS_INPUT)?, buffer(SERVICE_BUFFER)?);
        let mut found: Vec<(String, String)> = Vec::new();
        let mut cursor = ReaderCursor::start();
        // The list may change between pages: start again once, then give up.
        let mut again = true;
        loop {
            let page = match documents.readers(host, &format, cursor, &mut input, &mut out) {
                Ok(page) => page,
                Err(Error::State) if again => {
                    again = false;
                    found.clear();
                    cursor = ReaderCursor::start();
                    continue;
                }
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => return Ok(Vec::new()),
            };
            for reader in page.rows() {
                if found.len() >= MOST_READERS {
                    return Ok(found);
                }
                push(&mut found, (copy(reader.screen())?, copy(reader.title())?))?;
            }
            match page.end().next() {
                Some(next) => cursor = next,
                None => return Ok(found),
            }
        }
    }

    /// Opens the book at `path`: in the reader the user bound to
    /// `core.reader`, or in the one whose screen is `screen`.
    fn open_book(&self, host: &Host<'_>, path: &str, screen: Option<&str>) -> Result<(), Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let params = [Param::new("path", path)?];
        let handover = self.ui()?.handover(host, &params, &mut input, &mut out)?;
        let open = match screen {
            Some(screen) => Open::in_slot(screen, "core.reader")?,
            None => Open::slot("core.reader")?,
        };
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).open(open.handover(handover))
    }

    /// Asks which reading app opens the book at `path` (the screens named in
    /// the answer's order).
    fn ask_with(&mut self, host: &Host<'_>, path: String) -> Result<(), Error> {
        let readers = self.readers_for(host, &path)?;
        if readers.is_empty() {
            return Ok(());
        }
        let name = copy(crate::model::path::name(&path))?;
        let mut choices = String::new();
        let mut screens: Vec<String> = Vec::new();
        for (n, (screen, title)) in readers.into_iter().enumerate() {
            let separator = if n > 0 { "\n" } else { "" };
            choices = concat(&[&choices, separator, &title])?;
            push(&mut screens, screen)?;
        }
        let owned = copy(&path)?;
        let question = Question::choice(&name, &path, &choices)?;
        self.ask(
            host,
            question,
            Asked::With {
                path: owned,
                screens,
            },
        )
    }

    /// Deletes the book at `path` through the library, with everything kept
    /// for it (the reader said so in the question); a file the library does
    /// not take for a book goes from the card as any other.
    fn delete_book(&mut self, host: &Host<'_>, path: &str) -> Result<(), Error> {
        let at = Entry::new(crate::model::path::name(path), false, 0).ok();
        let library = self.library.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let result = match library.delete(host, &[path], true, &mut input, &mut out) {
            Ok(report) => report
                .rows()
                .next()
                .map_or(Err(Error::Invalid), |row| row.status()),
            Err(error) => Err(error),
        };
        match result {
            // Not a book after all: a file on the card.
            Err(Error::Invalid) => {
                self.browse(host, |browser, card| browser.delete(card, path))?;
            }
            // Gone already: the library only forgot it.
            Err(Error::NotFound) | Ok(()) => {
                self.browse(host, |browser, card| {
                    browser.deleted(card, path, at, Ok(()))
                })?;
            }
            Err(error) => {
                self.browse(host, |browser, card| {
                    browser.deleted(card, path, at, Err(error));
                })?;
            }
        }
        Ok(())
    }

    fn ask(&mut self, host: &Host<'_>, question: Question<'_>, asked: Asked) -> Result<(), Error> {
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let request = self.ui()?.ask(host, question, &mut input, &mut output)?;
        self.asked = Some((request, asked));
        Ok(())
    }

    fn ask_name(
        &mut self,
        host: &Host<'_>,
        title: &str,
        initial: &str,
        asked: Asked,
    ) -> Result<(), Error> {
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let request = self
            .ui()?
            .ask(host, Question::text(title, "")?, &mut input, &mut output)?;
        self.ui()?.text_write(
            host,
            request.text(),
            0,
            initial,
            TextWrite::Last,
            &mut input,
            &mut output,
        )?;
        self.asked = Some((request, asked));
        Ok(())
    }

    /// Asks whether to delete the entry at `path`; the answer starts on the
    /// safe side. PaperPaw's own folder is not asked about: it is not deleted.
    fn ask_delete(&mut self, host: &Host<'_>, path: String, book: bool) -> Result<(), Error> {
        let (own, folder) = with_card(host, |card| {
            Ok((
                is_own_data(card, &path),
                crate::model::path::is_dir(card, &path),
            ))
        })?;
        if own {
            self.browse(host, |browser, card| browser.delete(card, &path))?;
            return Ok(());
        }
        let mut messages = Messages::new()?;
        let key = if folder {
            "file-manager.delete-folder-question"
        } else if book {
            "file-manager.delete-book-question"
        } else {
            "file-manager.delete-question"
        };
        let name = crate::model::path::name(&path);
        let title = messages.format(
            host,
            key,
            &[Arg::Text {
                name: "name",
                value: name,
            }],
        )?;
        let labels = concat(&[
            &messages.copy(host, "file-manager.option.delete-yes")?,
            "\n",
            &messages.copy(host, "file-manager.option.cancel")?,
        ])?;
        let question = Question::confirm(&title, "")?.confirm_labels(&labels)?;
        self.ask(host, question, Asked::Delete { path, book })
    }

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
            Asked::Options {
                path,
                options,
                book,
            } => match options.get(answer.choice() as usize) {
                Some(&"open") => self.open_book(host, &path, None),
                Some(&"open-with") => self.ask_with(host, path),
                Some(&"rename") => {
                    let title = Messages::new()?.copy(host, "file-manager.rename")?;
                    let name = copy(crate::model::path::name(&path))?;
                    self.ask_name(host, &title, &name, Asked::Rename { path })
                }
                Some(&"move") => {
                    self.browse(host, |browser, card| browser.pick(card, &path, false))?;
                    Ok(())
                }
                Some(&"copy") => {
                    self.browse(host, |browser, card| browser.pick(card, &path, true))?;
                    Ok(())
                }
                Some(&"delete") => self.ask_delete(host, path, book),
                _ => Ok(()),
            },
            Asked::With { path, screens } => match screens.get(answer.choice() as usize) {
                Some(screen) => self.open_book(host, &path, Some(screen)),
                None => Ok(()),
            },
            Asked::Rename { path } => {
                let typed = uitext::read_all(self.ui()?, host, request.text(), NAME_LIMIT)?;
                self.browse(host, |browser, card| browser.rename(card, &path, &typed))?;
                Ok(())
            }
            Asked::NewFolder => {
                let typed = uitext::read_all(self.ui()?, host, request.text(), NAME_LIMIT)?;
                self.browse(host, |browser, card| browser.new_folder(card, &typed))?;
                Ok(())
            }
            Asked::Delete { path, book: true } if answer.choice() == 1 => {
                self.delete_book(host, &path)
            }
            Asked::Delete { path, .. } if answer.choice() == 1 => {
                self.browse(host, |browser, card| browser.delete(card, &path))?;
                Ok(())
            }
            Asked::Delete { .. } => Ok(()),
        }
    }
}

/// Every option, to see that the labels exist (see the tests).
#[cfg(test)]
pub(crate) fn option_keys() -> alloc::vec::Vec<String> {
    OPTIONS
        .iter()
        .map(|o| alloc::format!("file-manager.option.{o}"))
        .collect()
}

#[cfg(test)]
mod tests;
