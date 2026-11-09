//! Browsing a catalog, step by step through `core.net`, the data folder and
//! `core.library`: a feed's entries, each leading to a further feed or a book;
//! *Search* where the catalog can be searched and *More* where its list goes
//! on. A book is downloaded into the library, in the best format PaperPaw
//! reads. The same rules as interface 6's `BrowseScreen`, as an engine the
//! screen drives.
//!
//! - One request of each kind at a time (a feed, a description, a book); the
//!   end of a request is one `http_done` event ([`Browser::done`] takes it;
//!   an answer for a request that is not the one waited for is ignored). A
//!   feed goes to a file and is read in pieces by the readers
//!   ([`native_model`](crate::native_model), `native_json`, `native_dav`).
//! - Everything a catalog sends is hostile: the readers cap what they keep,
//!   names are cleaned, links are resolved and only the catalog's own address
//!   gets the password.
//! - A refused allocation is `Error::NoMemory` and ends the call; any other
//!   failure is said in the note and the screen goes on.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{
    native::{
        Error, Host,
        library::{Existing, Library},
        net::{HttpDone, HttpProgress},
    },
    web::host,
};
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy},
};

use crate::native_catalogs::Catalog;
use crate::native_download::{book_name, import, may_be_book};
use crate::native_files::{self as files, FileSource, Files};
use crate::native_json;
use crate::native_keep::Secret;
use crate::native_model::{self as model, Feed, Limits, Search};
use crate::native_search;
use crate::native_web::{
    self as web, BOOK, BOOK_BYTES, BOOK_FILE, COVER, COVER_BYTES, COVER_FILE, Call, DESCRIPTION,
    DESCRIPTION_FILE, Ended, FEED, FEED_FILE, SMALL_FILE, Web,
};

/// The feeds a screen goes back through, at most: the oldest is let go of.
const fn max_trail(small: bool) -> usize {
    if small { 12 } else { 32 }
}

/// The line is drawn again at most once it came this many thousandths
/// further, or this long after: each refresh holds the device (on the X4 Pro
/// some 0.6 s), and the download waits meanwhile.
const SHOWN_STEP: u16 = 100;
const SHOWN_MS: u64 = 5_000;

/// What the line below the list says.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Note {
    /// Nothing.
    #[default]
    Empty,
    Loading(String),
    NoNetwork,
    FailedHost(String),
    Unauthorized,
    Error {
        host: String,
        status: u32,
    },
    NotACatalog,
    NoMemory,
    NoSearch,
    NoFormat,
    NotABook,
    CannotSave,
    Downloading(String),
    Share {
        title: String,
        percent: u16,
    },
    Size {
        title: String,
        bytes: u64,
    },
    Stopped,
    Saved {
        name: String,
        folder: String,
    },
}

/// A row of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Row {
    Search,
    Entry(usize),
    More,
}

/// What the screen is to do after a call.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Step {
    /// Nothing more than a redraw.
    Nothing,
    /// Ask for the words to search for.
    AskTerms,
    /// A book was saved at this path: offer to read it.
    Saved(String),
    /// Back left the first feed: the screen closes.
    Leave,
}

/// A book being downloaded: its title, and how much came of how much (where
/// the server said).
#[derive(Debug)]
struct Download {
    id: paperpaw_sdk::native::net::RequestId,
    /// Where it comes from, for what is said of it.
    url: String,
    title: String,
    name: String,
    received: u64,
    total: Option<u64>,
    /// When the line was last drawn, and the share it showed.
    shown: Option<(u64, Option<u16>)>,
}

impl Download {
    /// How much came, in thousandths, where the size is known.
    fn permille(&self) -> Option<u16> {
        let total = self.total.filter(|t| *t > 0)?;
        let share = (u128::from(self.received) * 1000) / u128::from(total);
        u16::try_from(share.min(1000)).ok()
    }
}

#[derive(Debug)]
struct Loading {
    id: paperpaw_sdk::native::net::RequestId,
    url: String,
    /// Going back to a feed shown before, not following a link.
    back: bool,
}

/// The browsing of one catalog.
#[derive(Debug)]
pub struct Browser {
    catalog: Catalog,
    auth: Option<Secret>,
    library: String,
    limits: Limits,
    small: bool,
    /// The feeds shown, the current one last.
    trail: Vec<String>,
    feed: Feed,
    /// Where searches go, once known.
    template: Option<String>,
    loading: Option<Loading>,
    describing: Option<paperpaw_sdk::native::net::RequestId>,
    download: Option<Download>,
    note: Note,
    /// How many feeds were installed: told apart by the covers of each.
    generation: u16,
}

