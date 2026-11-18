//! The sync of marked catalogs against scripted `core.net` and `core.library`
//! services and an in-memory data folder, driven by a probe plugin that runs
//! `Sync` from events (the real plugin starts it from its list or its work):
//! the requests as they go out, what each answer leads to (books put into the
//! catalog's folder in the library under a name of their own and remembered, a
//! book deleted not fetched again, the limit, pages followed once, an
//! unreachable catalog passed over), what a stop leaves, and that no refused
//! service call or allocation aborts.
#![cfg(feature = "linked")]
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test assertions"
)]
mod fakes;
use fakes::services::*;
use paperpaw_native::host_glue::ERR_NO_MEMORY;
use paperpaw_plugin_opds::native_catalogs::Catalog;
use paperpaw_plugin_opds::native_files::Files;
use paperpaw_plugin_opds::native_keep::Secret;
use paperpaw_plugin_opds::native_sync::{Job, MAX_PAGES, Setup, Sync, record_path};
use paperpaw_plugin_opds::native_web::{self as web, Web};
use paperpaw_scripted_host::{ScriptedHost, Slot, sweep};
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    declare::{Declarations, EventKind},
    events::{Event, Incoming, kinds},
    library::Library,
    registrations,
    reply::Reply,
};
use std::cell::RefCell;

const START: u32 = 1;
const DONE: u32 = 2;
const STOP: u32 = 3;

/// A catalog the probe syncs.
#[derive(Clone)]
struct Cat {
    name: &'static str,
    url: &'static str,
    user: &'static str,
    password: &'static str,
    folder: &'static str,
    webdav: bool,
}

#[derive(Clone)]
struct Config {
    cats: Vec<Cat>,
    limit: usize,
    small: bool,
}

thread_local! {
    static CONFIG: RefCell<Option<Config>> = const { RefCell::new(None) };
    /// Reading the record of the catalog fails.
    static RECORD_FAILS: RefCell<bool> = const { RefCell::new(false) };
}

const EXTENSIONS: [&str; 2] = ["epub", "txt"];

fn cat(url: &'static str) -> Cat {
    Cat {
        name: "Mine",
        url,
        user: "",
        password: "",
        folder: "",
        webdav: false,
    }
}

fn extensions() -> Vec<String> {
    EXTENSIONS.iter().map(|e| (*e).to_string()).collect()
}

struct Probe {
    web: Web,
    library: Option<Library>,
    sync: Sync,
}

/// The data folder, with a record that cannot be read where asked.
struct Faulty;

impl Files for Faulty {
    fn size(&self, name: &str) -> Result<u64, Error> {
        Disk.size(name)
    }
    fn read_at(&self, name: &str, offset: u64, out: &mut [u8]) -> Result<usize, Error> {
        if name.starts_with("synced/") && RECORD_FAILS.with(|f| *f.borrow()) {
            return Err(Error::Io);
        }
        Disk.read_at(name, offset, out)
    }
    fn write(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        Disk.write(name, data)
    }
    fn append(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        Disk.append(name, data)
    }
    fn remove(&self, name: &str) -> Result<(), Error> {
        Disk.remove(name)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), Error> {
        Disk.rename(from, to)
    }
}

fn report(sync: &Sync) -> String {
    match sync.report() {
        Some(r) => format!(
            "{} loaded {} failed {} unreachable {} only_lists {} skipped {} {}",
            r.catalog,
            r.loaded,
            r.failed,
            r.unreachable,
            r.only_lists,
            r.skipped,
            if r.running { "running" } else { "done" }
        ),
        None => "no report".to_string(),
    }
}

