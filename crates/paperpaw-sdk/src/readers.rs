//! Reading apps: any plugin's screen may read books of some formats (a
//! comics reader for CBZ, a reader of one's own for EPUB), and books open
//! through one call that finds the reader chosen for them.
//!
//! - A plugin registers its screen as a reader
//!   ([`ReaderSpec`], `Registrar::add_reader`): the formats it reads
//!   (all, where it names none), its name the translation of its id. It
//!   is opened as the screen bound to
//!   [`slots::READER`] is, with the
//!   parameter [`PATH`](crate::builtin::params::PATH), in that slot (its
//!   key map layer applies), and keeps that slot's promises.
//! - Whatever opens a book (the library, the file manager, the browser, a
//!   catalog) opens it with [`open`] ([`Command::OpenBook`]), which the
//!   kernel turns into opening the reader chosen when it runs it
//!   ([`resolve`]): the reader chosen for the book, else
//!   for its folder, else for its format
//!   ([`settings::READING_APP`] per format,
//!   see [`format_setting`]), else the default one (`settings::READING_APP`),
//!   else the screen bound to `slots::READER`. [`open_with`] opens it with
//!   a reader named.
//! - A reader that cannot open a book after all (a file it does not know)
//!   says so with [`cannot_open`]: the kernel opens it with the next
//!   reader in that order, at last the slot's own screen, which shows why
//!   it cannot. Opening a book closes another reading app open in the
//!   slot, so that one book is held at a time.
//! - The choices for a book and for a folder are kept ([`remember`]): the
//!   book's moved and removed with it, the folders' in one small file
//!   (moved with [`folder_moved`] by whatever moves a folder).
//!   A choice of a reader no longer installed is passed over silently.
//!   They are read when a book opens, never kept in memory. [`ask`] lets
//!   the reader choose (*Open with*, a folder's reading app): the kernel
//!   lists the readers that fit, keeps the one chosen and, for a book,
//!   opens it with it.

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::builtin::{params, settings, slots};
use crate::{Command, Env, Id, Params, Storage, StorageError};

/// A reader contribution (see the [module documentation](self)): its
/// name is the translation of its id.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ReaderSpec {
    pub id: Id,
    /// The screen that shows a book (opened with `params::PATH`).
    pub screen: Id,
    /// The formats it reads, by id (`format-epub.epub`); all where empty.
    pub formats: Vec<Id>,
}

impl ReaderSpec {
    /// Reader `id` showing books in `screen`, of every format.
    pub fn new(id: Id, screen: Id) -> Self {
        Self {
            id,
            screen,
            formats: Vec::new(),
        }
    }

    /// Reads books of these formats only.
    pub fn formats(mut self, formats: &[Id]) -> Self {
        self.formats = formats.to_vec();
        self
    }

    /// Whether it reads books of format `format` (none known: only a
    /// reader of every format).
    pub fn reads(&self, format: Option<&Id>) -> bool {
        self.formats.is_empty() || format.is_some_and(|f| self.formats.contains(f))
    }
}

/// The registered readers, in the order of the plugins.
#[derive(Debug, Default)]
pub struct Readers {
    list: Vec<ReaderSpec>,
}

impl Readers {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add(&mut self, reader: ReaderSpec) {
        self.list.push(reader);
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.list.shrink_to_fit();
    }

    pub fn iter(&self) -> impl Iterator<Item = &ReaderSpec> {
        self.list.iter()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// The reader `id`.
    pub fn get(&self, id: &str) -> Option<&ReaderSpec> {
        self.list.iter().find(|r| r.id.as_str() == id)
    }

    /// The readers of books of format `format`.
    pub fn reading<'s>(&'s self, format: Option<&'s Id>) -> impl Iterator<Item = &'s ReaderSpec> {
        self.list.iter().filter(move |r| r.reads(format))
    }

    /// The readers of the book at `path`, by its format.
    pub fn for_book<'s>(&'s self, env: &'s Env<'_>, path: &str) -> Vec<&'s ReaderSpec> {
        let format = env.formats().for_file(env.storage(), path).map(|f| &f.id);
        self.reading(format).collect()
    }
}

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

/// The reader chosen for the book at `path`: for the book, else for the
/// nearest folder it is in, else for its format, else the default; one
/// there that reads its format. `None`: the screen bound to
/// `slots::READER`. A choice of a reader not installed (any more) is
/// passed over.
pub fn chosen<'e>(env: &'e Env<'_>, path: &str) -> Option<&'e ReaderSpec> {
    chosen_past(env, path, &[])
}

/// As [`chosen`], passing over the readers in `past` (those that could
/// not open the book).
fn chosen_past<'e>(env: &'e Env<'_>, path: &str, past: &[&str]) -> Option<&'e ReaderSpec> {
    let path = plain(path);
    let format = env
        .formats
        .for_file(env.storage(), path)
        .map(|f| f.id.clone());
    let fits = |id: &str| {
        env.readers()
            .get(id)
            .filter(|r| r.reads(format.as_ref()) && !past.contains(&r.id.as_str()))
    };
    if let Some(reader) = remembered(env.storage(), Choice::Book(path)).and_then(|id| fits(&id)) {
        return Some(reader);
    }
    // The nearest folder with a choice.
    let mut kept = folders(env.storage()).unwrap_or_default();
    paperpaw_sort::sort_by_cached_key(&mut kept, |(_, folder)| core::cmp::Reverse(folder.len()));
    let in_folder = kept.iter().find(|(_, folder)| {
        path.strip_prefix(folder.as_str())
            .is_some_and(|rest| rest.starts_with('/'))
    });
    if let Some(reader) = in_folder.and_then(|(id, _)| fits(id)) {
        return Some(reader);
    }
    let by_format = format
        .as_ref()
        .and_then(|f| env.settings().get(&format_setting(f)))
        .map(crate::SettingValue::to_config_text);
    if let Some(reader) = by_format.as_deref().and_then(fits) {
        return Some(reader);
    }
    fits(env.settings().read(&settings::READING_APP))
}

