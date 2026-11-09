//! The fetch against scripted `core.net` and `core.library` services and an
//! in-memory data folder, driven by a probe plugin that runs `Fetcher` from
//! events (the real plugin starts it from its set-up screen and its action):
//! the requests as they go out (the login form, the bearer header, the list
//! and the books saved to files), what each answer leads to (new articles
//! imported into the library with a free name, read ones archived and their
//! books taken away unless the library keeps them for their marks, an article
//! no longer listed asked about), what a bad login, a missing network, a list
//! cut short and a refused service call end in, and that no refusal aborts.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{
    EventHttpDone, Record, ServiceEnd, ServiceLibResult, ServiceNetBytes, ServiceNetStarted,
    record_encode_one, records_decode,
};
use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_STATE};
use paperpaw_plugin_wallabag::native_fetcher::{Fetcher, Login, REQUEST_TOKEN};
use paperpaw_plugin_wallabag::native_keep::Secret;
use paperpaw_plugin_wallabag::native_lists::{self as lists, Files};
use paperpaw_scripted_host::{ScriptedHost, Slot, event_batch, sweep};
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    declare::{Declarations, EventKind},
    events::{Event, Incoming, kinds},
    registrations,
    reply::Reply,
};
use std::cell::RefCell;
use std::collections::BTreeMap;

const START: u32 = 1;
const DONE: u32 = 2;

thread_local! {
    static SEEN: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// The data folder.
    static FILES: RefCell<BTreeMap<String, Vec<u8>>> = const { RefCell::new(BTreeMap::new()) };
    /// What the network asked, what the library was asked, in order.
    static LOG: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// The memory body of the next answer (the token).
    static BODY: RefCell<String> = const { RefCell::new(String::new()) };
    /// Names the library holds already, and how it answers a delete.
    static TAKEN: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static KEEPS_NOTES: RefCell<bool> = const { RefCell::new(false) };
}

fn say(line: String) {
    SEEN.with(|seen| seen.borrow_mut().push(line));
}

fn seen() -> Vec<String> {
    SEEN.with(|seen| seen.take())
}

fn log() -> Vec<String> {
    LOG.with(|log| log.take())
}

fn put(name: &str, bytes: &[u8]) {
    FILES.with(|f| f.borrow_mut().insert(name.to_string(), bytes.to_vec()));
}

fn file(name: &str) -> Option<String> {
    FILES.with(|f| {
        f.borrow()
            .get(name)
            .map(|b| String::from_utf8_lossy(b).into_owned())
    })
}

/// The data folder in memory.
struct Mem;

impl Files for Mem {
    fn size(&self, name: &str) -> Result<u64, Error> {
        FILES.with(|f| {
            f.borrow()
                .get(name)
                .map(|b| b.len() as u64)
                .ok_or(Error::NotFound)
        })
    }

    fn read_at(&self, name: &str, offset: u64, out: &mut [u8]) -> Result<usize, Error> {
        FILES.with(|f| {
            let f = f.borrow();
            let bytes = f.get(name).ok_or(Error::NotFound)?;
            let from = usize::try_from(offset).unwrap().min(bytes.len());
            let n = out.len().min(bytes.len() - from);
            out[..n].copy_from_slice(&bytes[from..from + n]);
            Ok(n)
        })
    }

    fn write(&self, name: &str, data: &[u8]) -> Result<(), Error> {
        put(name, data);
        Ok(())
    }

    fn remove(&self, name: &str) -> Result<(), Error> {
        FILES.with(|f| {
            f.borrow_mut()
                .remove(name)
                .map(|_| ())
                .ok_or(Error::NotFound)
        })
    }
}

struct Probe(Fetcher);