impl Plugin for Probe {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        let mut batch = Vec::new();
        let mut d = Declarations::new(|bytes: &[u8]| -> Result<(), i32> {
            batch.extend_from_slice(bytes);
            Ok(())
        });
        d.subscribe_event(EventKind::HttpDone, DONE)
            .map_err(|_| Error::Invalid)?;
        d.submit().map_err(|_| Error::Invalid)?;
        drop(d);
        registrations::submit(host, &batch)?;
        Ok(Self {
            web: Web::new(),
            library: None,
            sync: Sync::new(),
        })
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let config = CONFIG.with(|c| c.borrow().clone()).ok_or(Error::State)?;
        let extensions = extensions();
        let setup = Setup::new("Books", &extensions, config.small, config.limit);
        let result = match token {
            START => {
                self.library = Library::open(host).ok();
                let mut jobs = Vec::new();
                for c in &config.cats {
                    let mut catalog = Catalog::new(c.url)?;
                    catalog.name = c.name.to_string();
                    catalog.user = c.user.to_string();
                    catalog.folder = c.folder.to_string();
                    catalog.webdav = c.webdav;
                    catalog.sync = true;
                    let password = (!c.password.is_empty())
                        .then(|| Secret::copy(c.password))
                        .transpose()?;
                    jobs.push(Job::new(catalog, password));
                }
                let started = self.sync.start(host, &mut self.web, &Faulty, &setup, jobs);
                say(format!("start {started:?}"));
                started.map(|_| ())
            }
            DONE => match incoming.decode_as::<kinds::HttpDone>()? {
                Some(Event::HttpDone(done)) => {
                    let stepped = self.sync.done(
                        host,
                        &mut self.web,
                        &Faulty,
                        self.library.as_ref(),
                        &setup,
                        &done,
                    );
                    say(format!("done {stepped:?}"));
                    stepped.map(|_| ())
                }
                _ => Ok(()),
            },
            STOP => {
                self.sync.stop(host, &mut self.web, &Faulty);
                Ok(())
            }
            _ => Ok(()),
        };
        say(report(&self.sync));
        result
    }
}

paperpaw_sdk::native_plugin!(
    Probe,
    metadata = include_bytes!("../native/metadata.host.bin"),
    linked = PROBE_ENTRY
);

const URL: &str = "https://c.example/opds";

fn setup(host: &ScriptedHost, cats: Vec<Cat>, limit: usize) {
    DISK.with(|d| d.files.borrow_mut().clear());
    TAKEN.with(|t| t.borrow_mut().clear());
    FINAL.with(|f| f.borrow_mut().clear());
    IMPORT_FAILS.with(|f| *f.borrow_mut() = None);
    RECORD_FAILS.with(|f| *f.borrow_mut() = false);
    seen();
    log();
    CONFIG.with(|c| {
        *c.borrow_mut() = Some(Config {
            cats,
            limit,
            small: false,
        });
    });
    net(host);
    library(host);
}

fn entry(id: &str, title: &str, href: &str) -> String {
    format!(
        r#"<entry><id>{id}</id><title>{title}</title><author><name>Jane Austen</name></author><link rel="http://opds-spec.org/acquisition" type="application/epub+zip" href="{href}"/></entry>"#
    )
}

