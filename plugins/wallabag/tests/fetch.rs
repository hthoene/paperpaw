//! A fetch against answers as Wallabag gives them: the token, the list of
//! unread articles, their books; then archiving one read to the end.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::{HttpCall, HttpResponse, Io, Kernel};
use paperpaw_platform::{ButtonState, InputEvent, Rotation};
use paperpaw_plugin_wallabag::Wallabag;
use paperpaw_sdk::secrets::{FILE, SecretChange, Secrets};
use paperpaw_sdk::{
    DeviceInfo, DirEntry, DisplayInfo, InputCaps, Instant, MemoryClass, MemoryStorage, PixelFormat,
    Storage, StorageError,
};

const DEVICE: DeviceInfo = DeviceInfo::new(
    "test",
    "Test device",
    DisplayInfo::new(200, 300, 100, PixelFormat::Gray4, Rotation::Deg0, None),
    InputCaps::new(true, false, false, &[]),
    MemoryClass::Large,
);

/// A storage the test looks into while the kernel has it.
#[derive(Clone)]
struct Shared(Rc<MemoryStorage>);

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

/// The library at the storage's top.
const CONFIG: &str = "[core]\nlibrary-folder =\n[wallabag]\nserver = wb.example\nuser = me\nclient-id = cid\n\n\
    [keymap.global]\nbutton.fetch = wallabag.fetch\n";

const LIST: &str = r#"{"_embedded": {"items": [
    {"id": 9, "title": "Newer"},
    {"id": 7, "title": "Older"}
]}}"#;

fn fetch(kernel: &mut Kernel, at: u64) -> Vec<HttpCall> {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        kernel.handle(
            Instant(at),
            InputEvent::Button {
                name: "fetch",
                state,
            },
        );
    }
    requests(kernel)
}

/// The requests the kernel asks the shell to make, once it has shown
/// what the call drew (as a shell does: until then, the kernel may keep
/// what it read, and would miss the test changing a file).
fn requests(kernel: &mut Kernel) -> Vec<HttpCall> {
    kernel.render(&mut |_, _| {});
    let io = kernel.take_io().into_iter();
    io.filter_map(|io| match io {
        Io::Http(call) => Some(call),
        _ => None,
    })
    .collect()
}

fn answer(kernel: &mut Kernel, call: &HttpCall, status: u16, body: &str) -> Vec<HttpCall> {
    kernel.http_response(
        Instant(10),
        HttpResponse {
            id: call.id,
            status,
            body: body.as_bytes().to_vec(),
            ..HttpResponse::default()
        },
    );
    requests(kernel)
}

