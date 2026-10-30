//! The browser with the real kernel, the servers' answers canned: what
//! is typed becomes an address or a search, pages are kept with their
//! address as `<base>` so relative links resolve, the history keeps the
//! latest pages and deletes the others' files, bookmarks are added and
//! removed, books, fonts and other files go where they belong under safe
//! names, and errors, redirects, late answers and hostile pages and
//! addresses are handled.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::{HttpCall, HttpResponse, Io, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_plugin_browser::{Browser, OPEN};
use paperpaw_sdk::HttpError;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::ui::choice::Picked;
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, Id, InputCaps, Instant, MemoryClass, MemoryStorage, Params,
    PixelFormat, Storage, StorageError,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Large,
);

const DIR: &str = ".paperpaw/plugins/browser";
const HISTORY: &str = ".paperpaw/plugins/browser/history.txt";
const BOOKMARKS: &str = ".paperpaw/plugins/browser/bookmarks.txt";

/// A storage the test looks into while the kernel has it.
#[derive(Clone, Default)]
struct Shared(Rc<MemoryStorage>);

impl Shared {
    fn text(&self, path: &str) -> Option<String> {
        self.0
            .get(path)
            .map(|d| String::from_utf8_lossy(&d).into_owned())
    }

    /// Every file, by its path, below `dir` ("" for all).
    fn files(&self, dir: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut entries = Vec::new();
        let _ = self
            .0
            .list(dir, &mut |e| entries.push((e.name.to_string(), e.is_dir)));
        for (name, is_dir) in entries {
            let path = if dir.is_empty() {
                name
            } else {
                format!("{dir}/{name}")
            };
            if is_dir {
                found.extend(self.files(&path));
            } else {
                found.push(path);
            }
        }
        found.sort();
        found
    }

    /// The files outside PaperPaw's own folder.
    fn saved(&self) -> Vec<String> {
        self.files("")
            .into_iter()
            .filter(|p| !p.starts_with(".paperpaw/"))
            .collect()
    }
}

impl Storage for Shared {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.0.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.0.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.0.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.0.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.0.remove(path)
    }
    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.0.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.0.create_dir(path)
    }
}

struct Harness {
    kernel: Kernel,
    storage: Shared,
    now: u64,
}

impl Harness {
    /// The browser's start screen, opened.
    fn new() -> Self {
        Self::build(Shared::default(), None, true)
    }

    fn build(storage: Shared, fonts: Option<&str>, network: bool) -> Self {
        let mut builder = Kernel::builder(DEVICE)
            .storage(storage.clone())
            .user_config("[core]\nlanguage = en\nlibrary-folder =\n")
            .network(network)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(paperpaw_plugin_format_epub::EpubFormat)
            .plugin(Browser);
        if let Some(dir) = fonts {
            builder = builder.font_dir(dir);
        }
        let mut kernel = builder.build().unwrap();
        kernel.start(Instant(0));
        let mut h = Self {
            kernel,
            storage,
            now: 0,
        };
        h.run(OPEN);
        h
    }

    fn tick(&mut self) -> Instant {
        self.now += 100;
        Instant(self.now)
    }

    fn run(&mut self, action: Id) {
        let now = self.tick();
        self.kernel.run_action(now, action);
    }

    fn send(&mut self, event: &'static str, data: impl EventData) {
        let now = self.tick();
        self.kernel
            .send_event(now, Id::new_static(event), data.to_params());
    }

    /// Types `text` as the address.
    fn go(&mut self, text: &str) -> HttpCall {
        self.send(
            "browser.address",
            Typed::new(String::from(text), Params::new()),
        );
        self.request()
    }

    fn pick(&mut self, context: &str, option: &str) {
        self.send(
            "browser.chosen",
            Picked::new(String::from(context), String::from(option)),
        );
    }

    fn requests(&mut self) -> Vec<HttpCall> {
        self.kernel
            .take_io()
            .into_iter()
            .filter_map(|io| match io {
                Io::Http(call) => Some(call),
                _ => None,
            })
            .collect()
    }

    fn request(&mut self) -> HttpCall {
        let mut calls = self.requests();
        assert_eq!(calls.len(), 1, "{calls:?}");
        calls.remove(0)
    }

    fn answer(&mut self, call: &HttpCall, status: u16, kind: Option<&str>, body: &[u8]) {
        self.reply(HttpResponse {
            id: call.id,
            status,
            body: body.to_vec(),
            content_type: kind.map(String::from),
            ..HttpResponse::default()
        });
    }

    fn reply(&mut self, response: HttpResponse) {
        let now = self.tick();
        self.kernel.http_response(now, response);
    }

