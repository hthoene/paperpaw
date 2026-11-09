//! The files the plugin keeps in its data folder (`core.storage`, root 0): the
//! articles fetched (`articles.txt`), those read to the end (`finished.txt`)
//! and whose they are (`account.txt`), the same files with the same rules as
//! interface 6. Every read allocates fallibly and has a cap; a list that is
//! too large or unreadable is never taken for an empty one, so no caller
//! writes the file over what it holds.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{
    Storage, StorageError,
    native::{
        Error, Host,
        storage::{self, DataStorage, Root},
    },
};
use paperpaw_ui_kit::text::concat;

use crate::native_wallabag::{self as lists, ARTICLES_CAP, Article, FINISHED_CAP};

/// The plugin's data folder as the lists need it: files read in pieces,
/// written whole or not at all, and removed. [`DataFiles`] is the host's; a
/// test gives its own.
pub trait Files {
    /// The size of the file `name`.
    ///
    /// # Errors
    /// `NotFound` where there is none; what the host says.
    fn size(&self, name: &str) -> Result<u64, Error>;
    /// Reads at `offset`; a short read is the end.
    ///
    /// # Errors
    /// What the host says.
    fn read_at(&self, name: &str, offset: u64, out: &mut [u8]) -> Result<usize, Error>;
    /// Writes `data` as the whole file `name`.
    ///
    /// # Errors
    /// What the host says (`Full` for a full card).
    fn write(&self, name: &str, data: &[u8]) -> Result<(), Error>;
    /// Takes the file `name` away.
    ///
    /// # Errors
    /// What the host says; a file that is not there is `NotFound`.
    fn remove(&self, name: &str) -> Result<(), Error>;
}

/// The data folder through the host (`core.storage`, root 0).
#[derive(Debug, Clone, Copy)]
pub struct DataFiles<'h, 'cb>(&'h Host<'cb>);

impl<'h, 'cb> DataFiles<'h, 'cb> {
    #[must_use]
    pub const fn new(host: &'h Host<'cb>) -> Self {
        Self(host)
    }
}

impl Files for DataFiles<'_, '_> {
    fn size(&self, name: &str) -> Result<u64, Error> {
        storage::size(self.0, Root::Data, name)
    }

    fn read_at(&self, name: &str, offset: u64, out: &mut [u8]) -> Result<usize, Error> {
        DataStorage::new(self.0)
            .read_at(name, offset, out)
            .map_err(from_storage)
    }

    fn write(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        DataStorage::new(self.0)
            .write(name, data)
            .map_err(from_storage)
    }

    fn remove(&self, name: &str) -> Result<(), Error> {
        storage::remove(self.0, Root::Data, name)
    }
}

/// The articles fetched, `id<TAB>path` lines.
pub const ARTICLES: &str = "articles.txt";
/// The ids of the articles read to the end, to archive.
pub const FINISHED: &str = "finished.txt";
/// The server and the user name the lists belong to.
pub const ACCOUNT: &str = "account.txt";
/// What Wallabag answered a list and a single article with.
pub const ENTRIES: &str = "entries.json";
pub const ENTRY: &str = "entry.json";
/// The book being downloaded, before it goes into the library.
pub const TMP: &str = "article.tmp";
const ACCOUNT_CAP: usize = 4 * 1024;

/// The text of the file `name`: empty where there is none.
///
/// # Errors
/// `NoSpace` over `cap` bytes; `Utf8` for text that is not; `NoMemory`; what
/// the host says.
pub fn read_text(files: &dyn Files, name: &str, cap: usize) -> Result<String, Error> {
    let size = match files.size(name) {
        Ok(size) => size,
        Err(Error::NotFound) => return Ok(String::new()),
        Err(error) => return Err(error),
    };
    let len = usize::try_from(size).map_err(|_| Error::NoSpace)?;
    if len > cap {
        return Err(Error::NoSpace);
    }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
    bytes.resize(len, 0);
    let mut read = 0;
    while read < len {
        let part = bytes.get_mut(read..).ok_or(Error::Invalid)?;
        match files.read_at(name, read as u64, part)? {
            0 => break,
            n => read += n,
        }
    }
    bytes.truncate(read);
    String::from_utf8(bytes).map_err(|_| Error::Utf8)
}

