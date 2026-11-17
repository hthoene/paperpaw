//! Opening books, and the reading-app choices kept for books and folders.
//!
//! - Whatever opens a book (the library, the file manager, the browser, a
//!   catalog) opens it with [`open`] ([`Command::OpenBook`]): the kernel
//!   opens the screen bound to [`slots::READER`](crate::builtin::slots::READER) with what [`opened`]
//!   passes on. A reading app that cannot open a book after all says so
//!   with [`cannot_open`].
//! - The choices for a book and for a folder are kept ([`remember`]): the
//!   book's moved and removed with it, the folders' in one small file
//!   (moved with [`folder_moved`] by whatever moves a folder). [`ask`] lets
//!   the reader choose (*Open with*, a folder's reading app).

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::builtin::{params, settings};
use crate::{Command, Id, Params, Storage, StorageError};

/// [`settings::READING_APP`]'s value, and a format's: the reader the
/// defaults give (the screen bound to `slots::READER`, or for a format
/// the default reader).
pub use settings::READING_APP_DEFAULT as DEFAULT;

/// The setting of the reader for books of format `format`
/// (`core.reading-app.<format>`); the kernel registers one for each
/// format, offering the readers of that format.
pub fn format_setting(format: &Id) -> String {
    format!("{}.{format}", settings::READING_APP.as_str())
}

/// Where the choices for books are kept, a file a book (registered as
/// book data by the kernel).
pub const DIR: &str = crate::data_path!("readers");

/// Where the choices for folders are kept, a line each: reader, a tab,
/// the folder.
pub const FOLDERS: &str = crate::data_path!("readers-folders.txt");

/// The largest file of choices read.
const LIMIT: usize = 16 * 1024;

/// The parameter of a book's opening with the readers that could not
/// open it ([`cannot_open`]), comma-separated.
const PAST: &str = "readers-past";

/// The parameter of a page's opening ([`open_page`]): *Back* is offered on
/// it always.
pub const PAGE: &str = "readers-page";

/// A path as choices are kept by: without `/` at its ends.
fn plain(path: &str) -> &str {
    path.trim_matches('/')
}

