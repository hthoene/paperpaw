//! Syncing catalogs: their books that are not on the device yet are
//! downloaded into each catalog's sync folder, page after page of the
//! catalog's list, at most [`LIMIT`] books a catalog each time.
//!
//! Every book synced is remembered (by its id in the catalog), so a book
//! the user deleted is not fetched again. Books already in the folder
//! under the same name are skipped too. The record of a catalog is read
//! line by line for each page of its list, so it may grow without
//! limit; one that cannot be read skips the catalog rather than fetch
//! again what the user deleted.
//!
//! It runs in the background: the answers come as events to the plugin's
//! subscriptions, whatever screen is open. One book is downloaded at a
//! time, straight to storage.

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::ops::ControlFlow;

use paperpaw_sdk::digest::md5;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::HttpReply;
use paperpaw_sdk::web::{clean_name, file_name};
use paperpaw_sdk::{Command, Commands, Env, HttpRequest, Id, Params, Storage, StorageError, files};

use crate::catalogs::Catalog;
use crate::temp::{self, TempFiles};
use crate::{PLUGIN, feed};
use paperpaw_sdk::setting::Setting;

/// Answers to the sync's requests.
pub(crate) const FEED_LOADED: Event<HttpReply> = Event::new("opds.sync-feed");
pub(crate) const BOOK_LOADED: Event<HttpReply> = Event::new("opds.sync-book");
/// Emitted whenever a sync gets on or ends, for screens to show.
pub(crate) const CHANGED: Id = Id::new_static("opds.sync-changed");
/// Setting: the most books a catalog gets in one sync.
pub(crate) const LIMIT: Setting<i64> = Setting::new("opds.sync-limit");

/// Pages of a catalog's list read in one sync, at most.
const MAX_PAGES: usize = 50;
/// Parameter of the requests: which run they belong to.
const RUN: &str = "run";

/// The books synced from the catalog at `url`, one id a line (see
/// [`record_line`]).
fn record_path(url: &str) -> String {
    format!(
        "{}/synced/{}.txt",
        Env::plugin_dir(PLUGIN.as_str()),
        md5::of(url)
    )
}

/// How the book with `id` is written in the record: as it is, on one
/// line; an id longer than a line may be by its digest.
fn record_line(id: &str) -> String {
    let line = id.replace(['\n', '\r'], " ");
    if line.len() <= files::MAX_LINE {
        line
    } else {
        format!("md5:{}", md5::of(&line))
    }
}

/// Which of `lines` the record at `path` holds, read line by line. No
/// record yet holds none; one that cannot be read is an error, never
/// "none synced".
fn recorded(storage: &dyn Storage, path: &str, lines: &[&str]) -> Result<Vec<bool>, StorageError> {
    let mut found = alloc::vec![false; lines.len()];
    let read = files::for_each_line(storage, path, &mut |line| {
        for (i, wanted) in lines.iter().enumerate() {
            if *wanted == line {
                found[i] = true;
            }
        }
        ControlFlow::Continue(())
    });
    match read {
        Ok(()) | Err(StorageError::NotFound) => Ok(found),
        Err(e) => Err(e),
    }
}

/// What a sync did, for the screens.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Report {
    /// The catalog being synced, or the last one.
    pub catalog: String,
    pub loaded: usize,
    pub failed: usize,
    /// Catalogs that could not be read.
    pub unreachable: usize,
    /// Catalogs whose address lists no books, only further lists (such
    /// as Project Gutenberg's start page): nothing to sync there.
    pub only_lists: usize,
    /// Catalogs whose record could not be read: left alone, so books
    /// deleted here do not come again.
    pub skipped: usize,
    pub running: bool,
}

/// A book to download.
#[derive(Debug, Clone)]
struct Item {
    /// Its id, as the record has it ([`record_line`]).
    id: String,
    name: String,
    url: String,
}

#[derive(Debug)]
struct Run {
    /// Its number: answers to an earlier run are ignored.
    number: u32,
    /// The `User-Agent` header of its requests, taken at its start.
    agent: (String, String),
    catalog: Catalog,
    /// The catalogs after this one.
    waiting: VecDeque<Catalog>,
    folder: String,
    queue: VecDeque<Item>,
    /// The page of the list after the one read.
    next: Option<String>,
    /// The pages read.
    pages: Vec<String>,
    /// Books of this catalog loaded in this run.
    count: usize,
    limit: usize,
}

/// The sync of the plugin, shared by its screens and subscriptions.
#[derive(Debug)]
pub(crate) struct Sync {
    run: Option<Run>,
    runs: u32,
    pub report: Option<Report>,
    /// The files the answers are written to.
    temp: TempFiles,
}

impl Sync {
    pub(crate) fn new(temp: TempFiles) -> Self {
        Self {
            run: None,
            runs: 0,
            report: None,
            temp,
        }
    }