fn feed(entries: &str, next: Option<&str>) -> String {
    let next = next.map_or(String::new(), |n| {
        format!(r#"<link rel="next" type="application/atom+xml" href="{n}"/>"#)
    });
    format!(r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Cat</title>{next}{entries}</feed>"#)
}

/// The request ids count from 1; `n` is the next one.
struct Ids(u32);

impl Ids {
    fn next(&mut self) -> u32 {
        self.0 += 1;
        self.0
    }
}

/// Answers the request just asked: a feed (its file written first) or a book.
fn feed_answer(plugin: &paperpaw_scripted_host::Plugin<'_>, ids: &mut Ids, body: &str) {
    put("sync-feed.tmp", body.as_bytes());
    plugin
        .event(DONE, &done(ids.next(), web::SYNC_FEED, 200, 0))
        .unwrap();
}

fn book_answer(plugin: &paperpaw_scripted_host::Plugin<'_>, ids: &mut Ids, body: &[u8]) {
    put("sync-book.tmp", body);
    plugin
        .event(DONE, &done(ids.next(), web::SYNC_BOOK, 200, 0))
        .unwrap();
}

fn record(url: &str) -> Option<String> {
    file(&record_path(url).unwrap())
}

fn last_report() -> String {
    seen().last().cloned().unwrap_or_default()
}

#[test]
fn a_marked_catalog_is_synced_into_its_folder_and_remembered() {
    let host = ScriptedHost::new();
    setup(&host, vec![cat(URL)], 50);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    let mut ids = Ids(0);
    plugin.event(START, &nothing()).unwrap();
    let asked = log();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].starts_with(&format!(
        "GET {URL} #{} [User-Agent: PaperPaw/",
        web::SYNC_FEED
    )));
    assert!(asked[0].ends_with(&format!("file sync-feed.tmp max_file {}", web::SMALL_FILE)));
    assert!(last_report().ends_with("running"));
    let body = feed(
        &(entry("urn:1", "Emma", "/b/1.epub") + &entry("urn:2", "Persuasion", "/b/2.epub")),
        None,
    );
    feed_answer(&plugin, &mut ids, &body);
    assert!(file("sync-feed.tmp").is_none(), "the answer's file goes");
    let asked = log();
    assert!(
        asked[0].starts_with(&format!(
            "GET https://c.example/b/1.epub #{}",
            web::SYNC_BOOK
        )),
        "{asked:?}"
    );
    assert!(asked[0].ends_with(&format!("file sync-book.tmp max_file {}", web::BOOK_BYTES)));
    book_answer(&plugin, &mut ids, b"PK one");
    // In the catalog's folder (named after it), under the name of author and
    // title, refusing a name that is taken.
    let after = log();
    assert_eq!(
        after[0],
        "import sync-book.tmp Mine/Jane Austen - Emma.epub flags 0"
    );
    assert!(after[1].contains("/b/2.epub"), "{after:?}");
    book_answer(&plugin, &mut ids, b"PK two");
    assert_eq!(
        log()[0],
        "import sync-book.tmp Mine/Jane Austen - Persuasion.epub flags 0"
    );
    assert_eq!(
        last_report(),
        "Mine loaded 2 failed 0 unreachable 0 only_lists 0 skipped 0 done"
    );
    assert_eq!(record(URL).unwrap(), "urn:1\nurn:2\n");
    assert!(file("sync-book.tmp").is_none());
    drop(plugin);
}

#[test]
fn books_synced_before_are_not_fetched_again_even_when_deleted() {
    let host = ScriptedHost::new();
    setup(&host, vec![cat(URL)], 50);
    put(&record_path(URL).unwrap(), b"urn:1\r\nurn:2\n");
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    let mut ids = Ids(0);
    plugin.event(START, &nothing()).unwrap();
    log();
    let body = feed(
        &(entry("urn:1", "Emma", "/b/1.epub")
            + &entry("urn:2", "Persuasion", "/b/2.epub")
            + &entry("urn:3", "Lady Susan", "/b/3.epub")),
        None,
    );
    feed_answer(&plugin, &mut ids, &body);
    // Only the new one is asked for.
    let asked = log();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].contains("/b/3.epub"), "{asked:?}");
    book_answer(&plugin, &mut ids, b"PK three");
    assert_eq!(record(URL).unwrap(), "urn:1\r\nurn:2\nurn:3\n");
    assert_eq!(
        last_report(),
        "Mine loaded 1 failed 0 unreachable 0 only_lists 0 skipped 0 done"
    );
    drop(plugin);
}