/// Opens the book at `path` with the reader chosen for it (see
/// [`chosen`]), in the reader's slot: nothing is read before it runs.
pub fn open(path: impl Into<String>) -> Command {
    Command::OpenBook(Params::new().with(params::PATH, path.into()))
}

/// Opens `path`, a page a plugin shows one after another (a web page),
/// as [`open`] does; readers offer *Back* on it always, to the plugin's
/// page before (a plugin's other documents, e.g. a text it writes to be
/// read, show as books do).
pub fn open_page(path: impl Into<String>) -> Command {
    Command::OpenBook(
        Params::new()
            .with(params::PATH, path.into())
            .with(PAGE, "1"),
    )
}

/// Shows `text`, plain (paragraphs apart by an empty line), to read paged
/// as a book: written to `path` (a `.txt` in the plugin's own folder),
/// opened as a plugin's page ([`open_page`]), so *Back* returns to it.
/// For long texts such as *How it works*.
pub fn show_text(
    storage: &dyn crate::Storage,
    path: &str,
    text: &str,
) -> Result<Command, crate::StorageError> {
    storage.write(path, text.as_bytes())?;
    Ok(open_page(path))
}

/// What a reader that cannot open a book after all answers (instead of
/// showing an error of its own): `params` are those it was opened with,
/// `reader` its own id. The kernel opens the book with the next reader
/// for it, at last the slot's own screen; past that, nothing.
pub fn cannot_open(params: &Params, reader: &Id) -> Command {
    let mut past = String::from(params.get(PAST).unwrap_or_default());
    if !past.split(',').any(|p| p == reader.as_str()) {
        if !past.is_empty() {
            past.push(',');
        }
        past.push_str(reader.as_str());
    }
    let path = params.get(params::PATH).unwrap_or_default().to_string();
    let mut opened = Params::new().with(params::PATH, path).with(PAST, past);
    if let Some(page) = params.get(PAGE) {
        opened = opened.with(PAGE, page.to_string());
    }
    Command::OpenBook(opened)
}

/// The readers that could not open the book opened with `params`.
pub fn past(params: &Params) -> impl Iterator<Item = &str> {
    params
        .get(PAST)
        .unwrap_or_default()
        .split(',')
        .filter(|p| !p.is_empty())
}

/// What opening a book (`params` of [`Command::OpenBook`]) is now: the
/// reader chosen for it in the reader's slot, or the slot's own screen
/// (the kernel runs [`Command::OpenBook`] so), passing over the readers
/// that could not open it.
pub fn resolve(env: &Env<'_>, params: &Params) -> Command {
    let path = params.get(params::PATH).unwrap_or_default();
    let past: Vec<&str> = past(params).collect();
    let mut opened = Params::new().with(params::PATH, path.to_string());
    if !past.is_empty() {
        opened = opened.with(PAST, params.get(PAST).unwrap_or_default().to_string());
    }
    if let Some(page) = params.get(PAGE) {
        opened = opened.with(PAGE, page.to_string());
    }
    match chosen_past(env, path, &past) {
        Some(reader) => Command::OpenInSlot(slots::READER, reader.screen.clone(), opened),
        None => Command::OpenSlot(slots::READER, opened),
    }
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

/// Opens the book at `path` with `reader`'s screen, in the reader's
/// slot.
pub fn open_with(path: &str, reader: &ReaderSpec) -> Command {
    let params = Params::new().with(params::PATH, path.to_string());
    Command::OpenInSlot(slots::READER, reader.screen.clone(), params)
}

/// The names to show for readers: their ids, which are their names'
/// translation keys.
pub fn name<'e>(env: &'e Env<'_>, reader: &'e ReaderSpec) -> Cow<'e, str> {
    Cow::Borrowed(env.i18n().tr(reader.id.as_str()))
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

    #[test]
    fn a_reader_that_cannot_open_is_passed_over() {
        let comics = Id::new_static("comics.reader");
        let other = Id::new_static("other.reader");
        let opened = Params::new().with(params::PATH, "a.cbz");
        let Command::OpenBook(again) = cannot_open(&opened, &comics) else {
            panic!("not opened again");
        };
        assert_eq!(again.get(params::PATH), Some("a.cbz"));
        let Command::OpenBook(again) = cannot_open(&again, &other) else {
            panic!("not opened again");
        };
        assert_eq!(
            past(&again).collect::<Vec<_>>(),
            ["comics.reader", "other.reader"]
        );
    }

    #[test]
    fn a_reader_reads_its_formats_or_all() {
        let epub = Id::new_static("format-epub.epub");
        let all = ReaderSpec::new(Id::new_static("a.reader"), Id::new_static("a.screen"));
        let comics = ReaderSpec::new(Id::new_static("c.reader"), Id::new_static("c.screen"))
            .formats(&[Id::new_static("format-cbz.cbz")]);
        assert!(all.reads(Some(&epub)) && all.reads(None));
        assert!(!comics.reads(Some(&epub)) && !comics.reads(None));
    }
}
