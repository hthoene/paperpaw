//! The rules of the browser: the answers they gave on interface 6 (pinned here
//! from that code, which is gone), the deliberate bounds, and every allocation
//! refused in turn.
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;

use paperpaw_platform::storage::DirEntry;
use paperpaw_sdk::native::{Error, net::MAX_URL};
use paperpaw_sdk::{MemoryStorage, Storage, StorageError};
use proptest::prelude::*;

use crate::fail_alloc::sweep;
use crate::native_store::{
    self as native, BOOKMARKS, DOWNLOAD, Entry, HISTORY, HISTORY_FILE, MAX_ENTRIES, MAX_FILE,
    MAX_TITLE, StoreError,
};
use crate::native_web::{self, KeepError};

/// A storage that never allocates once built: files are slots with room
/// reserved for their name and bytes, so a sweep refuses only the code under
/// test.
struct Slots {
    files: RefCell<Vec<Slot>>,
}

struct Slot {
    name: String,
    data: Vec<u8>,
    used: bool,
}

impl Slots {
    fn new(count: usize, room: usize) -> Self {
        let files = (0..count)
            .map(|_| Slot {
                name: String::with_capacity(64),
                data: Vec::with_capacity(room),
                used: false,
            })
            .collect();
        Self {
            files: RefCell::new(files),
        }
    }

    fn put(&self, name: &str, data: &[u8]) {
        self.write(name, data).unwrap();
    }

    fn clear(&self) {
        for slot in self.files.borrow_mut().iter_mut() {
            slot.used = false;
            slot.data.clear();
        }
    }

    /// Whether the file `name` is there and starts with `prefix` (no
    /// allocation).
    fn starts(&self, name: &str, prefix: &[u8]) -> bool {
        let files = self.files.borrow();
        files
            .iter()
            .any(|s| s.used && s.name == name && s.data.starts_with(prefix))
    }

    fn has(&self, name: &str) -> bool {
        self.files.borrow().iter().any(|s| s.used && s.name == name)
    }
}

impl Storage for Slots {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let files = self.files.borrow();
        let slot = files
            .iter()
            .find(|s| s.used && s.name == path)
            .ok_or(StorageError::NotFound)?;
        let from = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(slot.data.len());
        let n = buf.len().min(slot.data.len() - from);
        buf[..n].copy_from_slice(&slot.data[from..from + n]);
        Ok(n)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        let files = self.files.borrow();
        files
            .iter()
            .find(|s| s.used && s.name == path)
            .map(|s| s.data.len() as u64)
            .ok_or(StorageError::NotFound)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let mut files = self.files.borrow_mut();
        let at = files
            .iter()
            .position(|s| s.used && s.name == path)
            .or_else(|| files.iter().position(|s| !s.used))
            .ok_or(StorageError::Full)?;
        let slot = &mut files[at];
        if data.len() > slot.data.capacity() || path.len() > slot.name.capacity() {
            return Err(StorageError::Full);
        }
        slot.name.clear();
        slot.name.push_str(path);
        slot.data.clear();
        slot.data.extend_from_slice(data);
        slot.used = true;
        Ok(())
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let mut files = self.files.borrow_mut();
        let slot = files
            .iter_mut()
            .find(|s| s.used && s.name == path)
            .ok_or(StorageError::NotFound)?;
        if slot.data.len() + data.len() > slot.data.capacity() {
            return Err(StorageError::Full);
        }
        slot.data.extend_from_slice(data);
        Ok(())
    }

    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Err(StorageError::Unsupported)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        for slot in self.files.borrow_mut().iter_mut() {
            if slot.used && slot.name == path {
                slot.used = false;
                slot.data.clear();
            }
        }
        Ok(())
    }
}

/// The search engine's address, which a search is appended to.
const SEARCH: &str = "https://html.duckduckgo.com/html/?q=";