#[test]
fn a_long_record_and_a_long_id_still_count() {
    let host = ScriptedHost::new();
    setup(&host, vec![cat(URL)], 50);
    // A record of 5,000 lines, the id sought the last; and an id too long for
    // a line, kept by its digest.
    let long_id = format!("https://c.example/{}", "u".repeat(700));
    let mut lines: String = (0..5000).map(|n| format!("urn:{n}\n")).collect();
    lines.push_str("urn:last\n");
    put(&record_path(URL).unwrap(), lines.as_bytes());
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    let mut ids = Ids(0);
    plugin.event(START, &nothing()).unwrap();
    log();
    // The entry without an id is known by its first file's address.
    let nameless = format!(
        r#"<entry><title>Long</title><link rel="http://opds-spec.org/acquisition" type="application/epub+zip" href="{long_id}"/></entry>"#
    );
    let body = feed(&(entry("urn:last", "Old", "/b/1.epub") + &nameless), None);
    feed_answer(&plugin, &mut ids, &body);
    assert!(log()[0].contains("uuuu"), "only the long id is new");
    book_answer(&plugin, &mut ids, b"PK");
    let kept = record(URL).unwrap();
    let last = kept.lines().last().unwrap();
    assert!(last.starts_with("md5:") && last.len() == 36, "{last}");
    // The same id again is known by its digest.
    let host2 = ScriptedHost::new();
    setup(&host2, vec![cat(URL)], 50);
    put(&record_path(URL).unwrap(), kept.as_bytes());
    let plugin2 = host2.load(PROBE_ENTRY.entry()).unwrap();
    let mut ids2 = Ids(0);
    plugin2.event(START, &nothing()).unwrap();
    log();
    feed_answer(&plugin2, &mut ids2, &feed(&nameless, None));
    assert!(log().is_empty(), "nothing to fetch");
    drop((plugin, plugin2));
}

#[test]
fn a_record_that_cannot_be_read_leaves_the_catalog_alone() {
    let host = ScriptedHost::new();
    setup(&host, vec![cat(URL), cat("https://d.example/opds")], 50);
    put(&record_path(URL).unwrap(), b"urn:1\n");
    RECORD_FAILS.with(|f| *f.borrow_mut() = true);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    let mut ids = Ids(0);
    plugin.event(START, &nothing()).unwrap();
    log();
    feed_answer(
        &plugin,
        &mut ids,
        &feed(&entry("urn:1", "Emma", "/b/1.epub"), None),
    );
    // Not one book is fetched for the first catalog, and the second is begun.
    let asked = log();
    assert_eq!(asked.len(), 1);
    assert!(
        asked[0].starts_with("GET https://d.example/opds"),
        "{asked:?}"
    );
    assert!(last_report().contains("skipped 1"), "{}", last_report());
    drop(plugin);
}

#[test]
fn a_book_already_in_the_folder_is_skipped_and_remembered() {
    let host = ScriptedHost::new();
    setup(&host, vec![cat(URL)], 50);
    TAKEN.with(|t| t.borrow_mut().push("Jane Austen - Emma.epub".to_string()));
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    let mut ids = Ids(0);
    plugin.event(START, &nothing()).unwrap();
    log();
    feed_answer(
        &plugin,
        &mut ids,
        &feed(&entry("urn:1", "Emma", "/b/1.epub"), None),
    );
    log();
    book_answer(&plugin, &mut ids, b"PK mine");
    // Refused by name (`Refuse`): not a failure, not counted, not asked again.
    assert_eq!(
        last_report(),
        "Mine loaded 0 failed 0 unreachable 0 only_lists 0 skipped 0 done"
    );
    assert_eq!(record(URL).unwrap(), "urn:1\n");
    drop(plugin);
}

#[test]
fn pages_are_followed_once_and_a_cycle_ends() {
    let host = ScriptedHost::new();
    setup(&host, vec![cat(URL)], 50);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    let mut ids = Ids(0);
    plugin.event(START, &nothing()).unwrap();
    log();
    // The page names the next, which names the first again, and itself.
    feed_answer(
        &plugin,
        &mut ids,
        &feed(&entry("urn:1", "A", "/b/1.epub"), Some("/opds?page=2")),
    );
    assert!(log()[0].contains("/b/1.epub"));
    book_answer(&plugin, &mut ids, b"PK");
    let asked = log();
    assert!(
        asked[1].starts_with("GET https://c.example/opds?page=2 #5"),
        "{asked:?}"
    );
    feed_answer(
        &plugin,
        &mut ids,
        &feed(&entry("urn:2", "B", "/b/2.epub"), Some("/opds?page=2")),
    );
    assert!(log().last().unwrap().contains("/b/2.epub"));
    book_answer(&plugin, &mut ids, b"PK");
    // Back to the first page: not asked again.
    assert_eq!(
        last_report(),
        "Mine loaded 2 failed 0 unreachable 0 only_lists 0 skipped 0 done"
    );
    drop(plugin);
    let host = ScriptedHost::new();
    setup(&host, vec![cat(URL)], 50);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    let mut ids = Ids(0);
    plugin.event(START, &nothing()).unwrap();
    log();
    feed_answer(&plugin, &mut ids, &feed("", Some(URL)));
    assert!(log().is_empty(), "the first page is not read twice");
    assert!(last_report().ends_with("done"));
    drop(plugin);
}