    /// All the text shown, as one line.
    fn said(&mut self) -> String {
        self.kernel.render(&mut |_, _| {});
        let labels: Vec<String> = self
            .kernel
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect();
        labels
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The pages kept.
    fn pages(&self) -> Vec<String> {
        self.storage.files(&format!("{DIR}/pages"))
    }

    fn history(&self) -> Vec<(String, String)> {
        lines(&self.storage.text(HISTORY).unwrap_or_default())
    }
}

fn lines(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(|l| {
            let (a, b) = l.split_once('\t').expect(l);
            (a.to_string(), b.to_string())
        })
        .collect()
}

const HTML: Option<&str> = Some("text/html; charset=utf-8");

#[test]
fn typed_words_are_searched_and_addresses_loaded() {
    let mut h = Harness::new();
    let call = h.go("  de.wikipedia.org  ");
    assert_eq!(call.url, "https://de.wikipedia.org");
    assert_eq!(call.method, "GET");
    assert!(call.save_to.is_some());
    assert!(
        call.headers
            .iter()
            .any(|(k, v)| k == "Accept" && v.contains("text/html"))
    );
    assert!(h.said().contains("Loading de.wikipedia.org"));
    h.answer(&call, 0, None, b"");
    let call = h.go("Kafka & Söhne");
    assert_eq!(
        call.url,
        "https://html.duckduckgo.com/html/?q=Kafka+%26+S%C3%B6hne"
    );
    h.answer(&call, 0, None, b"");
    // Nothing typed: nothing loaded.
    h.send(
        "browser.address",
        Typed::new(String::from("   "), Params::new()),
    );
    assert!(h.requests().is_empty());
}

#[test]
fn a_page_is_kept_with_its_address_and_remembered() {
    let mut h = Harness::new();
    let call = h.go("https://a.example/dir/page?x=1&y=\"2\"");
    let page = b"<html><head><title>\n  A  &amp;\tB  </title></head><body><a href=\"../other\">l</a></body></html>";
    h.answer(&call, 200, HTML, page);
    let pages = h.pages();
    assert_eq!(pages.len(), 1, "{pages:?}");
    let kept = h.storage.text(&pages[0]).unwrap();
    // Its links resolve against where it came from; the address cannot
    // break out of the attribute.
    assert!(
        kept.starts_with(
            "<base href=\"https://a.example/dir/page?x=1&amp;y=&quot;2&quot;\">\n<html>"
        ),
        "{kept}"
    );
    assert_eq!(
        h.history(),
        [(
            String::from("A & B"),
            String::from("https://a.example/dir/page?x=1&y=\"2\"")
        )]
    );
    assert!(!h.storage.0.exists(&format!("{DIR}/download.tmp")));
    // The same page again: one entry, one file.
    let call = h.go("https://a.example/dir/page?x=1&y=\"2\"");
    h.answer(&call, 200, HTML, page);
    assert_eq!(h.history().len(), 1);
    assert_eq!(h.pages().len(), 1);
}

#[test]
fn a_redirected_page_is_kept_under_where_it_landed() {
    let mut h = Harness::new();
    let call = h.go("http://short.example/x");
    h.reply(HttpResponse {
        id: call.id,
        status: 200,
        body: b"<title>Landed</title>".to_vec(),
        content_type: HTML.map(String::from),
        url: Some(String::from("https://long.example/a/b")),
        ..HttpResponse::default()
    });
    assert_eq!(
        h.history(),
        [(
            String::from("Landed"),
            String::from("https://long.example/a/b")
        )]
    );
    let kept = h.storage.text(&h.pages()[0]).unwrap();
    assert!(
        kept.starts_with("<base href=\"https://long.example/a/b\">"),
        "{kept}"
    );
}

#[test]
fn hostile_titles_keep_the_history_one_line_a_page() {
    let titles: [&[u8]; 6] = [
        b"<title>Tab\there\nand\r\nbreaks</title>",
        b"<title><b>bold</b> &lt;script&gt;</title>",
        b"<title>never closed",
        b"<title></title><title>Second</title>",
        b"\xff\xfe<title>\xff bytes \xfe</title>",
        b"<svg><title>inside svg</title></svg>",
    ];
    let mut h = Harness::new();
    for (n, page) in titles.iter().enumerate() {
        let call = h.go(&format!("https://h.example/{n}"));
        h.answer(&call, 200, HTML, page);
    }
    let history = h.storage.text(HISTORY).unwrap();
    assert_eq!(history.lines().count(), titles.len(), "{history:?}");
    for line in history.lines() {
        assert_eq!(line.split('\t').count(), 2, "{line:?}");
        assert!(!line.starts_with('\t'), "every page has a title: {line:?}");
    }
    // Without a title, the host names it.
    let call = h.go("https://untitled.example/");
    h.answer(&call, 200, HTML, b"<p>no title</p>");
    assert_eq!(h.history()[0].0, "untitled.example");
}

#[test]
fn the_history_keeps_the_latest_pages_and_deletes_the_others() {
    let mut h = Harness::new();
    for n in 0..35 {
        let call = h.go(&format!("https://h.example/{n}"));
        h.answer(
            &call,
            200,
            HTML,
            format!("<title>Page {n}</title>").as_bytes(),
        );
    }
    let history = h.history();
    assert_eq!(history.len(), 30);
    assert_eq!(history[0].0, "Page 34");
    assert_eq!(history[29].0, "Page 5");
    assert_eq!(h.pages().len(), 30, "the files of forgotten pages go");
    // The start screen lists them below the bookmarks.
    h.run(actions::BACK);
    h.run(OPEN);
    let shown = h.said();
    assert!(shown.contains("Wikipedia"), "{shown}");
    assert!(shown.contains("Page 34"), "{shown}");
}

/// Bookmarks that cannot be read (saved in another encoding here) are
/// not written over by the ones to start with and a new one.
#[test]
fn unreadable_bookmarks_are_not_written_over() {
    let storage = Shared::default();
    let kept = b"Caf\xe9\thttps://cafe.example/\n".to_vec();
    storage.write(BOOKMARKS, &kept).unwrap();
    let mut h = Harness::build(storage, None, true);
    let call = h.go("https://h.example/kept");
    h.answer(&call, 200, HTML, b"<title>Kept</title>");
    h.run(actions::BACK);
    h.run(OPEN);
    assert_eq!(h.history().len(), 1);
    h.pick("history:0", "bookmark");
    h.pick("bookmark:0", "delete");
    assert_eq!(h.storage.0.get(BOOKMARKS).unwrap(), kept);
}

#[test]
fn history_entries_become_bookmarks_and_go() {
    let mut h = Harness::new();
    let call = h.go("https://h.example/kept");
    h.answer(&call, 200, HTML, b"<title>Kept</title>");
    h.run(actions::BACK);
    h.run(OPEN);
    // Choices for entries that are not there do nothing.
    for (context, option) in [
        ("history:7", "bookmark"),
        ("bookmark:99", "delete"),
        ("history:x", "delete"),
        ("nonsense", "open"),
        ("history:0", "dance"),
    ] {
        h.pick(context, option);
    }
    assert!(h.storage.text(BOOKMARKS).is_none());
    assert_eq!(h.history().len(), 1);
    h.pick("history:0", "bookmark");
    h.pick("history:0", "bookmark");
    let bookmarks = lines(&h.storage.text(BOOKMARKS).unwrap());
    // The ones to start with, and this one once.
    assert_eq!(bookmarks.len(), 4, "{bookmarks:?}");
    assert_eq!(
        bookmarks[3],
        (String::from("Kept"), String::from("https://h.example/kept"))
    );
    h.pick("bookmark:0", "delete");
    let bookmarks = lines(&h.storage.text(BOOKMARKS).unwrap());
    assert_eq!(bookmarks.len(), 3);
    assert!(bookmarks.iter().all(|(t, _)| t != "Wikipedia"));
    // Deleted from the history: its page file goes too.
    h.pick("history:0", "delete");
    assert!(h.history().is_empty());
    assert!(h.pages().is_empty());
    // A bookmark opens.
    h.pick("bookmark:2", "open");
    assert_eq!(h.request().url, "https://h.example/kept");
}

#[test]
fn books_fonts_and_files_go_where_they_belong_under_safe_names() {
    let cases: [(&str, Option<&str>, &[u8], &str); 6] = [
        (
            "https://b.example/get/..%2F..%2F.paperpaw%2Fsecrets.epub",
            Some("application/epub+zip"),
            b"PK not really",
            "paperpaw secrets.epub",
        ),
        (
            "https://b.example/books/Alice.txt?download=1",
            None,
            b"Alice was beginning",
            "Alice.txt",
        ),
        (
            "https://b.example/",
            Some("application/octet-stream"),
            b"\x00\x01binary",
            "Downloads/download",
        ),
        (
            "https://b.example/a/..%5C..%5CC%3A%5Cwin.ini",
            Some("application/x-unknown"),
            b"x",
            "Downloads/C win.ini",
        ),
        (
            "https://b.example/files/report.pdf",
            Some("application/pdf"),
            b"%PDF-1.4",
            "Downloads/report.pdf",
        ),
        (
            "https://b.example/fonts/Serif.ttf",
            None,
            b"\x00\x01\x00\x00font",
            "fonts/Serif.ttf",
        ),
    ];
    for (url, kind, body, expected) in cases {
        let mut h = Harness::build(Shared::default(), Some("fonts"), true);
        let call = h.go(url);
        h.answer(&call, 200, kind, body);
        assert_eq!(h.storage.saved(), [expected], "{url}");
        assert!(!h.storage.0.exists(&format!("{DIR}/download.tmp")));
    }
}

#[test]
fn a_book_offers_to_be_read_and_a_second_gets_a_name_of_its_own() {
    let mut h = Harness::new();
    let call = h.go("https://b.example/book.epub");
    h.answer(&call, 200, Some("application/epub+zip"), b"PK one");
    assert!(h.said().contains("Read now"));
    h.run(actions::BACK);
    let call = h.go("https://b.example/book.epub");
    h.answer(&call, 200, Some("application/epub+zip"), b"PK two");
    assert_eq!(h.storage.saved(), ["book (2).epub", "book.epub"]);
}

#[test]
fn fonts_without_a_fonts_folder_are_not_kept() {
    let mut h = Harness::new();
    let call = h.go("https://f.example/Serif.otf");
    h.answer(&call, 200, Some("font/otf"), b"OTTO font");
    assert!(h.said().contains("Fonts cannot be added"));
    assert!(h.storage.saved().is_empty());
    assert!(!h.storage.0.exists(&format!("{DIR}/download.tmp")));
}

#[test]
fn errors_are_told_and_error_pages_shown() {
    let mut h = Harness::new();
    let call = h.go("https://e.example/missing.pdf");
    h.answer(&call, 404, Some("application/pdf"), b"%PDF");
    assert!(h.said().contains("e.example answers with error 404"));
    assert!(h.storage.saved().is_empty());
    assert!(!h.storage.0.exists(&format!("{DIR}/download.tmp")));
    // No answer at all.
    let call = h.go("https://down.example/");
    h.reply(HttpResponse::failed(
        call.id,
        HttpError::Timeout,
        "timed out",
    ));
    assert!(h.said().contains("down.example could not be reached"));
    // A server's own error page is a page to read.
    let call = h.go("https://e.example/gone");
    h.answer(&call, 500, HTML, b"<title>Server error</title><p>Sorry</p>");
    assert_eq!(h.history()[0].0, "Server error");
}

#[test]
fn script_file_and_data_addresses_are_searched_for_not_fetched() {
    let mut h = Harness::new();
    for typed in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "data:text/html,<b>x</b>",
    ] {
        let call = h.go(typed);
        // Only web addresses are fetched: these are words to search for.
        assert!(
            call.url.starts_with("https://html.duckduckgo.com/html/?q="),
            "{typed}: {}",
            call.url
        );
        assert!(!call.url[8..].contains(':'), "{}", call.url);
        h.reply(HttpResponse::failed(
            call.id,
            HttpError::Timeout,
            "timed out",
        ));
        assert!(h.said().contains("could not be reached"), "{typed}");
    }
    assert!(h.storage.saved().is_empty());
    assert!(h.history().is_empty());
}

