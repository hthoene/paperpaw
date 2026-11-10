//! Browsing a catalog against scripted `core.net` and `core.library` services
//! and an in-memory data folder, driven by a probe plugin that runs `Browser`
//! from events (the real plugin builds it when its screen opens): the requests
//! as they go out (the catalog's address, the headers, the files the answers
//! go to), what each answer leads to (a feed shown, a feed left, a book put
//! into the library with a free name, what goes wrong said), that the
//! credentials go only to the catalog and that no refusal aborts.
#![cfg(feature = "linked")]
#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test assertions"
)]
mod fakes;
use fakes::services::*;
use paperpaw_native::host_glue::ERR_NO_MEMORY;
use paperpaw_plugin_opds::native_browse::{Browser, Row};
use paperpaw_plugin_opds::native_catalogs::Catalog;
use paperpaw_plugin_opds::native_keep::Secret;
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
const PROGRESS: u32 = 3;
const ACTIVATE: u32 = 4;
const BACK: u32 = 5;
const STOP: u32 = 6;
const TERMS: u32 = 7;
const COVER_OF: u32 = 8;

thread_local! {
    static CONFIG: RefCell<Option<Config>> = const { RefCell::new(None) };
    /// The words the next `TERMS` types, and the row the next `ACTIVATE` chooses.
    static ARG: RefCell<(String, usize)> = const { RefCell::new((String::new(), 0)) };
}

/// How the probe's catalog is set up.
#[derive(Clone)]
struct Config {
    url: String,
    user: String,
    password: String,
    webdav: bool,
    small: bool,
}

/// What a browsing shows: the line below, and the rows.
fn view(browser: &Browser) -> String {
    let extensions = vec!["epub".to_string(), "txt".to_string()];
    // A refused allocation shows as a row of question marks, never a panic
    // (a panic in a callback aborts).
    let rows: Vec<String> = browser
        .rows()
        .unwrap_or_default()
        .iter()
        .map(|row| match row {
            Row::Search => "Search".to_string(),
            Row::More => "More".to_string(),
            Row::Entry(_) => match browser.text_of(*row, &extensions) {
                Ok((title, value, note)) => {
                    format!("{}|{value}|{note}", title.unwrap_or_default())
                }
                Err(_) => "?".to_string(),
            },
            _ => "?".to_string(),
        })
        .collect();
    format!("{:?} [{}]", browser.note(), rows.join("; "))
}

struct Probe {
    web: Web,
    library: Option<Library>,
    browser: Option<Browser>,
}

impl Plugin for Probe {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        let mut batch = Vec::new();
        let mut d = Declarations::new(|bytes: &[u8]| -> Result<(), i32> {
            batch.extend_from_slice(bytes);
            Ok(())
        });
        for (kind, token) in [
            (EventKind::HttpDone, DONE),
            (EventKind::HttpProgress, PROGRESS),
        ] {
            d.subscribe_event(kind, token).map_err(|_| Error::Invalid)?;
        }
        d.submit().map_err(|_| Error::Invalid)?;
        drop(d);
        registrations::submit(host, &batch)?;
        Ok(Self {
            web: Web::new(),
            library: None,
            browser: None,
        })
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let result = self.run(host, token, &incoming);
        if let Some(browser) = &self.browser {
            say(view(browser));
        }
        result
    }
}

