//! Syncing catalogs: their books that are not on the device yet are downloaded
//! into each catalog's folder in the library, page after page of the catalog's
//! list, at most the limit set books a catalog each time. The interface-6 sync
//! as an engine through `core.net`, the data folder and `core.library`.
//!
//! Every book synced is remembered (by its id in the catalog) in a record of
//! the data folder, so a book the user deleted is not fetched again. A book
//! whose name is taken in the folder is remembered without being fetched
//! again. The record is read line by line for each page of the list, so it
//! may grow without limit; one that cannot be read skips the catalog rather
//! than fetch again what the user deleted.
//!
//! It runs in the background: the answers come as `http_done` events whatever
//! screen is open, one request at a time, a book straight to a file of the data
//! folder and into the library once it is whole. Everything a catalog sends is
//! hostile: the readers cap what they keep, the queue of books is bounded in
//! bytes, pages are followed at most [`MAX_PAGES`] times and never twice, and
//! names are cleaned. A refused allocation ends the run (`Error::NoMemory`);
//! any other failure of a step counts as one failed book or catalog.
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::ops::ControlFlow;

use paperpaw_sdk::{
    digest::{fnv1a64, md5::Md5},
    native::{
        Error, Host,
        library::{Existing, Library},
        net::HttpDone,
    },
};
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy},
};

use crate::native_catalogs::Catalog;
use crate::native_dav;
use crate::native_download::{book_name, import, may_be_book};
use crate::native_files::{self as files, FileSource, Files};
use crate::native_json;
use crate::native_keep::Secret;
use crate::native_model::{self as model, Feed, Limits};
use crate::native_web::{
    self as web, BOOK_BYTES, Call, Ended, SMALL_FILE, SYNC_BOOK, SYNC_BOOK_FILE, SYNC_FEED,
    SYNC_FEED_FILE, Web,
};

/// Pages of a catalog's list read in one sync, at most.
pub const MAX_PAGES: usize = 50;
/// A record's line is at most this long: an id longer is written by its digest.
const MAX_ID: usize = 512;

/// A catalog to sync, with the password it is asked with.
#[derive(Debug)]
#[non_exhaustive]
pub struct Job {
    pub catalog: Catalog,
    pub password: Option<Secret>,
}

impl Job {
    /// `catalog` and its `password`.
    #[must_use]
    pub const fn new(catalog: Catalog, password: Option<Secret>) -> Self {
        Self { catalog, password }
    }
}

/// What a sync did, for the screens.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Report {
    /// The catalog being synced, or the last one.
    pub catalog: String,
    pub loaded: u32,
    pub failed: u32,
    /// Catalogs that could not be read.
    pub unreachable: u32,
    /// Catalogs whose address lists no books, only further lists (such as
    /// Project Gutenberg's start page): nothing to sync there.
    pub only_lists: u32,
    /// Catalogs whose record could not be read: left alone, so books deleted
    /// here do not come again.
    pub skipped: u32,
    pub running: bool,
}

/// What a step of the sync tells the plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Update {
    /// The sync goes on: the screens show the progress.
    Progress,
    /// The sync ended: its report is final.
    Finished,
}

/// A book to download.
#[derive(Debug)]
struct Item {
    /// Its id, as the record has it.
    id: String,
    name: String,
    url: String,
}

impl Item {
    fn weight(&self) -> usize {
        self.id.len() + self.name.len() + self.url.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Waiting {
    Feed,
    Book,
}

#[derive(Debug)]
struct Run {
    job: Job,
    auth: Option<Secret>,
    /// The catalogs after this one.
    waiting: VecDeque<Job>,
    folder: String,
    queue: VecDeque<Item>,
    queued: usize,
    /// The page of the list after the one read.
    next: Option<String>,
    /// The pages asked for, by digest of their addresses.
    pages: Vec<u64>,
    /// Books of this catalog loaded in this run.
    count: usize,
    pending: Option<(paperpaw_sdk::native::net::RequestId, Waiting)>,
    /// The page being read is the first.
    first: bool,
    /// The address of the page asked for last.
    asked: String,
}

/// What a sync needs of the device: the library folder, the extensions of the
/// formats it reads, whether it is a small one, and how many books a catalog
/// gets at most.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct Setup<'a> {
    pub library: &'a str,
    pub extensions: &'a [String],
    pub small: bool,
    pub limit: usize,
}

impl<'a> Setup<'a> {
    /// A device's setup.
    #[must_use]
    pub const fn new(
        library: &'a str,
        extensions: &'a [String],
        small: bool,
        limit: usize,
    ) -> Self {
        Self {
            library,
            extensions,
            small,
            limit,
        }
    }