#[test]
fn an_endless_chain_of_pages_stops_after_fifty() {
    let host = ScriptedHost::new();
    setup(&host, vec![cat(URL)], 50);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    let mut ids = Ids(0);
    plugin.event(START, &nothing()).unwrap();
    let mut pages = 1;
    loop {
        let asked = log();
        if asked.is_empty() {
            break;
        }
        assert!(asked[0].starts_with("GET https://c.example/opds"));
        feed_answer(
            &plugin,
            &mut ids,
            &feed("", Some(&format!("/opds?page={pages}"))),
        );
        pages += 1;
        assert!(pages < 200, "it must stop");
    }
    assert_eq!(pages - 1, MAX_PAGES);
    assert!(last_report().ends_with("done"));
    drop(plugin);
}

#[test]
fn at_most_the_limit_of_books_comes_with_one_sync() {
    let host = ScriptedHost::new();
    setup(&host, vec![cat(URL)], 2);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    let mut ids = Ids(0);
    plugin.event(START, &nothing()).unwrap();
    log();
    let entries: String = (0..5)
        .map(|n| {
            entry(
                &format!("urn:{n}"),
                &format!("Book {n}"),
                &format!("/b/{n}.epub"),
            )
        })
        .collect();
    feed_answer(&plugin, &mut ids, &feed(&entries, Some("/opds?page=2")));
    book_answer(&plugin, &mut ids, b"PK");
    book_answer(&plugin, &mut ids, b"PK");
    assert_eq!(
        last_report(),
        "Mine loaded 2 failed 0 unreachable 0 only_lists 0 skipped 0 done"
    );
    assert_eq!(record(URL).unwrap(), "urn:0\nurn:1\n");
    // The rest comes with the next sync: nothing else is asked for now.
    assert!(
        !log()
            .iter()
            .any(|l| l.contains("/b/2.epub") || l.contains("page=2"))
    );
    drop(plugin);
}

#[test]
fn failed_downloads_are_counted_and_tried_again_next_time() {
    let host = ScriptedHost::new();
    setup(&host, vec![cat(URL)], 50);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    let mut ids = Ids(0);
    plugin.event(START, &nothing()).unwrap();
    log();
    let entries = entry("urn:1", "One", "/b/1.epub")
        + &entry("urn:2", "Two", "/b/2.epub")
        + &entry("urn:3", "Three", "/b/3.epub");
    feed_answer(&plugin, &mut ids, &feed(&entries, None));
    // One answers 404, one is a page to sign in, one is fine.
    put("sync-book.tmp", b"<html>missing</html>");
    plugin
        .event(DONE, &done(ids.next(), web::SYNC_BOOK, 404, 0))
        .unwrap();
    put("sync-book.tmp", b"<!DOCTYPE html><html>Sign in</html>");
    plugin
        .event(DONE, &done(ids.next(), web::SYNC_BOOK, 200, 0))
        .unwrap();
    book_answer(&plugin, &mut ids, b"PK three");
    assert_eq!(
        last_report(),
        "Mine loaded 1 failed 2 unreachable 0 only_lists 0 skipped 0 done"
    );
    // Only the one that came is remembered.
    assert_eq!(record(URL).unwrap(), "urn:3\n");
    assert!(file("sync-book.tmp").is_none());
    // The library refusing (a full card) is a failure too.
    let host = ScriptedHost::new();
    setup(&host, vec![cat(URL)], 50);
    IMPORT_FAILS.with(|f| *f.borrow_mut() = Some(-13));
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    let mut ids = Ids(0);
    plugin.event(START, &nothing()).unwrap();
    log();
    feed_answer(
        &plugin,
        &mut ids,
        &feed(&entry("urn:1", "One", "/b/1.epub"), None),
    );
    book_answer(&plugin, &mut ids, b"PK");
    assert_eq!(
        last_report(),
        "Mine loaded 0 failed 1 unreachable 0 only_lists 0 skipped 0 done"
    );
    assert!(record(URL).is_none());
    drop(plugin);
}

