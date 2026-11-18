//! The fetch (`native_fetch`) over a volume in memory, a network that records
//! what it was asked and a library that records what it was given. The cases
//! are those interface 6 was tested on (`tests/fetch.rs`, run on a kernel),
//! now without one, and a few that only the native fetch has (a cancelled
//! fetch, the end of the work run, a name taken, a memory sweep).
#![allow(clippy::unwrap_used, reason = "test assertions")]
use crate::fail_alloc::{exempt, sweep};
use crate::native_fetch::{
    BOOK_FILE, Call, DOC_FILE, Done, Ended, Fetcher, LIST_FILE, REQUEST_TOKEN, Shelf, Started,
    Stop, Wire,
};
use crate::native_lists::{self as lists, ARTICLES_FILE, FINISHED_FILE};
use crate::native_read_later::account;
use crate::native_secret::Secret;
use crate::test_volume::Mem;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error,
    net::{Reason, RequestId},
};
use paperpaw_ui_kit::files::Volume;

const TOKEN: &str = "T0ken-not-real";
const LIST: &str = r#"{"count": 2, "nextPageCursor": null, "results": [
    {"id": "01newer", "title": "Newer <one>", "category": "article", "location": "later"},
    {"id": "01older", "title": "Older", "category": "article", "location": "later"}
]}"#;
const EMPTY: &str = r#"{"count": 0, "results": []}"#;

fn doc(id: &str, location: &str, html: &str) -> String {
    format!(
        r#"{{"count": 1, "nextPageCursor": null, "results": [{{"id": "{id}", "author": "Ann & Bo", "location": "{location}", "html_content": "{html}"}}]}}"#
    )
}

/// What one request was.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Sent {
    token: String,
    method: &'static str,
    path: String,
    body: Option<&'static str>,
    save: Option<(String, u32)>,
}

struct World {
    volume: Rc<Mem>,
    token: Option<String>,
    account: Option<String>,
    network: bool,
    sent: Vec<Sent>,
    cancelled: usize,
    next: u32,
}

impl World {
    fn new() -> Self {
        Self {
            volume: Rc::default(),
            token: Some(TOKEN.to_string()),
            account: None,
            network: true,
            sent: Vec::new(),
            cancelled: 0,
            next: 0,
        }
    }

    /// What the host does when a request ends well: the answer is in its file.
    fn answer(&self, file: &str, body: &str) {
        self.volume.write(file, body.as_bytes()).unwrap();
    }

    fn id(&self) -> RequestId {
        RequestId::new(self.next).unwrap()
    }

    /// The account the lists belong to is the token's.
    fn owns_the_lists(&mut self) {
        self.account = Some(account(TOKEN).unwrap());
    }

    fn last(&self) -> &Sent {
        self.sent.last().unwrap()
    }

    fn text(&self, name: &str) -> String {
        String::from_utf8(self.volume.get(name).unwrap()).unwrap()
    }
}

impl Wire for World {
    fn volume(&self) -> &dyn Volume {
        &*self.volume
    }

    fn network(&mut self) -> Result<(), Error> {
        if self.network {
            Ok(())
        } else {
            Err(Error::Unsupported)
        }
    }

    fn token(&mut self) -> Result<Option<Secret>, Error> {
        self.token.as_deref().map(Secret::new).transpose()
    }

    fn account(&mut self) -> Result<Option<String>, Error> {
        Ok(self.account.clone())
    }

    fn keep_account(&mut self, account: &str) -> Result<(), Error> {
        self.account = Some(account.to_string());
        Ok(())
    }

    fn send(&mut self, token: &Secret, call: &Call<'_>) -> Result<RequestId, Error> {
        if !self.network {
            return Err(Error::Unsupported);
        }
        self.sent.push(Sent {
            token: token.as_str().to_string(),
            method: call.method,
            path: call.path.to_string(),
            body: call.body,
            save: call.save.map(|s| (s.file.to_string(), s.max)),
        });
        self.next += 1;
        Ok(self.id())
    }

    fn cancel(&mut self) {
        self.cancelled += 1;
    }
}

/// The library: books by path, those the reader made marks in, those taken.
struct Library {
    volume: Rc<Mem>,
    books: Vec<(String, Vec<u8>)>,
    marked: Vec<String>,
    removed: Vec<String>,
}

