//! What a book held in the list offers: open, finished, information, the
//! archive and delete (asked about first), as a choice the host shows.
use super::*;
use crate::{
    native_list::Row as ListRow,
    native_sheet::{self, Action, Facts},
};

/// The title of the book's question is cut at this many bytes.
const MOST_TITLE: usize = 96;

impl NativeFiles {
    /// The archive folder the setting names, without slashes around it; none
    /// when the setting says nothing.
    fn archive_folder(host: &Host<'_>) -> Result<String, Error> {
        let mut scratch = [0; 512];
        match settings::read_setting(host, ARCHIVE_SETTING, &mut scratch) {
            Ok(SettingValue::Text(text)) => copy(text.trim_matches('/')),
            Ok(_) | Err(Error::Denied | Error::NotFound | Error::NoSpace) => copy(""),
            Err(error) => Err(error),
        }
    }

    /// Offers what can be done with the book at cell `index` of the list on
    /// `token`.
    pub(super) fn offer(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        self.ensure(host)?;
        let Some(View::Books(view)) = self.screens.top(token) else {
            return Ok(());
        };
        let Some(ListRow::Book(at)) = view.list.cells().get(index).map(|cell| cell.row) else {
            return Ok(());
        };
        let Some(book) = view.window.books.get(at) else {
            return Ok(());
        };
        let reference = BookRef::catalog(book.key, view.window.generation)?;
        let library = self.library.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let (path, finished, title) = match library.book(host, reference, &mut input, &mut out) {
            Ok(book) => {
                let (title, cut) = native_shelves::short(book.title(), MOST_TITLE);
                let title = if cut {
                    paperpaw_ui_kit::text::concat(&[title, "…"])?
                } else {
                    copy(title)?
                };
                (copy(book.path())?, book.finished(), title)
            }
            Err(Error::State | Error::NotFound) => {
                // The book is gone: the list is read again and the user is told.
                self.reread(host)?;
                return self.notify(host, "files.list.gone");
            }
            Err(error) => return Err(error),
        };
        let can_delete = match library.can_delete(host, &[&path], &mut input, &mut out) {
            Ok(report) => report.rows().next().is_some_and(|row| row.status().is_ok()),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // A library that cannot say offers no delete.
            Err(_) => false,
        };
        let archive = Self::archive_folder(host)?;
        let actions = native_sheet::actions(Facts {
            finished,
            can_archive: native_sheet::can_archive(&archive, &path),
            can_delete,
        })?;
        let mut say = Messages::new()?;
        let mut words = Vec::new();
        for action in &actions {
            push(&mut words, say.copy(host, action.key())?)?;
        }
        let choices = native_sheet::choices(&words)?;
        let question = Question::choice(&title, native_sheet::file_name(&path), &choices)?;
        let request = self
            .ui
            .as_ref()
            .ok_or(Error::State)?
            .ask(host, question, &mut input, &mut out)?;
        self.asked = Some((request, Asked::Sheet { path, actions }));
        Ok(())
    }

    /// Does what the sheet's choice says to the book at `path`.
    pub(super) fn act(&mut self, host: &Host<'_>, path: &str, action: Action) -> Result<(), Error> {
        match action {
            Action::Open => self.open_path(host, path),
            Action::Info => self.open_info(host, path),
            Action::Finish => self.finish(host, path, true),
            Action::Unfinish => self.finish(host, path, false),
            Action::Archive => self.archive(host, path),
            Action::Delete => self.ask_delete(host, path),
        }
    }

    /// Marks the book at `path` finished or not; the list is read again.
    fn finish(&mut self, host: &Host<'_>, path: &str, finished: bool) -> Result<(), Error> {
        let library = self.library.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        match library.set_finished(host, BookRef::path(path)?, finished, &mut input, &mut out) {
            Ok(()) => {}
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // The book is gone or the library refuses: the list shows how it is.
            Err(_) => {}
        }
        self.reread(host)
    }

    /// Moves the book at `path` into the archive folder (a free name there)
    /// and says how it went. The host follows a move of its own accord.
    fn archive(&mut self, host: &Host<'_>, path: &str) -> Result<(), Error> {
        let folder = Self::archive_folder(host)?;
        let moved = Self::move_to_archive(host, &folder, path)?;
        self.notify(
            host,
            if moved {
                "files.archived"
            } else {
                "files.archive-failed"
            },
        )?;
        self.reread(host)
    }

    /// Whether the book was moved: tries the book's own name in the archive,
    /// then `name (2)` and on.
    fn move_to_archive(host: &Host<'_>, folder: &str, path: &str) -> Result<bool, Error> {
        let files = match Files::open(host) {
            Ok(files) => files,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(false),
        };
        let (mut input, mut out) = (buffer(files_input())?, buffer(SERVICE_BUFFER)?);
        let result = (|| {
            // Made where missing; moving into it tells if it is not there.
            match files.make_dir(host, Root::Files, folder, &mut input, &mut out) {
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Ok(()) | Err(_) => {}
            }
            for attempt in 0..native_sheet::ATTEMPTS {
                let target = native_sheet::archive_path(folder, path, attempt)?;
                match files.move_entry(
                    host,
                    Root::Files,
                    path,
                    Root::Files,
                    &target,
                    false,
                    &mut input,
                    &mut out,
                ) {
                    Ok(()) => return Ok(true),
                    // The name is taken: the next one.
                    Err(Error::State) => {}
                    Err(Error::NoMemory) => return Err(Error::NoMemory),
                    Err(_) => return Ok(false),
                }
            }
            Ok(false)
        })();
        files.release(host).ok();
        result
    }

    /// Asks whether to delete the book at `path`; the answer starts on the
    /// safe side.
    fn ask_delete(&mut self, host: &Host<'_>, path: &str) -> Result<(), Error> {
        let mut say = Messages::new()?;
        let title = say.copy(host, "files.delete.question")?;
        let labels = paperpaw_ui_kit::text::concat(&[
            &say.copy(host, "files.delete.yes")?,
            "\n",
            &say.copy(host, "files.delete.keep")?,
        ])?;
        let question = Question::confirm(&title, "")?.confirm_labels(&labels)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let request = self
            .ui
            .as_ref()
            .ok_or(Error::State)?
            .ask(host, question, &mut input, &mut out)?;
        self.asked = Some((request, Asked::Delete { path: copy(path)? }));
        Ok(())
    }

    /// Deletes the book at `path` through the library, with everything kept
    /// for it (the question said so), and says how it went.
    pub(super) fn delete_book(&mut self, host: &Host<'_>, path: &str) -> Result<(), Error> {
        let library = self.library.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let result = match library.delete(host, &[path], true, &mut input, &mut out) {
            Ok(report) => report
                .rows()
                .next()
                .map_or(Err(Error::Invalid), |row| row.status()),
            Err(error) => Err(error),
        };
        let said = match result {
            // Gone already: the library only forgot it.
            Ok(()) | Err(Error::NotFound) => "files.deleted",
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // Not a book of the library after all.
            Err(Error::Invalid) => "files.delete-failed.not-book",
            Err(_) => "files.delete-failed",
        };
        self.notify(host, said)?;
        self.reread(host)
    }
}

/// The input buffer of a `core.files` call.
const fn files_input() -> usize {
    paperpaw_sdk::native::files::INPUT_BYTES
}