    /// The most bytes of text the queue of books holds.
    const fn queue_bytes(&self) -> usize {
        if self.small { 24 * 1024 } else { 96 * 1024 }
    }
}

/// The sync of the plugin, shared by its screens and its work.
#[derive(Debug, Default)]
pub struct Sync {
    run: Option<Run>,
    report: Option<Report>,
}

/// The record of the books synced from the catalog at `url`.
///
/// # Errors
/// `NoMemory`.
pub fn record_path(url: &str) -> Result<String, Error> {
    let mut hash = Md5::new();
    hash.update(url.as_bytes());
    let mut hex = [0u8; 32];
    concat(&[files::SYNCED, "/", hash.hex_into(&mut hex), ".txt"])
}

/// How the book with `id` is written in the record: as it is, on one line; an
/// id longer than [`MAX_ID`] by its digest.
///
/// # Errors
/// `NoMemory`.
pub fn record_line(id: &str) -> Result<String, Error> {
    let mut line = String::new();
    if id.len() <= MAX_ID {
        line.try_reserve(id.len()).map_err(|_| Error::NoMemory)?;
        line.extend(
            id.chars()
                .map(|c| if matches!(c, '\n' | '\r') { ' ' } else { c }),
        );
        return Ok(line);
    }
    let mut hash = Md5::new();
    hash.update(id.as_bytes());
    let mut hex = [0u8; 32];
    concat(&["md5:", hash.hex_into(&mut hex)])
}

/// Which of `lines` the record at `path` holds, read line by line. No record
/// yet holds none; one that cannot be read is an error, never "none synced".
///
/// # Errors
/// `NoMemory`; what the card says.
pub fn recorded(files: &dyn Files, path: &str, lines: &[&str]) -> Result<Vec<bool>, Error> {
    let mut found = Vec::new();
    found
        .try_reserve_exact(lines.len())
        .map_err(|_| Error::NoMemory)?;
    found.resize(lines.len(), false);
    let read = files::for_each_line(files, path, &mut |line| {
        for (wanted, hit) in lines.iter().zip(found.iter_mut()) {
            if *wanted == line {
                *hit = true;
            }
        }
        Ok(ControlFlow::Continue(()))
    });
    match read {
        Ok(()) | Err(Error::NotFound) => Ok(found),
        Err(error) => Err(error),
    }
}

/// Appends `line` to the record. A write that fails is let go of (the book
/// is then fetched, refused by name, and noted again at the next sync);
/// memory is not.
fn remember(files: &dyn Files, path: &str, line: &str) -> Result<(), Error> {
    match files.append(path, line.as_bytes()) {
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Ok(()) | Err(_) => Ok(()),
    }
}

impl Sync {
    /// A sync that is not running.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            run: None,
            report: None,
        }
    }

    /// Whether a sync is going on.
    #[must_use]
    pub const fn running(&self) -> bool {
        self.run.is_some()
    }

    /// How the last sync goes or went.
    #[must_use]
    pub const fn report(&self) -> Option<&Report> {
        self.report.as_ref()
    }

    /// Syncs `jobs`, one after the other, unless a sync is running (nothing
    /// happens then). Returns `None` where it does, the end where it could not
    /// start.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn start(
        &mut self,
        host: &Host<'_>,
        web: &mut Web,
        files: &dyn Files,
        setup: &Setup<'_>,
        jobs: Vec<Job>,
    ) -> Result<Option<Update>, Error> {
        if self.run.is_some() {
            return Ok(None);
        }
        // What a sync cut short left (a power cut, a stop).
        for name in [SYNC_FEED_FILE, SYNC_BOOK_FILE] {
            files::remove(files, name);
        }
        let mut waiting: VecDeque<Job> = jobs.into();
        let Some(job) = waiting.pop_front() else {
            return Ok(None);
        };
        self.report = Some(Report {
            running: true,
            ..Report::default()
        });
        self.begin(host, web, files, setup, job, waiting)
    }

    /// Stops the sync; the books loaded stay. What is still on its way is
    /// thrown away when it comes.
    pub fn stop(&mut self, host: &Host<'_>, web: &mut Web, files: &dyn Files) {
        if self.run.take().is_some() {
            web.cancel(host, SYNC_FEED);
            web.cancel(host, SYNC_BOOK);
            for name in [SYNC_FEED_FILE, SYNC_BOOK_FILE] {
                files::remove(files, name);
            }
            if let Some(report) = &mut self.report {
                report.running = false;
            }
        }
    }

    fn report_mut(&mut self) -> &mut Report {
        self.report.get_or_insert_with(Report::default)
    }

    /// Begins the catalog `job`: its first page asked for.
    fn begin(
        &mut self,
        host: &Host<'_>,
        web: &mut Web,
        files: &dyn Files,
        setup: &Setup<'_>,
        job: Job,
        waiting: VecDeque<Job>,
    ) -> Result<Option<Update>, Error> {
        let mut job = job;
        let mut waiting = waiting;
        loop {
            let started = self.begin_one(host, web, setup, job, waiting);
            match started {
                Ok(()) => return Ok(Some(Update::Progress)),
                Err((Error::NoMemory, _)) => {
                    self.end(files);
                    return Err(Error::NoMemory);
                }
                // The first page cannot even be asked for: this catalog is
                // unreachable, and the next one is tried.
                Err((_, rest)) => {
                    self.report_mut().unreachable += 1;
                    let mut rest = rest;
                    match rest.pop_front() {
                        Some(next) => {
                            job = next;
                            waiting = rest;
                        }
                        None => {
                            self.finish();
                            return Ok(Some(Update::Finished));
                        }
                    }
                }
            }
        }
    }

    /// The first page of `job` asked for, or why not (with the catalogs left).
    fn begin_one(
        &mut self,
        host: &Host<'_>,
        web: &mut Web,
        setup: &Setup<'_>,
        job: Job,
        waiting: VecDeque<Job>,
    ) -> Result<(), (Error, VecDeque<Job>)> {
        let prepared = (|| {
            let auth = match &job.password {
                Some(password) if !job.catalog.user.is_empty() => {
                    Some(Secret::basic(&job.catalog.user, password)?)
                }
                _ => None,
            };
            let folder = job.catalog.sync_folder(setup.library)?;
            let name = copy(&job.catalog.name)?;
            Ok::<_, Error>((auth, folder, name))
        })();
        let (auth, folder, name) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => return Err((error, waiting)),
        };
        self.report_mut().catalog = name;
        let first = copy(&job.catalog.url);
        let first = match first {
            Ok(url) => url,
            Err(error) => return Err((error, waiting)),
        };
        let call = Call::new(SYNC_FEED, &first, SYNC_FEED_FILE, SMALL_FILE).list();
        let id = match web.send(host, &job.catalog, auth.as_ref(), &call) {
            Ok(id) => id,
            Err(error) => return Err((error, waiting)),
        };
        let mut pages = Vec::new();
        if let Err(error) = push(&mut pages, fnv1a64(first.as_bytes())) {
            return Err((error, waiting));
        }
        self.run = Some(Run {
            job,
            auth,
            waiting,
            folder,
            queue: VecDeque::new(),
            queued: 0,
            next: None,
            pages,
            count: 0,
            pending: Some((id, Waiting::Feed)),
            first: true,
            asked: first,
        });
        Ok(())
    }

    /// Ends the run: nothing is waited for, the files go.
    fn end(&mut self, files: &dyn Files) {
        self.run = None;
        for name in [SYNC_FEED_FILE, SYNC_BOOK_FILE] {
            files::remove(files, name);
        }
        if let Some(report) = &mut self.report {
            report.running = false;
        }
    }

    fn finish(&mut self) {
        self.run = None;
        if let Some(report) = &mut self.report {
            report.running = false;
        }
    }

    /// The end of a request of the sync; `None` for one that is not its.
    ///
    /// # Errors
    /// `NoMemory`; the sync is over then.
    pub fn done(
        &mut self,
        host: &Host<'_>,
        web: &mut Web,
        files: &dyn Files,
        library: Option<&Library>,
        setup: &Setup<'_>,
        done: &HttpDone,
    ) -> Result<Option<Update>, Error> {
        let Some(run) = self.run.as_mut() else {
            return Ok(None);
        };
        let Some((id, waiting)) = run.pending else {
            return Ok(None);
        };
        if !matches!(done.token(), SYNC_FEED | SYNC_BOOK) || id != done.id() {
            return Ok(None);
        }
        run.pending = None;
        let stepped = match waiting {
            Waiting::Feed => self.page(host, web, files, setup, done),
            Waiting::Book => self.stored(host, web, files, library, setup, done),
        };
        if stepped.is_err() {
            self.end(files);
        }
        stepped.map(Some)
    }

    /// A page of the catalog's list arrived.
    fn page(
        &mut self,
        host: &Host<'_>,
        web: &mut Web,
        files: &dyn Files,
        setup: &Setup<'_>,
        done: &HttpDone,
    ) -> Result<Update, Error> {
        let base = if web::ended(done) == Ended::Fine {
            web.final_url(host, done.id())?
        } else {
            None
        };
        let outcome = self.read_page(files, setup, done, base.as_deref());
        files::remove(files, SYNC_FEED_FILE);
        match outcome? {
            Some(()) => self.go_on(host, web, files, setup),
            None => self.next_catalog(host, web, files, setup),
        }
    }

    /// Reads the page that arrived into the queue; `None` where the catalog is
    /// given up (said in the report).
    fn read_page(
        &mut self,
        files: &dyn Files,
        setup: &Setup<'_>,
        done: &HttpDone,
        final_url: Option<&str>,
    ) -> Result<Option<()>, Error> {
        let first = self.run.as_ref().is_some_and(|run| run.first);
        let unreachable = |sync: &mut Self| {
            if first {
                sync.report_mut().unreachable += 1;
            }
            Ok(None)
        };
        if web::ended(done) != Ended::Fine {
            return unreachable(self);
        }
        let Some(run) = self.run.as_mut() else {
            return Ok(None);
        };
        run.first = false;
        // Links are resolved where the page came from, after redirects.
        let base = copy(final_url.unwrap_or(&run.asked))?;
        let page = read_list(&run.job.catalog, files, &base, setup.small);
        let page = match page {
            Ok(page) => page,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // A page that cannot be read: a list that could not be read.
            Err(_) => return unreachable(self),
        };
        // No room to read the list: a list that could not be read.
        if page.memory {
            return unreachable(self);
        }
        // Sync follows no further lists: a catalog of lists has no books.
        if first && !page.entries.is_empty() && page.entries.iter().all(|e| e.books.is_empty()) {
            self.report_mut().only_lists += 1;
        }
        self.enqueue(files, setup, &page)
    }

    /// The new books of `page` into the queue, and where the list goes on.
    fn enqueue(
        &mut self,
        files: &dyn Files,
        setup: &Setup<'_>,
        page: &Feed,
    ) -> Result<Option<()>, Error> {
        let Some(run) = self.run.as_mut() else {
            return Ok(None);
        };
        let readable = |ext: &str| setup.extensions.iter().any(|e| e == ext);
        let mut found: Vec<Item> = Vec::new();
        for entry in &page.entries {
            let Some((book, ext)) = entry.readable_book(readable) else {
                continue;
            };
            let id = if entry.id.is_empty() {
                &book.url
            } else {
                &entry.id
            };
            let item = Item {
                id: record_line(id)?,
                name: book_name(entry, &book.url, ext)?,
                url: copy(&book.url)?,
            };
            push(&mut found, item)?;
        }
        let ids: Vec<&str> = found.iter().map(|item| item.id.as_str()).collect();
        let path = record_path(&run.job.catalog.url)?;
        let synced = match recorded(files, &path, &ids) {
            Ok(synced) => synced,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => {
                self.report_mut().skipped += 1;
                return Ok(None);
            }
        };
        let Some(run) = self.run.as_mut() else {
            return Ok(None);
        };
        for (item, synced) in found.into_iter().zip(synced) {
            let known = synced
                || run
                    .queue
                    .iter()
                    .any(|queued| queued.id == item.id || queued.name == item.name);
            // What does not fit the queue waits for the next sync (it is not
            // recorded).
            let room = run.queued.saturating_add(item.weight()) <= setup.queue_bytes();
            if !known && room {
                run.queued += item.weight();
                run.queue.try_reserve(1).map_err(|_| Error::NoMemory)?;
                run.queue.push_back(item);
            }
        }
        run.next = match page.next.as_deref() {
            Some(next)
                if run.pages.len() < MAX_PAGES
                    && !run.pages.contains(&fnv1a64(next.as_bytes())) =>
            {
                Some(copy(next)?)
            }
            _ => None,
        };
        Ok(Some(()))
    }

    /// The next book, else the next page, else the next catalog.
    fn go_on(
        &mut self,
        host: &Host<'_>,
        web: &mut Web,
        files: &dyn Files,
        setup: &Setup<'_>,
    ) -> Result<Update, Error> {
        loop {
            let Some(run) = self.run.as_mut() else {
                return Ok(Update::Finished);
            };
            if run.count >= setup.limit {
                return self.next_catalog(host, web, files, setup);
            }
            if let Some(item) = run.queue.front() {
                let call = Call::new(SYNC_BOOK, &item.url, SYNC_BOOK_FILE, BOOK_BYTES);
                match web.send(host, &run.job.catalog, run.auth.as_ref(), &call) {
                    Ok(id) => {
                        run.pending = Some((id, Waiting::Book));
                        return Ok(Update::Progress);
                    }
                    Err(Error::NoMemory) => return Err(Error::NoMemory),
                    // A book that cannot be asked for is a failed one.
                    Err(_) => {
                        if let Some(item) = run.queue.pop_front() {
                            run.queued = run.queued.saturating_sub(item.weight());
                        }
                        self.report_mut().failed += 1;
                        continue;
                    }
                }
            }
            match run.next.take() {
                Some(next) => {
                    let call = Call::new(SYNC_FEED, &next, SYNC_FEED_FILE, SMALL_FILE).list();
                    match web.send(host, &run.job.catalog, run.auth.as_ref(), &call) {
                        Ok(id) => {
                            push(&mut run.pages, fnv1a64(next.as_bytes()))?;
                            run.pending = Some((id, Waiting::Feed));
                            run.asked = next;
                            return Ok(Update::Progress);
                        }
                        Err(Error::NoMemory) => return Err(Error::NoMemory),
                        // The list cannot go on: what came is what there is.
                        Err(_) => continue,
                    }
                }
                None => return self.next_catalog(host, web, files, setup),
            }
        }
    }

    /// A book arrived: put into the library, remembered.
    fn stored(
        &mut self,
        host: &Host<'_>,
        web: &mut Web,
        files: &dyn Files,
        library: Option<&Library>,
        setup: &Setup<'_>,
        done: &HttpDone,
    ) -> Result<Update, Error> {
        let fine = web::ended(done) == Ended::Fine;
        let Some(run) = self.run.as_mut() else {
            return Ok(Update::Finished);
        };
        let Some(item) = run.queue.pop_front() else {
            files::remove(files, SYNC_BOOK_FILE);
            return self.go_on(host, web, files, setup);
        };
        run.queued = run.queued.saturating_sub(item.weight());
        let checked = if fine {
            may_be_book(files, SYNC_BOOK_FILE, &item.name)?
        } else {
            false
        };
        let saved = match library {
            Some(library) if checked => Some(import(
                host,
                library,
                SYNC_BOOK_FILE,
                &run.folder,
                &item.name,
                Existing::Refuse,
            )),
            _ => None,
        };
        let path = record_path(&run.job.catalog.url)?;
        let mut line = copy(&item.id)?;
        line.try_reserve(1).map_err(|_| Error::NoMemory)?;
        line.push('\n');
        let mut loaded = false;
        let mut failed = false;
        match saved {
            Some(Ok(_)) => {
                run.count += 1;
                loaded = true;
                remember(files, &path, &line)?;
            }
            // A book of that name is in the folder already: remembered, not
            // fetched again.
            Some(Err(Error::State)) => remember(files, &path, &line)?,
            Some(Err(Error::NoMemory)) => return Err(Error::NoMemory),
            Some(Err(_)) | None => failed = true,
        }
        files::remove(files, SYNC_BOOK_FILE);
        let report = self.report_mut();
        report.loaded += u32::from(loaded);
        report.failed += u32::from(failed);
        self.go_on(host, web, files, setup)
    }

    /// The next catalog, else the end.
    fn next_catalog(
        &mut self,
        host: &Host<'_>,
        web: &mut Web,
        files: &dyn Files,
        setup: &Setup<'_>,
    ) -> Result<Update, Error> {
        let Some(mut run) = self.run.take() else {
            return Ok(Update::Finished);
        };
        match run.waiting.pop_front() {
            Some(job) => {
                let waiting = core::mem::take(&mut run.waiting);
                drop(run);
                match self.begin(host, web, files, setup, job, waiting)? {
                    Some(update) => Ok(update),
                    None => Ok(Update::Finished),
                }
            }
            None => {
                self.finish();
                Ok(Update::Finished)
            }
        }
    }
}

/// Reads the list of `catalog` in the sync's feed file: OPDS 2 where it is JSON,
/// a WebDAV listing for a cloud folder, else OPDS 1.
fn read_list(catalog: &Catalog, files: &dyn Files, base: &str, small: bool) -> Result<Feed, Error> {
    let limits = Limits::of_class(small);
    let mut source = FileSource::new(files, SYNC_FEED_FILE);
    if catalog.webdav {
        native_dav::read(&mut source, base, limits)
    } else if native_json::is_json(&mut source)? {
        native_json::read(&mut source, base, limits)
    } else {
        model::read(&mut source, base, limits)
    }
}