#[test]
fn an_unreachable_catalog_does_not_stop_the_others() {
    let host = ScriptedHost::new();
    setup(
        &host,
        vec![
            cat("https://down.example/opds"),
            cat("https://bad.example/opds"),
            cat(URL),
        ],
        50,
    );
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    let mut ids = Ids(0);
    plugin.event(START, &nothing()).unwrap();
    log();
    // The first is not there (no answer), the second answers with garbage.
    plugin
        .event(DONE, &done(ids.next(), web::SYNC_FEED, 0, 4))
        .unwrap();
    assert!(log()[0].starts_with("GET https://bad.example/opds"));
    put("sync-feed.tmp", b"\x00\x01 not a feed");
    plugin
        .event(DONE, &done(ids.next(), web::SYNC_FEED, 200, 0))
        .unwrap();
    // Garbage is an empty page: nothing to fetch. The third is begun.
    assert!(log()[0].starts_with(&format!("GET {URL}")));
    feed_answer(
        &plugin,
        &mut ids,
        &feed(&entry("urn:1", "One", "/b/1.epub"), None),
    );
    book_answer(&plugin, &mut ids, b"PK");
    assert_eq!(
        last_report(),
        "Mine loaded 1 failed 0 unreachable 1 only_lists 0 skipped 0 done"
    );
    drop(plugin);
}

#[test]
fn a_catalog_of_lists_only_is_reported_not_followed() {
    let host = ScriptedHost::new();
    setup(&host, vec![cat(URL)], 50);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    let mut ids = Ids(0);
    plugin.event(START, &nothing()).unwrap();
    log();
    feed_answer(
        &plugin,
        &mut ids,
        r#"<feed><title>Start</title><entry><title>Shelf</title><link rel="subsection" type="application/atom+xml" href="/shelf"/></entry></feed>"#,
    );
    assert!(log().is_empty(), "no further lists");
    assert_eq!(
        last_report(),
        "Mine loaded 0 failed 0 unreachable 0 only_lists 1 skipped 0 done"
    );
    drop(plugin);
}

#[test]
fn hostile_titles_and_links_stay_inside_the_sync_folder() {
    let host = ScriptedHost::new();
    let mut hostile = cat(URL);
    hostile.name = "../../Evil: Cat?";
    setup(&host, vec![hostile], 50);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    let mut ids = Ids(0);
    plugin.event(START, &nothing()).unwrap();
    log();
    let entries = entry("a", "../../../../evil", "/ebooks/1.epub")
        + &entry("b", "C:\\Windows\\system32", "../../../../../two.epub")
        + &entry("c", "", "/dl/%2e%2e%2F%2e%2e%2Fthree.epub?x=/../y");
    feed_answer(&plugin, &mut ids, &feed(&entries, None));
    for _ in 0..3 {
        book_answer(&plugin, &mut ids, b"PK book");
    }
    let imports: Vec<String> = log()
        .into_iter()
        .filter(|l| l.starts_with("import"))
        .collect();
    assert_eq!(imports.len(), 3, "{imports:?}");
    for line in &imports {
        let rest = line.strip_prefix("import sync-book.tmp ").unwrap();
        let (path, _) = rest.rsplit_once(" flags ").unwrap();
        let (folder, name) = path.split_once('/').unwrap();
        assert!(
            !folder.contains(['\\', ':', '?']) && !folder.contains(".."),
            "{folder}"
        );
        assert!(
            !name.contains(['/', '\\', ':', '<', '>', '|', '?', '"']),
            "{name}"
        );
        assert!(!name.starts_with('.') && name.ends_with(".epub"), "{name}");
    }
    drop(plugin);
}