    pub(crate) fn running(&self) -> bool {
        self.run.is_some()
    }

    /// Syncs `catalogs`, one after the other, unless a sync is running.
    pub(crate) fn start(&mut self, env: &Env<'_>, catalogs: Vec<Catalog>, out: &mut Commands) {
        if self.run.is_some() {
            return;
        }
        let mut waiting: VecDeque<Catalog> = catalogs.into();
        let Some(catalog) = waiting.pop_front() else {
            return;
        };
        self.runs += 1;
        self.report = Some(Report {
            running: true,
            ..Report::default()
        });
        self.begin(env, catalog, waiting, out);
    }

    /// Stops the sync; the books loaded stay. What is still on its way
    /// is thrown away when it comes.
    pub(crate) fn stop(&mut self, out: &mut Commands) {
        if self.run.take().is_some() {
            if let Some(report) = &mut self.report {
                report.running = false;
            }
            out.push(Command::Emit(CHANGED, Params::new()));
        }
    }

    fn begin(
        &mut self,
        env: &Env<'_>,
        catalog: Catalog,
        waiting: VecDeque<Catalog>,
        out: &mut Commands,
    ) {
        if let Some(report) = &mut self.report {
            report.catalog = catalog.name.clone();
        }
        let limit = env.settings().read(&LIMIT).max(1) as usize;
        let run = Run {
            number: self.runs,
            agent: paperpaw_sdk::web::user_agent_of(env),
            folder: catalog.sync_folder(env),
            waiting,
            queue: VecDeque::new(),
            next: None,
            pages: Vec::new(),
            count: 0,
            limit,
            catalog,
        };
        let url = run.catalog.url.clone();
        self.run = Some(run);
        self.request_feed(&url, out);
        out.push(Command::Emit(CHANGED, Params::new()));
    }

    fn request(&self, url: &str, save_to: String, reply: &Event<HttpReply>) -> Option<HttpRequest> {
        let run = self.run.as_ref()?;
        let params = Params::new().with(RUN, run.number.to_string());
        Some(crate::request(
            &run.agent,
            &run.catalog,
            url,
            save_to,
            reply,
            params,
        ))
    }

    fn request_feed(&mut self, url: &str, out: &mut Commands) {
        if let Some(run) = &mut self.run {
            run.pages.push(url.to_string());
        }
        if let Some(run) = &self.run {
            let params = Params::new()
                .with(RUN, run.number.to_string())
                .with(crate::URL, url.to_string());
            out.push(Command::http(crate::list_request(
                &run.agent,
                &run.catalog,
                url,
                self.temp.next("xml"),
                &FEED_LOADED,
                params,
            )));
        }
    }

    /// Whether `params` answer a request of the run going on.
    fn current(&self, reply: &HttpReply) -> bool {
        let number = reply.context.get(RUN).and_then(|n| n.parse::<u32>().ok());
        self.run
            .as_ref()
            .is_some_and(|run| Some(run.number) == number)
    }

    /// A page of the catalog's list arrived.
    pub(crate) fn feed_loaded(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
        self.read_page(env, reply, out);
        temp::discard(env.storage(), reply);
    }

    fn read_page(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
        if !self.current(reply) {
            return;
        }
        let file = temp::file(reply).filter(|_| (200..300).contains(&reply.status));
        let Some(file) = file else {
            let first = self.run.as_ref().is_some_and(|run| run.pages.len() <= 1);
            if first && let Some(report) = &mut self.report {
                report.unreachable += 1;
            }
            return self.next_catalog(env, out);
        };
        let base = reply
            .final_url
            .as_deref()
            .or(reply.context.get(crate::URL))
            .unwrap_or_default()
            .to_string();
        let Some(catalog) = self.run.as_ref().map(|run| run.catalog.clone()) else {
            return;
        };
        let page = crate::read_list(&catalog, env.storage(), file, &base);
        let Some(run) = &mut self.run else {
            return;
        };
        // No room to read the list: a list that could not be read.
        if page.memory {
            let first = run.pages.len() <= 1;
            if first && let Some(report) = &mut self.report {
                report.unreachable += 1;
            }
            return self.next_catalog(env, out);
        }
        // Sync follows no further lists: a catalog of lists has no books.
        let first = run.pages.len() == 1;
        if first
            && !page.entries.is_empty()
            && page.entries.iter().all(|e| e.books.is_empty())
            && let Some(report) = &mut self.report
        {
            report.only_lists += 1;
        }
        let readable = |ext: &str| env.formats().supports(&format!("book.{ext}"));
        let found: Vec<Item> = page
            .entries
            .iter()
            .filter_map(|entry| {
                let (book, ext) = entry.readable_book(readable)?;
                let id = if entry.id.is_empty() {
                    &book.url
                } else {
                    &entry.id
                };
                Some(Item {
                    id: record_line(id),
                    name: book_name(entry, &book.url, ext),
                    url: book.url.clone(),
                })
            })
            .collect();
        let ids: Vec<&str> = found.iter().map(|item| item.id.as_str()).collect();
        let Ok(synced) = recorded(env.storage(), &record_path(&catalog.url), &ids) else {
            if let Some(report) = &mut self.report {
                report.skipped += 1;
            }
            return self.next_catalog(env, out);
        };
        for (item, synced) in found.into_iter().zip(synced) {
            let known = synced
                || run
                    .queue
                    .iter()
                    .any(|queued| queued.id == item.id || queued.name == item.name)
                || env.storage().exists(&files::join(&run.folder, &item.name));
            if !known {
                run.queue.push_back(item);
            }
        }
        run.next = page
            .next
            .filter(|next| run.pages.len() < MAX_PAGES && !run.pages.contains(next));
        self.go_on(env, out);
    }