/// What was typed and the address interface 6 made of it.
fn typed_addresses() -> [(&'static str, String); 24] {
    [
        ("", SEARCH.into()),
        (" ", SEARCH.into()),
        (" de.wikipedia.org ", "https://de.wikipedia.org".into()),
        ("http://a.org/x", "http://a.org/x".into()),
        ("HTTPS://A.org", "HTTPS://A.org".into()),
        ("hTtP://x", "hTtP://x".into()),
        ("Kafka Verwandlung", format!("{SEARCH}Kafka+Verwandlung")),
        ("Größe", format!("{SEARCH}Gr%C3%B6%C3%9Fe")),
        ("localhost", format!("{SEARCH}localhost")),
        ("localhost:8080", "https://localhost:8080".into()),
        ("example.org:8080/x", "https://example.org:8080/x".into()),
        ("note: 8080", format!("{SEARCH}note%3A+8080")),
        (
            "javascript:alert(1)",
            format!("{SEARCH}javascript%3Aalert%281%29"),
        ),
        (
            "data:text/html,x",
            format!("{SEARCH}data%3Atext%2Fhtml%2Cx"),
        ),
        (
            "file:///etc/passwd",
            format!("{SEARCH}file%3A%2F%2F%2Fetc%2Fpasswd"),
        ),
        ("a:b c", format!("{SEARCH}a%3Ab+c")),
        ("...", format!("{SEARCH}...")),
        (".a.b", format!("{SEARCH}.a.b")),
        ("a.b.", format!("{SEARCH}a.b.")),
        ("calibre.local:8080", "https://calibre.local:8080".into()),
        ("a b.c", format!("{SEARCH}a+b.c")),
        ("ünï.de/ü", "https://ünï.de/ü".into()),
        ("http:/x", format!("{SEARCH}http%3A%2Fx")),
        ("https", format!("{SEARCH}https")),
    ]
}

#[test]
fn what_is_typed_becomes_what_it_did() {
    for (typed, address) in typed_addresses() {
        assert_eq!(native_web::address(typed).unwrap(), address, "{typed:?}");
    }
}

#[test]
fn an_address_over_the_longest_a_request_takes_is_not_made() {
    let fits = format!(
        "https://a.org/{}",
        "x".repeat(MAX_URL - "https://a.org/".len())
    );
    assert_eq!(native_web::address(&fits).unwrap(), fits);
    assert_eq!(
        native_web::address(&format!("{fits}x")),
        Err(Error::NoSpace)
    );
    // A search grows when encoded: 400 non-ASCII letters are 2,400 bytes.
    assert_eq!(native_web::address(&"ü".repeat(400)), Err(Error::NoSpace));
    let longest = "ü ".repeat(60);
    assert!(native_web::address(&longest).unwrap().len() <= MAX_URL);
}

/// An EPUB's first bytes: a zip whose first entry is `mimetype`.
const EPUB_HEAD: &[u8] =
    b"PK\x03\x04\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0mimetypeapplication/epub+zip";

#[test]
fn answers_are_told_apart_as_before() {
    use native_web::Kind::{Book, Font, Other, Page};
    let html: &[u8] = b"\n <!DOCTYPE html>";
    let bom_html: &[u8] = b"\xef\xbb\xbf  <html>";
    // (content type, address, first bytes) and what interface 6 made of it.
    let cases: [(Option<&str>, &str, &[u8], native_web::Kind); 28] = [
        (None, "https://a.org/", b"", Other),
        (None, "https://a.org/", html, Page),
        (None, "https://a.org/b.EPUB", b"OTTO\0\x0a", Book),
        (None, "https://a.org/get?id=3", b"", Other),
        (None, "https://a.org/get?id=3", html, Page),
        (None, "https://a.org/get?id=3", EPUB_HEAD, Book),
        (None, "https://a.org/a.ttf", b"OTTO\0\x0a", Font),
        (None, "https://a.org/dir/x.TTC?y=1", b"", Font),
        (None, "https://a.org/x.html#z", b"ttcf", Page),
        (None, "https://a.org/x%2Eepub", b"", Book),
        (None, "https://a.org/x.epub.", b"", Book),
        (None, "https://a.org/.txt", b"", Other),
        (None, "https://a.org/.txt", b"ttcf", Font),
        (None, "https://a.org/.txt", bom_html, Page),
        (None, "https://a.org/x.pdf", b"%PDF-1.4", Other),
        (Some(""), "https://a.org/", b"", Other),
        (
            Some("text/html; charset=utf-8"),
            "https://a.org/",
            b"",
            Page,
        ),
        (
            Some("text/html; charset=utf-8"),
            "https://a.org/b.EPUB",
            b"",
            Page,
        ),
        (Some("TEXT/HTML"), "https://a.org/a.ttf", b"", Page),
        (Some(" application/epub+zip "), "https://a.org/", b"", Book),
        (Some("text/plain"), "https://a.org/x.pdf", b"", Book),
        (Some("font/ttf"), "https://a.org/b.EPUB", b"", Font),
        (
            Some("Application/X-Font-OTF;x"),
            "https://a.org/",
            html,
            Font,
        ),
        (
            Some("application/octet-stream"),
            "https://a.org/get?id=3",
            b"",
            Other,
        ),
        (
            Some("application/octet-stream"),
            "https://a.org/get?id=3",
            EPUB_HEAD,
            Book,
        ),
        (Some("application/pdf"), "https://a.org/x.pdf", b"", Other),
        (Some(";"), "https://a.org/get?id=3", b"", Other),
        (Some("\u{e9}"), "https://a.org/get?id=3", b"", Other),
    ];
    for (mime, url, head, kind) in cases {
        assert_eq!(
            native_web::kind(mime, url, head).unwrap(),
            kind,
            "{mime:?} {url:?} {head:?}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    #[test]
    fn anything_typed_is_an_address_or_too_long(input in ".{0,300}") {
        match native_web::address(&input) {
            Ok(url) => {
                prop_assert!(url.len() <= MAX_URL);
                prop_assert!(url.to_ascii_lowercase().starts_with("http"), "{}", url);
            }
            Err(error) => prop_assert_eq!(error, Error::NoSpace),
        }
    }

    #[test]
    fn any_answer_is_told_apart_without_failing(
        mime in prop::option::of(".{0,30}"),
        url in ".{0,60}",
        head in prop::collection::vec(any::<u8>(), 0..64),
    ) {
        prop_assert!(native_web::kind(mime.as_deref(), &url, &head).is_ok());
    }

    #[test]
    fn any_page_keeps_its_address_first(url in ".{0,80}") {
        let storage = MemoryStorage::new().with("tmp", b"<p>x</p>".to_vec());
        native_web::keep_page(&storage, "tmp", "page.html", &url).unwrap();
        let kept = String::from_utf8(storage.get("page.html").unwrap()).unwrap();
        prop_assert!(kept.starts_with("<base href=\""));
        prop_assert!(kept.ends_with("\">\n<p>x</p>"));
        prop_assert!(!storage.exists("tmp"));
    }

    #[test]
    fn page_names_are_a_hash_and_an_extension(url in ".{0,80}") {
        let name = native_web::page_name(&url).unwrap();
        prop_assert_eq!(name.len(), 16 + ".html".len());
        prop_assert!(name.ends_with(".html"));
    }
}

#[test]
fn pages_are_kept_and_named_as_before() {
    for (url, base, name) in [
        ("https://a.org/", "https://a.org/", "584676589f49c1ae.html"),
        ("", "", "cbf29ce484222325.html"),
        (
            "https://a.org/?a=1&b=\"2\"",
            "https://a.org/?a=1&amp;b=&quot;2&quot;",
            "02149bbd72761c4f.html",
        ),
        ("\u{fc}", "\u{fc}", "0ac20a07b71807ea.html"),
    ] {
        let storage = MemoryStorage::new().with("tmp", b"<p>x</p>".to_vec());
        native_web::keep_page(&storage, "tmp", "page.html", url).unwrap();
        assert_eq!(
            storage.get("page.html"),
            Some(format!("<base href=\"{base}\">\n<p>x</p>").into_bytes()),
            "{url:?}"
        );
        assert_eq!(native_web::page_name(url).unwrap(), name, "{url:?}");
        assert_eq!(
            native::page_path(url).unwrap(),
            format!("pages/{name}"),
            "{url:?}"
        );
    }
}

#[test]
fn titles_are_what_they_were() {
    let pages: [&[u8]; 6] = [
        b"<html><head><title> Ein\n Titel </title></head><body>x</body></html>",
        b"<TITLE>A &amp; B &#x41; &unknown;</TITLE>",
        b"<svg:title>Svg</svg:title><title>Real</title>",
        b"<title>   </title>",
        b"<title>cut",
        b"no title <p>here",
    ];
    let titles = [
        Some("Ein Titel"),
        Some("A & B A &unknown;"),
        Some("Svg"),
        None,
        Some("cut"),
        None,
    ];
    for (page, title) in pages.into_iter().zip(titles) {
        let storage = MemoryStorage::new().with("p.html", page.to_vec());
        assert_eq!(
            native_web::title(&storage, "p.html").unwrap().as_deref(),
            title,
            "{}",
            String::from_utf8_lossy(page)
        );
    }
    let storage = MemoryStorage::new();
    assert_eq!(native_web::title(&storage, "missing.html").unwrap(), None);
    // As before, a title far into a page is not looked for.
    let late = format!("{}<title>Late</title>", " ".repeat(20_000));
    let storage = MemoryStorage::new().with("p.html", late);
    assert_eq!(native_web::title(&storage, "p.html").unwrap(), None);
}

#[test]
fn a_page_that_cannot_be_kept_is_an_error() {
    let storage = MemoryStorage::new();
    assert_eq!(
        native_web::keep_page(&storage, "missing", "page.html", "https://a.org/"),
        Err(KeepError::Storage(StorageError::NotFound))
    );
    let storage = MemoryStorage::new().with("tmp", b"x".to_vec());
    assert!(native_web::keep_page(&storage, "tmp", "../page.html", "https://a.org/").is_err());
}

fn entry(title: &str, url: &str) -> Entry {
    Entry::new(title, url).unwrap()
}

#[test]
fn lists_are_written_and_read_as_before() {
    let entries = [
        entry("Wikipedia", "https://de.m.wikipedia.org/"),
        entry("A\tB\nC\r", "https://a.org/?x=1\ty"),
        entry("", "https://empty-title.org/"),
    ];
    let two = MemoryStorage::new();
    native::save(&two, "l.txt", &entries).unwrap();
    // Tabs and line ends in a title or address are written as spaces, as on
    // interface 6.
    assert_eq!(
        two.get("l.txt"),
        Some(
            b"Wikipedia\thttps://de.m.wikipedia.org/\nA B C \thttps://a.org/?x=1 y\n\thttps://empty-title.org/\n"
                .to_vec()
        )
    );
    let read = native::load(&two, "l.txt").unwrap().unwrap();
    assert_eq!(read.len(), 3);
    assert_eq!(read[1], entry("A B C ", "https://a.org/?x=1 y"));
    // What version 1 wrote stays readable, with a byte order mark too.
    for (v1, count) in [
        (include_str!("../testdata/bookmarks.txt.v1"), 2),
        (include_str!("../testdata/history.txt.v1"), 3),
    ] {
        let storage = MemoryStorage::new()
            .with("a.txt", v1)
            .with("b.txt", format!("\u{feff}{v1}"));
        for name in ["a.txt", "b.txt"] {
            let list = native::load(&storage, name).unwrap().unwrap();
            assert_eq!(list.len(), count);
            native::save(&storage, "again.txt", &list).unwrap();
            assert_eq!(storage.get("again.txt"), Some(v1.as_bytes().to_vec()));
        }
    }
    assert_eq!(native::load(&two, "none.txt"), Ok(None));
    assert_eq!(
        native::load(&two, "../bad"),
        Err(StoreError::Storage(StorageError::InvalidPath))
    );
    // Not text: not read, so not written over.
    let storage = MemoryStorage::new().with("l.txt", vec![0xff, 0xfe, b'\t', b'x']);
    assert_eq!(
        native::load(&storage, "l.txt"),
        Err(StoreError::Storage(StorageError::InvalidData))
    );
}

#[test]
fn lists_are_bounded() {
    // Too big a file, too many entries: unreadable, never cut and written back.
    let big = "t\thttps://a.org/\n".repeat(MAX_FILE / 16 + 1);
    let storage = MemoryStorage::new().with("l.txt", big);
    assert_eq!(
        native::load(&storage, "l.txt"),
        Err(StoreError::Storage(StorageError::TooLarge))
    );
    let many = "t\tu\n".repeat(MAX_ENTRIES + 1);
    let storage = MemoryStorage::new().with("l.txt", many.clone());
    assert_eq!(
        native::load(&storage, "l.txt"),
        Err(StoreError::Storage(StorageError::TooLarge))
    );
    let storage = MemoryStorage::new().with("l.txt", &many["t\tu\n".len()..]);
    assert_eq!(
        native::load(&storage, "l.txt").unwrap().unwrap().len(),
        MAX_ENTRIES
    );
    // An address no request can carry is dropped, a title is cut on a character.
    let long = format!("t\thttps://a.org/{}\n", "x".repeat(MAX_URL));
    let ok = format!("{}\thttps://b.org/\n", "é".repeat(MAX_TITLE));
    let storage = MemoryStorage::new().with("l.txt", format!("{long}{ok}"));
    let list = native::load(&storage, "l.txt").unwrap().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].title.len(), MAX_TITLE);
    assert!(list[0].title.chars().all(|c| c == 'é'));
}

#[test]
fn the_history_keeps_the_latest_pages_as_before() {
    let new = MemoryStorage::new();
    for n in 0..HISTORY + 2 {
        let url = format!("https://a.org/{n}");
        new.write(&native::page_path(&url).unwrap(), b"page")
            .unwrap();
        native::remember(&new, entry("A\tB", &url))
            .unwrap()
            .unwrap();
    }
    // Visited again: first, once.
    let again = native::remember(&new, entry("Again", "https://a.org/5"))
        .unwrap()
        .unwrap();
    assert_eq!(again.len(), HISTORY);
    assert_eq!(again[0], entry("Again", "https://a.org/5"));
    assert_eq!(
        again.iter().filter(|e| e.url == "https://a.org/5").count(),
        1
    );
    // The oldest pages are gone.
    for (n, kept) in [(0, false), (1, false), (2, true), (31, true)] {
        let url = format!("https://a.org/{n}");
        assert_eq!(new.exists(&native::page_path(&url).unwrap()), kept, "{url}");
    }
    let loaded = native::load(&new, HISTORY_FILE).unwrap().unwrap();
    assert_eq!(loaded, again);
    assert_eq!(loaded[1].title, "A B");
    // A history that cannot be read is left as it is.
    let unreadable = MemoryStorage::new().with(HISTORY_FILE, vec![0xff]);
    assert_eq!(
        native::remember(&unreadable, entry("A", "https://a.org/")),
        Ok(None)
    );
    assert_eq!(unreadable.get(HISTORY_FILE), Some(vec![0xff]));
}

#[test]
fn the_bookmarks_start_with_a_few_places_in_the_language() {
    let storage = MemoryStorage::new();
    for (language, host) in [("de", "de"), ("de-AT", "de"), ("en_GB", "en"), ("", "")] {
        let list = native::bookmarks(&storage, language).unwrap().unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].url, format!("https://{host}.m.wikipedia.org/"));
        assert_eq!(list[1].url, "https://www.gutenberg.org/");
    }
    // Saved ones are shown instead; unreadable ones are not shown at all.
    storage.write(BOOKMARKS, b"A\thttps://a.org/\n").unwrap();
    assert_eq!(native::bookmarks(&storage, "de").unwrap().unwrap().len(), 1);
    storage.write(BOOKMARKS, &[0xff]).unwrap();
    assert_eq!(native::bookmarks(&storage, "de"), Ok(None));
}