impl Library {
    fn new(world: &World) -> Self {
        Self {
            volume: world.volume.clone(),
            books: Vec::new(),
            marked: Vec::new(),
            removed: Vec::new(),
        }
    }

    fn names(&self) -> Vec<&str> {
        self.books.iter().map(|(p, _)| p.as_str()).collect()
    }

    fn text(&self, path: &str) -> String {
        let book = self.books.iter().find(|(p, _)| p == path).unwrap();
        String::from_utf8(book.1.clone()).unwrap()
    }
}

impl Shelf for Library {
    fn put(&mut self, from: &str, file: &str, free_name: bool) -> Result<String, Error> {
        let mut path = format!("Reader/{file}");
        if self.books.iter().any(|(p, _)| *p == path) {
            if !free_name {
                return Err(Error::State);
            }
            let stem = file.strip_suffix(".html").unwrap();
            let mut n = 2;
            while self.books.iter().any(|(p, _)| *p == path) {
                path = format!("Reader/{stem} ({n}).html");
                n += 1;
            }
        }
        let bytes = self.volume.get(from).ok_or(Error::NotFound)?;
        self.volume.remove(from).unwrap();
        self.books.push((path.clone(), bytes));
        Ok(path)
    }

    fn remove(&mut self, path: &str) -> Result<(), Error> {
        if self.marked.iter().any(|p| p == path) {
            return Err(Error::State);
        }
        self.books.retain(|(p, _)| p != path);
        self.removed.push(path.to_string());
        Ok(())
    }
}

fn ok(world: &World) -> Done {
    Done::new(world.id(), REQUEST_TOKEN, Reason::Ok, 200)
}

fn answer(f: &mut Fetcher, w: &mut World, lib: &mut Library, file: &str, body: &str) -> Ended {
    w.answer(file, body);
    let done = ok(w);
    f.ended(w, lib, &done)
}

fn start(f: &mut Fetcher, w: &mut World) -> Started {
    f.start(w, "read-later-cloud.from.later").unwrap()
}

/// Fetches the two articles of [`LIST`]: the books exist.
fn fetched() -> (World, Library, Fetcher) {
    let mut w = World::new();
    let mut lib = Library::new(&w);
    let mut f = Fetcher::new();
    assert_eq!(start(&mut f, &mut w), Started::Fetching);
    assert_eq!(
        answer(&mut f, &mut w, &mut lib, LIST_FILE, LIST),
        Ended::Next
    );
    let old = doc("01older", "later", "<p>Old \\u00e9 \\\"text\\\"</p>");
    assert_eq!(
        answer(&mut f, &mut w, &mut lib, DOC_FILE, &old),
        Ended::Next
    );
    let new = doc("01newer", "later", "<p>New</p>");
    let end = answer(&mut f, &mut w, &mut lib, DOC_FILE, &new);
    assert_eq!(
        end,
        Ended::Finished {
            fetched: 2,
            archived: 0,
            failed: 0
        }
    );
    (w, lib, f)
}

#[test]
fn fetches_new_articles_as_books() {
    let mut w = World::new();
    let mut lib = Library::new(&w);
    let mut f = Fetcher::new();
    assert_eq!(start(&mut f, &mut w), Started::Fetching);
    assert!(f.busy());
    assert_eq!(
        w.last(),
        &Sent {
            token: TOKEN.to_string(),
            method: "GET",
            path: "/api/v3/list/?location=later&category=article&limit=20".to_string(),
            body: None,
            save: Some((LIST_FILE.to_string(), 512 * 1024)),
        }
    );
    // The books, the oldest first.
    answer(&mut f, &mut w, &mut lib, LIST_FILE, LIST);
    assert_eq!(
        w.last().path,
        "/api/v3/list/?id=01older&withHtmlContent=true"
    );
    assert_eq!(w.last().save, Some((DOC_FILE.to_string(), 4 * 1024 * 1024)));
    let old = doc("01older", "later", "<p>Old \\u00e9 \\\"text\\\"</p>");
    answer(&mut f, &mut w, &mut lib, DOC_FILE, &old);
    assert_eq!(
        w.last().path,
        "/api/v3/list/?id=01newer&withHtmlContent=true"
    );
    let new = doc("01newer", "later", "<p>New</p>");
    let end = answer(&mut f, &mut w, &mut lib, DOC_FILE, &new);
    assert!(matches!(end, Ended::Finished { fetched: 2, .. }));
    assert!(!f.busy());
    let book = lib.text("Reader/Older.html");
    assert!(
        book.contains("<h1>Older</h1><p>Ann &amp; Bo</p><p>Old é \"text\"</p></body></html>"),
        "{book}"
    );
    assert!(book.starts_with("<!DOCTYPE html>"));
    // A title with markup is text in the book; the file's name is clean.
    let new = lib
        .names()
        .into_iter()
        .find(|n| *n != "Reader/Older.html")
        .unwrap();
    assert!(!new.contains('<'), "{new}");
    assert!(lib.text(new).contains("<h1>Newer &lt;one&gt;</h1>"));
    // Nothing is left of the answers.
    for name in [LIST_FILE, DOC_FILE, BOOK_FILE] {
        assert!(w.volume.get(name).is_none(), "{name}");
    }
    // A second fetch with the same list changes nothing.
    assert_eq!(start(&mut f, &mut w), Started::Fetching);
    let end = answer(&mut f, &mut w, &mut lib, LIST_FILE, LIST);
    assert!(matches!(end, Ended::Finished { fetched: 0, .. }));
    assert_eq!(lib.books.len(), 2);
    // The fetch's books are noted: the title's article ids, in order.
    let list = lists::load_articles(&*w.volume).unwrap();
    assert_eq!(
        list.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
        ["01older", "01newer"]
    );
}