    /// The next book, else the next page, else the next catalog.
    fn go_on(&mut self, env: &Env<'_>, out: &mut Commands) {
        let Some(run) = &mut self.run else {
            return;
        };
        if run.count >= run.limit {
            return self.next_catalog(env, out);
        }
        if let Some(item) = run.queue.front() {
            let (url, id) = (item.url.clone(), item.id.clone());
            let file = self.temp.next("tmp");
            if let Some(request) = self.request(&url, file, &BOOK_LOADED) {
                out.push(Command::http(request.param(crate::ID, id)));
            }
            return;
        }
        match run.next.take() {
            Some(next) => self.request_feed(&next, out),
            None => self.next_catalog(env, out),
        }
    }

    /// A book arrived.
    pub(crate) fn book_loaded(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
        self.store_book(env, reply, out);
        temp::discard(env.storage(), reply);
    }

    fn store_book(&mut self, env: &Env<'_>, reply: &HttpReply, out: &mut Commands) {
        if !self.current(reply) {
            return;
        }
        let Some(run) = &mut self.run else {
            return;
        };
        let Some(item) = run.queue.pop_front() else {
            return;
        };
        let file = temp::file(reply)
            .filter(|_| (200..300).contains(&reply.status))
            .filter(|file| temp::may_be_book(env.storage(), file, &item.name));
        let stored = file.and_then(|file| {
            let _ = env.storage().create_dir(&run.folder);
            files::put_in_free_place(env.storage(), file, &run.folder, &item.name).ok()
        });
        let report = self.report.get_or_insert_with(Report::default);
        if let Some(path) = stored {
            // In the library: caught up with the others, not walked.
            out.push(paperpaw_sdk::library::added(&path, "opds"));
            run.count += 1;
            let _ = env.storage().append(
                &record_path(&run.catalog.url),
                format!("{}\n", item.id).as_bytes(),
            );
            report.loaded += 1;
        } else {
            report.failed += 1;
        }
        out.push(Command::Emit(CHANGED, Params::new()));
        self.go_on(env, out);
    }

    fn next_catalog(&mut self, env: &Env<'_>, out: &mut Commands) {
        let Some(mut run) = self.run.take() else {
            return;
        };
        match run.waiting.pop_front() {
            Some(catalog) => self.begin(env, catalog, core::mem::take(&mut run.waiting), out),
            None => {
                if let Some(report) = &mut self.report {
                    report.running = false;
                    out.push(Command::Notify(crate::list::report_text(env, report)));
                }
                out.push(Command::Emit(CHANGED, Params::new()));
            }
        }
    }
}

/// The file name for a book: its authors and title, else the name in its
/// address.
pub(crate) fn book_name(entry: &feed::Entry, url: &str, ext: &str) -> String {
    let title = entry.title.trim();
    let name = match entry.authors.split(" & ").next().map(str::trim) {
        Some(author) if !author.is_empty() && !title.is_empty() => format!("{author} - {title}"),
        _ => title.to_string(),
    };
    match clean_name(&name, 120) {
        Some(name) => format!("{name}.{ext}"),
        None => {
            let name = file_name(url);
            if name.ends_with(&format!(".{ext}")) {
                name
            } else {
                format!("{name}.{ext}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed::Entry;

    #[test]
    fn books_are_named_after_author_and_title() {
        let entry = Entry {
            title: String::from("Pride and Prejudice"),
            authors: String::from("Austen, Jane & Someone Else"),
            ..Entry::default()
        };
        assert_eq!(
            book_name(&entry, "https://a.org/1342.epub.noimages", "epub"),
            "Austen, Jane - Pride and Prejudice.epub"
        );
        let untitled = Entry::default();
        assert_eq!(
            book_name(&untitled, "https://a.org/get/1342", "epub"),
            "1342.epub"
        );
        assert_eq!(
            book_name(&untitled, "https://a.org/b.epub", "epub"),
            "b.epub"
        );
    }
}

#[cfg(test)]
mod naming_tests;