impl Browser {
    /// Browsing `catalog` with its `password`, its books going below the
    /// library folder `library`; `small` for a small device.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new(
        catalog: Catalog,
        password: Option<Secret>,
        library: &str,
        small: bool,
    ) -> Result<Self, Error> {
        let auth = match password {
            Some(password) if !catalog.user.is_empty() => {
                Some(Secret::basic(&catalog.user, &password)?)
            }
            _ => None,
        };
        Ok(Self {
            library: copy(library)?,
            limits: Limits::of_class(small),
            small,
            catalog,
            auth,
            trail: Vec::new(),
            feed: Feed::default(),
            template: None,
            loading: None,
            describing: None,
            download: None,
            note: Note::Empty,
            generation: 0,
        })
    }

    /// The catalog.
    #[must_use]
    pub const fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// The feed shown.
    #[must_use]
    pub const fn feed(&self) -> &Feed {
        &self.feed
    }

    /// Which feed is shown: it changes each time a feed is installed.
    #[must_use]
    pub const fn generation(&self) -> u16 {
        self.generation
    }

    /// Asks for the cover at `url` (it goes to the file of covers, and only
    /// the catalog's own address gets its password).
    ///
    /// # Errors
    /// What `Web::send` says.
    pub fn cover_request(
        &self,
        host_: &Host<'_>,
        web: &mut Web,
        url: &str,
    ) -> Result<paperpaw_sdk::native::net::RequestId, Error> {
        let call = Call::new(COVER, url, COVER_FILE, COVER_BYTES);
        web.send(host_, &self.catalog, self.auth.as_ref(), &call)
    }

    /// The line below the list.
    #[must_use]
    pub const fn note(&self) -> &Note {
        &self.note
    }

    /// Whether a feed is being loaded.
    #[must_use]
    pub const fn loading(&self) -> bool {
        self.loading.is_some()
    }

    /// Whether a book is being downloaded.
    #[must_use]
    pub const fn downloading(&self) -> bool {
        self.download.is_some()
    }

    /// How far the download came, in thousandths, where known.
    #[must_use]
    pub fn permille(&self) -> Option<u16> {
        self.download.as_ref().and_then(Download::permille)
    }

    /// The rows of the list.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn rows(&self) -> Result<Vec<Row>, Error> {
        let mut rows = Vec::new();
        rows.try_reserve(self.feed.entries.len() + 2)
            .map_err(|_| Error::NoMemory)?;
        if self.feed.search.is_some() {
            rows.push(Row::Search);
        }
        rows.extend((0..self.feed.entries.len()).map(Row::Entry));
        if self.feed.next.is_some() {
            rows.push(Row::More);
        }
        Ok(rows)
    }

    /// Loads the catalog's first feed.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn start(&mut self, host: &Host<'_>, web: &mut Web) -> Result<(), Error> {
        let url = copy(&self.catalog.url)?;
        self.load(host, web, &url, false)
    }

    /// Says `note`.
    fn say(&mut self, note: Note) {
        self.note = note;
    }

    /// The text of `Loading` for `url`.
    fn loading_note(url: &str) -> Result<Note, Error> {
        Ok(Note::Loading(copy(host(url))?))
    }

    /// Loads feed `url`; `back` when returning to it.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn load(
        &mut self,
        host_: &Host<'_>,
        web: &mut Web,
        url: &str,
        back: bool,
    ) -> Result<(), Error> {
        let call = Call::new(FEED, url, FEED_FILE, SMALL_FILE).list();
        match web.send(host_, &self.catalog, self.auth.as_ref(), &call) {
            Ok(id) => {
                self.loading = Some(Loading {
                    id,
                    url: copy(url)?,
                    back,
                });
                self.note = Self::loading_note(url)?;
                Ok(())
            }
            Err(error) => self.refused(error, url),
        }
    }

    /// A request that could not be made: said, or `NoMemory`.
    fn refused(&mut self, error: Error, url: &str) -> Result<(), Error> {
        let note = match error {
            Error::NoMemory => return Err(Error::NoMemory),
            Error::Unsupported | Error::Denied => Note::NoNetwork,
            _ => Note::FailedHost(copy(host(url))?),
        };
        self.say(note);
        Ok(())
    }

    /// What an end that is not an answer says: `None` where it is fine (or
    /// not told).
    fn told(&mut self, ended: Ended, url: &str) -> Result<bool, Error> {
        let note = match ended {
            Ended::Fine => return Ok(true),
            Ended::Stopped => return Ok(false),
            Ended::Unauthorized => Note::Unauthorized,
            Ended::NoNetwork => Note::NoNetwork,
            Ended::Unreached => Note::FailedHost(copy(host(url))?),
            Ended::Status(status) => Note::Error {
                host: copy(host(url))?,
                status,
            },
        };
        self.say(note);
        Ok(false)
    }

    /// The end of a request of this browsing; `None` for one that is not.
    ///
    /// # Errors
    /// `NoMemory`.
    #[allow(
        clippy::too_many_arguments,
        reason = "the services a step may need, passed as they are"
    )]
    pub fn done(
        &mut self,
        host_: &Host<'_>,
        web: &mut Web,
        files: &dyn Files,
        library: Option<&Library>,
        done: &HttpDone,
    ) -> Result<Option<Step>, Error> {
        match done.token() {
            FEED if self.loading.as_ref().is_some_and(|l| l.id == done.id()) => {
                self.loaded(host_, web, files, done).map(Some)
            }
            DESCRIPTION if self.describing == Some(done.id()) => {
                self.described(host_, web, files, done).map(Some)
            }
            BOOK if self.download.as_ref().is_some_and(|d| d.id == done.id()) => {
                self.downloaded(host_, files, library, done).map(Some)
            }
            _ => Ok(None),
        }
    }

    /// A feed arrived.
    fn loaded(
        &mut self,
        host_: &Host<'_>,
        web: &mut Web,
        files: &dyn Files,
        done: &HttpDone,
    ) -> Result<Step, Error> {
        let Some(Loading { url, back, .. }) = self.loading.take() else {
            return Ok(Step::Nothing);
        };
        let ended = web::ended(done);
        if !self.told(ended, &url)? {
            files::remove(files, FEED_FILE);
            return Ok(Step::Nothing);
        }
        // Links are resolved where the feed came from, after redirects.
        let base = web.final_url(host_, done.id())?;
        let read = self.read_feed(files, FEED_FILE, base.as_deref().unwrap_or(&url));
        files::remove(files, FEED_FILE);
        let feed = match read {
            Ok(feed) => feed,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => {
                self.say(Note::NotACatalog);
                return Ok(Step::Nothing);
            }
        };
        if feed.memory {
            self.say(Note::NoMemory);
            return Ok(Step::Nothing);
        }
        if feed.entries.is_empty() && feed.title.is_empty() {
            self.say(Note::NotACatalog);
            return Ok(Step::Nothing);
        }
        if back {
            self.trail.pop();
        } else {
            if self.trail.len() >= max_trail(self.small) {
                self.trail.remove(0);
            }
            push(&mut self.trail, url)?;
        }
        if feed.search != self.feed.search {
            self.template = None;
        }
        self.feed = feed;
        self.generation = self.generation.wrapping_add(1);
        self.note = Note::Empty;
        Ok(Step::Nothing)
    }

    /// The feed in `file`, which came from `base`: OPDS 2 where it is JSON,
    /// a WebDAV listing for a cloud folder, else OPDS 1. A final address the
    /// host followed is not told here: links are resolved against the address
    /// asked for.
    fn read_feed(&self, files: &dyn Files, file: &str, base: &str) -> Result<Feed, Error> {
        let mut source = FileSource::new(files, file);
        if self.catalog.webdav {
            crate::native_dav::read(&mut source, base, self.limits)
        } else if native_json::is_json(&mut source)? {
            native_json::read(&mut source, base, self.limits)
        } else {
            model::read(&mut source, base, self.limits)
        }
    }

    /// Asks for the words to search for, once the address is known; the
    /// description asked for first where the catalog gives one.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn search(&mut self, host_: &Host<'_>, web: &mut Web) -> Result<Step, Error> {
        match (&self.template, &self.feed.search) {
            (Some(_), _) | (None, Some(Search::Template(_))) => {
                if self.template.is_none()
                    && let Some(Search::Template(template)) = &self.feed.search
                {
                    self.template = Some(copy(template)?);
                }
                Ok(Step::AskTerms)
            }
            (None, Some(Search::Description(url))) => {
                let url = copy(url)?;
                let call = Call::new(DESCRIPTION, &url, DESCRIPTION_FILE, SMALL_FILE);
                match web.send(host_, &self.catalog, self.auth.as_ref(), &call) {
                    Ok(id) => {
                        self.describing = Some(id);
                        self.note = Self::loading_note(&url)?;
                    }
                    Err(error) => self.refused(error, &url)?,
                }
                Ok(Step::Nothing)
            }
            (None, None) => Ok(Step::Nothing),
        }
    }

    /// The description arrived: where to search, and the words asked for.
    fn described(
        &mut self,
        host_: &Host<'_>,
        web: &mut Web,
        files: &dyn Files,
        done: &HttpDone,
    ) -> Result<Step, Error> {
        self.describing = None;
        let url = match &self.feed.search {
            Some(Search::Description(url)) => copy(url)?,
            _ => String::new(),
        };
        let ended = web::ended(done);
        if !self.told(ended, &url)? {
            files::remove(files, DESCRIPTION_FILE);
            return Ok(Step::Nothing);
        }
        let base = web.final_url(host_, done.id())?;
        let found = native_search::template(
            &mut FileSource::new(files, DESCRIPTION_FILE),
            base.as_deref().unwrap_or(&url),
        );
        files::remove(files, DESCRIPTION_FILE);
        match found {
            Ok(Some(template)) => {
                self.template = Some(template);
                self.note = Note::Empty;
                self.search(host_, web)
            }
            Ok(None) => {
                self.say(Note::NoSearch);
                Ok(Step::Nothing)
            }
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => {
                self.say(Note::NoSearch);
                Ok(Step::Nothing)
            }
        }
    }

    /// The words are typed: the search is loaded as a feed.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn terms(&mut self, host_: &Host<'_>, web: &mut Web, terms: &str) -> Result<(), Error> {
        let terms = terms.trim();
        let Some(template) = self.template.as_deref().filter(|_| !terms.is_empty()) else {
            return Ok(());
        };
        let url = match native_search::url(template, terms) {
            Ok(url) => url,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => {
                self.say(Note::NoSearch);
                return Ok(());
            }
        };
        self.load(host_, web, &url, false)
    }

    /// A row was chosen: a search, a book to download, a feed to open, the
    /// next page.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn activate(
        &mut self,
        host_: &Host<'_>,
        web: &mut Web,
        row: Row,
        extensions: &[String],
    ) -> Result<Step, Error> {
        match row {
            Row::Search => self.search(host_, web),
            Row::More => {
                if let Some(next) = self.feed.next.as_deref().map(copy).transpose()? {
                    self.load(host_, web, &next, false)?;
                }
                Ok(Step::Nothing)
            }
            Row::Entry(i) => self.open(host_, web, i, extensions).map(|()| Step::Nothing),
        }
    }

    /// Downloads entry `i`'s book, or opens the feed it leads to.
    fn open(
        &mut self,
        host_: &Host<'_>,
        web: &mut Web,
        i: usize,
        extensions: &[String],
    ) -> Result<(), Error> {
        let Some(entry) = self.feed.entries.get(i) else {
            return Ok(());
        };
        let readable = |ext: &str| extensions.iter().any(|e| e == ext);
        if let Some((book, ext)) = entry.readable_book(readable) {
            let (url, title) = (copy(&book.url)?, copy(&entry.title)?);
            let name = book_name(entry, &book.url, ext)?;
            let call = Call::new(BOOK, &url, BOOK_FILE, BOOK_BYTES).progress();
            match web.send(host_, &self.catalog, self.auth.as_ref(), &call) {
                Ok(id) => {
                    self.note = Note::Downloading(copy(&title)?);
                    self.download = Some(Download {
                        id,
                        url,
                        title,
                        name,
                        received: 0,
                        total: None,
                        shown: None,
                    });
                    return Ok(());
                }
                Err(error) => return self.refused(error, &url),
            }
        }
        if let Some(url) = entry.feed.as_deref().map(copy).transpose()? {
            return self.load(host_, web, &url, false);
        }
        self.say(Note::NoFormat);
        Ok(())
    }

    /// How far the download came: its share (or its bytes, where its size is
    /// not known) in the line below. `true` where the line is to be drawn
    /// again (at most once in a while).
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn progressed(&mut self, progress: &HttpProgress) -> Result<bool, Error> {
        let Some(download) = self.download.as_mut().filter(|d| d.id == progress.id()) else {
            return Ok(false);
        };
        download.received = progress.received();
        download.total = progress.total();
        let now = progress.now_ms();
        let share = download.permille();
        let due = download.shown.is_none_or(|(at, was)| {
            now.saturating_sub(at) >= SHOWN_MS
                || matches!((share, was), (Some(now), Some(was)) if now >= was.saturating_add(SHOWN_STEP))
        });
        if !due {
            return Ok(false);
        }
        download.shown = Some((now, share));
        let title = copy(&download.title)?;
        self.note = match share {
            Some(permille) => Note::Share {
                title,
                percent: permille / 10,
            },
            None => Note::Size {
                title,
                bytes: download.received,
            },
        };
        Ok(true)
    }

    /// Stops the download going on: its answer is not waited for, and what
    /// came of it goes with its file.
    pub fn stop_download(&mut self, host_: &Host<'_>, web: &mut Web, files: &dyn Files) {
        if self.download.take().is_some() {
            web.cancel(host_, BOOK);
            files::remove(files, BOOK_FILE);
            self.say(Note::Stopped);
        }
    }

    /// The download ended: the file checked and put into the library.
    fn downloaded(
        &mut self,
        host_: &Host<'_>,
        files: &dyn Files,
        library: Option<&Library>,
        done: &HttpDone,
    ) -> Result<Step, Error> {
        let Some(download) = self.download.take() else {
            return Ok(Step::Nothing);
        };
        let ended = web::ended(done);
        if !self.told(ended, &download.url)? {
            files::remove(files, BOOK_FILE);
            return Ok(Step::Nothing);
        }
        let checked = may_be_book(files, BOOK_FILE, &download.name);
        if !checked? {
            files::remove(files, BOOK_FILE);
            self.say(Note::NotABook);
            return Ok(Step::Nothing);
        }
        let folder = self.catalog.sync_folder(&self.library)?;
        let saved = match library {
            Some(library) => import(
                host_,
                library,
                BOOK_FILE,
                &folder,
                &download.name,
                Existing::FreeName,
            ),
            None => Err(Error::Unsupported),
        };
        match saved {
            Ok(path) => {
                let shown = self.catalog.shown_folder(&self.library)?;
                let name = path.rsplit('/').next().unwrap_or(&path);
                self.note = Note::Saved {
                    name: copy(name)?,
                    folder: shown,
                };
                Ok(Step::Saved(path))
            }
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => {
                files::remove(files, BOOK_FILE);
                self.say(Note::CannotSave);
                Ok(Step::Nothing)
            }
        }
    }

    /// Back: to the feed before, and from the first to the list of catalogs.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn back(&mut self, host_: &Host<'_>, web: &mut Web) -> Result<Step, Error> {
        let Some(previous) = self
            .trail
            .len()
            .checked_sub(2)
            .and_then(|at| self.trail.get(at))
            .map(|url| copy(url))
            .transpose()?
        else {
            return Ok(Step::Leave);
        };
        self.load(host_, web, &previous, true)?;
        Ok(Step::Nothing)
    }

    /// What the screen shows of row `row`: its title, the value at the right
    /// and the note below.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn text_of(
        &self,
        row: Row,
        extensions: &[String],
    ) -> Result<(Option<String>, String, String), Error> {
        let Row::Entry(i) = row else {
            return Ok((None, copy("›")?, String::new()));
        };
        let Some(entry) = self.feed.entries.get(i) else {
            return Ok((None, String::new(), String::new()));
        };
        let readable = |ext: &str| extensions.iter().any(|e| e == ext);
        let book = entry.readable_book(readable);
        let value = match book {
            Some((_, ext)) => upper(ext)?,
            None if entry.feed.is_some() => copy("›")?,
            None => String::new(),
        };
        // Which edition, where a catalog offers several.
        let mut note = copy(entry.note())?;
        if let Some((book, _)) = book.filter(|(b, _)| !b.title.is_empty()) {
            note = if note.is_empty() {
                copy(&book.title)?
            } else {
                concat(&[&note, " · ", &book.title])?
            };
        }
        Ok((Some(copy(&entry.title)?), value, note))
    }
}

/// `text` in capitals.
fn upper(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
    out.extend(text.chars().flat_map(char::to_uppercase));
    Ok(out)
}