#[test]
fn the_inbox_is_asked_for_when_chosen() {
    let mut w = World::new();
    let mut f = Fetcher::new();
    f.start(&mut w, "read-later-cloud.from.inbox").unwrap();
    assert!(w.last().path.contains("location=new&"), "{:?}", w.last());
}

#[test]
fn archives_what_was_read_and_follows_archiving_elsewhere() {
    let (mut w, mut lib, mut f) = fetched();
    // Article `01older` read to the end: archived with the next fetch, its
    // book removed.
    w.volume.write(FINISHED_FILE, b"01older\n").unwrap();
    assert_eq!(start(&mut f, &mut w), Started::Fetching);
    assert_eq!(w.last().method, "PATCH");
    assert_eq!(w.last().path, "/api/v3/update/01older/");
    assert_eq!(w.last().body, Some(r#"{"location":"archive"}"#));
    assert_eq!(w.last().save, None);
    let done = ok(&w);
    assert_eq!(f.ended(&mut w, &mut lib, &done), Ended::Next);
    assert_eq!(lib.removed, ["Reader/Older.html"]);
    assert_eq!(lib.books.len(), 1);
    assert_eq!(w.text(FINISHED_FILE), "");
    let end = answer(&mut f, &mut w, &mut lib, LIST_FILE, EMPTY);
    assert_eq!(end, Ended::Next);
    // The other is not in the list (empty): Reader is asked; it is archived
    // there.
    assert_eq!(w.last().path, "/api/v3/list/?id=01newer");
    assert_eq!(w.last().save, Some((DOC_FILE.to_string(), 512 * 1024)));
    let end = answer(
        &mut f,
        &mut w,
        &mut lib,
        DOC_FILE,
        &doc("01newer", "archive", ""),
    );
    assert_eq!(
        end,
        Ended::Finished {
            fetched: 0,
            archived: 1,
            failed: 0
        }
    );
    assert!(lib.books.is_empty());
    assert!(lists::load_articles(&*w.volume).unwrap().is_empty());
}

#[test]
fn a_book_stays_where_reader_still_has_it() {
    let (mut w, mut lib, mut f) = fetched();
    start(&mut f, &mut w);
    answer(&mut f, &mut w, &mut lib, LIST_FILE, EMPTY);
    // Both are asked about, the oldest first; in the Inbox or the list they
    // stay, and an answer that is not about them changes nothing.
    assert!(w.last().path.ends_with("?id=01older"));
    let stays = doc("01older", "shortlist", "");
    assert_eq!(
        answer(&mut f, &mut w, &mut lib, DOC_FILE, &stays),
        Ended::Next
    );
    assert!(w.last().path.ends_with("?id=01newer"));
    let end = answer(
        &mut f,
        &mut w,
        &mut lib,
        DOC_FILE,
        &doc("zzz", "archive", ""),
    );
    assert_eq!(
        end,
        Ended::Finished {
            fetched: 0,
            archived: 0,
            failed: 1
        }
    );
    assert_eq!(lib.books.len(), 2);
    // Deleted in Reader (no result): its book goes.
    start(&mut f, &mut w);
    answer(&mut f, &mut w, &mut lib, LIST_FILE, EMPTY);
    answer(&mut f, &mut w, &mut lib, DOC_FILE, EMPTY);
    let end = answer(&mut f, &mut w, &mut lib, DOC_FILE, EMPTY);
    assert!(matches!(end, Ended::Finished { failed: 0, .. }), "{end:?}");
    assert!(lib.books.is_empty());
}

#[test]
fn a_book_with_notes_stays_in_the_library() {
    let (mut w, mut lib, mut f) = fetched();
    lib.marked.push("Reader/Older.html".to_string());
    w.volume.write(FINISHED_FILE, b"01older\n").unwrap();
    start(&mut f, &mut w);
    assert_eq!(w.last().method, "PATCH");
    let done = ok(&w);
    f.ended(&mut w, &mut lib, &done);
    let list = r#"{"count":1,"results":[{"id":"01newer","title":"Newer <one>"}]}"#;
    let end = answer(&mut f, &mut w, &mut lib, LIST_FILE, list);
    assert!(
        matches!(end, Ended::Finished { archived: 1, .. }),
        "{end:?}"
    );
    // Archived in Reader, let go of here: the book stays.
    assert!(lib.names().contains(&"Reader/Older.html"));
    assert!(
        w.sent
            .iter()
            .skip(3)
            .all(|s| s.method == "PATCH" || !s.path.contains("01older"))
    );
    let articles = lists::load_articles(&*w.volume).unwrap();
    assert!(articles.iter().all(|a| a.id != "01older"));
    assert_eq!(w.text(FINISHED_FILE), "");
}

#[test]
fn stops_where_reader_says_so() {
    let cases = [
        (Reason::Ok, 401, Stop::BadToken),
        (Reason::Ok, 403, Stop::BadToken),
        (Reason::Ok, 429, Stop::Wait),
        (Reason::Dns, 0, Stop::Unreachable),
        (Reason::Tls, 0, Stop::Unreachable),
        (Reason::Timeout, 0, Stop::Unreachable),
        (Reason::NoNetwork, 0, Stop::NoNetwork),
        (Reason::Offline, 0, Stop::NoNetwork),
        (Reason::NetworkOff, 0, Stop::NoNetwork),
        (Reason::TooLarge, 0, Stop::TooBig),
    ];
    for (reason, status, stop) in cases {
        let mut w = World::new();
        let mut lib = Library::new(&w);
        let mut f = Fetcher::new();
        start(&mut f, &mut w);
        let done = Done::new(w.id(), REQUEST_TOKEN, reason, status);
        assert_eq!(f.ended(&mut w, &mut lib, &done), Ended::Stopped(stop));
        assert!(lib.books.is_empty());
        assert!(!f.busy());
        // Nothing noted: the next fetch starts anew.
        assert_eq!(start(&mut f, &mut w), Started::Fetching, "{reason:?}");
    }
}

#[test]
fn a_start_says_why_it_cannot() {
    let mut f = Fetcher::new();
    let mut w = World::new();
    w.token = None;
    assert_eq!(start(&mut f, &mut w), Started::SetupNeeded);
    w.token = Some(String::new());
    assert_eq!(start(&mut f, &mut w), Started::SetupNeeded);
    w.token = Some(TOKEN.to_string());
    w.network = false;
    assert_eq!(start(&mut f, &mut w), Started::NoNetwork);
    assert!(!f.busy());
    w.network = true;
    // A list that cannot be read is not written over.
    w.volume
        .write(ARTICLES_FILE, b"a1\tReader/A.html\n\xff\n")
        .unwrap();
    assert_eq!(start(&mut f, &mut w), Started::Unreadable);
    assert_eq!(
        w.volume.get(ARTICLES_FILE).unwrap(),
        b"a1\tReader/A.html\n\xff\n"
    );
    let mut w = World::new();
    w.volume
        .write(FINISHED_FILE, "1\n".repeat(10_000).as_bytes())
        .unwrap();
    assert_eq!(start(&mut f, &mut w), Started::Unreadable);
    assert!(w.sent.is_empty() && w.account.is_none());
    // A fetch under way is not started again.
    let mut w = World::new();
    assert_eq!(start(&mut f, &mut w), Started::Fetching);
    assert_eq!(start(&mut f, &mut w), Started::Running);
    assert_eq!(w.sent.len(), 1);
}

#[test]
fn an_article_without_text_or_cut_is_not_a_book() {
    for body in [
        r#"{"count":1,"results":[{"id":"01older","author":"A","location":"later","html_content":null}]}"#,
        r#"{"count":1,"results":[{"id":"01older","html_content":"<p>cut"#,
        "<html>Maintenance</html>",
    ] {
        let mut w = World::new();
        let mut lib = Library::new(&w);
        let mut f = Fetcher::new();
        start(&mut f, &mut w);
        answer(&mut f, &mut w, &mut lib, LIST_FILE, LIST);
        assert!(w.last().path.contains("01older"));
        answer(&mut f, &mut w, &mut lib, DOC_FILE, body);
        // The other article goes on.
        assert!(w.last().path.contains("01newer"));
        let new = doc("01newer", "later", "<p>x</p>");
        let end = answer(&mut f, &mut w, &mut lib, DOC_FILE, &new);
        assert_eq!(
            end,
            Ended::Finished {
                fetched: 1,
                archived: 0,
                failed: 1
            },
            "{body}"
        );
        assert_eq!(lib.books.len(), 1, "{body}");
        assert!(w.volume.get(BOOK_FILE).is_none());
    }
}

#[test]
fn a_list_that_is_not_whole_changes_nothing() {
    let (mut w, mut lib, mut f) = fetched();
    // A list cut short, or the server's own page: nothing is asked about,
    // nothing removed, and the fetch counts one failure.
    for body in [
        r#"{"results": [{"id": "a1", "title": "T""#,
        "<html>Maintenance</html>",
    ] {
        start(&mut f, &mut w);
        let end = answer(&mut f, &mut w, &mut lib, LIST_FILE, body);
        assert_eq!(
            end,
            Ended::Finished {
                fetched: 0,
                archived: 0,
                failed: 1
            }
        );
        assert_eq!(lib.books.len(), 2);
    }
    // An error status is a failed list too.
    start(&mut f, &mut w);
    let done = Done::new(w.id(), REQUEST_TOKEN, Reason::Ok, 500);
    let end = f.ended(&mut w, &mut lib, &done);
    assert!(matches!(end, Ended::Finished { failed: 1, .. }), "{end:?}");
    assert_eq!(lib.books.len(), 2);
}

#[test]
fn another_token_lets_go_of_the_books() {
    let (mut w, mut lib, mut f) = fetched();
    // The account is told by a secret beside the token; the data folder holds
    // nothing made of the token.
    let marker = w.account.clone().unwrap();
    assert!(w.volume.get("account.txt").is_none());
    let all = format!(
        "{}{}",
        w.text(ARTICLES_FILE),
        w.volume
            .get(FINISHED_FILE)
            .map_or(String::new(), |b| String::from_utf8(b).unwrap())
    );
    assert!(!all.contains(&marker));
    // The same token again: the books are still its own.
    start(&mut f, &mut w);
    let end = answer(&mut f, &mut w, &mut lib, LIST_FILE, LIST);
    assert!(matches!(end, Ended::Finished { fetched: 0, .. }));
    assert_eq!(lib.books.len(), 2);
    // Another token (changed in the list of passwords and keys): the books of
    // the other account are not asked about nor removed, and the new token is
    // the account.
    w.token = Some("other-token".to_string());
    start(&mut f, &mut w);
    assert_eq!(w.last().token, "other-token");
    let end = answer(&mut f, &mut w, &mut lib, LIST_FILE, EMPTY);
    assert!(matches!(
        end,
        Ended::Finished {
            fetched: 0,
            failed: 0,
            ..
        }
    ));
    assert_eq!(lib.books.len(), 2);
    assert_ne!(w.account.as_deref(), Some(marker.as_str()));
    // And again: nothing is let go of any more.
    start(&mut f, &mut w);
    answer(&mut f, &mut w, &mut lib, LIST_FILE, LIST);
    assert!(w.last().path.contains("01older"), "{:?}", w.last());
}

/// A saved answer larger than it can be ends the fetch.
#[test]
fn an_answer_that_is_too_big_ends_the_fetch() {
    let put = |w: &World, file: &str, size: usize| {
        let mut bytes = br#"{"count":0,"results":[],"x":""#.to_vec();
        bytes.resize(bytes.len() + size, b'x');
        bytes.extend_from_slice(b"\"}");
        w.volume.files.borrow_mut().insert(file.to_string(), bytes);
    };
    // The list.
    let mut w = World::new();
    let mut lib = Library::new(&w);
    let mut f = Fetcher::new();
    start(&mut f, &mut w);
    put(&w, LIST_FILE, 600 * 1024);
    let done = ok(&w);
    assert_eq!(
        f.ended(&mut w, &mut lib, &done),
        Ended::Stopped(Stop::TooBig)
    );
    assert!(w.volume.get(LIST_FILE).is_none());
    // An article's answer (the text is not too long, the answer is).
    let mut w = World::new();
    let mut lib = Library::new(&w);
    start(&mut f, &mut w);
    answer(&mut f, &mut w, &mut lib, LIST_FILE, LIST);
    put(&w, DOC_FILE, 4 * 1024 * 1024 + 10);
    let done = ok(&w);
    assert_eq!(
        f.ended(&mut w, &mut lib, &done),
        Ended::Stopped(Stop::TooBig)
    );
    for name in [LIST_FILE, DOC_FILE, BOOK_FILE] {
        assert!(w.volume.get(name).is_none(), "{name}");
    }
    assert!(lib.books.is_empty());
    assert!(!f.busy(), "no more is asked of it");
    // An article just within it is a book.
    let mut w = World::new();
    let mut lib = Library::new(&w);
    start(&mut f, &mut w);
    answer(&mut f, &mut w, &mut lib, LIST_FILE, LIST);
    let fine = doc("01older", "later", &"x".repeat(2 * 1024 * 1024));
    answer(&mut f, &mut w, &mut lib, DOC_FILE, &fine);
    assert_eq!(lib.books.len(), 1);
    // The text is cut at 1 MiB (`MAX_TEXT`); the answer's size is not the book's.
    let size = lib.books[0].1.len();
    assert!((1024 * 1024..1024 * 1024 + 400).contains(&size), "{size}");
}

#[test]
fn a_book_is_written_in_calls_the_storage_takes() {
    let (w, ..) = fetched();
    // The 2 MiB book above went through `Mem::append`, which refuses a call
    // over one storage call's data; here a long title is the start.
    assert!(*w.volume.longest.borrow() <= paperpaw_sdk::native::storage::MAX_DATA);
}

#[test]
fn a_name_taken_is_not_replaced() {
    let mut w = World::new();
    w.owns_the_lists();
    let mut lib = Library::new(&w);
    // The reader's own book has the name; an article fetched before, whose
    // book was deleted, has the other.
    lib.books
        .push(("Reader/Older.html".to_string(), b"mine".to_vec()));
    w.volume
        .write(ARTICLES_FILE, b"01gone\tReader/Newer one.html\n")
        .unwrap();
    let mut f = Fetcher::new();
    start(&mut f, &mut w);
    answer(&mut f, &mut w, &mut lib, LIST_FILE, LIST);
    // The one not in the list is asked about first; Reader still has it.
    assert!(w.last().path.ends_with("?id=01gone"));
    answer(
        &mut f,
        &mut w,
        &mut lib,
        DOC_FILE,
        &doc("01gone", "later", ""),
    );
    let old = doc("01older", "later", "<p>Old</p>");
    answer(&mut f, &mut w, &mut lib, DOC_FILE, &old);
    let new = doc("01newer", "later", "<p>New</p>");
    answer(&mut f, &mut w, &mut lib, DOC_FILE, &new);
    assert_eq!(lib.text("Reader/Older.html"), "mine");
    let names = lib.names();
    assert!(names.contains(&"Reader/Older (01older).html"), "{names:?}");
    assert!(
        names.contains(&"Reader/Newer one (01newer).html"),
        "{names:?}"
    );
}

#[test]
fn at_most_ten_books_and_eight_questions() {
    let ids: Vec<String> = (0..20).map(|n| format!("a{n:02}")).collect();
    let entries: Vec<String> = ids
        .iter()
        .map(|id| format!(r#"{{"id":"{id}","title":"T{id}"}}"#))
        .collect();
    let list = format!(r#"{{"results":[{}]}}"#, entries.join(","));
    let mut w = World::new();
    let mut lib = Library::new(&w);
    let mut f = Fetcher::new();
    start(&mut f, &mut w);
    answer(&mut f, &mut w, &mut lib, LIST_FILE, &list);
    // The oldest of the newest ten come first.
    assert!(w.last().path.contains("id=a09&"), "{:?}", w.last());
    let mut asked = 1;
    loop {
        let body = doc(
            &w.last().path.split("id=").nth(1).unwrap()[..3],
            "later",
            "<p>x</p>",
        );
        match answer(&mut f, &mut w, &mut lib, DOC_FILE, &body) {
            Ended::Next => asked += 1,
            end => {
                assert!(matches!(
                    end,
                    Ended::Finished {
                        fetched: 10,
                        failed: 0,
                        ..
                    }
                ));
                break;
            }
        }
    }
    assert_eq!(asked, 10);
    // 1 list and 10 books: with the 8 questions of an older fetch, 19 at most.
    assert_eq!(w.sent.len(), 11);
    // Twelve articles fetched earlier, none in the list: eight are asked about.
    let mut w = World::new();
    w.owns_the_lists();
    let mut lib = Library::new(&w);
    let old: String = (0..12)
        .map(|n| format!("b{n:02}\tReader/b{n}.html\n"))
        .collect();
    w.volume.write(ARTICLES_FILE, old.as_bytes()).unwrap();
    start(&mut f, &mut w);
    answer(&mut f, &mut w, &mut lib, LIST_FILE, EMPTY);
    for _ in 0..8 {
        answer(&mut f, &mut w, &mut lib, DOC_FILE, &doc("b00", "later", ""));
    }
    assert_eq!(w.sent.len(), 9);
    assert!(!f.busy());
}

#[test]
fn a_cancelled_fetch_is_over_and_its_end_is_ignored() {
    let mut w = World::new();
    let mut lib = Library::new(&w);
    let mut f = Fetcher::new();
    start(&mut f, &mut w);
    w.answer(LIST_FILE, LIST);
    let done = ok(&w);
    f.cancel(&mut w);
    assert_eq!(w.cancelled, 1);
    assert!(w.volume.get(LIST_FILE).is_none(), "its files go");
    assert_eq!(f.ended(&mut w, &mut lib, &done), Ended::Ignored);
    f.cancel(&mut w);
    assert_eq!(w.cancelled, 1, "nothing to cancel");
    // A request that is not the fetch's is ignored while it runs.
    start(&mut f, &mut w);
    let other = Done::new(RequestId::new(99).unwrap(), REQUEST_TOKEN, Reason::Ok, 200);
    assert_eq!(f.ended(&mut w, &mut lib, &other), Ended::Ignored);
    let other = Done::new(w.id(), REQUEST_TOKEN + 1, Reason::Ok, 200);
    assert_eq!(f.ended(&mut w, &mut lib, &other), Ended::Ignored);
    assert!(f.busy());
    // Cancelled by the host, replaced: the fetch is over, quietly.
    let done = Done::new(w.id(), REQUEST_TOKEN, Reason::Cancelled, 0);
    assert_eq!(f.ended(&mut w, &mut lib, &done), Ended::Ignored);
    assert!(!f.busy());
}

#[test]
fn the_end_of_the_work_run_ends_the_fetch_with_its_counts() {
    let (mut w, mut lib, mut f) = fetched();
    start(&mut f, &mut w);
    let done = Done::new(w.id(), REQUEST_TOKEN, Reason::WorkEnded, 0);
    let end = f.ended(&mut w, &mut lib, &done);
    assert_eq!(
        end,
        Ended::Finished {
            fetched: 0,
            archived: 0,
            failed: 0
        }
    );
    assert!(!f.busy());
}

#[test]
fn a_refused_next_request_ends_the_fetch() {
    let mut w = World::new();
    let mut lib = Library::new(&w);
    let mut f = Fetcher::new();
    start(&mut f, &mut w);
    w.network = false;
    let end = answer(&mut f, &mut w, &mut lib, LIST_FILE, LIST);
    assert_eq!(end, Ended::Stopped(Stop::NoNetwork));
    assert!(!f.busy());
}

#[test]
fn a_page_turn_notes_the_article_read_to_the_end() {
    let (w, ..) = fetched();
    let path = "Reader/Older.html";
    let v = &*w.volume;
    // Not at the end, not an article's book, not in the folder.
    lists::page_turned(v, path, (Some(3), Some(5), 600_000)).unwrap();
    lists::page_turned(v, "Reader/Other.html", (Some(5), Some(5), 1_000_000)).unwrap();
    lists::page_turned(v, "Books/Older.html", (Some(5), Some(5), 1_000_000)).unwrap();
    assert!(w.volume.get(FINISHED_FILE).is_none());
    // On the last page, or near the end where pages are not counted.
    lists::page_turned(v, path, (Some(5), Some(5), 1_000_000)).unwrap();
    assert_eq!(w.text(FINISHED_FILE), "01older\n");
    lists::page_turned(v, "Reader/Newer one.html", (None, None, 950_000)).unwrap();
    lists::page_turned(v, path, (None, None, 990_000)).unwrap();
    assert!(w.text(FINISHED_FILE).starts_with("01older\n"));
    assert_eq!(w.text(FINISHED_FILE).lines().count(), 2);
    // A list that cannot be read is left alone.
    v.write(ARTICLES_FILE, &[0xff]).unwrap();
    lists::page_turned(v, path, (Some(5), Some(5), 1_000_000)).unwrap();
}

/// A world that allocates nothing of its own, so that a sweep refuses only the
/// fetch's allocations: fixed answers, writes counted and dropped.
struct Fixed {
    list: &'static str,
    book: String,
    calls: usize,
}

impl Volume for Fixed {
    fn size(&self, name: &str) -> Result<u64, Error> {
        match name {
            LIST_FILE => Ok(self.list.len() as u64),
            DOC_FILE => Ok(self.book.len() as u64),
            _ => Err(Error::NotFound),
        }
    }

    fn read_at(&self, name: &str, at: u64, out: &mut [u8]) -> Result<usize, Error> {
        let source = match name {
            LIST_FILE => self.list.as_bytes(),
            DOC_FILE => self.book.as_bytes(),
            _ => return Err(Error::NotFound),
        };
        let from = usize::try_from(at).unwrap().min(source.len());
        let n = out.len().min(source.len() - from);
        out[..n].copy_from_slice(&source[from..from + n]);
        Ok(n)
    }

    fn write(&self, _: &str, _: &[u8]) -> Result<(), Error> {
        Ok(())
    }
    fn append(&self, _: &str, _: &[u8]) -> Result<(), Error> {
        Ok(())
    }
    fn rename(&self, _: &str, _: &str) -> Result<(), Error> {
        Ok(())
    }
    fn remove(&self, _: &str) -> Result<(), Error> {
        Ok(())
    }
    fn sync(&self) -> Result<(), Error> {
        Ok(())
    }
}

impl Wire for Fixed {
    fn volume(&self) -> &dyn Volume {
        self
    }
    fn network(&mut self) -> Result<(), Error> {
        Ok(())
    }
    fn token(&mut self) -> Result<Option<Secret>, Error> {
        Secret::new(TOKEN).map(Some)
    }
    fn account(&mut self) -> Result<Option<String>, Error> {
        Ok(None)
    }
    fn keep_account(&mut self, _: &str) -> Result<(), Error> {
        Ok(())
    }
    fn send(&mut self, _: &Secret, _: &Call<'_>) -> Result<RequestId, Error> {
        self.calls += 1;
        RequestId::new(1)
    }
    fn cancel(&mut self) {}
}

impl Shelf for Fixed {
    fn put(&mut self, _: &str, _: &str, _: bool) -> Result<String, Error> {
        paperpaw_ui_kit::text::copy("Reader/Older.html")
    }
    fn remove(&mut self, _: &str) -> Result<(), Error> {
        Ok(())
    }
}

#[test]
fn refused_memory_ends_in_no_memory_never_an_abort() {
    sweep(|| {
        let mut world = Fixed {
            list: r#"{"results":[{"id":"01older","title":"Older: a title"}]}"#,
            book: exempt(|| doc("01older", "later", "<p>Old \\u00e9 \\\"text\\\"</p>")),
            calls: 0,
        };
        let mut f = Fetcher::new();
        let mut shelf = Fixed {
            list: "",
            book: String::new(),
            calls: 0,
        };
        f.start(&mut world, "read-later-cloud.from.later")?;
        let done = Done::new(RequestId::new(1).unwrap(), REQUEST_TOKEN, Reason::Ok, 200);
        for _ in 0..2 {
            if f.ended(&mut world, &mut shelf, &done) == Ended::Memory {
                return Err(Error::NoMemory);
            }
        }
        Ok(())
    });
}