impl Probe {
    fn run(&mut self, host: &Host<'_>, token: u32, incoming: &Incoming<'_>) -> Result<(), Error> {
        let extensions = vec!["epub".to_string(), "txt".to_string()];
        match token {
            START => {
                let config = CONFIG.with(|c| c.borrow().clone()).ok_or(Error::State)?;
                self.library = Library::open(host).ok();
                let mut catalog = Catalog::new(&config.url)?;
                catalog.name = "Project Gutenberg".to_string();
                catalog.user = config.user.clone();
                catalog.webdav = config.webdav;
                let password = (!config.password.is_empty())
                    .then(|| Secret::copy(&config.password))
                    .transpose()?;
                let mut browser = Browser::new(catalog, password, "Books", config.small)?;
                browser.start(host, &mut self.web)?;
                self.browser = Some(browser);
            }
            DONE => {
                if let Some(Event::HttpDone(done)) = incoming.decode_as::<kinds::HttpDone>()?
                    && let Some(browser) = &mut self.browser
                {
                    let step =
                        browser.done(host, &mut self.web, &Disk, self.library.as_ref(), &done);
                    say(format!("step {step:?}"));
                    if let Ok(Some(paperpaw_plugin_opds::native_browse::Step::AskTerms)) = &step {
                        say("asks for words".to_string());
                    }
                    step?;
                }
            }
            PROGRESS => {
                if let Some(Event::HttpProgress(progress)) =
                    incoming.decode_as::<kinds::HttpProgress>()?
                    && let Some(browser) = &mut self.browser
                {
                    let redraw = browser.progressed(&progress)?;
                    say(format!("progress redraw {redraw}"));
                }
            }
            ACTIVATE => {
                let at = ARG.with(|a| a.borrow().1);
                if let Some(browser) = &mut self.browser {
                    let Some(row) = browser.rows()?.get(at).copied() else {
                        return Ok(());
                    };
                    let step = browser.activate(host, &mut self.web, row, &extensions)?;
                    say(format!("step {step:?}"));
                }
            }
            BACK => {
                if let Some(browser) = &mut self.browser {
                    let step = browser.back(host, &mut self.web)?;
                    say(format!("back {step:?}"));
                }
            }
            STOP => {
                if let Some(browser) = &mut self.browser {
                    browser.stop_download(host, &mut self.web, &Disk);
                }
            }
            COVER_OF => {
                let url = ARG.with(|a| a.borrow().0.clone());
                if let Some(browser) = &self.browser {
                    let id = browser.cover_request(host, &mut self.web, &url);
                    say(format!("cover {}", id.is_ok()));
                }
            }
            TERMS => {
                let terms = ARG.with(|a| a.borrow().0.clone());
                if let Some(browser) = &mut self.browser {
                    browser.terms(host, &mut self.web, &terms)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

paperpaw_sdk::native_plugin!(
    Probe,
    metadata = include_bytes!("../native/metadata.host.bin"),
    linked = PROBE_ENTRY
);

const START_URL: &str = "https://www.gutenberg.org/ebooks.opds/";
const AGENT: &str = "[User-Agent: PaperPaw/";

fn setup(host: &ScriptedHost, url: &str) {
    DISK.with(|d| d.files.borrow_mut().clear());
    TAKEN.with(|t| t.borrow_mut().clear());
    FINAL.with(|f| f.borrow_mut().clear());
    IMPORT_FAILS.with(|f| *f.borrow_mut() = None);
    seen();
    log();
    CONFIG.with(|c| {
        *c.borrow_mut() = Some(Config {
            url: url.to_string(),
            user: String::new(),
            password: String::new(),
            webdav: false,
            small: false,
        });
    });
    net(host);
    library(host);
}

fn configure(change: impl FnOnce(&mut Config)) {
    CONFIG.with(|c| change(c.borrow_mut().as_mut().unwrap()));
}

fn activate(plugin: &paperpaw_scripted_host::Plugin<'_>, row: usize) {
    ARG.with(|a| a.borrow_mut().1 = row);
    plugin.event(ACTIVATE, &nothing()).unwrap();
}

fn feed(entries: &str) -> String {
    format!(r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Cat</title>{entries}</feed>"#)
}

fn book(title: &str, href: &str) -> String {
    format!(
        r#"<entry><title>{title}</title><author><name>Jane Austen</name></author><link rel="http://opds-spec.org/acquisition" type="application/epub+zip" href="{href}" title="EPUB"/></entry>"#
    )
}

const ROOT: &str = r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Project Gutenberg</title>
<link rel="search" type="application/opensearchdescription+xml" href="/catalog/osd-books.xml"/>
<link rel="next" type="application/atom+xml" href="/ebooks.opds/?start_index=26"/>
<entry><title>Popular books</title><content type="text">Most downloaded</content>
<link rel="subsection" type="application/atom+xml" href="/ebooks/popular.opds/"/></entry>
<entry><title>Alice</title><author><name>Lewis Carroll</name></author>
<link rel="http://opds-spec.org/acquisition" type="application/epub+zip" href="/ebooks/11.epub" title="EPUB"/></entry>
</feed>"#;

const POPULAR: &str = r#"<feed><title>Popular</title>
<entry><title>Die Verwandlung</title><link rel="http://opds-spec.org/acquisition" type="application/epub+zip" href="/ebooks/22367.epub"/></entry></feed>"#;

/// A started browsing: its first feed asked for and answered with `body`.
fn opened(plugin: &paperpaw_scripted_host::Plugin<'_>, body: &str) {
    plugin.event(START, &nothing()).unwrap();
    let first = log();
    assert_eq!(first.len(), 1, "{first:?}");
    put("feed.tmp", body.as_bytes());
    plugin.event(DONE, &done(1, web::FEED, 200, 0)).unwrap();
    assert!(file("feed.tmp").is_none(), "the answer's file goes");
}

#[test]
fn a_feed_is_asked_for_and_shown_with_search_and_more() {
    let host = ScriptedHost::new();
    setup(&host, START_URL);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    plugin.event(START, &nothing()).unwrap();
    let asked = log();
    assert_eq!(asked.len(), 1);
    assert!(
        asked[0].starts_with(&format!("GET {START_URL} #1 {AGENT}")),
        "{asked:?}"
    );
    assert!(
        asked[0].ends_with(&format!("file feed.tmp max_file {}", web::SMALL_FILE)),
        "{asked:?}"
    );
    assert!(!asked[0].contains("Authorization"));
    assert_eq!(seen(), ["Loading(\"www.gutenberg.org\") []"]);
    put("feed.tmp", ROOT.as_bytes());
    plugin.event(DONE, &done(1, web::FEED, 200, 0)).unwrap();
    let shown = seen();
    assert_eq!(
        shown.last().unwrap(),
        "Empty [Search; Popular books|›|Most downloaded; Alice|EPUB|Lewis Carroll · EPUB; More]"
    );
    drop(plugin);
}

#[test]
fn entries_lead_on_and_back() {
    let host = ScriptedHost::new();
    setup(&host, START_URL);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    opened(&plugin, ROOT);
    // Past *Search*, to *Popular books*: relative to the feed it came in.
    activate(&plugin, 1);
    let asked = log();
    assert!(
        asked[0].starts_with("GET https://www.gutenberg.org/ebooks/popular.opds/ #1"),
        "{asked:?}"
    );
    put("feed.tmp", POPULAR.as_bytes());
    plugin.event(DONE, &done(2, web::FEED, 200, 0)).unwrap();
    assert!(seen().last().unwrap().contains("Die Verwandlung|EPUB|"));
    // Back: the first feed again, then the list of catalogs.
    plugin.event(BACK, &nothing()).unwrap();
    assert!(log()[0].starts_with(&format!("GET {START_URL} #1")));
    put("feed.tmp", ROOT.as_bytes());
    plugin.event(DONE, &done(3, web::FEED, 200, 0)).unwrap();
    assert!(seen().last().unwrap().contains("Popular books"));
    seen();
    plugin.event(BACK, &nothing()).unwrap();
    assert!(seen().iter().any(|l| l.contains("back Leave")));
    assert!(log().is_empty(), "nothing is asked for");
    drop(plugin);
}

#[test]
fn the_next_page_and_a_next_link_to_itself() {
    let host = ScriptedHost::new();
    setup(&host, START_URL);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    opened(&plugin, ROOT);
    // *More* is the last row.
    activate(&plugin, 3);
    assert!(log()[0].starts_with("GET https://www.gutenberg.org/ebooks.opds/?start_index=26 #1"));
    // A page whose next is itself is shown, and *More* asks for it again only
    // when it is chosen.
    let itself = format!(
        r#"<feed><title>P</title><link rel="next" type="application/atom+xml" href="{START_URL}?start_index=26"/>{}</feed>"#,
        r#"<entry><title>A</title><link rel="subsection" type="application/atom+xml" href="/a"/></entry>"#
    );
    put("feed.tmp", itself.as_bytes());
    plugin.event(DONE, &done(2, web::FEED, 200, 0)).unwrap();
    assert!(log().is_empty(), "no request of its own");
    assert!(seen().last().unwrap().ends_with("[A|›|; More]"));
    drop(plugin);
}

#[test]
fn a_book_is_downloaded_into_the_library() {
    let host = ScriptedHost::new();
    setup(&host, START_URL);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    opened(&plugin, ROOT);
    // Search, Popular books, then the book.
    activate(&plugin, 2);
    let asked = log();
    assert!(
        asked[0].starts_with("GET https://www.gutenberg.org/ebooks/11.epub #3"),
        "{asked:?}"
    );
    assert!(
        asked[0].ends_with(&format!(
            "file book.tmp max_file {} progress",
            web::BOOK_BYTES
        )),
        "{asked:?}"
    );
    assert!(seen().last().unwrap().starts_with("Downloading(\"Alice\")"));
    put("book.tmp", b"PK the book");
    seen();
    plugin.event(DONE, &done(2, web::BOOK, 200, 0)).unwrap();
    // In the catalog's folder in the library, named after the author and the
    // title, under a free name.
    assert_eq!(
        log(),
        ["import book.tmp Project Gutenberg/Lewis Carroll - Alice.epub flags 2"]
    );
    let shown = seen();
    assert!(
        shown
            .iter()
            .any(|l| l.contains("Saved(\"Books/Project Gutenberg/Lewis Carroll - Alice.epub\")")),
        "{shown:?}"
    );
    assert!(
        shown.last().unwrap().starts_with(
            "Saved { name: \"Lewis Carroll - Alice.epub\", folder: \"Books/Project Gutenberg\" }"
        ),
        "{shown:?}"
    );
    assert!(
        file("book.tmp").is_none(),
        "the file moved into the library"
    );
    // The same book twice: the second gets a name of its own.
    activate(&plugin, 2);
    log();
    put("book.tmp", b"PK two");
    plugin.event(DONE, &done(3, web::BOOK, 200, 0)).unwrap();
    assert_eq!(
        log(),
        ["import book.tmp Project Gutenberg/Lewis Carroll - Alice.epub flags 2"]
    );
    assert!(TAKEN.with(|t| t.borrow().len()) == 2);
    drop(plugin);
}

#[test]
fn a_download_says_how_far_it_came() {
    let host = ScriptedHost::new();
    setup(&host, START_URL);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    opened(&plugin, ROOT);
    activate(&plugin, 2);
    log();
    seen();
    // The first progress is drawn, a step too small is not, 10 % further is,
    // and so is a long time later.
    let steps = [
        (
            1_000,
            100,
            Some(1000),
            "Share { title: \"Alice\", percent: 10 }",
            true,
        ),
        (1_500, 150, Some(1000), "", false),
        (
            2_000,
            250,
            Some(1000),
            "Share { title: \"Alice\", percent: 25 }",
            true,
        ),
        (
            8_000,
            255,
            Some(1000),
            "Share { title: \"Alice\", percent: 25 }",
            true,
        ),
        (8_100, 2048, None, "", false),
    ];
    for (now, received, total, note, drawn) in steps {
        plugin
            .event(PROGRESS, &progress(2, web::BOOK, now, received, total))
            .unwrap();
        let lines = seen();
        assert!(
            lines
                .iter()
                .any(|l| l == &format!("progress redraw {drawn}")),
            "{lines:?}"
        );
        if drawn {
            assert!(lines.last().unwrap().starts_with(note), "{lines:?}");
        }
    }
    // Without a size, the bytes so far; progress of another request is not it.
    plugin
        .event(PROGRESS, &progress(2, web::BOOK, 20_000, 2048, None))
        .unwrap();
    assert!(
        seen()
            .last()
            .unwrap()
            .starts_with("Size { title: \"Alice\", bytes: 2048 }")
    );
    plugin
        .event(PROGRESS, &progress(9, web::BOOK, 40_000, 5, None))
        .unwrap();
    assert!(seen().iter().any(|l| l == "progress redraw false"));
    drop(plugin);
}

#[test]
fn back_stops_a_download_and_leaves_nothing() {
    let host = ScriptedHost::new();
    setup(&host, START_URL);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    opened(&plugin, ROOT);
    activate(&plugin, 2);
    log();
    put("book.tmp", b"PK half");
    plugin.event(STOP, &nothing()).unwrap();
    assert_eq!(log(), ["cancel #3"]);
    assert!(file("book.tmp").is_none());
    assert!(seen().last().unwrap().starts_with("Stopped"));
    // The end the host reports for it is not waited for, and not told.
    seen();
    plugin.event(DONE, &done(2, web::BOOK, 0, 8)).unwrap();
    assert!(seen().iter().any(|l| l == "step Ok(None)"));
    assert!(log().is_empty());
    drop(plugin);
}

#[test]
fn an_answer_that_is_no_book_is_not_saved() {
    let host = ScriptedHost::new();
    setup(&host, START_URL);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    opened(&plugin, ROOT);
    activate(&plugin, 2);
    log();
    put("book.tmp", b"<!DOCTYPE html><html>Sign in</html>");
    plugin.event(DONE, &done(2, web::BOOK, 200, 0)).unwrap();
    assert!(log().is_empty(), "nothing goes into the library");
    assert!(seen().last().unwrap().starts_with("NotABook"));
    assert!(file("book.tmp").is_none());
    drop(plugin);
}

#[test]
fn what_goes_wrong_is_said() {
    // (status, reason, what the line below says)
    let cases = [
        (404, 0, "Error { host: \"www.gutenberg.org\", status: 404 }"),
        (500, 0, "Error { host: \"www.gutenberg.org\", status: 500 }"),
        (401, 0, "Unauthorized"),
        (403, 0, "Unauthorized"),
        (0, 4, "FailedHost(\"www.gutenberg.org\")"),
        (0, 2, "FailedHost(\"www.gutenberg.org\")"),
        (0, 1, "NoNetwork"),
        (0, 11, "NoNetwork"),
        (0, 12, "NoNetwork"),
        (0, 13, "NoNetwork"),
    ];
    for (status, reason, said) in cases {
        let host = ScriptedHost::new();
        setup(&host, START_URL);
        let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
        plugin.event(START, &nothing()).unwrap();
        log();
        seen();
        put("feed.tmp", ROOT.as_bytes());
        plugin
            .event(DONE, &done(1, web::FEED, status, reason))
            .unwrap();
        let shown = seen();
        assert!(
            shown.last().unwrap().starts_with(said),
            "{status} {reason}: {shown:?}"
        );
        assert!(file("feed.tmp").is_none(), "the answer's file goes");
    }
}

#[test]
fn malformed_feeds_are_not_catalogs_and_do_not_panic() {
    for body in [
        "",
        "not xml at all",
        "<html><body>Sign in</body></html>",
        "\u{0}\u{0}\u{0}",
        "{\"unrelated\": true}",
    ] {
        let host = ScriptedHost::new();
        setup(&host, START_URL);
        let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
        plugin.event(START, &nothing()).unwrap();
        put("feed.tmp", body.as_bytes());
        plugin.event(DONE, &done(1, web::FEED, 200, 0)).unwrap();
        let shown = seen();
        assert!(
            shown.last().unwrap().starts_with("NotACatalog"),
            "{body:?}: {shown:?}"
        );
    }
}

#[test]
fn a_truncated_feed_keeps_the_entries_that_came() {
    let host = ScriptedHost::new();
    setup(&host, START_URL);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    let cut = format!(
        "{}<entry><title>Cut o",
        r#"<feed><title>Cat</title><entry><title>Whole</title><link rel="subsection" type="application/atom+xml" href="/w"/></entry>"#
    );
    opened(&plugin, &cut);
    assert!(seen().last().unwrap().ends_with("[Whole|›|]"));
    drop(plugin);
}

#[test]
fn thousands_of_entries_are_cut_to_a_page_with_more() {
    let host = ScriptedHost::new();
    setup(&host, START_URL);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    let mut body = String::from(
        r#"<feed><title>Many</title><link rel="next" type="application/atom+xml" href="?page=2"/>"#,
    );
    for n in 0..5000 {
        body.push_str(&format!(
            r#"<entry><title>E{n}</title><link rel="subsection" type="application/atom+xml" href="/{n}"/></entry>"#
        ));
    }
    body.push_str("</feed>");
    opened(&plugin, &body);
    let shown = seen();
    let last = shown.last().unwrap();
    assert_eq!(last.matches("|›|").count(), 100, "a page");
    assert!(last.ends_with("; More]"));
    drop(plugin);
}

#[test]
fn a_small_device_keeps_a_smaller_page() {
    let host = ScriptedHost::new();
    setup(&host, START_URL);
    configure(|c| c.small = true);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    let mut body = String::from("<feed><title>Many</title>");
    for n in 0..500 {
        body.push_str(&format!(
            r#"<entry><title>E{n}</title><link rel="subsection" type="application/atom+xml" href="/{n}"/></entry>"#
        ));
    }
    body.push_str("</feed>");
    opened(&plugin, &body);
    assert_eq!(seen().last().unwrap().matches("|›|").count(), 40);
    drop(plugin);
}

#[test]
fn a_redirected_feed_resolves_its_links_where_it_landed() {
    let host = ScriptedHost::new();
    setup(&host, "https://old.example/opds");
    FINAL.with(|f| *f.borrow_mut() = "https://new.example/library/opds/".to_string());
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    opened(
        &plugin,
        r#"<feed><title>C</title><entry><title>Shelf</title><link rel="subsection" type="application/atom+xml" href="shelf"/></entry></feed>"#,
    );
    activate(&plugin, 0);
    assert!(
        log()[0].starts_with("GET https://new.example/library/opds/shelf #1"),
        "relative to where it landed"
    );
    drop(plugin);
}

#[test]
fn javascript_links_lead_nowhere_harmful() {
    let host = ScriptedHost::new();
    setup(&host, START_URL);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    opened(
        &plugin,
        &feed(
            r#"<entry><title>Evil</title><link rel="http://opds-spec.org/acquisition" type="application/epub+zip" href="javascript:alert(1)"/></entry>"#,
        ),
    );
    activate(&plugin, 0);
    // The request is refused as the invalid address it is: said, not sent.
    let asked = log();
    assert!(asked.is_empty(), "{asked:?}");
    assert!(seen().last().unwrap().starts_with("FailedHost"));
    drop(plugin);
}

#[test]
fn hostile_downloads_land_in_the_catalogs_folder_with_safe_names() {
    let titles = [
        ("../../../../evil", "/ebooks/1.epub"),
        ("..", "../../../../../two.epub"),
        ("", "/dl/%2e%2e%2F%2e%2e%2Fthree.epub?x=/../y"),
        ("C:\\Windows\\system32", "/ebooks/4.epub"),
        (
            "A &lt;b&gt;bold&lt;/b&gt; &amp; &quot;quoted&quot; | title?",
            "/ebooks/5.epub",
        ),
    ];
    for (title, href) in titles {
        let host = ScriptedHost::new();
        setup(&host, START_URL);
        let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
        opened(&plugin, &feed(&book(title, href)));
        activate(&plugin, 0);
        let asked = log();
        assert!(
            asked[0].starts_with("GET https://www.gutenberg.org/"),
            "{asked:?}"
        );
        put("book.tmp", b"PK book");
        plugin.event(DONE, &done(2, web::BOOK, 200, 0)).unwrap();
        let imports = log();
        assert_eq!(imports.len(), 1, "{title}: {imports:?}");
        let line = imports[0]
            .strip_prefix("import book.tmp Project Gutenberg/")
            .unwrap();
        assert!(
            !line.contains(['\\', ':', '<', '>', '|', '?', '"']) && !line.contains('/'),
            "{line}"
        );
        assert!(!line.starts_with('.') && line.contains(".epub"), "{line}");
    }
}

#[test]
fn a_failed_download_saves_nothing_and_says_why() {
    let host = ScriptedHost::new();
    setup(&host, START_URL);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    opened(&plugin, &feed(&book("Alice", "/ebooks/11.epub")));
    activate(&plugin, 0);
    log();
    put("book.tmp", b"<html>Not found</html>");
    plugin.event(DONE, &done(2, web::BOOK, 404, 0)).unwrap();
    assert!(
        seen()
            .last()
            .unwrap()
            .starts_with("Error { host: \"www.gutenberg.org\", status: 404 }")
    );
    assert!(log().is_empty() && file("book.tmp").is_none());
    // The library refuses (the card is full): said, and the file goes.
    activate(&plugin, 0);
    log();
    put("book.tmp", b"PK ok");
    IMPORT_FAILS.with(|f| *f.borrow_mut() = Some(-13));
    plugin.event(DONE, &done(3, web::BOOK, 200, 0)).unwrap();
    assert!(seen().last().unwrap().starts_with("CannotSave"));
    assert!(file("book.tmp").is_none());
    drop(plugin);
}

#[test]
fn entries_without_a_readable_format_say_so() {
    let host = ScriptedHost::new();
    setup(&host, START_URL);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    opened(
        &plugin,
        &feed(
            r#"<entry><title>Only PDF</title><link rel="http://opds-spec.org/acquisition" type="application/pdf" href="/x.pdf"/><link rel="http://opds-spec.org/acquisition" type="application/x-unknown" href="/x.bin"/></entry>"#,
        ),
    );
    log();
    activate(&plugin, 0);
    assert!(log().is_empty());
    assert!(seen().last().unwrap().starts_with("NoFormat"));
    drop(plugin);
}

#[test]
fn searches_by_template_encode_the_words() {
    let host = ScriptedHost::new();
    setup(&host, START_URL);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    opened(
        &plugin,
        r#"<feed><title>C</title><link rel="search" href="/search?q={searchTerms}&amp;p={startPage?}"/><entry><title>S</title><link rel="subsection" type="application/atom+xml" href="/s"/></entry></feed>"#,
    );
    activate(&plugin, 0);
    assert!(seen().iter().any(|l| l == "step AskTerms"));
    // The terms need a template: set by the step above.
    ARG.with(|a| a.borrow_mut().0 = "  Jane & Auste&n é ".to_string());
    plugin.event(TERMS, &nothing()).unwrap();
    assert!(
        log()[0]
            .starts_with("GET https://www.gutenberg.org/search?q=Jane+%26+Auste%26n+%C3%A9&p= #1"),
        "the words bring no syntax of their own"
    );
    // Empty words ask for nothing.
    ARG.with(|a| a.borrow_mut().0 = "   ".to_string());
    plugin.event(TERMS, &nothing()).unwrap();
    assert!(log().is_empty());
    drop(plugin);
}

#[test]
fn searches_by_description_first_ask_where_to_search() {
    let host = ScriptedHost::new();
    setup(&host, START_URL);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    opened(&plugin, ROOT);
    activate(&plugin, 0);
    let asked = log();
    assert!(
        asked[0].starts_with("GET https://www.gutenberg.org/catalog/osd-books.xml #2"),
        "{asked:?}"
    );
    assert!(asked[0].ends_with(&format!(
        "file description.tmp max_file {}",
        web::SMALL_FILE
    )));
    put(
        "description.tmp",
        br#"<OpenSearchDescription><Url type="application/atom+xml" template="/ebooks/search.opds/?query={searchTerms}"/></OpenSearchDescription>"#,
    );
    seen();
    plugin
        .event(DONE, &done(2, web::DESCRIPTION, 200, 0))
        .unwrap();
    assert!(seen().iter().any(|l| l == "asks for words"));
    assert!(file("description.tmp").is_none());
    ARG.with(|a| a.borrow_mut().0 = "alice".to_string());
    plugin.event(TERMS, &nothing()).unwrap();
    assert!(
        log()[0].starts_with("GET https://www.gutenberg.org/ebooks/search.opds/?query=alice #1")
    );
    // A description without an OPDS address cannot be searched.
    let host = ScriptedHost::new();
    setup(&host, START_URL);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    opened(&plugin, ROOT);
    activate(&plugin, 0);
    put(
        "description.tmp",
        br#"<OpenSearchDescription><Url type="text/html" template="/x?q={searchTerms}"/></OpenSearchDescription>"#,
    );
    seen();
    plugin
        .event(DONE, &done(2, web::DESCRIPTION, 200, 0))
        .unwrap();
    assert!(seen().last().unwrap().starts_with("NoSearch"));
    drop(plugin);
}

#[test]
fn a_late_answer_for_another_page_is_ignored() {
    let host = ScriptedHost::new();
    setup(&host, START_URL);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    opened(&plugin, ROOT);
    activate(&plugin, 1);
    log();
    // The answer of a request that is not the one waited for (id 7).
    put("feed.tmp", POPULAR.as_bytes());
    seen();
    plugin.event(DONE, &done(7, web::FEED, 200, 0)).unwrap();
    let shown = seen();
    assert!(shown.iter().any(|l| l == "step Ok(None)"), "{shown:?}");
    assert!(shown.last().unwrap().starts_with("Loading"));
    drop(plugin);
}

#[test]
fn credentials_go_only_to_the_catalog() {
    let host = ScriptedHost::new();
    setup(&host, "https://books.example.org/opds");
    configure(|c| {
        c.user = "me".to_string();
        c.password = "s3cret".to_string();
    });
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    plugin.event(START, &nothing()).unwrap();
    // The feed's own request carried the account, once, to its own address.
    let first = log();
    assert!(
        first[0].contains("[Authorization: <hidden>]<Basic bWU6czNjcmV0>"),
        "{first:?}"
    );
    let entries = book("Own", "/b/1.epub")
        + &book("Away", "https://elsewhere.example/b/2.epub")
        + &book("Clear", "http://books.example.org/b/3.epub")
        + &book("Other port", "https://books.example.org:8443/b/4.epub");
    put("feed.tmp", feed(&entries).as_bytes());
    plugin.event(DONE, &done(1, web::FEED, 200, 0)).unwrap();
    // Books on the same server get it; anywhere else (another host, in clear,
    // another port) they do not.
    for (row, authorized) in [(0, true), (1, false), (2, false), (3, false)] {
        activate(&plugin, row);
        let asked = log();
        assert_eq!(
            asked[0].contains("Authorization"),
            authorized,
            "{row}: {asked:?}"
        );
    }
    // What the probe shows of the account, in any line, is never the password.
    for line in seen() {
        assert!(!line.contains("s3cret") && !line.contains("bWU6"), "{line}");
    }
    drop(plugin);
}

#[test]
fn a_cover_is_asked_for_to_its_own_file_and_gets_the_account_only_at_home() {
    let host = ScriptedHost::new();
    setup(&host, "https://books.example.org/opds");
    configure(|c| {
        c.user = "me".to_string();
        c.password = "s3cret".to_string();
    });
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    opened_without_check(&plugin);
    for (url, authorized) in [
        ("https://books.example.org/covers/1.jpg", true),
        ("https://cdn.example.net/covers/1.jpg", false),
    ] {
        ARG.with(|a| a.borrow_mut().0 = url.to_string());
        plugin.event(COVER_OF, &nothing()).unwrap();
        let asked = log();
        assert!(
            asked[0].starts_with(&format!("GET {url} #{}", web::COVER)),
            "{asked:?}"
        );
        assert!(
            asked[0].ends_with(&format!("file cover.tmp max_file {}", web::COVER_BYTES)),
            "{asked:?}"
        );
        assert_eq!(asked[0].contains("Authorization"), authorized, "{asked:?}");
    }
    drop(plugin);
}

/// A browsing started, its first request left unanswered.
fn opened_without_check(plugin: &paperpaw_scripted_host::Plugin<'_>) {
    plugin.event(START, &nothing()).unwrap();
    log();
}

#[test]
fn a_webdav_folder_is_listed_with_propfind() {
    let host = ScriptedHost::new();
    setup(
        &host,
        "https://cloud.example.org/remote.php/dav/files/me/Books/",
    );
    configure(|c| c.webdav = true);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    plugin.event(START, &nothing()).unwrap();
    let asked = log();
    assert!(
        asked[0].starts_with(
            "PROPFIND https://cloud.example.org/remote.php/dav/files/me/Books/ #1 [User-Agent: PaperPaw/"
        ),
        "{asked:?}"
    );
    assert!(
        asked[0].contains("[Depth: 1]")
            && asked[0].contains("[Content-Type: application/xml; charset=utf-8]")
    );
    assert!(
        asked[0].contains(" body <?xml version=\"1.0\"?><d:propfind"),
        "{asked:?}"
    );
    put(
        "feed.tmp",
        br#"<d:multistatus xmlns:d="DAV:">
<d:response><d:href>/remote.php/dav/files/me/Books/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
<d:response><d:href>/remote.php/dav/files/me/Books/Die%20Verwandlung.epub</d:href></d:response>
<d:response><d:href>/remote.php/dav/files/me/Books/Krimis/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop></d:propstat></d:response>
</d:multistatus>"#,
    );
    plugin.event(DONE, &done(1, web::FEED, 207, 0)).unwrap();
    // 207 is not a 2xx the host calls fine? It is: 200 to 299.
    assert!(
        seen()
            .last()
            .unwrap()
            .ends_with("[Krimis|›|; Die Verwandlung|EPUB|]")
    );
    drop(plugin);
}

#[test]
fn an_opds_2_feed_is_read_where_the_answer_is_json() {
    let host = ScriptedHost::new();
    setup(&host, "https://komga.example/opds/v2/catalog");
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    opened(
        &plugin,
        r#"{"metadata":{"title":"K"},"publications":[{"metadata":{"title":"Alice","author":"Lewis"},"links":[{"rel":"http://opds-spec.org/acquisition","href":"/b/1","type":"application/epub+zip"}]}]}"#,
    );
    assert!(seen().last().unwrap().ends_with("[Alice|EPUB|Lewis]"));
    drop(plugin);
}

#[test]
fn a_refused_service_call_or_allocation_ends_a_step_without_aborting() {
    let run = |host: &mut ScriptedHost, _: Option<usize>| {
        setup(host, START_URL);
        configure(|c| {
            c.user = "me".to_string();
            c.password = "pw".to_string();
        });
        let Ok(plugin) = host.load(PROBE_ENTRY.entry()) else {
            return;
        };
        let steps: [(u32, Vec<u8>); 7] = [
            (START, nothing()),
            (DONE, done(1, web::FEED, 200, 0)),
            (ACTIVATE, nothing()),
            (PROGRESS, progress(2, web::BOOK, 10_000, 500, Some(1000))),
            (DONE, done(2, web::BOOK, 200, 0)),
            (ACTIVATE, nothing()),
            (STOP, nothing()),
        ];
        ARG.with(|a| a.borrow_mut().1 = 2);
        for (token, batch) in &steps {
            if *token == DONE {
                put("feed.tmp", ROOT.as_bytes());
                put("book.tmp", b"PK x");
            }
            // A refusal is an error status or an end; never a panic.
            let _ = plugin.event(*token, batch);
        }
    };
    sweep(Slot::ServiceCall, ERR_NO_MEMORY, run);
    sweep(Slot::Heap, ERR_NO_MEMORY, run);
}