#[test]
fn late_answers_for_another_address_are_ignored() {
    let mut h = Harness::new();
    let first = h.go("https://first.example/");
    let second = h.go("https://second.example/");
    h.answer(&first, 200, HTML, b"<title>First</title>");
    assert!(h.history().is_empty());
    h.answer(&second, 200, HTML, b"<title>Second</title>");
    assert_eq!(h.history()[0].0, "Second");
}

#[test]
fn without_a_network_nothing_is_asked() {
    let mut h = Harness::build(Shared::default(), None, false);
    h.send(
        "browser.address",
        Typed::new(String::from("a.example"), Params::new()),
    );
    assert!(h.requests().is_empty());
    assert!(h.said().contains("needs a network"));
}

#[test]
fn odd_lines_in_the_lists_are_skipped() {
    let storage = Shared::default();
    storage
        .write(
            HISTORY,
            b"Good\thttps://good.example/\r\nNo address\t\nno tab at all\n\t\n",
        )
        .unwrap();
    storage.write(BOOKMARKS, b"").unwrap();
    let mut h = Harness::build(storage.clone(), None, true);
    let shown = h.said();
    assert!(shown.contains("Good"), "{shown}");
    assert!(!shown.contains("No address"), "{shown}");
    assert!(!shown.contains("no tab"), "{shown}");
    // An empty list of bookmarks stays empty: none to start with.
    assert!(!shown.contains("Wikipedia"), "{shown}");
    // A list that is not UTF-8 cannot be read: not shown, and kept as
    // it is, not written over.
    let odd = b"\xff\xfe\thttps://bytes.example/\n";
    storage.write(HISTORY, odd).unwrap();
    let mut h = Harness::build(storage, None, true);
    assert!(!h.said().contains("bytes.example"));
    let call = h.go("https://h.example/kept");
    h.answer(&call, 200, HTML, b"<title>Kept</title>");
    assert_eq!(h.storage.0.get(HISTORY).unwrap(), odd);
}