/// Maps the errors of this plugin to the sweep's.
fn memory<T, E: core::fmt::Debug>(
    result: Result<T, E>,
    is_memory: impl Fn(&E) -> bool,
) -> Result<T, Error> {
    result.map_err(|e| {
        assert!(is_memory(&e), "{e:?}");
        Error::NoMemory
    })
}

#[test]
fn refused_allocations_end_in_no_memory_and_recover() {
    for (typed, _) in typed_addresses() {
        sweep(|| native_web::address(typed).map(|url| url.len()));
    }
    for (mime, url, head) in [
        (None, "https://a.org/get?id=3", &b"x"[..]),
        (
            Some("application/octet-stream"),
            "https://a.org/a%2Ettf?x#y",
            &b""[..],
        ),
        (Some("text/html"), "https://a.org/", &b""[..]),
    ] {
        sweep(|| native_web::kind(mime, url, head).map(|kind| kind as usize));
    }
    let slots = Slots::new(4, 40 * 1024);
    let page = b"<html><head><title> A &amp;\n B </title></head><body>x</body></html>";
    sweep(|| {
        slots.clear();
        slots.put("p.html", page);
        native_web::title(&slots, "p.html").map(|title| title.map(|t| t.len()))
    });
    sweep(|| {
        slots.clear();
        slots.put(DOWNLOAD, page);
        let kept = native_web::keep_page(
            &slots,
            DOWNLOAD,
            "pages/x.html",
            "https://a.org/?a=1&b=\"2\"",
        );
        memory(kept, |e| *e == KeepError::NoMemory)?;
        assert!(slots.starts("pages/x.html", b"<base href="));
        assert!(!slots.has(DOWNLOAD));
        Ok(())
    });
    sweep(|| native_web::page_name("https://a.org/").map(|name| name.len()));
    sweep(|| native::page_path("https://a.org/").map(|path| path.len()));
    let lists = b"A\thttps://a.org/\nB \xc3\xa4\thttps://b.org/x?y=\xc3\xa4\n";
    sweep(|| {
        slots.clear();
        slots.put(BOOKMARKS, lists);
        let list = memory(native::load(&slots, BOOKMARKS), |e| {
            *e == StoreError::NoMemory
        })?;
        Ok(list.map(|l| l.len()))
    });
    sweep(|| {
        let entries = [
            Entry::new("A\tB", "https://a.org/")?,
            Entry::new("C", "https://c.org/")?,
        ];
        slots.clear();
        memory(native::save(&slots, "l.txt", &entries), |e| {
            *e == StoreError::NoMemory
        })
    });
    sweep(|| {
        slots.clear();
        let list = memory(native::bookmarks(&slots, "de"), |e| {
            *e == StoreError::NoMemory
        })?;
        Ok(list.map(|l| l.len()))
    });
    // The history, full: the oldest page files go and the list is written.
    let mut full = String::new();
    for n in 0..HISTORY {
        full.push_str(&format!("T{n}\thttps://a.org/{n}\n"));
    }
    sweep(|| {
        slots.clear();
        slots.put(HISTORY_FILE, full.as_bytes());
        let history = memory(
            native::remember(&slots, Entry::new("New", "https://new.org/")?),
            |e| *e == StoreError::NoMemory,
        )?;
        Ok(history.map(|h| h.len()))
    });
    sweep(|| Entry::new("é", "https://a.org/").map(|e| e.title.len()));
}

/// The list is written before the dropped pages' files go: a refused write
/// leaves every file where it was.
#[test]
fn the_history_is_saved_before_old_pages_are_removed() {
    let mut history = String::new();
    for n in 0..HISTORY {
        history.push_str(&format!("T\tu{n}\n"));
    }
    let slots = Slots::new(4, history.len() + 8);
    slots.put(HISTORY_FILE, history.as_bytes());
    let oldest = native::page_path(&format!("u{}", HISTORY - 1)).unwrap();
    slots.put(&oldest, b"page");
    // The new line does not fit the room the file has: the write is refused.
    let result = native::remember(
        &slots,
        entry("A longer title", "https://new.example.org/path"),
    );
    assert_eq!(result, Err(StoreError::Storage(StorageError::Full)));
    assert!(slots.has(&oldest));
    assert!(slots.starts(HISTORY_FILE, b"T\tu0\n"));
    // Where it fits, the oldest page goes.
    let result = native::remember(&slots, entry("", "u")).unwrap().unwrap();
    assert_eq!(result.len(), HISTORY);
    assert!(!slots.has(&oldest));
}
