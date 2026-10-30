//! Wallabag with the real kernel against the answers a server may give:
//! failed logins, unreachable servers, malformed and hostile lists
//! (odd ids and titles, huge lists), failed downloads and archives tried
//! again, articles read to the end noted from page turns, the account's
//! secrets typed in kept veiled and sent encoded, and `articles.txt` and
//! `finished.txt` kept one line an article.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::RefCell;
use std::rc::Rc;

use paperpaw_core::{HttpCall, HttpResponse, Io, Kernel};
use paperpaw_platform::Rotation;
use paperpaw_plugin_wallabag::{FETCH, PASSWORD, SECRET, Wallabag};
use paperpaw_sdk::builtin::events::Reading;
use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::event::EventData;
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::secrets::{FILE, SecretChange, Secrets};
use paperpaw_sdk::{
    Canvas, Cx, DeviceInfo, DirEntry, DisplayInfo, Env, Handled, Id, InputCaps, Instant, Manifest,
    MemoryClass, MemoryStorage, Params, PixelFormat, Plugin, Priority, RegisterError, Registrar,
    Screen, ScreenSpec, Storage, StorageError, Version,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(200, 300, 100, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Large,
);

const ARTICLES: &str = ".paperpaw/plugins/wallabag/articles.txt";
const FINISHED: &str = ".paperpaw/plugins/wallabag/finished.txt";
const ACCOUNT: &str =
    "[core]\nlanguage = en\n[wallabag]\nserver = wb.example/\nuser = me\nclient-id = cid\n";

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

    fn books(&self) -> Vec<String> {
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
    /// An account set up with password `pw` and client secret `cs`.
    fn new() -> Self {
        let mut secrets = Secrets::default();
        secrets.apply(SecretChange::keep(PASSWORD, "", "pw"));
        secrets.apply(SecretChange::keep(SECRET, "", "cs"));
        let storage = Shared(Rc::new(
            MemoryStorage::new().with(FILE, secrets.encode().into_bytes()),
        ));
        Self::build(storage, ACCOUNT, true)
    }

    fn build(storage: Shared, config: &str, network: bool) -> Self {
        let notices = Rc::new(RefCell::new(Vec::new()));
        let mut kernel = Kernel::builder(DEVICE)
            .storage(storage.clone())
            // The library at the storage's top.
            .user_config(format!("[core]\nlibrary-folder =\n{config}"))
            .network(network)
            .plugin(Wallabag)
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

    fn fetch(&mut self) -> Vec<HttpCall> {
        let now = self.tick();
        self.kernel.run_action(now, FETCH);
        self.requests()
    }

    fn answer(&mut self, call: &HttpCall, status: u16, body: &str) -> Vec<HttpCall> {
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
        self.requests()
    }

    /// A fetch logged in, up to the request for the list.
    fn listing(&mut self) -> HttpCall {
        let calls = self.fetch();
        assert_eq!(calls.len(), 1, "{calls:?}");
        let mut calls = self.answer(&calls[0], 200, r#"{"access_token": "T"}"#);
        assert_eq!(calls.len(), 1, "{calls:?}");
        assert!(
            calls[0].url.contains("/api/entries.json"),
            "{}",
            calls[0].url
        );
        calls.remove(0)
    }

    /// Answers the list with `body`, then every book with `status`; the
    /// addresses of the books asked for.
    fn list_and_download(&mut self, body: &str, status: u16) -> Vec<String> {
        let list = self.listing();
        let mut calls = self.answer(&list, 200, body);
        let mut asked = Vec::new();
        while let Some(call) = calls.pop() {
            asked.push(call.url.clone());
            assert!(asked.len() < 200, "the fetch does not end");
            calls = self.answer(&call, status, "EPUB");
        }
        asked
    }

    fn told(&self) -> String {
        self.notices.borrow().last().cloned().unwrap_or_default()
    }
}

fn items(list: &[(&str, &str)]) -> String {
    let items: Vec<String> = list
        .iter()
        .map(|(id, title)| format!(r#"{{"id": {id}, "title": {title}}}"#))
        .collect();
    format!(r#"{{"_embedded": {{"items": [{}]}}}}"#, items.join(","))
}

#[test]
fn logins_that_fail_say_why_and_fetch_nothing() {
    for (status, body, said) in [
        (
            401,
            r#"{"error": "invalid_client"}"#,
            "did not accept the login",
        ),
        (400, "", "did not accept the login"),
        (
            200,
            r#"{"token_type": "bearer"}"#,
            "did not accept the login",
        ),
        (200, "not json at all", "did not accept the login"),
        (200, r#"{"access_token": 7}"#, "did not accept the login"),
        (0, "timed out", "cannot be reached"),
    ] {
        let mut h = Harness::new();
        let calls = h.fetch();
        assert!(h.told().contains("Fetching"), "{}", h.told());
        let calls = h.answer(&calls[0], status, body);
        assert!(calls.is_empty(), "{status} {body}");
        assert!(h.told().contains(said), "{status} {body}: {}", h.told());
        // And may start again.
        assert_eq!(h.fetch().len(), 1);
    }
}

#[test]
fn nothing_is_asked_before_the_account_is_set_up_or_without_network() {
    let mut h = Harness::build(Shared::default(), ACCOUNT, true);
    assert!(h.fetch().is_empty());
    assert!(h.told().contains("needs the server"), "{}", h.told());
    let mut secrets = Secrets::default();
    secrets.apply(SecretChange::keep(PASSWORD, "", "pw"));
    secrets.apply(SecretChange::keep(SECRET, "", "cs"));
    let storage = Shared(Rc::new(
        MemoryStorage::new().with(FILE, secrets.encode().into_bytes()),
    ));
    let mut h = Harness::build(storage.clone(), "[wallabag]\nserver = wb.example\n", true);
    assert!(h.fetch().is_empty());
    assert!(h.told().contains("needs the server"), "{}", h.told());
    let mut h = Harness::build(storage, ACCOUNT, false);
    assert!(h.fetch().is_empty());
    assert!(h.told().contains("needs a network"), "{}", h.told());
}

#[test]
fn secrets_typed_in_are_kept_veiled_and_sent_encoded() {
    let storage = Shared::default();
    let mut h = Harness::build(storage, "[core]\nlanguage = en\n", true);
    let typed = |field: &'static str, text: &str| {
        Typed::new(String::from(text), Params::new().with("field", field))
    };
    h.send(
        "wallabag.entered",
        typed("server", "  http://wb.local:8080/  "),
    );
    h.send("wallabag.entered", typed("user", "Jo Doe"));
    h.send("wallabag.entered", typed("client", "1_abc"));
    h.send("wallabag.entered", typed("password", " p&ss=w0rd ü+% "));
    h.send("wallabag.entered", typed("secret", "s3cr3t-client"));
    h.send("wallabag.entered", typed("nonsense", "ignored"));
    assert_eq!(h.kernel.secrets().get(PASSWORD), Some("p&ss=w0rd ü+%"));
    assert_eq!(h.kernel.secrets().get(SECRET), Some("s3cr3t-client"));
    // Neither is in any file in clear.
    for path in h.storage.files("") {
        let data = h.storage.0.get(&path).unwrap();
        for secret in [&b"p&ss=w0rd"[..], b"s3cr3t-client"] {
            assert!(
                !data.windows(secret.len()).any(|w| w == secret),
                "{path} holds a secret in clear"
            );
        }
    }
    let calls = h.fetch();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].url, "http://wb.local:8080/oauth/v2/token");
    let body = String::from_utf8(calls[0].body.clone().unwrap()).unwrap();
    assert_eq!(
        body,
        "grant_type=password&client_id=1_abc&client_secret=s3cr3t-client\
         &username=Jo+Doe&password=p%26ss%3Dw0rd+%C3%BC%2B%25"
    );
    // An empty answer (confirmed by mistake) keeps what was kept.
    h.send("wallabag.entered", typed("password", "   "));
    h.send("wallabag.entered", typed("secret", ""));
    assert_eq!(h.kernel.secrets().get(PASSWORD), Some("p&ss=w0rd ü+%"));
    assert_eq!(h.kernel.secrets().get(SECRET), Some("s3cr3t-client"));
}

#[test]
fn hostile_titles_and_ids_stay_inside_the_folder() {
    let mut h = Harness::new();
    let list = items(&[
        ("1", r#""../../../.paperpaw/secrets""#),
        ("2", r#""""#),
        ("\"../../x\"", r#""""#),
        ("\"a/b\"", r#""Slash id""#),
        ("5", r#""C:\\Windows\\win.ini""#),
        ("6", r#""Same""#),
        ("7", r#""Same""#),
        ("8", "null"),
        ("9", r#""\u0000\u001f control""#),
    ]);
    let asked = h.list_and_download(&list, 200);
    assert!(
        asked
            .iter()
            .all(|u| u.starts_with("https://wb.example/api/entries/")),
        "{asked:?}"
    );
    let books = h.storage.books();
    assert!(
        books.iter().all(|p| p.starts_with("Wallabag/")),
        "{books:?}"
    );
    for path in &books {
        let name = &path["Wallabag/".len()..];
        assert!(!name.contains('/') && !name.starts_with('.'), "{path}");
        assert!(name.ends_with(".epub"), "{path}");
    }
    assert!(
        books.contains(&String::from("Wallabag/Same.epub")),
        "{books:?}"
    );
    // The oldest come first: the second of a title gets its id.
    assert!(
        books.contains(&String::from("Wallabag/Same (6).epub")),
        "{books:?}"
    );
    assert!(
        books.contains(&String::from("Wallabag/2.epub")),
        "{books:?}"
    );
    // The list of articles stays one line, two fields an article.
    let articles = h.storage.text(ARTICLES).unwrap();
    for line in articles.lines() {
        let (id, path) = line.split_once('\t').expect(line);
        assert!(!id.is_empty() && h.storage.0.exists(path), "{line}");
    }
    assert_eq!(articles.lines().count(), books.len(), "{articles}");
}

#[test]
fn malformed_lists_fetch_nothing_and_end() {
    for body in [
        "",
        "{",
        r#"{"_embedded": {"items": {"id": 1}}}"#,
        r#"{"_embedded": [1, 2, 3]}"#,
        r#"[{"id": 1, "title": "Not where items are"}]"#,
        r#"{"_embedded": {"items": [{"title": "No id"}]}}"#,
    ] {
        let mut h = Harness::new();
        let asked = h.list_and_download(body, 200);
        assert!(asked.is_empty(), "{body}: {asked:?}");
        assert!(h.told().contains("0 new"), "{body}: {}", h.told());
        assert!(h.storage.books().is_empty());
    }
}

#[test]
fn a_huge_list_fetches_thirty_at_most() {
    let mut h = Harness::new();
    let list: Vec<(String, String)> = (0..500)
        .map(|n| (n.to_string(), format!("\"Article {n}\"")))
        .collect();
    let list: Vec<(&str, &str)> = list.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let asked = h.list_and_download(&items(&list), 200);
    assert_eq!(asked.len(), 30);
    assert_eq!(h.storage.books().len(), 30);
    assert!(h.told().contains("30 new"), "{}", h.told());
}

#[test]
fn failed_downloads_are_counted_and_tried_again() {
    let mut h = Harness::new();
    let list = items(&[("1", r#""One""#), ("2", r#""Two""#)]);
    let asked = h.list_and_download(&list, 500);
    assert_eq!(asked.len(), 2);
    assert!(h.storage.books().is_empty());
    assert!(h.told().contains("2 failed"), "{}", h.told());
    assert!(!h.storage.0.exists(".paperpaw/plugins/wallabag/article.tmp"));
    let asked = h.list_and_download(&list, 200);
    assert_eq!(asked.len(), 2, "tried again");
    assert_eq!(h.storage.books().len(), 2);
}

#[test]
fn a_list_that_fails_keeps_the_books_fetched() {
    let mut h = Harness::new();
    h.list_and_download(&items(&[("1", r#""One""#)]), 200);
    let list = h.listing();
    let calls = h.answer(&list, 503, "busy");
    assert!(calls.is_empty());
    assert_eq!(h.storage.books(), ["Wallabag/One.epub"]);
    assert!(h.told().contains("1 failed"), "{}", h.told());
}

#[test]
fn articles_read_to_the_end_are_noted_and_archived_even_after_a_failure() {
    let mut h = Harness::new();
    h.list_and_download(&items(&[("1", r#""One""#), ("2", r#""Two""#)]), 200);
    let reading = |path: &str, page: Option<u32>, pages: Option<u32>, progress: f32| {
        Reading::new(
            String::from(path),
            String::new(),
            progress,
            0,
            0,
            String::new(),
        )
        .page(page)
        .pages(pages)
    };
    // Not at the end yet; another book; then the end, twice.
    h.send(
        "core.page-turned",
        reading("Wallabag/One.epub", Some(3), Some(10), 30.0),
    );
    h.send(
        "core.page-turned",
        reading("Books/Other.epub", Some(10), Some(10), 100.0),
    );
    h.send(
        "core.page-turned",
        reading("Wallabag/Two.epub", None, None, 50.0),
    );
    assert!(h.storage.text(FINISHED).is_none());
    h.send(
        "core.page-turned",
        reading("Wallabag/One.epub", Some(10), Some(10), 100.0),
    );
    h.send(
        "core.page-turned",
        reading("Wallabag/One.epub", Some(10), Some(10), 100.0),
    );
    h.send(
        "core.page-turned",
        reading("Wallabag/Two.epub", None, None, 96.0),
    );
    assert_eq!(h.storage.text(FINISHED).unwrap(), "1\n2\n");
    // The archiving fails for one: it stays noted and its book stays.
    let calls = h.fetch();
    let mut calls = h.answer(&calls[0], 200, r#"{"access_token": "T"}"#);
    let mut archived = Vec::new();
    while calls.len() == 1 && calls[0].method == "PATCH" {
        let call = calls.remove(0);
        archived.push(call.url.clone());
        let status = if call.url.ends_with("/2.json") {
            500
        } else {
            200
        };
        calls = h.answer(&call, status, "{}");
    }
    assert_eq!(archived.len(), 2, "{archived:?}");
    assert_eq!(h.storage.text(FINISHED).unwrap(), "2\n");
    assert_eq!(h.storage.books(), ["Wallabag/Two.epub"]);
}

#[test]
fn unread_articles_beyond_the_thirty_listed_keep_their_books() {
    let mut h = Harness::new();
    let first: Vec<(String, String)> = (1..=30)
        .rev()
        .map(|n| (n.to_string(), format!("\"Article {n}\"")))
        .collect();
    let first: Vec<(&str, &str)> = first
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    h.list_and_download(&items(&first), 200);
    assert_eq!(h.storage.books().len(), 30);
    // One more saved in Wallabag: the newest thirty no longer hold
    // article 1, which is still unread.
    let second: Vec<(String, String)> = (2..=31)
        .rev()
        .map(|n| (n.to_string(), format!("\"Article {n}\"")))
        .collect();
    let second: Vec<(&str, &str)> = second
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    h.list_and_download(&items(&second), 200);
    assert!(
        h.storage.0.exists("Wallabag/Article 1.epub"),
        "{:?}",
        h.storage.books()
    );
}
