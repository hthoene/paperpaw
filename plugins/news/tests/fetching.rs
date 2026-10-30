//! Fetching feeds with the real kernel, the servers' answers canned: each
//! feed becomes a book in `News` named after its title and marked with
//! its address, failures (and answers that are no feed) are counted and
//! the rest fetched, `feeds.txt` and the headlines stay one
//! line a feed whatever the titles hold, addresses are checked when
//! added, removing a feed keeps its book, and hostile feeds (scripts,
//! `javascript:` links, entity bombs, deep nesting, traversal in titles)
//! give plain, safe pages.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::{HttpCall, HttpResponse, Io, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_plugin_news::{FETCH, News, OPEN};
use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::ui::choice::Picked;
use paperpaw_sdk::{
    Canvas, Cx, DeviceInfo, DirEntry, DisplayInfo, Env, Handled, Id, InputCaps, Instant, Manifest,
    MemoryClass, MemoryStorage, Params, PixelFormat, Plugin, Priority, RegisterError, Registrar,
    Screen, ScreenSpec, Storage, StorageError, Version,
};

const fn device(memory: MemoryClass) -> DeviceInfo {
    DeviceInfo::new(
        "test",
        "Test device",
        DisplayInfo::new(480, 800, 220, PixelFormat::Gray4, Rotation::Deg0, None),
        InputCaps::new(true, false, false, &[]),
        memory,
    )
}

const FEEDS: &str = ".paperpaw/plugins/news/feeds.txt";
const HEADLINES: &str = ".paperpaw/plugins/news/headlines.txt";

/// A storage the test looks into while the kernel has it.
#[derive(Clone, Default)]
struct Shared(Rc<MemoryStorage>);

impl Shared {
    fn text(&self, path: &str) -> Option<String> {
        self.0
            .get(path)
            .map(|d| String::from_utf8_lossy(&d).into_owned())
    }

