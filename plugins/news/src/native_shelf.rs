//! The library as the fetch uses it: the page of a feed goes in with
//! `Library::import` as the book `Name (mark).html` in the folder `News`
//! (the library says where it went), and the book's path and the feed's new
//! headlines are kept in the data folder for the list and the widget.
use alloc::string::String;
use paperpaw_sdk::native::snapshot::capture_snapshot;
use paperpaw_sdk::native::{
    Error, Host,
    library::{Existing, Import, Library},
    settings::{self, SettingValue},
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
use crate::native_news::{FETCHED_FILE, Feed, HEADLINES_CAP, HEADLINES_FILE, Mark, headlines};
use crate::native_plugin::DataVolume;

/// The folder of the news, below the library's.
pub const FOLDER: &str = "News";
/// The setting that names the library's folder on the card.
const LIBRARY_FOLDER: &str = "core.library-folder";
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

/// The library calls a page's import makes, so that the choice of its name
/// is tested without a host.
pub(crate) trait Shelving {
    /// Imports `page` as `file` in [`FOLDER`]; the book's path.
    fn import(&mut self, page: &str, file: &str, existing: Existing) -> Result<String, Error>;
    /// Deletes the book `file` in [`FOLDER`] with what is kept for it, unless
    /// the reader marked something in it.
    fn delete(&mut self, file: &str) -> Result<(), Error>;
}

/// Puts `page` in as the book `file`, replacing the book of the last fetch.
///
/// A book at that name the library does not know as this plugin's import is
/// either Beta 3's (it wrote `News/<name> (<mark>).html` itself, so the
/// library has no importer for it, #459) or one moved there: it is the page
/// of an earlier fetch, so it is deleted and the page takes its name. Where
/// it stays (the reader marked something in it), the new one takes a free
/// name beside it.
///
/// # Errors
/// The library's refusals.
pub(crate) fn place(shelf: &mut impl Shelving, page: &str, file: &str) -> Result<String, Error> {
    match shelf.import(page, file, Existing::Replace) {
        Err(Error::Denied) => {}
        placed => return placed,
    }
    if shelf.delete(file).is_ok() {
        match shelf.import(page, file, Existing::Replace) {
            Err(Error::Denied) => {}
            placed => return placed,
        }
    }
    shelf.import(page, file, Existing::FreeName)
}

impl Shelving for HostShelf<'_, '_> {
    fn import(&mut self, page: &str, file: &str, existing: Existing) -> Result<String, Error> {
        let host = self.host;
        let library = self.opened(host)?;
        let (mut input, mut out) = (buffer(2048)?, buffer(4096)?);
        let import = Import::new(page, file).folder(FOLDER).existing(existing);
        text::copy(library.import(host, &import, &mut input, &mut out)?)
    }

    fn delete(&mut self, file: &str) -> Result<(), Error> {
        let host = self.host;
        // The book's path on the card: below the library's folder.
        let mut scratch = [0; 256];
        let path = match settings::read_setting(host, LIBRARY_FOLDER, &mut scratch)? {
            SettingValue::Text(folder) if !folder.trim_matches('/').is_empty() => {
                text::format(format_args!("{}/{FOLDER}/{file}", folder.trim_matches('/')))?
            }
            _ => text::format(format_args!("{FOLDER}/{file}"))?,
        };
        let path = path.as_str();
        let library = self.opened(host)?;
        let (mut input, mut out) = (buffer(4096)?, buffer(4096)?);
        let report = library.delete(host, &[path], false, &mut input, &mut out)?;
        report.rows().next().ok_or(Error::Invalid)?.status()
    }
}

impl HostShelf<'_, '_> {
    /// The library, opened when first needed.
    fn opened(&mut self, host: &Host<'_>) -> Result<&Library, Error> {
        if self.library.is_none() {
            *self.library = Some(Library::open(host)?);
        }
        self.library.as_ref().ok_or(Error::State)
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
        // When they were kept, for the sleep screen; without the clock, none.
        let unix = capture_snapshot(self.host)
            .ok()
            .and_then(|snapshot| snapshot.unix_seconds())
            .unwrap_or(0);
        write(&volume, FETCHED_FILE, &alloc::format!("{unix}\n"))?;
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
        let path = place(self, page, &file)?;
        self.keep(feed, &path, converted)
    }
}

#[cfg(test)]
mod tests {
    use super::{Existing, Shelving, place};
    use alloc::string::{String, ToString};
    use alloc::vec::Vec;
    use paperpaw_sdk::native::Error;

    /// A library with one book at the name, imported by Beta 3 (no
    /// importer), or marked by the reader (it cannot be deleted).
    struct Library {
        importer_known: bool,
        marked: bool,
        calls: Vec<String>,
    }

    impl Shelving for Library {
        fn import(&mut self, _: &str, file: &str, existing: Existing) -> Result<String, Error> {
            self.calls.push(alloc::format!("import {existing:?}"));
            match existing {
                Existing::Replace if !self.importer_known => Err(Error::Denied),
                Existing::FreeName => Ok(alloc::format!(
                    "News/{} (2).html",
                    file.trim_end_matches(".html")
                )),
                _ => Ok(alloc::format!("News/{file}")),
            }
        }
        fn delete(&mut self, file: &str) -> Result<(), Error> {
            self.calls.push(alloc::format!("delete {file}"));
            if self.marked {
                return Err(Error::State);
            }
            // Gone: the name is free, the next import takes it.
            self.importer_known = true;
            Ok(())
        }
    }

    fn library(importer_known: bool, marked: bool) -> Library {
        Library {
            importer_known,
            marked,
            calls: Vec::new(),
        }
    }

    #[test]
    fn the_book_of_the_last_fetch_is_replaced() {
        let mut ours = library(true, false);
        assert_eq!(
            place(&mut ours, "page", "Feed (3fa9c2).html").unwrap(),
            "News/Feed (3fa9c2).html"
        );
        assert_eq!(ours.calls, ["import Replace"]);
    }

    #[test]
    fn beta_3s_book_of_a_feed_is_replaced_not_doubled() {
        // #459: Beta 3 wrote the book itself; the first fetch made a second
        // "(2)" book beside it for every feed.
        let mut beta3 = library(false, false);
        assert_eq!(
            place(&mut beta3, "page", "Feed (3fa9c2).html").unwrap(),
            "News/Feed (3fa9c2).html"
        );
        assert_eq!(
            beta3.calls,
            [
                "import Replace",
                "delete Feed (3fa9c2).html",
                "import Replace"
            ]
        );
    }

    #[test]
    fn a_book_the_reader_marked_stays_and_the_page_takes_a_free_name() {
        let mut marked = library(false, true);
        assert_eq!(
            place(&mut marked, "page", "Feed (3fa9c2).html").unwrap(),
            "News/Feed (3fa9c2) (2).html"
        );
        assert_eq!(
            marked.calls.last().map(ToString::to_string).as_deref(),
            Some("import FreeName")
        );
    }
}