/// Writes `text` as the file `name`, whole or not at all.
///
/// # Errors
/// What the host says (`Full` for a full card); `NoMemory`.
pub fn write_text(files: &dyn Files, name: &str, text: &str) -> Result<(), Error> {
    files.write(name, text.as_bytes())
}

/// Takes the file `name` away; one that is not there is as good.
pub fn remove(files: &dyn Files, name: &str) {
    match files.remove(name) {
        Ok(()) | Err(_) => {}
    }
}

fn from_storage(error: StorageError) -> Error {
    match error {
        StorageError::NotFound => Error::NotFound,
        StorageError::Full => Error::Full,
        StorageError::OutOfMemory => Error::NoMemory,
        _ => Error::Io,
    }
}

/// The articles fetched.
///
/// # Errors
/// As [`read_text`], and `NoMemory`.
pub fn articles(files: &dyn Files) -> Result<Vec<Article>, Error> {
    lists::articles(&read_text(files, ARTICLES, ARTICLES_CAP)?)
}

/// Keeps the articles fetched.
///
/// # Errors
/// As [`write_text`].
pub fn save_articles(files: &dyn Files, list: &[Article]) -> Result<(), Error> {
    write_text(files, ARTICLES, &lists::articles_text(list)?)
}

/// The ids of the articles read to the end.
///
/// # Errors
/// As [`read_text`], and `NoMemory`.
pub fn finished(files: &dyn Files) -> Result<Vec<String>, Error> {
    lists::finished(&read_text(files, FINISHED, FINISHED_CAP)?)
}

/// Keeps the ids of the articles read to the end.
///
/// # Errors
/// As [`write_text`].
pub fn save_finished(files: &dyn Files, ids: &[String]) -> Result<(), Error> {
    let mut text = String::new();
    let size = ids.iter().map(|id| id.len() + 1).sum();
    text.try_reserve_exact(size).map_err(|_| Error::NoMemory)?;
    for id in ids {
        text.push_str(id);
        text.push('\n');
    }
    write_text(files, FINISHED, &text)
}

/// Whose the lists are: the server and the user name, one a line.
///
/// # Errors
/// `NoMemory`.
pub fn account(server: &str, user: &str) -> Result<String, Error> {
    concat(&[server, "\n", user, "\n"])
}

/// The articles fetched and those read to the end, from this `account`. Those
/// of another server or user (the settings changed) are let go of first:
/// their books stay in the library, never asked about at a server that does
/// not know them. An older version kept no account: its lists are this one's.
///
/// # Errors
/// As [`read_text`], and `NoMemory`.
pub fn of_account(files: &dyn Files, account: &str) -> Result<(Vec<Article>, Vec<String>), Error> {
    let (fetched, finished) = (articles(files)?, finished(files)?);
    let kept = read_text(files, ACCOUNT, ACCOUNT_CAP)?;
    if kept == account {
        return Ok((fetched, finished));
    }
    let lists = if kept.is_empty() {
        (fetched, finished)
    } else {
        save_articles(files, &[])?;
        write_text(files, FINISHED, "")?;
        (Vec::new(), Vec::new())
    };
    write_text(files, ACCOUNT, account)?;
    Ok(lists)
}

/// Notes the article whose book is at `path` as read to the end (to archive
/// with the next fetch). Only a book in the plugin's folder is looked up, so
/// a page turn in any other book costs no read of the lists.
///
/// # Errors
/// As [`articles`] and [`save_finished`]; the caller lets go of all but `NoMemory`.
pub fn note_read(files: &dyn Files, path: &str) -> Result<(), Error> {
    const FOLDER: &str = "Wallabag";
    let in_folder = path
        .strip_prefix(FOLDER)
        .is_some_and(|rest| rest.starts_with('/'))
        || path.contains("/Wallabag/");
    if !in_folder {
        return Ok(());
    }
    let Some(article) = articles(files)?.into_iter().find(|a| a.path == path) else {
        return Ok(());
    };
    let mut ids = finished(files)?;
    if !ids.contains(&article.id) {
        ids.try_reserve(1).map_err(|_| Error::NoMemory)?;
        ids.push(article.id);
        save_finished(files, &ids)?;
    }
    Ok(())
}