    /// The files in `dir`.
    fn names(&self, dir: &str) -> Vec<String> {
        let mut names = Vec::new();
        let _ = self.0.list(dir, &mut |e| names.push(e.name.to_string()));
        names.sort();
        names
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

/// Takes the place of the notice, to hear what the plugin tells.
struct Notices(Rc<RefCell<Vec<String>>>);

struct Silent;

impl Screen for Silent {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

impl Plugin for Notices {
    fn manifest(&self) -> Manifest {
        Manifest::new(
            Id::new_static("test-notices"),
            "Notices",
            Version::new(0, 1, 0),
        )
        .description("Keeps the notices for the test.")
        .uses(paperpaw_sdk::Uses::NETWORK)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        let screen = Id::new_static("test-notices.notice");
        let heard = self.0.clone();
        r.add_screen(ScreenSpec::new(
            screen.clone(),
            "notice",
            Box::new(move |_, p| {
                heard
                    .borrow_mut()
                    .push(p.get(params::TEXT).unwrap_or_default().to_string());
                Box::new(Silent)
            }),
        ))?;
        r.bind_slot(slots::NOTICE, screen, Priority::PREFERRED)
    }
}

struct Harness {
    kernel: Kernel,
    storage: Shared,
    notices: Rc<RefCell<Vec<String>>>,
    now: u64,
}

impl Harness {
    fn new(feeds: &str) -> Self {
        Self::build(feeds, MemoryClass::Large, "", true)
    }

    fn build(feeds: &str, memory: MemoryClass, config: &str, network: bool) -> Self {
        let storage = Shared::default();
        if !feeds.is_empty() {
            storage.write(FEEDS, feeds.as_bytes()).unwrap();
        }
        let notices = Rc::new(RefCell::new(Vec::new()));
        let mut kernel = Kernel::builder(device(memory))
            .storage(storage.clone())
            .user_config(format!("[core]\nlanguage = en\nlibrary-folder =\n{config}"))
            .network(network)
            .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
            .plugin(News)
            .plugin(Notices(notices.clone()))
            .build()
            .unwrap();
        kernel.start(Instant(0));
        Self {
            kernel,
            storage,
            notices,
            now: 0,
        }
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

    fn answer(&mut self, call: &HttpCall, status: u16, body: &str) {
        let now = self.tick();
        self.kernel.http_response(
            now,
            HttpResponse {
                id: call.id,
                status,
                body: body.as_bytes().to_vec(),
                ..HttpResponse::default()
            },
        );
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

    /// The last notice the plugin gave.
    fn told(&self) -> String {
        self.notices.borrow().last().cloned().unwrap_or_default()
    }

    /// The names of the books in `News`, without the marks of their
    /// feeds' addresses (each has one).
    fn books(&self) -> Vec<String> {
        self.storage
            .names("News")
            .iter()
            .map(|file| {
                let name = without_mark(file);
                assert!(name.is_some(), "{file} has no mark");
                name.unwrap_or_default().to_string()
            })
            .collect()
    }

    /// The book of the feed named `name`: `News/<name> (<mark>).html`.
    fn book(&self, name: &str) -> String {
        let names = self.storage.names("News");
        let found: Vec<&String> = names
            .iter()
            .filter(|file| without_mark(file) == Some(name))
            .collect();
        assert_eq!(found.len(), 1, "{name} in {names:?}");
        format!("News/{}", found[0])
    }

    fn page(&self, name: &str) -> String {
        self.storage.text(&self.book(name)).unwrap()
    }
}

/// A book's file name without `.html` and the mark of its feed's address,
/// six hex digits (`Daily Paper (3fa9c2).html` is "Daily Paper"); `None`
/// without a mark.
fn without_mark(file: &str) -> Option<&str> {
    let (name, mark) = file.strip_suffix(").html")?.rsplit_once(" (")?;
    (mark.len() == 6 && mark.bytes().all(|b| b.is_ascii_hexdigit())).then_some(name)
}

fn rss(title: &str, items: &[(&str, &str)]) -> String {
    let items: String = items
        .iter()
        .map(|(t, d)| format!("<item><title>{t}</title><description>{d}</description></item>"))
        .collect();
    format!(
        r#"<?xml version="1.0"?><rss version="2.0"><channel><title>{title}</title>{items}</channel></rss>"#
    )
}

fn atom(title: &str, entries: &[&str]) -> String {
    let entries: String = entries
        .iter()
        .map(|t| format!("<entry><title>{t}</title><summary>About {t}</summary></entry>"))
        .collect();
    format!(r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>{title}</title>{entries}</feed>"#)
}

fn typed(text: &str) -> Typed {
    Typed::new(String::from(text), Params::new())
}

#[test]
fn every_feed_becomes_a_book_named_after_its_title() {
    let mut h = Harness::new("https://a.example/rss\t\nhttps://b.example/atom\tMy name\n");
    h.run(FETCH);
    let first = h.request();
    assert_eq!(first.url, "https://a.example/rss");
    assert!(first.save_to.is_some());
    assert!(first.headers.iter().any(|(k, _)| k == "User-Agent"));
    h.answer(
        &first,
        200,
        &rss("Daily Paper - all the news", &[("Budget agreed", "Text")]),
    );
    let second = h.request();
    assert_eq!(second.url, "https://b.example/atom");
    h.answer(&second, 200, &atom("Atom title", &["Storm", "Moon"]));
    assert!(h.requests().is_empty());
    assert_eq!(h.told(), "2 feeds fetched.");
    // The first named after its feed's title, the second keeps its name;
    // each marked with its address.
    assert_eq!(h.books(), ["Daily Paper", "My name"]);
    assert!(h.page("Daily Paper").contains("<h2>Budget agreed</h2>"));
    assert!(h.page("My name").contains("<h2>Moon</h2>"));
    assert_eq!(
        h.storage.text(FEEDS).unwrap(),
        "https://a.example/rss\tDaily Paper\nhttps://b.example/atom\tMy name\n"
    );
    // The headlines of the feed fetched last come first.
    let headlines = h.storage.text(HEADLINES).unwrap();
    let (mine, daily) = (h.book("My name"), h.book("Daily Paper"));
    assert_eq!(
        headlines,
        format!("Storm\t{mine}\nMoon\t{mine}\nBudget agreed\t{daily}")
    );
    // What was loading is not left behind.
    assert_eq!(
        h.storage.names(".paperpaw/plugins/news"),
        ["feeds.txt", "headlines.txt"]
    );
}

#[test]
fn failures_are_counted_and_the_rest_fetched() {
    let mut h =
        Harness::new("https://a.example/1\tA\nhttps://b.example/2\tB\nhttps://c.example/3\tC\n");
    h.run(FETCH);
    let a = h.request();
    h.answer(&a, 404, "Not found");
    let b = h.request();
    h.answer(&b, 0, "timed out");
    let c = h.request();
    h.answer(&c, 200, &rss("C", &[("One", "x")]));
    assert!(h.requests().is_empty());
    assert_eq!(h.books(), ["C"]);
    assert_eq!(h.told(), "1 fetched, 2 could not be fetched.");
}

#[test]
fn a_failed_fetch_keeps_the_book_of_the_last_one() {
    let mut h = Harness::new("https://a.example/1\tA\n");
    h.run(FETCH);
    let call = h.request();
    h.answer(&call, 200, &rss("A", &[("Good news", "x")]));
    h.run(FETCH);
    let call = h.request();
    h.answer(&call, 500, "Internal Server Error");
    assert!(h.page("A").contains("Good news"));
}

#[test]
fn an_answer_that_is_no_feed_keeps_the_book_of_the_last_one() {
    let mut h = Harness::new("https://a.example/1\tA\n");
    h.run(FETCH);
    let call = h.request();
    h.answer(&call, 200, &rss("A", &[("Good news", "x")]));
    // A hotel's Wi-Fi answers every address with its login page.
    h.run(FETCH);
    let call = h.request();
    h.answer(
        &call,
        200,
        "<html><head><title>Log in</title></head><body><form>Room number</form></body></html>",
    );
    assert!(h.page("A").contains("Good news"), "{}", h.page("A"));
    assert!(h.told().contains("could not be fetched"), "{}", h.told());
}

#[test]
fn feeds_with_the_same_short_title_keep_books_of_their_own() {
    let mut h = Harness::new("https://spiegel.example/a\t\nhttps://spiegel.example/b\t\n");
    h.run(FETCH);
    let a = h.request();
    h.answer(
        &a,
        200,
        &rss("DER SPIEGEL - Schlagzeilen", &[("Headline", "x")]),
    );
    let b = h.request();
    h.answer(&b, 200, &rss("DER SPIEGEL - Politik", &[("Politics", "y")]));
    assert_eq!(
        h.storage.names("News").len(),
        2,
        "{:?}",
        h.storage.names("News")
    );
}

#[test]
fn hostile_titles_stay_one_line_and_inside_the_news_folder() {
    let mut h = Harness::new("https://evil.example/feed\t\n");
    h.run(FETCH);
    let call = h.request();
    let feed = rss(
        "../../../.paperpaw/secrets\tx&#10;y",
        &[("Tab\there&#13;&#10;and\tnewline", "x")],
    );
    h.answer(&call, 200, &feed);
    let names = h.storage.names("News");
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(!names[0].starts_with('.'), "{names:?}");
    assert!(!names[0].contains(['/', '\\']), "{names:?}");
    assert!(h.storage.0.get(".paperpaw/secrets.html").is_none());
    // feeds.txt keeps one line, two fields.
    let feeds = h.storage.text(FEEDS).unwrap();
    assert_eq!(feeds.lines().count(), 1, "{feeds:?}");
    assert_eq!(
        feeds.lines().next().unwrap().split('\t').count(),
        2,
        "{feeds:?}"
    );
    let headlines = h.storage.text(HEADLINES).unwrap();
    assert_eq!(headlines.lines().count(), 1, "{headlines:?}");
    assert_eq!(headlines.split('\t').count(), 2, "{headlines:?}");
}

#[test]
fn hostile_articles_become_plain_text() {
    let mut h = Harness::new("https://evil.example/feed\tEvil\n");
    h.run(FETCH);
    let call = h.request();
    let content = "&lt;script&gt;alert(1)&lt;/script&gt;&lt;a href=\"javascript:alert(2)\"&gt;click&lt;/a&gt;\
        &lt;img src=x onerror=alert(3)&gt;&lt;iframe src=\"https://evil.example\"&gt;&lt;/iframe&gt;";
    let deep = format!(
        "{}deep{}",
        "&lt;div&gt;".repeat(5000),
        "&lt;/div&gt;".repeat(5000)
    );
    let feed = format!(
        r#"<!DOCTYPE rss [<!ENTITY a "aaaaaaaaaa"><!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;">]>
<rss><channel><title>Evil &b;</title>
<item><title>&lt;b onclick="x"&gt;Bold&lt;/b&gt; &b;</title><description>{content}</description></item>
<item><title>Deep</title><description>{deep}</description></item>
<item><title>Inline</title><content:encoded><![CDATA[<script>alert(4)</script><p>Kept</p><style>p{{}}</style>]]></content:encoded></item>
</channel></rss>"#
    );
    h.answer(&call, 200, &feed);
    let page = h.page("Evil");
    for bad in [
        "<script",
        "<a ",
        "<img",
        "<iframe",
        "onclick",
        "javascript:",
        "alert(1)",
        "alert(4)",
    ] {
        assert!(!page.contains(bad), "{bad}: {page}");
    }
    assert!(page.contains("click"), "{page}");
    assert!(page.contains("<p>deep</p>"), "{page}");
    assert!(page.contains("<p>Kept</p>"), "{page}");
    assert!(page.len() < 10_000, "{}", page.len());
    assert!(page.ends_with("</body>\n</html>\n"));
}

#[test]
fn the_number_of_articles_and_their_text_are_limited() {
    let items: Vec<(String, String)> = (0..40)
        .map(|n| (format!("Item {n}"), "word ".repeat(5000)))
        .collect();
    let items: Vec<(&str, &str)> = items
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let feed = rss("Many", &items);
    // Five articles; on a small device little text each.
    let mut h = Harness::build(
        "https://m.example/\tMany\n",
        MemoryClass::Small,
        "[news]\narticles = 5\n",
        true,
    );
    h.run(FETCH);
    let call = h.request();
    h.answer(&call, 200, &feed);
    let page = h.page("Many");
    assert_eq!(page.matches("<h2>").count(), 5);
    assert!(page.len() < 5 * 10 * 1024, "{}", page.len());
    // More on a large device.
    let mut h = Harness::new("https://m.example/\tMany\n");
    h.run(FETCH);
    let call = h.request();
    h.answer(&call, 200, &feed);
    let page = h.page("Many");
    assert_eq!(page.matches("<h2>").count(), 30, "the default");
    assert!(page.len() > 5 * 10 * 1024, "{}", page.len());
}

#[test]
fn nothing_is_fetched_without_feeds_or_network() {
    let mut h = Harness::new("");
    h.run(FETCH);
    assert!(h.requests().is_empty());
    assert_eq!(h.told(), "There are no feeds to fetch.");
    let mut h = Harness::build("https://a.example/\tA\n", MemoryClass::Large, "", false);
    h.run(FETCH);
    assert!(h.requests().is_empty());
    assert_eq!(h.told(), "Fetching the news needs a network.");
}

#[test]
fn addresses_are_checked_when_added() {
    let mut h = Harness::new("");
    h.run(OPEN);
    for bad in [
        "",
        "news.example/rss",
        "javascript:alert(1)",
        "file:///etc/passwd",
        "ftp://files.example/feed",
        "http://a",
    ] {
        h.send("news.added", typed(bad));
        assert!(h.requests().is_empty(), "{bad}");
        assert_eq!(h.told(), "That is no web address.", "{bad}");
    }
    assert!(h.storage.text(FEEDS).is_none());
    // A good one is kept and fetched at once, once.
    h.send("news.added", typed("  https://good.example/rss  "));
    let call = h.request();
    assert_eq!(call.url, "https://good.example/rss");
    h.answer(&call, 200, &rss("Good", &[("Hello", "x")]));
    h.send("news.added", typed("https://good.example/rss"));
    let again = h.request();
    h.answer(&again, 200, &rss("Good", &[("Hello", "x")]));
    assert_eq!(
        h.storage.text(FEEDS).unwrap(),
        "https://good.example/rss\tGood\n"
    );
    assert!(h.said().contains("Good"));
}

/// A file of the reader's own at the name version 1 gave a feed's book
/// is not taken for that book: a fetch leaves it.
#[test]
fn a_file_of_the_feeds_old_name_stays_unless_news_wrote_it() {
    let mut h = Harness::new("https://a.example/1\tA\n");
    h.storage.write("News/A.html", b"<p>Mine</p>").unwrap();
    h.run(FETCH);
    let call = h.request();
    h.answer(&call, 200, &rss("A", &[("Good news", "x")]));
    assert!(h.page("A").contains("Good news"));
    assert_eq!(
        h.storage.text("News/A.html").as_deref(),
        Some("<p>Mine</p>")
    );
}

#[test]
fn removing_a_feed_keeps_its_book() {
    let mut h = Harness::new("https://a.example/\tA\nhttps://b.example/\tB\n");
    h.storage.write("News/A.html", b"<p>a</p>").unwrap();
    h.storage.write("News/B.html", b"<p>b</p>").unwrap();
    h.run(OPEN);
    // Choices for feeds that are not there do nothing.
    for (index, option) in [
        ("9", "remove"),
        ("x", "remove"),
        ("-1", "fetch"),
        ("0", "dance"),
    ] {
        h.send(
            "news.chosen",
            Picked::new(String::from(index), String::from(option)),
        );
    }
    assert!(h.requests().is_empty());
    assert_eq!(h.storage.names("News").len(), 2);
    h.send(
        "news.chosen",
        Picked::new(String::from("0"), String::from("remove")),
    );
    assert_eq!(h.storage.text(FEEDS).unwrap(), "https://b.example/\tB\n");
    // Its book stays, a book like any other now.
    assert_eq!(h.storage.names("News"), ["A.html", "B.html"]);
    // *Fetch* of the one left asks for it alone.
    h.send(
        "news.chosen",
        Picked::new(String::from("0"), String::from("fetch")),
    );
    let call = h.request();
    assert_eq!(call.url, "https://b.example/");
    h.answer(&call, 200, &rss("B", &[("x", "y")]));
    assert!(h.requests().is_empty());
}

#[test]
fn feeds_removed_while_fetching_end_the_round() {
    let mut h = Harness::new("https://a.example/\tA\nhttps://b.example/\tB\n");
    h.run(OPEN);
    h.run(FETCH);
    let a = h.request();
    // Both removed before the answer comes.
    for index in ["1", "0"] {
        h.send(
            "news.chosen",
            Picked::new(String::from(index), String::from("remove")),
        );
    }
    assert_eq!(h.storage.text(FEEDS).as_deref(), Some(""));
    h.answer(&a, 200, &rss("A", &[("x", "y")]));
    let calls = h.requests();
    assert!(calls.is_empty(), "{calls:?}");
}

#[test]
fn odd_lines_in_the_list_of_feeds_are_read_as_well_as_they_can() {
    let mut text = Vec::new();
    text.extend_from_slice(b"https://a.example/\tA\r\n");
    text.extend_from_slice(b"\t\t\n   \n");
    text.extend_from_slice(b"https://b.example/ \t B \xff\xfe \n");
    text.extend_from_slice(b"https://c.example/\n");
    let storage_text = String::from_utf8_lossy(&text).into_owned();
    let mut h = Harness::new(&storage_text);
    h.run(OPEN);
    let shown = h.said();
    assert!(shown.contains("A"), "{shown}");
    assert!(shown.contains("c.example"), "{shown}");
    h.run(FETCH);
    let mut urls = Vec::new();
    while let Some(call) = h.requests().pop() {
        urls.push(call.url.clone());
        h.answer(&call, 404, "");
    }
    assert_eq!(
        urls,
        [
            "https://a.example/",
            "https://b.example/",
            "https://c.example/"
        ]
    );
}
