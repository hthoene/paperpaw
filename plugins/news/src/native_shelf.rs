//! The library as the fetch uses it: the page of a feed goes in with
//! `Library::import` as the book `Name (mark).html` in the folder `News`
//! (the library says where it went), and the book's path and the feed's new
//! headlines are kept in the data folder for the list and the widget.
use alloc::string::String;
use paperpaw_sdk::native::{
    Error, Host,
    library::{Existing, Import, Library},
};
use paperpaw_sdk::web::try_clean_name;
use paperpaw_ui_kit::{
    files::{Volume, read_all_on, replace_on},
    mem::buffer,
    text,
};

use crate::native_book::Failure;
use crate::native_books::{BOOKS_CAP, BOOKS_FILE, with as books_with};
use crate::native_convert::Converted;
use crate::native_fetch::Shelf;
use crate::native_news::{Feed, HEADLINES_CAP, HEADLINES_FILE, Mark, headlines};
use crate::native_plugin::DataVolume;

/// The folder of the news, below the library's.
pub const FOLDER: &str = "News";
/// The longest name made of a feed's name.
const NAME_MAX: usize = 60;

/// The library handle, opened when first needed.
#[derive(Debug, Default)]
pub(crate) struct Shelved {
    library: Option<Library>,
}

impl Shelved {
    /// The shelf for `host`.
    pub(crate) fn on<'a, 'h>(&'a mut self, host: &'a Host<'h>) -> HostShelf<'a, 'h> {
        HostShelf {
            host,
            library: &mut self.library,
        }
    }
}

/// The library of the host, as [`Shelf`].
pub(crate) struct HostShelf<'a, 'h> {
    host: &'a Host<'h>,
    library: &'a mut Option<Library>,
}

/// The name of `feed`'s book: its name and the mark of its address
/// (`Tagesschau (3fa9c2).html`; feeds of the same name keep books of their own).
///
/// # Errors
/// `NoMemory`.
pub fn book_file(feed: &Feed) -> Result<String, Error> {
    let name = try_clean_name(feed.shown(), NAME_MAX).map_err(|_| Error::NoMemory)?;
    text::format(format_args!(
        "{} ({}).html",
        name.as_deref().unwrap_or(FOLDER),
        Mark::of(&feed.url).as_str()
    ))
}

impl HostShelf<'_, '_> {
    fn import(&mut self, page: &str, file: &str) -> Result<String, Error> {
        let host = self.host;
        if self.library.is_none() {
            *self.library = Some(Library::open(host)?);
        }
        let library = self.library.as_ref().ok_or(Error::State)?;
        let (mut input, mut out) = (buffer(2048)?, buffer(4096)?);
        // The book of the last fetch is replaced; one that is not the plugin's
        // any more (moved, or a file of the reader's own at that name) stays,
        // and the new one takes a free name.
        let mut existing = Existing::Replace;
        loop {
            let import = Import::new(page, file).folder(FOLDER).existing(existing);
            match library.import(host, &import, &mut input, &mut out) {
                Ok(path) => return text::copy(path),
                Err(Error::Denied) if existing == Existing::Replace => {
                    existing = Existing::FreeName;
                }
                Err(error) => return Err(error),
            }
        }
    }

    /// Keeps the path of the book and the headlines of the feed. A card that
    /// does not take them costs the list and the widget this one change only.
    fn keep(&self, feed: &Feed, path: &str, converted: &Converted) -> Result<(), Failure> {
        let volume = DataVolume(self.host);
        let old = lenient(read_all_on(&volume, BOOKS_FILE, BOOKS_CAP))?;
        let old = old.as_deref().map(String::from_utf8_lossy);
        let books = books_with(old.as_deref(), Mark::of(&feed.url).as_str(), path)?;
        write(&volume, BOOKS_FILE, &books)?;
        let old = lenient(read_all_on(&volume, HEADLINES_FILE, HEADLINES_CAP + 1))?;
        let lines = headlines(&converted.headlines, path, old.as_deref())?;
        write(&volume, HEADLINES_FILE, &lines)?;
        Ok(())
    }
}

/// A file that cannot be read is no file; only a refused allocation is an error.
fn lenient(
    read: Result<Option<alloc::vec::Vec<u8>>, Error>,
) -> Result<Option<alloc::vec::Vec<u8>>, Failure> {
    match read {
        Ok(bytes) => Ok(bytes),
        Err(Error::NoMemory) => Err(Failure::Memory),
        Err(_) => Ok(None),
    }
}

fn write(volume: &dyn Volume, name: &str, text: &str) -> Result<(), Failure> {
    match replace_on(volume, name, text.as_bytes()) {
        Err(Error::NoMemory) => Err(Failure::Memory),
        Ok(()) | Err(_) => Ok(()),
    }
}

impl Shelf for HostShelf<'_, '_> {
    fn put(
        &mut self,
        feed: &Feed,
        _: &str,
        page: &str,
        converted: &Converted,
    ) -> Result<(), Failure> {
        let file = book_file(feed)?;
        let path = self.import(page, &file)?;
        self.keep(feed, &path, converted)
    }
}