#[test]
fn answers_to_a_stopped_sync_are_ignored_and_leave_no_file() {
    let host = ScriptedHost::new();
    setup(&host, vec![cat(URL)], 50);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    let mut ids = Ids(0);
    plugin.event(START, &nothing()).unwrap();
    log();
    feed_answer(
        &plugin,
        &mut ids,
        &feed(&entry("urn:1", "One", "/b/1.epub"), None),
    );
    log();
    put("sync-book.tmp", b"PK half");
    plugin.event(STOP, &nothing()).unwrap();
    assert_eq!(
        log(),
        [
            format!("cancel #{}", web::SYNC_FEED),
            format!("cancel #{}", web::SYNC_BOOK)
        ]
    );
    assert!(file("sync-book.tmp").is_none() && file("sync-feed.tmp").is_none());
    assert!(last_report().ends_with("done"));
    // Its answer, when it comes, is not taken.
    put("sync-book.tmp", b"PK late");
    seen();
    plugin
        .event(DONE, &done(2, web::SYNC_BOOK, 200, 0))
        .unwrap();
    assert!(seen().iter().any(|l| l == "done Ok(None)"));
    assert!(log().is_empty() && record(URL).is_none());
    drop(plugin);
}

#[test]
fn what_a_cut_sync_left_goes_when_the_next_starts() {
    let host = ScriptedHost::new();
    setup(&host, vec![cat(URL)], 50);
    put("sync-feed.tmp", b"left");
    put("sync-book.tmp", b"left");
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    plugin.event(START, &nothing()).unwrap();
    assert!(file("sync-feed.tmp").is_none() && file("sync-book.tmp").is_none());
    drop(plugin);
}

#[test]
fn a_webdav_folder_is_listed_with_propfind_and_synced() {
    let host = ScriptedHost::new();
    let mut dav = cat("https://cloud.example/remote.php/dav/files/me/Books/");
    dav.name = "Cloud";
    dav.webdav = true;
    setup(&host, vec![dav], 50);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    let mut ids = Ids(0);
    plugin.event(START, &nothing()).unwrap();
    let asked = log();
    assert!(
        asked[0].starts_with("PROPFIND https://cloud.example/remote.php/dav/files/me/Books/ #5"),
        "{asked:?}"
    );
    assert!(asked[0].contains("[Depth: 1]"));
    feed_answer(
        &plugin,
        &mut ids,
        r#"<d:multistatus xmlns:d="DAV:">
<d:response><d:href>/remote.php/dav/files/me/Books/</d:href></d:response>
<d:response><d:href>/remote.php/dav/files/me/Books/Die%20Verwandlung.epub</d:href></d:response>
</d:multistatus>"#,
    );
    // The book is asked for with GET.
    let asked = log();
    assert!(
        asked[0].starts_with(
            "GET https://cloud.example/remote.php/dav/files/me/Books/Die%20Verwandlung.epub #6"
        ),
        "{asked:?}"
    );
    book_answer(&plugin, &mut ids, b"PK");
    assert_eq!(
        log()[0],
        "import sync-book.tmp Cloud/Die Verwandlung.epub flags 0"
    );
    drop(plugin);
}