impl Plugin for Probe {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        let mut batch = Vec::new();
        let mut d = Declarations::new(|bytes: &[u8]| -> Result<(), i32> {
            batch.extend_from_slice(bytes);
            Ok(())
        });
        for (kind, token) in [(EventKind::Started, START), (EventKind::HttpDone, DONE)] {
            d.subscribe_event(kind, token).map_err(|_| Error::Invalid)?;
        }
        d.submit().map_err(|_| Error::Invalid)?;
        drop(d);
        registrations::submit(host, &batch)?;
        Ok(Self(Fetcher::new()))
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        match token {
            START => {
                let (password, secret) = (Secret::copy("pa ss&1")?, Secret::copy("cl=sec")?);
                let login = Login::new("https://w.example", "me@x", "cid", &password, &secret);
                let started = self.0.start(host, &login);
                say(format!("start {started:?}"));
                started.map(|_| ())
            }
            DONE => {
                if let Some(Event::HttpDone(done)) = incoming.decode_as::<kinds::HttpDone>()? {
                    let stepped = self.0.done(host, &Mem, &done);
                    say(format!("done {stepped:?}"));
                    stepped.map(|_| ())?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

paperpaw_sdk::native_plugin!(
    Probe,
    metadata = include_bytes!("../native/metadata.host.bin"),
    linked = PROBE_ENTRY
);

fn stream(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        let mut one = vec![0; 4096];
        let used = record_encode_one(record, &mut one).unwrap();
        out.extend_from_slice(&one[..used]);
    }
    out
}

/// A scripted `core.net`: requests get the ids 1, 2, ...; the memory body is
/// [`BODY`], read in pieces of 5 bytes.
fn net(host: &ScriptedHost) {
    let next = RefCell::new(0_u32);
    host.service("core.net", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        let rows: Vec<Record<'_>> = batch.iter().collect();
        match (op, rows.first()) {
            (1, Some(Record::ServiceNetRequest(request))) => {
                let mut line = format!("{} {}", request.method, request.url);
                for row in &rows[1..] {
                    match row {
                        Record::ServiceNetHeader(h) => {
                            line += &format!(" [{}: {}]", h.name, h.value);
                        }
                        Record::ServiceNetBody(b) => {
                            line += &format!(" body {}", String::from_utf8_lossy(b.data));
                        }
                        Record::ServiceNetFile(f) => line += &format!(" file {}", f.path),
                        _ => return Err(-1),
                    }
                }
                assert_eq!(request.token, REQUEST_TOKEN);
                LOG.with(|log| log.borrow_mut().push(line));
                *next.borrow_mut() += 1;
                Ok(stream(&[Record::ServiceNetStarted(ServiceNetStarted {
                    id: *next.borrow(),
                })]))
            }
            (2, Some(Record::ServiceNetToken(_))) => Ok(Vec::new()),
            (4, Some(Record::ServiceNetRead(read))) => {
                let body = BODY.with(|b| b.borrow().clone());
                let from = (read.offset as usize).min(body.len());
                let to = (from + 5).min(body.len());
                Ok(stream(&[Record::ServiceNetBytes(ServiceNetBytes {
                    data: &body.as_bytes()[from..to],
                })]))
            }
            _ => Err(-1),
        }
    });
}

/// A scripted `core.library` with `import` (a taken name is `ERR_STATE`
/// unless a free one is asked for; the file moves out of the data folder) and
/// `delete` (a book with marks is `ERR_STATE` row unless [`KEEPS_NOTES`] is
/// off).
fn library(host: &ScriptedHost) {
    host.service("core.library", 1, 0, move |op, input| {
        let batch = records_decode(input).map_err(|_| -1)?;
        match (op, batch.iter().next()) {
            (12, Some(Record::ServiceLibImport(i))) => {
                LOG.with(|log| {
                    log.borrow_mut().push(format!(
                        "import {} {}/{} flags {}",
                        i.from, i.folder, i.name, i.flags
                    ));
                });
                let mut name = i.name.to_string();
                let taken = TAKEN.with(|t| t.borrow().contains(&name));
                if taken && i.flags & 3 == 0 {
                    return Err(ERR_STATE);
                }
                if taken {
                    name = name.replace(".epub", " (2).epub");
                }
                FILES.with(|f| f.borrow_mut().remove(i.from));
                let path = format!("Books/{}/{name}", i.folder);
                TAKEN.with(|t| t.borrow_mut().push(name));
                Ok(stream(&[Record::ServiceLibResult(ServiceLibResult {
                    path: &path,
                    status: 0,
                    flags: 0,
                })]))
            }
            (10, Some(Record::ServiceLibPaths(p))) => {
                assert_eq!(p.flags, 0, "never with the reader's marks");
                LOG.with(|log| log.borrow_mut().push(format!("delete {}", p.paths)));
                let status = if KEEPS_NOTES.with(|k| *k.borrow()) {
                    ERR_STATE
                } else {
                    0
                };
                let rows: Vec<Record<'_>> = p
                    .paths
                    .split('\n')
                    .map(|path| {
                        Record::ServiceLibResult(ServiceLibResult {
                            path,
                            status,
                            flags: 0,
                        })
                    })
                    .collect();
                let mut all = stream(&rows);
                all.extend(stream(&[Record::ServiceEnd(ServiceEnd {
                    flags: 0,
                    total: u32::try_from(rows.len()).unwrap(),
                    generation: 0,
                    next: 0,
                    cursor: None,
                })]));
                Ok(all)
            }
            _ => Err(-1),
        }
    });
}

fn done(id: u32, status: u32, reason: u32, body_len: usize) -> Vec<u8> {
    event_batch(&Record::EventHttpDone(EventHttpDone {
        now_ms: 0,
        id,
        token: REQUEST_TOKEN,
        status,
        reason,
        flags: 0,
        body_len: u32::try_from(body_len).unwrap(),
    }))
    .unwrap()
}

const TOKEN_ANSWER: &str = r#"{"access_token":"T0K","expires_in":3600,"token_type":"bearer"}"#;
const LIST_PATH: &str =
    "/api/entries.json?archive=0&detail=metadata&sort=created&order=desc&perPage=30";
const AGENT: &str = "[User-Agent: PaperPaw/0.1 (e-reader)]";
const AUTH: &str = "[Authorization: Bearer T0K]";

fn list(items: &[(&str, &str)]) -> String {
    let items: Vec<String> = items
        .iter()
        .map(|(id, title)| format!(r#"{{"id":{id},"title":"{title}","url":"http://a/{id}"}}"#))
        .collect();
    format!(
        r#"{{"page":1,"_embedded":{{"items":[{}]}}}}"#,
        items.join(",")
    )
}

fn setup(host: &ScriptedHost) {
    FILES.with(|f| f.borrow_mut().clear());
    TAKEN.with(|t| t.borrow_mut().clear());
    KEEPS_NOTES.with(|k| *k.borrow_mut() = false);
    seen();
    log();
    BODY.with(|b| *b.borrow_mut() = TOKEN_ANSWER.to_string());
    net(host);
    library(host);
}

/// Answers `n` with 200 and no memory body.
fn ok(plugin: &paperpaw_scripted_host::Plugin<'_>, id: u32) -> Result<(), i32> {
    plugin.event(DONE, &done(id, 200, 0, 0)).map(|_| ())
}

/// A started fetch: logged in, the list asked for.
fn started(plugin: &paperpaw_scripted_host::Plugin<'_>) {
    plugin.event(START, &done(0, 0, 0, 0)).unwrap();
    assert_eq!(seen(), ["start Ok(None)"]);
    assert_eq!(
        log(),
        ["POST https://w.example/oauth/v2/token \
          [Content-Type: application/x-www-form-urlencoded] [User-Agent: PaperPaw/0.1 (e-reader)] \
          body grant_type=password&client_id=cid&client_secret=cl%3Dsec&username=me%40x&password=pa+ss%261"]
    );
    plugin
        .event(DONE, &done(1, 200, 0, TOKEN_ANSWER.len()))
        .unwrap();
}

#[test]
fn a_fetch_logs_in_lists_downloads_and_puts_the_books_into_the_library() {
    let host = ScriptedHost::new();
    setup(&host);
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    started(&plugin);
    assert_eq!(seen(), ["done Ok(None)"]);
    assert_eq!(
        log(),
        [format!(
            "GET https://w.example{LIST_PATH} {AUTH} {AGENT} file entries.json"
        )]
    );
    // Newest first in the list; the oldest is fetched first.
    put(
        "entries.json",
        list(&[("9", "Nine: a/b"), ("7", "Seven")]).as_bytes(),
    );
    ok(&plugin, 2).unwrap();
    assert_eq!(
        log(),
        [format!(
            "GET https://w.example/api/entries/7/export.epub {AUTH} {AGENT} file article.tmp"
        )]
    );
    put("article.tmp", b"seven");
    ok(&plugin, 3).unwrap();
    assert_eq!(
        log(),
        [
            "import article.tmp Wallabag/Seven.epub flags 0".to_string(),
            format!(
                "GET https://w.example/api/entries/9/export.epub {AUTH} {AGENT} file article.tmp"
            ),
        ]
    );
    put("article.tmp", b"nine");
    ok(&plugin, 4).unwrap();
    assert_eq!(log(), ["import article.tmp Wallabag/Nine a b.epub flags 0"]);
    assert_eq!(
        seen().last().map(String::as_str),
        Some("done Ok(Some(Finish { key: \"wallabag.done\", counts: Some((2, 0, 0)) }))")
    );
    assert_eq!(
        file("articles.txt").unwrap(),
        "7\tBooks/Wallabag/Seven.epub\n9\tBooks/Wallabag/Nine a b.epub\n"
    );
    assert_eq!(file("account.txt").unwrap(), "https://w.example\nme@x\n");
    // What the fetch left in the data folder is gone.
    assert!(file("entries.json").is_none() && file("article.tmp").is_none());
    drop(plugin);
}

#[test]
fn a_name_taken_by_a_book_takes_the_article_s_id_and_a_free_name() {
    let host = ScriptedHost::new();
    setup(&host);
    TAKEN.with(|t| t.borrow_mut().push("Seven.epub".to_string()));
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    started(&plugin);
    put("entries.json", list(&[("7", "Seven")]).as_bytes());
    ok(&plugin, 2).unwrap();
    log();
    put("article.tmp", b"seven");
    ok(&plugin, 3).unwrap();
    assert_eq!(
        log(),
        [
            "import article.tmp Wallabag/Seven.epub flags 0",
            "import article.tmp Wallabag/Seven (7).epub flags 2"
        ]
    );
    assert_eq!(
        file("articles.txt").unwrap(),
        "7\tBooks/Wallabag/Seven (7).epub\n"
    );
}

#[test]
fn articles_read_to_the_end_are_archived_and_their_books_taken_away() {
    let host = ScriptedHost::new();
    setup(&host);
    put(
        "articles.txt",
        b"7\tBooks/Wallabag/Seven.epub\n8\tBooks/Wallabag/Eight.epub\n",
    );
    put("finished.txt", b"7\nunknown\n");
    put("account.txt", b"https://w.example\nme@x\n");
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    started(&plugin);
    // 7 was read: archived first, and (the answer says so) its book goes.
    assert_eq!(
        log(),
        [format!(
            "PATCH https://w.example/api/entries/7.json {AUTH} {AGENT} \
             [Content-Type: application/x-www-form-urlencoded] body archive=1"
        )]
    );
    ok(&plugin, 2).unwrap();
    assert_eq!(log()[0], "delete Books/Wallabag/Seven.epub".to_string());
    assert_eq!(
        file("articles.txt").unwrap(),
        "8\tBooks/Wallabag/Eight.epub\n"
    );
    assert_eq!(file("finished.txt").unwrap(), "unknown\n");
    // The list holds neither: 8 is asked about, and is archived at Wallabag.
    put("entries.json", list(&[]).as_bytes());
    ok(&plugin, 3).unwrap();
    assert_eq!(
        log(),
        [format!(
            "GET https://w.example/api/entries/8.json {AUTH} {AGENT} file entry.json"
        )]
    );
    put("entry.json", br#"{"id":8,"is_archived":1}"#);
    KEEPS_NOTES.with(|k| *k.borrow_mut() = true);
    ok(&plugin, 4).unwrap();
    // The library keeps a book with the reader's marks (`State`): only the
    // article is let go of.
    assert_eq!(log(), ["delete Books/Wallabag/Eight.epub"]);
    assert_eq!(file("articles.txt").unwrap(), "");
    assert!(
        seen()
            .last()
            .unwrap()
            .contains("key: \"wallabag.done\", counts: Some((0, 1, 0))")
    );
}

#[test]
fn an_article_wallabag_still_has_unarchived_keeps_its_book() {
    let host = ScriptedHost::new();
    setup(&host);
    put("articles.txt", b"8\tBooks/Wallabag/Eight.epub\n");
    put("account.txt", b"https://w.example\nme@x\n");
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    started(&plugin);
    put("entries.json", list(&[]).as_bytes());
    ok(&plugin, 2).unwrap();
    log();
    put("entry.json", br#"{"id":8,"is_archived":0}"#);
    ok(&plugin, 3).unwrap();
    assert!(log().is_empty());
    assert_eq!(
        file("articles.txt").unwrap(),
        "8\tBooks/Wallabag/Eight.epub\n"
    );
}

#[test]
fn the_lists_of_another_account_are_let_go_of() {
    let host = ScriptedHost::new();
    setup(&host);
    put("articles.txt", b"8\tBooks/Wallabag/Eight.epub\n");
    put("finished.txt", b"8\n");
    put("account.txt", b"https://other.example\nsomeone\n");
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    started(&plugin);
    // No archive request for the other account's article: the list is next.
    assert_eq!(log().len(), 1);
    assert!(file("finished.txt").unwrap().is_empty());
    assert_eq!(file("articles.txt").unwrap(), "");
}

#[test]
fn what_is_not_a_whole_list_changes_nothing() {
    let host = ScriptedHost::new();
    setup(&host);
    put("articles.txt", b"8\tBooks/Wallabag/Eight.epub\n");
    put("account.txt", b"https://w.example\nme@x\n");
    let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
    started(&plugin);
    // A list cut short, and an id that is not a number.
    put(
        "entries.json",
        br#"{"_embedded":{"items":[{"id":"../x","title":"a"},{"id":9,"title":"b""#,
    );
    log();
    ok(&plugin, 2).unwrap();
    assert!(log().is_empty());
    assert!(
        seen()
            .last()
            .unwrap()
            .contains("key: \"wallabag.done-failed\", counts: Some((0, 0, 1))")
    );
    assert_eq!(
        file("articles.txt").unwrap(),
        "8\tBooks/Wallabag/Eight.epub\n"
    );
}

#[test]
fn a_login_that_fails_says_why() {
    for (status, reason, key) in [
        (401, 0, "wallabag.bad-login"),
        (200, 0, "wallabag.bad-login"),
        (0, 1, "wallabag.no-network"),
        (0, 11, "wallabag.no-network"),
        (0, 2, "wallabag.unreachable"),
        (0, 4, "wallabag.unreachable"),
    ] {
        let host = ScriptedHost::new();
        setup(&host);
        // A 200 with no token in it.
        BODY.with(|b| *b.borrow_mut() = r#"{"error":"x"}"#.to_string());
        let plugin = host.load(PROBE_ENTRY.entry()).unwrap();
        plugin.event(START, &done(0, 0, 0, 0)).unwrap();
        seen();
        plugin.event(DONE, &done(1, status, reason, 13)).unwrap();
        assert_eq!(
            seen(),
            [format!(
                "done Ok(Some(Finish {{ key: \"{key}\", counts: None }}))"
            )],
            "{status} {reason}"
        );
    }
}

#[test]
fn a_refused_service_call_or_allocation_ends_a_fetch_without_aborting() {
    let run = |host: &mut ScriptedHost, _: Option<usize>| {
        setup(host);
        put(
            "articles.txt",
            b"8\tBooks/Wallabag/Eight.epub\n9\tBooks/Wallabag/Nine.epub\n",
        );
        put("finished.txt", b"9\n");
        put("account.txt", b"https://w.example\nme@x\n");
        let Ok(plugin) = host.load(PROBE_ENTRY.entry()) else {
            return;
        };
        let steps: [(u32, Vec<u8>); 6] = [
            (START, done(0, 0, 0, 0)),
            (DONE, done(1, 200, 0, TOKEN_ANSWER.len())),
            (DONE, done(2, 200, 0, 0)),
            (DONE, done(3, 200, 0, 0)),
            (DONE, done(4, 200, 0, 0)),
            (DONE, done(5, 200, 0, 0)),
        ];
        for (token, batch) in &steps {
            if token == &DONE {
                put("entries.json", list(&[("11", "Eleven")]).as_bytes());
                put("entry.json", br#"{"id":8,"is_archived":1}"#);
                put("article.tmp", b"x");
            }
            // A refusal is an error status or an end; never a panic.
            let _ = plugin.event(*token, batch);
        }
    };
    sweep(Slot::ServiceCall, ERR_NO_MEMORY, run);
    sweep(Slot::Heap, ERR_NO_MEMORY, run);
}

#[test]
fn a_page_turn_in_the_folder_notes_the_article_once() {
    FILES.with(|f| f.borrow_mut().clear());
    put("articles.txt", b"7\tBooks/Wallabag/Seven.epub\n");
    for path in ["Books/Other/Seven.epub", "Books/Wallabag/Eight.epub"] {
        lists::note_read(&Mem, path).unwrap();
        assert!(file("finished.txt").is_none(), "{path}");
    }
    lists::note_read(&Mem, "Books/Wallabag/Seven.epub").unwrap();
    lists::note_read(&Mem, "Books/Wallabag/Seven.epub").unwrap();
    assert_eq!(file("finished.txt").unwrap(), "7\n");
}