/// What a choice is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Choice<'a> {
    /// The book at this path.
    Book(&'a str),
    /// The books in this folder and below it.
    Folder(&'a str),
}

fn book_file(book: &str) -> String {
    format!("{DIR}/{}.txt", crate::library::key(plain(book)))
}

/// The folders' choices: (reader, folder); none where there is no file.
/// A line for no folder (the whole card) is left out: it would decide
/// for every book unseen.
fn folders(storage: &dyn Storage) -> Result<Vec<(String, String)>, StorageError> {
    let data = match crate::read_file(storage, FOLDERS, LIMIT) {
        Ok(data) => data,
        Err(StorageError::NotFound) => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let text = String::from_utf8_lossy(&data);
    Ok(text
        .lines()
        .filter_map(|line| {
            let (reader, folder) = line.split_once('\t')?;
            let folder = plain(folder);
            (!folder.is_empty()).then(|| (reader.to_string(), folder.to_string()))
        })
        .collect())
}

/// Writes the folders' choices.
fn write_folders(storage: &dyn Storage, all: &[(String, String)]) -> Result<(), StorageError> {
    let text: String = all
        .iter()
        .map(|(reader, folder)| format!("{reader}\t{folder}\n"))
        .collect();
    let _ = storage.create_dir(crate::data::DIR);
    storage.write(FOLDERS, text.as_bytes())
}

/// Keeps `reader` as the one for `choice` (`None` forgets the choice).
/// The choices kept for folders are read first and never overwritten
/// unread: where they cannot be read (too many), this fails. A folder
/// must be one (not the whole card: [`StorageError::InvalidPath`]).
pub fn remember(
    storage: &dyn Storage,
    choice: Choice<'_>,
    reader: Option<&Id>,
) -> Result<(), StorageError> {
    match choice {
        Choice::Book(book) => {
            let file = book_file(book);
            match reader {
                Some(reader) => {
                    let _ = storage.create_dir(DIR);
                    storage.write(&file, reader.as_str().as_bytes())
                }
                None if storage.exists(&file) => storage.remove(&file),
                None => Ok(()),
            }
        }
        Choice::Folder(folder) => {
            let folder = plain(folder);
            if folder.is_empty() {
                return Err(StorageError::InvalidPath);
            }
            let mut all: Vec<(String, String)> = folders(storage)?
                .into_iter()
                .filter(|(_, f)| f != folder)
                .collect();
            if let Some(reader) = reader {
                all.push((reader.as_str().to_string(), folder.to_string()));
            }
            write_folders(storage, &all)
        }
    }
}

/// The folder `from` is now at `to` (moved or renamed): the choices kept
/// for it and the folders in it go along. Whatever moves folders (a file
/// manager) calls it; the choices for books go along with the books.
pub fn folder_moved(storage: &dyn Storage, from: &str, to: &str) -> Result<(), StorageError> {
    let (from, to) = (plain(from), plain(to));
    if from.is_empty() || to.is_empty() || from == to {
        return Ok(());
    }
    let mut all = folders(storage)?;
    let mut moved = false;
    for (_, folder) in &mut all {
        let rest = match folder.strip_prefix(from) {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => rest.to_string(),
            _ => continue,
        };
        *folder = format!("{to}{rest}");
        moved = true;
    }
    match moved {
        true => write_folders(storage, &all),
        false => Ok(()),
    }
}

/// The reader kept for `choice`, if any (whether or not it is there).
pub fn remembered(storage: &dyn Storage, choice: Choice<'_>) -> Option<String> {
    match choice {
        Choice::Book(book) => {
            let data = crate::read_file(storage, &book_file(book), 256).ok()?;
            let reader = String::from_utf8_lossy(&data).trim().to_string();
            (!reader.is_empty()).then_some(reader)
        }
        Choice::Folder(folder) => {
            let folder = plain(folder);
            folders(storage)
                .ok()?
                .into_iter()
                .find(|(_, f)| f == folder)
                .map(|(reader, _)| reader)
        }
    }
}

/// Opens the book at `path` in the reader's slot: nothing is read before
/// it runs.
pub fn open(path: impl Into<String>) -> Command {
    Command::OpenBook(Params::new().with(params::PATH, path.into()))
}

/// The readers that could not open the book opened with `params`.
pub fn past(params: &Params) -> impl Iterator<Item = &str> {
    params
        .get(PAST)
        .unwrap_or_default()
        .split(',')
        .filter(|p| !p.is_empty())
}

/// What opening a book (`params` of [`Command::OpenBook`]) passes on to
/// the reader's screen: the book's path, the readers passed over and the
/// page.
pub fn opened(params: &Params) -> Params {
    let path = params.get(params::PATH).unwrap_or_default();
    let mut opened = Params::new().with(params::PATH, path.to_string());
    if past(params).next().is_some() {
        opened = opened.with(PAST, params.get(PAST).unwrap_or_default().to_string());
    }
    if let Some(page) = params.get(PAGE) {
        opened = opened.with(PAGE, page.to_string());
    }
    opened
}

/// Asks which reading app opens the book at `path` this time (*Open
/// with…*), nothing kept: the readers that read it, the other ways to
/// open it plugins add ([`menus::OPEN_WITH`](crate::builtin::menus::OPEN_WITH)),
/// and *Always open it with…*, which asks as [`ask`] does and keeps the
/// choice.
pub fn ask_once(path: &str) -> Command {
    crate::builtin::events::READING_APP_WANTED
        .emit(crate::builtin::events::ReadingAppWanted::new(path.to_string(), false).once(true))
}

/// Asks which reader `choice` gets, kept once chosen (a book is opened
/// with it); *Standard* forgets the choice.
pub fn ask(choice: Choice<'_>) -> Command {
    let (path, folder) = match choice {
        Choice::Book(path) => (path, false),
        Choice::Folder(path) => (path, true),
    };
    crate::builtin::events::READING_APP_WANTED.emit(crate::builtin::events::ReadingAppWanted::new(
        path.to_string(),
        folder,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;

    #[test]
    fn choices_for_books_and_folders_are_kept_and_forgotten() {
        let storage = MemoryStorage::new();
        let comics = Id::new_static("comics.reader");
        remember(&storage, Choice::Book("books/a.cbz"), Some(&comics)).unwrap();
        remember(&storage, Choice::Folder("/manga/"), Some(&comics)).unwrap();
        remember(
            &storage,
            Choice::Folder("novels"),
            Some(&Id::new_static("x.reader")),
        )
        .unwrap();
        assert_eq!(
            remembered(&storage, Choice::Book("books/a.cbz")).as_deref(),
            Some("comics.reader")
        );
        assert_eq!(remembered(&storage, Choice::Book("books/b.cbz")), None);
        assert_eq!(
            remembered(&storage, Choice::Folder("manga")).as_deref(),
            Some("comics.reader")
        );
        remember(&storage, Choice::Folder("manga"), None).unwrap();
        assert_eq!(remembered(&storage, Choice::Folder("manga")), None);
        assert_eq!(
            remembered(&storage, Choice::Folder("novels")).as_deref(),
            Some("x.reader")
        );
        remember(&storage, Choice::Book("books/a.cbz"), None).unwrap();
        assert_eq!(remembered(&storage, Choice::Book("books/a.cbz")), None);
        // A book is the same with `/` at its ends or without.
        remember(&storage, Choice::Book("/books/a.cbz"), Some(&comics)).unwrap();
        assert!(remembered(&storage, Choice::Book("books/a.cbz")).is_some());
        // Not the whole card.
        assert_eq!(
            remember(&storage, Choice::Folder("/"), Some(&comics)),
            Err(StorageError::InvalidPath)
        );
    }

    #[test]
    fn folder_choices_follow_their_folders() {
        let storage = MemoryStorage::new();
        let comics = Id::new_static("comics.reader");
        remember(&storage, Choice::Folder("manga"), Some(&comics)).unwrap();
        remember(&storage, Choice::Folder("manga/old"), Some(&comics)).unwrap();
        remember(&storage, Choice::Folder("mangaka"), Some(&comics)).unwrap();
        folder_moved(&storage, "/manga/", "comics/manga").unwrap();
        assert!(remembered(&storage, Choice::Folder("comics/manga")).is_some());
        assert!(remembered(&storage, Choice::Folder("comics/manga/old")).is_some());
        assert!(remembered(&storage, Choice::Folder("manga")).is_none());
        // Only the folder, not one whose name starts alike.
        assert!(remembered(&storage, Choice::Folder("mangaka")).is_some());
    }

    #[test]
    fn choices_that_cannot_be_read_are_not_overwritten() {
        let big = "x\tfolder\n".repeat(LIMIT / 8);
        let storage = MemoryStorage::new().with(FOLDERS, big.as_str());
        let comics = Id::new_static("comics.reader");
        assert_eq!(
            remember(&storage, Choice::Folder("manga"), Some(&comics)),
            Err(StorageError::TooLarge)
        );
        let kept = crate::read_file(&storage, FOLDERS, 2 * LIMIT).unwrap();
        assert_eq!(kept.len(), big.len());
    }
}
