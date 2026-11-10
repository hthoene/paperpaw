//! The lists the plugin keeps in its data folder, over a [`Volume`]: the
//! articles fetched (`articles.txt`) and those read to the end, to archive
//! (`finished.txt`), what tells whose they are, and the noting of an article
//! read to the end. The caps and the shapes are those of `native_read_later`
//! (interface 6's); a list that cannot be read is an error and is never
//! written over (what it holds would be lost).
use alloc::{string::String, vec::Vec};

use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{
    files::{Volume, read_all_on, replace_on},
    mem::push,
    text::copy,
};

use crate::native_read_later::{
    ARTICLES_CAP, Article, FINISHED_CAP, articles, articles_text, finished, is_read,
};

/// The articles fetched, in the data folder.
pub const ARTICLES_FILE: &str = "articles.txt";
/// The articles read to the end, in the data folder.
pub const FINISHED_FILE: &str = "finished.txt";
/// The folder of the articles' books, below the library folder.
pub const FOLDER: &str = "Reader";

/// A list's file as text: `None` where there is none yet.
fn text_of(volume: &dyn Volume, name: &str, cap: usize) -> Result<Option<String>, Error> {
    let Some(bytes) = read_all_on(volume, name, cap)? else {
        return Ok(None);
    };
    String::from_utf8(bytes).map(Some).map_err(|_| Error::Utf8)
}

/// The articles fetched. An error for a file over its cap or not text: the
/// list is then not to be written over.
///
/// # Errors
/// `NoMemory`, `NoSpace`, `Utf8` or what the card says.
pub fn load_articles(volume: &dyn Volume) -> Result<Vec<Article>, Error> {
    match text_of(volume, ARTICLES_FILE, ARTICLES_CAP)? {
        Some(text) => articles(&text),
        None => Ok(Vec::new()),
    }
}

/// The ids of the articles read to the end.
///
/// # Errors
/// As [`load_articles`].
pub fn load_finished(volume: &dyn Volume) -> Result<Vec<String>, Error> {
    match text_of(volume, FINISHED_FILE, FINISHED_CAP)? {
        Some(text) => finished(&text),
        None => Ok(Vec::new()),
    }
}

/// Keeps the articles fetched.
///
/// # Errors
/// `NoMemory`; what the card says.
pub fn save_articles(volume: &dyn Volume, list: &[Article]) -> Result<(), Error> {
    replace_on(volume, ARTICLES_FILE, articles_text(list)?.as_bytes())
}

/// Keeps the ids of the articles read to the end.
///
/// # Errors
/// `NoMemory`; what the card says.
pub fn save_finished(volume: &dyn Volume, ids: &[String]) -> Result<(), Error> {
    let size = ids.iter().map(|id| id.len() + 1).sum();
    let mut out = String::new();
    out.try_reserve_exact(size).map_err(|_| Error::NoMemory)?;
    for id in ids {
        out.push_str(id);
        out.push('\n');
    }
    replace_on(volume, FINISHED_FILE, out.as_bytes())
}

/// Whether `path` is in a folder `Reader` (the cheap test before a list is
/// read on a page turn; the list says whether it is an article's book).
fn in_folder(path: &str) -> bool {
    path.rsplit_once('/')
        .is_some_and(|(dir, _)| dir == FOLDER || dir.ends_with("/Reader"))
}

/// A page of the book at `path` was turned: where that is the end of an
/// article's book, its id is noted among those to archive. Nothing is
/// noted for a book that is not an article's, or when a list cannot be read.
///
/// # Errors
/// `NoMemory`.
pub fn page_turned(
    volume: &dyn Volume,
    path: &str,
    (page, pages, progress_ppm): (Option<u32>, Option<u32>, u32),
) -> Result<(), Error> {
    if !is_read(page, pages, progress_ppm) || !in_folder(path) {
        return Ok(());
    }
    let list = match load_articles(volume) {
        Ok(list) => list,
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => return Ok(()),
    };
    let Some(article) = list.iter().find(|a| a.path == path) else {
        return Ok(());
    };
    let mut ids = match load_finished(volume) {
        Ok(ids) => ids,
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => return Ok(()),
    };
    if ids.contains(&article.id) {
        return Ok(());
    }
    push(&mut ids, copy(&article.id)?)?;
    // A card that does not take it costs the noting.
    settle(save_finished(volume, &ids))
}

/// A failed write is let go of (the lists stay as they were); memory is not.
pub fn settle(result: Result<(), Error>) -> Result<(), Error> {
    match result {
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Ok(()) | Err(_) => Ok(()),
    }
}