#[test]
fn the_catalogs_password_goes_with_its_own_requests_only() {
    let host = ScriptedHost::new();
    let mut mine = cat("https://books.example.org/opds");
    mine.user = "me";
    mine.password = "s3cret";
    setup(&host, vec![mine], 50);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    let mut ids = Ids(0);
    plugin.event(START, &nothing()).unwrap();
    let first = log();
    assert!(first[0].contains("<Basic bWU6czNjcmV0>"), "{first:?}");
    let entries =
        entry("a", "Own", "/b/1.epub") + &entry("b", "Away", "https://elsewhere.example/b/2.epub");
    feed_answer(&plugin, &mut ids, &feed(&entries, None));
    assert!(log()[0].contains("Authorization"));
    book_answer(&plugin, &mut ids, b"PK");
    let asked = log();
    assert!(
        !asked
            .iter()
            .any(|l| l.starts_with("import") && l.contains("s3cret"))
    );
    let away = asked.iter().find(|l| l.starts_with("GET")).unwrap();
    assert!(
        away.contains("elsewhere.example") && !away.contains("Authorization"),
        "{asked:?}"
    );
    for line in seen() {
        assert!(!line.contains("s3cret") && !line.contains("bWU6"), "{line}");
    }
    drop(plugin);
}

#[test]
fn a_redirected_feed_resolves_links_against_where_it_came_from() {
    let host = ScriptedHost::new();
    setup(&host, vec![cat("https://old.example/opds")], 50);
    FINAL.with(|f| *f.borrow_mut() = "https://new.example/library/opds/".to_string());
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    let mut ids = Ids(0);
    plugin.event(START, &nothing()).unwrap();
    log();
    feed_answer(
        &plugin,
        &mut ids,
        &feed(&entry("a", "One", "files/1.epub"), None),
    );
    assert!(log()[0].starts_with("GET https://new.example/library/opds/files/1.epub"));
    drop(plugin);
}

#[test]
fn truncated_and_garbage_feeds_end_the_sync_quietly() {
    for body in [
        "",
        "not xml",
        "<feed><entry><title>Cut o",
        "{\"publications\": [",
        "\u{0}\u{0}\u{0}",
    ] {
        let host = ScriptedHost::new();
        setup(&host, vec![cat(URL)], 50);
        let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
        let mut ids = Ids(0);
        plugin.event(START, &nothing()).unwrap();
        log();
        feed_answer(&plugin, &mut ids, body);
        assert!(log().is_empty(), "{body:?}");
        assert!(last_report().ends_with("done"), "{body:?}");
    }
}

#[test]
fn a_sync_in_progress_is_not_started_again() {
    let host = ScriptedHost::new();
    setup(&host, vec![cat(URL)], 50);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    plugin.event(START, &nothing()).unwrap();
    log();
    seen();
    plugin.event(START, &nothing()).unwrap();
    assert!(log().is_empty(), "no second request");
    assert!(seen().iter().any(|l| l == "start Ok(None)"));
    drop(plugin);
}

#[test]
fn a_refused_service_call_or_allocation_ends_a_sync_without_aborting() {
    let run = |host: &mut ScriptedHost, _: Option<usize>| {
        setup(host, vec![cat(URL), cat("https://d.example/opds")], 50);
        put(&record_path(URL).unwrap(), b"urn:0\n");
        let Ok(plugin) = host.load(PROBE_ENTRY.entry()) else {
            return;
        };
        let steps: [(u32, Vec<u8>); 6] = [
            (START, nothing()),
            (DONE, done(1, web::SYNC_FEED, 200, 0)),
            (DONE, done(2, web::SYNC_BOOK, 200, 0)),
            (DONE, done(3, web::SYNC_BOOK, 200, 0)),
            (DONE, done(4, web::SYNC_FEED, 200, 0)),
            (STOP, nothing()),
        ];
        for (token, batch) in &steps {
            if *token == DONE {
                let entries = entry("urn:0", "Zero", "/b/0.epub")
                    + &entry("urn:1", "One", "/b/1.epub")
                    + &entry("urn:2", "Two", "/b/2.epub");
                put("sync-feed.tmp", feed(&entries, None).as_bytes());
                put("sync-book.tmp", b"PK x");
            }
            // A refusal is an error status or an end; never a panic.
            let _ = plugin.event(*token, batch);
        }
    };
    sweep(Slot::ServiceCall, ERR_NO_MEMORY, run);
    sweep(Slot::Heap, ERR_NO_MEMORY, run);
}