#[test]
fn fetches_new_articles_and_archives_read_ones() {
    let mut secrets = Secrets::default();
    secrets.apply(SecretChange::keep("wallabag.password", "", "pw"));
    secrets.apply(SecretChange::keep("wallabag.client-secret", "", "cs"));
    let storage = Shared(Rc::new(
        MemoryStorage::new().with(FILE, secrets.encode().into_bytes()),
    ));
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage.clone())
        .user_config(CONFIG)
        .network(true)
        .plugin(Wallabag)
        .build()
        .unwrap();
    kernel.start(Instant(0));

    // A token for the account.
    let calls = fetch(&mut kernel, 1);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].method, "POST");
    assert_eq!(calls[0].url, "https://wb.example/oauth/v2/token");
    let body = String::from_utf8(calls[0].body.clone().unwrap()).unwrap();
    assert_eq!(
        body,
        "grant_type=password&client_id=cid&client_secret=cs&username=me&password=pw"
    );
    // The list, then the books, the oldest first.
    let calls = answer(
        &mut kernel,
        &calls[0],
        200,
        r#"{"access_token": "T", "x": 1}"#,
    );
    assert!(calls[0].url.contains("/api/entries.json?archive=0"));
    assert!(
        calls[0]
            .headers
            .contains(&(String::from("Authorization"), String::from("Bearer T")))
    );
    let calls = answer(&mut kernel, &calls[0], 200, LIST);
    assert_eq!(calls[0].url, "https://wb.example/api/entries/7/export.epub");
    let calls = answer(&mut kernel, &calls[0], 200, "EPUB 7");
    assert_eq!(calls[0].url, "https://wb.example/api/entries/9/export.epub");
    let calls = answer(&mut kernel, &calls[0], 200, "EPUB 9");
    assert!(calls.is_empty());
    assert_eq!(storage.0.get("Wallabag/Older.epub").unwrap(), b"EPUB 7");
    assert_eq!(storage.0.get("Wallabag/Newer.epub").unwrap(), b"EPUB 9");

    // Article 7 read to the end: archived with the next fetch, its book
    // removed; nothing new to download.
    storage
        .0
        .write(".paperpaw/plugins/wallabag/finished.txt", b"7\n")
        .unwrap();
    let calls = fetch(&mut kernel, 20);
    let calls = answer(&mut kernel, &calls[0], 200, r#"{"access_token": "U"}"#);
    assert_eq!(calls[0].method, "PATCH");
    assert_eq!(calls[0].url, "https://wb.example/api/entries/7.json");
    let calls = answer(&mut kernel, &calls[0], 200, "{}");
    assert!(storage.0.get("Wallabag/Older.epub").is_none());
    let calls = answer(
        &mut kernel,
        &calls[0],
        200,
        r#"{"_embedded": {"items": [{"id": 9, "title": "Newer"}]}}"#,
    );
    assert!(calls.is_empty());
    assert!(storage.0.get("Wallabag/Newer.epub").is_some());

    // Archived elsewhere: its book goes too, once Wallabag says so.
    let calls = fetch(&mut kernel, 30);
    let calls = answer(&mut kernel, &calls[0], 200, r#"{"access_token": "V"}"#);
    let calls = answer(
        &mut kernel,
        &calls[0],
        200,
        r#"{"_embedded": {"items": []}}"#,
    );
    assert_eq!(calls[0].url, "https://wb.example/api/entries/9.json");
    let calls = answer(
        &mut kernel,
        &calls[0],
        200,
        r#"{"id": 9, "is_archived": 1}"#,
    );
    assert!(calls.is_empty());
    assert!(storage.0.get("Wallabag/Newer.epub").is_none());
}

/// A kernel with Wallabag set up and articles 7 and 9 fetched.
fn fetched() -> (Kernel, Shared) {
    let mut secrets = Secrets::default();
    secrets.apply(SecretChange::keep("wallabag.password", "", "pw"));
    secrets.apply(SecretChange::keep("wallabag.client-secret", "", "cs"));
    let storage = Shared(Rc::new(
        MemoryStorage::new().with(FILE, secrets.encode().into_bytes()),
    ));
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage.clone())
        .user_config(CONFIG)
        .network(true)
        .plugin(Wallabag)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let calls = fetch(&mut kernel, 1);
    let calls = answer(&mut kernel, &calls[0], 200, r#"{"access_token": "T"}"#);
    let calls = answer(&mut kernel, &calls[0], 200, LIST);
    let calls = answer(&mut kernel, &calls[0], 200, "EPUB 7");
    let calls = answer(&mut kernel, &calls[0], 200, "EPUB 9");
    assert!(calls.is_empty());
    (kernel, storage)
}

const BOOKS: [&str; 2] = ["Wallabag/Older.epub", "Wallabag/Newer.epub"];

/// What the server answers instead of the list (a page of its own, an
/// error, a list cut short) removes no book and asks for nothing more.
#[test]
fn an_answer_that_is_no_list_removes_nothing() {
    let (mut kernel, storage) = fetched();
    let answers = [
        "<!DOCTYPE html><html><body><h1>Down for maintenance</h1></body></html>",
        r#"{"error": "server_error", "error_description": "Try again later"}"#,
        r#"{"page": 1, "pages": 1, "_embedded": {"items": [{"id": 9, "title": "Newer"}, {"id""#,
    ];
    for (n, body) in answers.into_iter().enumerate() {
        let calls = fetch(&mut kernel, 10 + n as u64);
        let calls = answer(&mut kernel, &calls[0], 200, r#"{"access_token": "U"}"#);
        assert!(calls[0].url.contains("/api/entries.json?"));
        let calls = answer(&mut kernel, &calls[0], 200, body);
        assert!(calls.is_empty(), "{body}: {calls:?}");
        for book in BOOKS {
            assert!(storage.0.get(book).is_some(), "{book} after {body}");
        }
    }
}

/// Articles still unread but no longer among the newest stay; one goes
/// only when Wallabag says it is archived or no longer there.
#[test]
fn unread_articles_beyond_the_newest_stay() {
    let (mut kernel, storage) = fetched();
    // Article 10 is new; 7 is no longer in the list.
    let newest =
        r#"{"_embedded": {"items": [{"id": 10, "title": "Newest"}, {"id": 9, "title": "Newer"}]}}"#;
    let calls = fetch(&mut kernel, 10);
    let calls = answer(&mut kernel, &calls[0], 200, r#"{"access_token": "U"}"#);
    let calls = answer(&mut kernel, &calls[0], 200, newest);
    assert_eq!(calls[0].method, "GET");
    assert_eq!(calls[0].url, "https://wb.example/api/entries/7.json");
    let calls = answer(
        &mut kernel,
        &calls[0],
        200,
        r#"{"id": 7, "title": "Older", "is_archived": 0, "content": "<p>Text</p>"}"#,
    );
    assert_eq!(
        calls[0].url,
        "https://wb.example/api/entries/10/export.epub"
    );
    let calls = answer(&mut kernel, &calls[0], 200, "EPUB 10");
    assert!(calls.is_empty());
    assert!(storage.0.get("Wallabag/Older.epub").is_some());
    assert!(storage.0.get("Wallabag/Newest.epub").is_some());

    // An answer that says nothing about it keeps it too.
    for (n, (status, body)) in [(500, "Internal error"), (200, "<html>Login</html>")]
        .into_iter()
        .enumerate()
    {
        let calls = fetch(&mut kernel, 20 + n as u64);
        let calls = answer(&mut kernel, &calls[0], 200, r#"{"access_token": "V"}"#);
        let calls = answer(&mut kernel, &calls[0], 200, newest);
        assert_eq!(calls[0].url, "https://wb.example/api/entries/7.json");
        let calls = answer(&mut kernel, &calls[0], status, body);
        assert!(calls.is_empty());
        assert!(storage.0.get("Wallabag/Older.epub").is_some(), "{body}");
    }

    // Deleted in Wallabag: its book goes.
    let calls = fetch(&mut kernel, 30);
    let calls = answer(&mut kernel, &calls[0], 200, r#"{"access_token": "W"}"#);
    let calls = answer(&mut kernel, &calls[0], 200, newest);
    let calls = answer(
        &mut kernel,
        &calls[0],
        404,
        r#"{"code": 404, "message": "Not Found"}"#,
    );
    assert!(calls.is_empty());
    assert!(storage.0.get("Wallabag/Older.epub").is_none());
    assert!(storage.0.get("Wallabag/Newer.epub").is_some());
}

const ARTICLES: &str = ".paperpaw/plugins/wallabag/articles.txt";

/// Highlights, notes or bookmarks made in an article keep its book when
/// it is archived, read here or elsewhere: Wallabag lets go of it, and
/// the notes stay.
#[test]
fn annotated_articles_keep_their_books() {
    let (mut kernel, storage) = fetched();
    let line = "h\t1\t0\t1\t20\t5\t0\t0\tSome text\tMy note\n";
    for book in BOOKS {
        let file = paperpaw_sdk::annotations::file(book);
        storage.0.write(&file, line.as_bytes()).unwrap();
    }
    storage
        .0
        .write(".paperpaw/plugins/wallabag/finished.txt", b"7\n")
        .unwrap();
    let calls = fetch(&mut kernel, 10);
    let calls = answer(&mut kernel, &calls[0], 200, r#"{"access_token": "U"}"#);
    assert_eq!(calls[0].method, "PATCH");
    let calls = answer(&mut kernel, &calls[0], 200, "{}");
    let calls = answer(
        &mut kernel,
        &calls[0],
        200,
        r#"{"_embedded": {"items": []}}"#,
    );
    assert_eq!(calls[0].url, "https://wb.example/api/entries/9.json");
    let calls = answer(
        &mut kernel,
        &calls[0],
        200,
        r#"{"id": 9, "is_archived": 1}"#,
    );
    assert!(calls.is_empty());
    for book in BOOKS {
        assert!(storage.0.get(book).is_some(), "{book}");
        let file = paperpaw_sdk::annotations::file(book);
        assert_eq!(storage.0.get(&file).unwrap(), line.as_bytes(), "{book}");
    }
    assert_eq!(storage.0.get(ARTICLES).unwrap(), b"");
    // Nothing more is asked about them.
    let calls = fetch(&mut kernel, 20);
    let calls = answer(&mut kernel, &calls[0], 200, r#"{"access_token": "V"}"#);
    let calls = answer(
        &mut kernel,
        &calls[0],
        200,
        r#"{"_embedded": {"items": []}}"#,
    );
    assert!(calls.is_empty(), "{calls:?}");
}

/// The reader deletes article 7's book; a new article of the same title
/// does not take its name, so 7 archived elsewhere later does not take
/// the new one's book with it.
#[test]
fn stale_path_removes_another_article() {
    let (mut kernel, storage) = fetched();
    storage.0.remove("Wallabag/Older.epub").unwrap();
    let list = r#"{"_embedded": {"items": [{"id": 11, "title": "Older"}, {"id": 9, "title": "Newer"}, {"id": 7, "title": "Older"}]}}"#;
    let calls = fetch(&mut kernel, 10);
    let calls = answer(&mut kernel, &calls[0], 200, r#"{"access_token": "U"}"#);
    let calls = answer(&mut kernel, &calls[0], 200, list);
    assert_eq!(
        calls[0].url,
        "https://wb.example/api/entries/11/export.epub"
    );
    let calls = answer(&mut kernel, &calls[0], 200, "EPUB 11");
    assert!(calls.is_empty());
    let list =
        r#"{"_embedded": {"items": [{"id": 11, "title": "Older"}, {"id": 9, "title": "Newer"}]}}"#;
    let calls = fetch(&mut kernel, 20);
    let calls = answer(&mut kernel, &calls[0], 200, r#"{"access_token": "V"}"#);
    let calls = answer(&mut kernel, &calls[0], 200, list);
    assert_eq!(calls[0].url, "https://wb.example/api/entries/7.json");
    let calls = answer(
        &mut kernel,
        &calls[0],
        200,
        r#"{"id": 7, "is_archived": 1}"#,
    );
    assert!(calls.is_empty());
    assert_eq!(
        storage.0.get("Wallabag/Older (11).epub").unwrap(),
        b"EPUB 11"
    );

    // A list an older version wrote, with two articles at one path: the
    // one archived leaves the other's book.
    storage
        .0
        .write(
            ARTICLES,
            b"9\tWallabag/Newer.epub\n12\tWallabag/Newer.epub\n",
        )
        .unwrap();
    let calls = fetch(&mut kernel, 30);
    let calls = answer(&mut kernel, &calls[0], 200, r#"{"access_token": "W"}"#);
    let list = r#"{"_embedded": {"items": [{"id": 12, "title": "Newer"}]}}"#;
    let calls = answer(&mut kernel, &calls[0], 200, list);
    assert_eq!(calls[0].url, "https://wb.example/api/entries/9.json");
    let calls = answer(&mut kernel, &calls[0], 404, r#"{"code": 404}"#);
    assert!(calls.is_empty());
    assert_eq!(storage.0.get("Wallabag/Newer.epub").unwrap(), b"EPUB 9");
}

/// Another server or account: the articles fetched from the old one are
/// let go of, not asked about there, and their books stay.
#[test]
fn another_account_keeps_the_books_fetched() {
    let (kernel, storage) = fetched();
    drop(kernel);
    let config = CONFIG.replace("wb.example", "other.example");
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage.clone())
        .user_config(&config)
        .network(true)
        .plugin(Wallabag)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let calls = fetch(&mut kernel, 10);
    let calls = answer(&mut kernel, &calls[0], 200, r#"{"access_token": "U"}"#);
    assert!(
        calls[0]
            .url
            .starts_with("https://other.example/api/entries.json")
    );
    let calls = answer(
        &mut kernel,
        &calls[0],
        200,
        r#"{"_embedded": {"items": []}}"#,
    );
    assert!(calls.is_empty(), "{calls:?}");
    for book in BOOKS {
        assert!(storage.0.get(book).is_some(), "{book}");
    }
    assert_eq!(storage.0.get(ARTICLES).unwrap(), b"");
}

/// A list of fetched articles that cannot be read stops the fetch before
/// anything is written over it.
#[test]
fn an_unreadable_list_stops_the_fetch() {
    let (mut kernel, storage) = fetched();
    let list = ".paperpaw/plugins/wallabag/articles.txt";
    let mut broken = storage.0.get(list).unwrap();
    broken.push(0xff);
    storage.0.write(list, &broken).unwrap();
    let calls = fetch(&mut kernel, 10);
    let calls = answer(&mut kernel, &calls[0], 200, r#"{"access_token": "U"}"#);
    assert!(calls.is_empty());
    assert_eq!(storage.0.get(list).unwrap(), broken);
}

#[test]
fn a_wrong_login_says_so() {
    let mut secrets = Secrets::default();
    secrets.apply(SecretChange::keep("wallabag.password", "", "pw"));
    secrets.apply(SecretChange::keep("wallabag.client-secret", "", "cs"));
    let storage = MemoryStorage::new().with(FILE, secrets.encode().into_bytes());
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .user_config(CONFIG)
        .network(true)
        .plugin(Wallabag)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let calls = fetch(&mut kernel, 1);
    let calls = answer(&mut kernel, &calls[0], 400, r#"{"error": "invalid_grant"}"#);
    assert!(calls.is_empty());
    // And may start again.
    assert_eq!(fetch(&mut kernel, 2).len(), 1);
}
