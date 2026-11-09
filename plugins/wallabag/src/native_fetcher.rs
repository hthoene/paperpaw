//! The fetch, step by step through `core.net`, the data folder and
//! `core.library` (`docs/abi/network.md`, `library.md` §2.3): a token for the
//! account, the articles read to the end archived (and their books taken away
//! unless they carry the reader's marks), the list of unread articles, each
//! article fetched earlier that is no longer in it asked about, and the new
//! ones downloaded and put into the library. The same order and rules as the
//! interface-6 fetch (`Fetch` in `lib.rs`).
//!
//! - One request at a time; its end is one `http_done` event ([`Fetcher::done`]
//!   takes it, answers from another request are ignored). The list and the
//!   single article go to files in the data folder and are read in pieces by
//!   the readers of `native_fetch`; the token comes as a memory body and is read
//!   in pieces too. A book goes to `article.tmp` and into the library with
//!   [`Library::import`] (a free name, never over a book of the reader's).
//! - Everything Wallabag sends is hostile: the readers cap what they keep, an
//!   id is a number, a name is cleaned, and what is not a whole answer changes
//!   nothing.
//! - The password, the client's secret, the form that carries them and the
//!   header that carries the token are zeroed when the call is over. A refused
//!   allocation ends the fetch (`Error::NoMemory`); any other failure of a step
//!   counts as one failed article and the fetch goes on.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::{
    Error, Host,
    library::{Existing, Import, Library},
    net::{
        BODY_PART, FileRoot, Header, HttpDone, MAX_INLINE_BODY, Net, REPLY, REQUEST_INPUT, Reason,
        Request, RequestId,
    },
};
use paperpaw_ui_kit::{
    mem::{buffer, push, wipe},
    text::{concat, copy},
};

use crate::native_fetch::{Archived, Entry, Token, Unread};
use crate::native_keep::Secret;
use crate::native_lists::{self as lists, ENTRIES, ENTRY, Files, TMP};
use crate::native_wallabag::{Article, MAX_ARTICLES, is_id};

/// The plugin's own number of its one request, in its `http_done` event.
pub const REQUEST_TOKEN: u32 = 1;
/// The `User-Agent` of interface 6.
const USER_AGENT: &str = "PaperPaw/0.1 (e-reader)";
/// The folder of the books, below the library folder.
pub const FOLDER: &str = "Wallabag";
const LIST_PATH: &str =
    "/api/entries.json?archive=0&detail=metadata&sort=created&order=desc&perPage=30";
/// The most a list, a single article or a token answer may be, and a book.
const SMALL_FILE: u64 = 1 << 20;
const BOOK_FILE: u64 = 32 << 20;
const TOKEN_ANSWER: usize = 4096;
/// Bytes of a file fed to a reader at a time.
const PIECE: usize = 2048;

const _: () = assert!(MAX_ARTICLES == 30, "the list path asks for 30");

/// The account to fetch for.
#[derive(Debug)]
#[non_exhaustive]
pub struct Login<'a> {
    /// The server as a base address (`https://…`, no `/` at the end).
    pub server: &'a str,
    pub user: &'a str,
    pub client: &'a str,
    pub password: &'a Secret,
    pub secret: &'a Secret,
}

impl<'a> Login<'a> {
    /// The account: the server as a base address, the user name, the API
    /// client's ID and the two secrets.
    #[must_use]
    pub const fn new(
        server: &'a str,
        user: &'a str,
        client: &'a str,
        password: &'a Secret,
        secret: &'a Secret,
    ) -> Self {
        Self {
            server,
            user,
            client,
            password,
            secret,
        }
    }
}

/// How a fetch ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Finish {
    /// The text that says it (a key of the plugin).
    pub key: &'static str,
    /// The articles fetched, archived and failed, where the fetch ran to its
    /// end (the text counts them).
    pub counts: Option<(u32, u32, u32)>,
}

impl Finish {
    /// An end that is a message, with no counts.
    #[must_use]
    pub const fn said(key: &'static str) -> Self {
        Self { key, counts: None }
    }
}

#[derive(Debug, Default)]
enum Phase {
    #[default]
    Idle,
    Login,
    Archive(String),
    List,
    Check(String),
    Download(Entry),
}

#[derive(Debug)]
struct Run {
    server: String,
    account: String,
    token: Option<Secret>,
    pending: Option<RequestId>,
    phase: Phase,
    /// Articles to archive, then to ask about (those fetched that are not in
    /// the list), then to download (the list's, oldest last).
    archive: Vec<String>,
    checks: Vec<String>,
    downloads: Vec<Entry>,
    listed: bool,
    fetched: u32,
    archived: u32,
    failed: u32,
}

/// The services a fetch opens once, and the fetch in progress.
#[derive(Debug, Default)]
pub struct Fetcher {
    net: Option<Net>,
    library: Option<Library>,
    run: Option<Run>,
}

/// A form for the login: fields percent-encoded as in a query, zeroed when
/// dropped (it holds the password and the client's secret).
struct Form(Vec<u8>);

impl Form {
    fn new() -> Result<Self, Error> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(MAX_INLINE_BODY)
            .map_err(|_| Error::NoMemory)?;
        Ok(Self(bytes))
    }

    fn put(&mut self, byte: u8) -> Result<(), Error> {
        if self.0.len() >= MAX_INLINE_BODY {
            return Err(Error::NoSpace);
        }
        self.0.push(byte);
        Ok(())
    }

    fn field(&mut self, name: &str, value: &str) -> Result<(), Error> {
        if !self.0.is_empty() {
            self.put(b'&')?;
        }
        for byte in name.bytes().chain(core::iter::once(b'=')) {
            self.put(byte)?;
        }
        for byte in value.bytes() {
            match byte {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    self.put(byte)?;
                }
                b' ' => self.put(b'+')?,
                byte => {
                    self.put(b'%')?;
                    self.put(hex(byte >> 4))?;
                    self.put(hex(byte & 15))?;
                }
            }
        }
        Ok(())
    }
}

/// The upper-case hexadecimal digit of `nibble` (0 to 15).
const fn hex(nibble: u8) -> u8 {
    if nibble < 10 {
        b'0' + nibble
    } else {
        b'A' + nibble - 10
    }
}

impl Drop for Form {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

impl Fetcher {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            net: None,
            library: None,
            run: None,
        }
    }

    /// Whether a fetch is in progress.
    #[must_use]
    pub const fn running(&self) -> bool {
        self.run.is_some()
    }

    fn net(&mut self, host: &Host<'_>) -> Result<&Net, Error> {
        if self.net.is_none() {
            self.net = Some(Net::open(host)?);
        }
        self.net.as_ref().ok_or(Error::State)
    }

    fn library(&mut self, host: &Host<'_>) -> Result<&Library, Error> {
        if self.library.is_none() {
            self.library = Some(Library::open(host)?);
        }
        self.library.as_ref().ok_or(Error::State)
    }

    fn send(&mut self, host: &Host<'_>, request: &Request<'_>) -> Result<RequestId, Error> {
        let (mut input, mut reply) = (buffer(REQUEST_INPUT)?, buffer(REPLY)?);
        let sent = self
            .net(host)?
            .request(host, request, &mut input, &mut reply);
        wipe(&mut input);
        wipe(&mut reply);
        sent
    }

    /// Starts a fetch with a token for the account: `None` while it runs
    /// (nothing happens where one runs already), the end where it cannot start.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn start(
        &mut self,
        host: &Host<'_>,
        files: &dyn Files,
        login: &Login<'_>,
    ) -> Result<Option<Finish>, Error> {
        if self.run.is_some() {
            return Ok(None);
        }
        // What a fetch cut short left (a power cut, a stop): up to 32 MiB.
        for name in [ENTRIES, ENTRY, TMP] {
            lists::remove(files, name);
        }
        let run = Run {
            server: copy(login.server)?,
            account: lists::account(login.server, login.user)?,
            token: None,
            pending: None,
            phase: Phase::Login,
            archive: Vec::new(),
            checks: Vec::new(),
            downloads: Vec::new(),
            listed: false,
            fetched: 0,
            archived: 0,
            failed: 0,
        };
        let sent = self.login(host, login, &run.server);
        match sent {
            Ok(id) => {
                self.run = Some(Run {
                    pending: Some(id),
                    ..run
                });
                Ok(None)
            }
            Err(Error::NoMemory) => Err(Error::NoMemory),
            // No network service or right to it; a request that cannot be made.
            Err(Error::Unsupported | Error::Denied) => {
                Ok(Some(Finish::said("wallabag.no-network")))
            }
            Err(Error::NoSpace | Error::Invalid) => Ok(Some(Finish::said("wallabag.setup-needed"))),
            Err(_) => Ok(Some(Finish::said("wallabag.unreachable"))),
        }
    }

    fn login(
        &mut self,
        host: &Host<'_>,
        login: &Login<'_>,
        server: &str,
    ) -> Result<RequestId, Error> {
        let mut form = Form::new()?;
        form.field("grant_type", "password")?;
        form.field("client_id", login.client)?;
        form.field("client_secret", login.secret.as_str())?;
        form.field("username", login.user)?;
        form.field("password", login.password.as_str())?;
        let url = concat(&[server, "/oauth/v2/token"])?;
        let headers = [
            Header::new("Content-Type", "application/x-www-form-urlencoded")?,
            Header::new("User-Agent", USER_AGENT)?,
        ];
        let request = Request::new(REQUEST_TOKEN, "POST", &url)?
            .headers(&headers)?
            .body(&form.0)?
            .max_body(u32::try_from(TOKEN_ANSWER).map_err(|_| Error::Invalid)?);
        self.send(host, &request)
    }

    /// Cancels the fetch (its `http_done` is ignored) and clears what it left.
    pub fn cancel(&mut self, host: &Host<'_>, files: &dyn Files) {
        if self.run.as_ref().is_some_and(|run| run.pending.is_some())
            && let (Some(net), Ok(mut input), Ok(mut reply)) =
                (self.net.as_ref(), buffer(REQUEST_INPUT), buffer(REPLY))
        {
            match net.cancel(host, REQUEST_TOKEN, &mut input, &mut reply) {
                Ok(()) | Err(_) => {}
            }
        }
        self.end(files);
    }

    fn end(&mut self, files: &dyn Files) -> Option<Run> {
        for name in [ENTRIES, ENTRY, TMP] {
            lists::remove(files, name);
        }
        self.run.take()
    }

    fn finish_with(&mut self, files: &dyn Files, key: &'static str) -> Finish {
        self.end(files);
        Finish::said(key)
    }

    /// What the end of a request says; `None` for one that is not the fetch's.
    ///
    /// # Errors
    /// `NoMemory`; the fetch is over then.
    pub fn done(
        &mut self,
        host: &Host<'_>,
        files: &dyn Files,
        done: &HttpDone,
    ) -> Result<Option<Finish>, Error> {
        let Some(run) = self.run.as_mut() else {
            return Ok(None);
        };
        if done.token() != REQUEST_TOKEN || run.pending != Some(done.id()) {
            return Ok(None);
        }
        run.pending = None;
        let phase = core::mem::take(&mut run.phase);
        let stepped = match phase {
            Phase::Idle => Ok(None),
            Phase::Login => self.logged(host, files, done),
            Phase::Archive(id) => self.archived(host, files, done, &id),
            Phase::List => self.listed(host, files, done),
            Phase::Check(id) => self.checked(host, files, done, &id),
            Phase::Download(entry) => self.downloaded(host, files, done, &entry),
        };
        if stepped.is_err() {
            self.end(files);
        }
        stepped
    }

    fn run(&mut self) -> Result<&mut Run, Error> {
        self.run.as_mut().ok_or(Error::State)
    }

    fn logged(
        &mut self,
        host: &Host<'_>,
        files: &dyn Files,
        done: &HttpDone,
    ) -> Result<Option<Finish>, Error> {
        let token = if done.succeeded() {
            self.read_token(host, done)?
        } else {
            None
        };
        let Some(token) = token else {
            let key = match done.reason() {
                Reason::Ok => "wallabag.bad-login",
                Reason::NoNetwork | Reason::Offline | Reason::Saving | Reason::NetworkOff => {
                    "wallabag.no-network"
                }
                _ => "wallabag.unreachable",
            };
            return Ok(Some(self.finish_with(files, key)));
        };
        let run = self.run()?;
        run.token = Some(token);
        let loaded = lists::of_account(files, &run.account);
        let (fetched, finished) = match loaded {
            Ok(lists) => lists,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(Some(self.finish_with(files, "wallabag.unreadable"))),
        };
        let run = self.run()?;
        for id in finished {
            if fetched.iter().any(|a| a.id == id) {
                push(&mut run.archive, id)?;
            }
        }
        self.next(host, files)
    }

    /// The token in the memory body of the login's answer, read in pieces.
    fn read_token(&mut self, host: &Host<'_>, done: &HttpDone) -> Result<Option<Secret>, Error> {
        let length = usize::try_from(done.body_len()).map_err(|_| Error::NoSpace)?;
        if length > TOKEN_ANSWER {
            return Ok(None);
        }
        let (mut input, mut reply) = (buffer(REQUEST_INPUT)?, buffer(REPLY)?);
        let net = self.net(host)?;
        let mut reading = Token::new();
        let mut offset = 0usize;
        let result = loop {
            if offset >= length {
                break Ok(());
            }
            let at = u32::try_from(offset).map_err(|_| Error::NoSpace)?;
            match net.body(host, done.id(), at, &mut input, &mut reply) {
                Ok([]) => break Ok(()),
                Ok(part) => {
                    let take = part.len().min(BODY_PART).min(length - offset);
                    let fed = part
                        .get(..take)
                        .ok_or(Error::Invalid)
                        .and_then(|p| reading.feed(p));
                    if let Err(error) = fed {
                        break Err(error);
                    }
                    offset += take;
                }
                Err(error) => break Err(error),
            }
        };
        wipe(&mut input);
        wipe(&mut reply);
        match result {
            Ok(()) => reading.finish(),
            // A body that cannot be read is no token; memory is another thing.
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(None),
        }
    }

    fn archived(
        &mut self,
        host: &Host<'_>,
        files: &dyn Files,
        done: &HttpDone,
        id: &str,
    ) -> Result<Option<Finish>, Error> {
        if done.succeeded() {
            self.run()?.archived += 1;
            self.remove_article(host, files, id)?;
        } else {
            self.run()?.failed += 1;
        }
        self.next(host, files)
    }

    fn listed(
        &mut self,
        host: &Host<'_>,
        files: &dyn Files,
        done: &HttpDone,
    ) -> Result<Option<Finish>, Error> {
        self.run()?.listed = true;
        // Only a whole list counts: an answer that is not one (a page of the
        // server's own, an error, a list cut short) changes nothing.
        let unread = if done.succeeded() {
            read_file(files, ENTRIES, Unread::new(), Unread::feed, Unread::finish)?
        } else {
            None
        };
        let fetched = match lists::articles(files) {
            Ok(list) => Some(list),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => None,
        };
        let (Some(unread), Some(fetched)) = (unread.flatten(), fetched) else {
            self.run()?.failed += 1;
            return self.next(host, files);
        };
        let run = self.run()?;
        // Those not in the list may be archived or deleted elsewhere, or just
        // older than the newest: each is asked about before its book goes.
        for article in &fetched {
            if !unread.iter().any(|e| e.id == article.id) {
                push(&mut run.checks, copy(&article.id)?)?;
            }
        }
        // The list is newest first: popped from the end, the oldest come
        // first, so the newest are the most recent books.
        for entry in unread {
            if !fetched.iter().any(|a| a.id == entry.id) {
                push(&mut run.downloads, entry)?;
            }
        }
        self.next(host, files)
    }

    /// An article fetched but not among the newest unread: its book goes only
    /// when Wallabag says it is archived or no longer there.
    fn checked(
        &mut self,
        host: &Host<'_>,
        files: &dyn Files,
        done: &HttpDone,
        id: &str,
    ) -> Result<Option<Finish>, Error> {
        let gone = if done.answered() && done.status() == 404 {
            Some(true)
        } else if done.succeeded() {
            read_file(
                files,
                ENTRY,
                Archived::new(id),
                Archived::feed,
                Archived::finish,
            )?
            .flatten()
        } else {
            None
        };
        match gone {
            Some(true) => self.remove_article(host, files, id)?,
            Some(false) => {}
            None => self.run()?.failed += 1,
        }
        lists::remove(files, ENTRY);
        self.next(host, files)
    }

    fn downloaded(
        &mut self,
        host: &Host<'_>,
        files: &dyn Files,
        done: &HttpDone,
        entry: &Entry,
    ) -> Result<Option<Finish>, Error> {
        // The id goes into a file's name: a number only.
        let kept = if done.succeeded() && is_id(&entry.id) {
            self.keep_book(host, files, entry)?
        } else {
            false
        };
        let run = self.run()?;
        if kept {
            run.fetched += 1;
        } else {
            run.failed += 1;
            lists::remove(files, TMP);
        }
        self.next(host, files)
    }

    /// Puts the book of `entry` into the library as `name.epub`, or as
    /// `name (id).epub` where that is taken, by a book or by an article fetched
    /// before (whose book the reader may have deleted); a free name in any
    /// case, so no book is replaced; and notes it. `false` where it was not
    /// kept (the book stays as any other where it is not noted).
    fn keep_book(
        &mut self,
        host: &Host<'_>,
        files: &dyn Files,
        entry: &Entry,
    ) -> Result<bool, Error> {
        let mut list = match lists::articles(files) {
            Ok(list) => list,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(false),
        };
        let name = if entry.title.is_empty() {
            &entry.id
        } else {
            &entry.title
        };
        let plain = concat(&[name, ".epub"])?;
        let with_id = concat(&[name, " (", &entry.id, ").epub"])?;
        let listed = |file: &str| {
            list.iter().any(|a| {
                a.path
                    .rsplit('/')
                    .next()
                    .is_some_and(|last| last.eq_ignore_ascii_case(file))
            })
        };
        let first = if listed(&plain) {
            None
        } else {
            match self.import(host, &plain, Existing::Refuse) {
                Ok(path) => Some(path),
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                // Taken (`State`) or refused: the name with the id.
                Err(_) => None,
            }
        };
        let path = match first {
            Some(path) => path,
            None if listed(&with_id) => return Ok(false),
            None => match self.import(host, &with_id, Existing::FreeName) {
                Ok(path) => path,
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => return Ok(false),
            },
        };
        push(
            &mut list,
            Article {
                id: copy(&entry.id)?,
                path,
            },
        )?;
        match lists::save_articles(files, &list) {
            Ok(()) => Ok(true),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(false),
        }
    }

    fn import(&mut self, host: &Host<'_>, name: &str, existing: Existing) -> Result<String, Error> {
        let (mut input, mut out) = (buffer(2048)?, buffer(4096)?);
        let file = Import::new(TMP, name).folder(FOLDER).existing(existing);
        let library = self.library(host)?;
        let path = library.import(host, &file, &mut input, &mut out)?;
        copy(path)
    }

    /// Removes an article's book and what is noted about it (nothing from a
    /// list that cannot be read). A book with the reader's annotations stays
    /// (the library says so), as does one another article in the list is at
    /// too (an older version gave two articles one name): the article is only
    /// let go of.
    fn remove_article(
        &mut self,
        host: &Host<'_>,
        files: &dyn Files,
        id: &str,
    ) -> Result<(), Error> {
        match lists::articles(files) {
            Ok(mut list) => {
                if let Some(at) = list.iter().position(|a| a.id == id) {
                    let article = list.remove(at);
                    if !list.iter().any(|a| a.path == article.path) {
                        self.delete_book(host, &article.path)?;
                    }
                    settle(lists::save_articles(files, &list))?;
                }
            }
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => {}
        }
        match lists::finished(files) {
            Ok(mut ids) => {
                if let Some(at) = ids.iter().position(|f| f == id) {
                    ids.remove(at);
                    settle(lists::save_finished(files, &ids))?;
                }
            }
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => {}
        }
        Ok(())
    }

    /// Takes the book at `path` out of the library, never with the reader's
    /// marks (`with_notes` false: the row says `State` then and the book
    /// stays); a book that cannot be told about stays too.
    fn delete_book(&mut self, host: &Host<'_>, path: &str) -> Result<(), Error> {
        let (mut input, mut out) = (buffer(4096)?, buffer(4096)?);
        let library = match self.library(host) {
            Ok(library) => library,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(()),
        };
        match library.delete(host, &[path], false, &mut input, &mut out) {
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Ok(_) | Err(_) => Ok(()),
        }
    }

    /// The next step: archive, list, ask about articles, download, or done.
    fn next(&mut self, host: &Host<'_>, files: &dyn Files) -> Result<Option<Finish>, Error> {
        loop {
            let run = self.run()?;
            let phase = if let Some(id) = run.archive.pop() {
                Phase::Archive(id)
            } else if !run.listed {
                Phase::List
            } else if let Some(id) = run.checks.pop() {
                Phase::Check(id)
            } else if let Some(entry) = run.downloads.pop() {
                Phase::Download(entry)
            } else {
                return Ok(Some(self.finish(files)));
            };
            match self.launch(host, &phase) {
                Ok(id) => {
                    let run = self.run()?;
                    run.pending = Some(id);
                    run.phase = phase;
                    return Ok(None);
                }
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                // A step that cannot be started is a failed one.
                Err(_) => {
                    let run = self.run()?;
                    if matches!(phase, Phase::List) {
                        run.listed = true;
                    }
                    run.failed += 1;
                }
            }
        }
    }

    fn finish(&mut self, files: &dyn Files) -> Finish {
        match self.end(files) {
            Some(run) => Finish {
                key: if run.failed > 0 {
                    "wallabag.done-failed"
                } else {
                    "wallabag.done"
                },
                counts: Some((run.fetched, run.archived, run.failed)),
            },
            None => Finish::said("wallabag.unreadable"),
        }
    }

    fn launch(&mut self, host: &Host<'_>, phase: &Phase) -> Result<RequestId, Error> {
        let (method, path, body, save): (&str, String, &[u8], Option<(&str, u64)>) = match phase {
            Phase::Archive(id) => (
                "PATCH",
                concat(&["/api/entries/", id, ".json"])?,
                b"archive=1",
                None,
            ),
            Phase::List => ("GET", copy(LIST_PATH)?, b"", Some((ENTRIES, SMALL_FILE))),
            Phase::Check(id) => (
                "GET",
                concat(&["/api/entries/", id, ".json"])?,
                b"",
                Some((ENTRY, SMALL_FILE)),
            ),
            Phase::Download(entry) => (
                "GET",
                concat(&["/api/entries/", &entry.id, "/export.epub"])?,
                b"",
                Some((TMP, BOOK_FILE)),
            ),
            Phase::Idle | Phase::Login => return Err(Error::State),
        };
        let run = self.run.as_ref().ok_or(Error::State)?;
        let url = concat(&[&run.server, &path])?;
        let bearer = Secret::bearer(run.token.as_ref().ok_or(Error::State)?)?;
        let headers = [
            Header::new("Authorization", bearer.as_str())?,
            Header::new("User-Agent", USER_AGENT)?,
            Header::new("Content-Type", "application/x-www-form-urlencoded")?,
        ];
        let used = headers
            .get(..if body.is_empty() { 2 } else { 3 })
            .ok_or(Error::Invalid)?;
        let mut request = Request::new(REQUEST_TOKEN, method, &url)?.headers(used)?;
        if !body.is_empty() {
            request = request.body(body)?;
        }
        if let Some((file, cap)) = save {
            request = request.save_to(FileRoot::Data, file)?.max_file(cap)?;
        }
        self.send(host, &request)
    }
}

/// A failed write is let go of (the files stay as they were); memory is not.
fn settle(result: Result<(), Error>) -> Result<(), Error> {
    match result {
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Ok(()) | Err(_) => Ok(()),
    }
}

/// Reads the data file `name` into `reader` in pieces and asks for its result.
/// `Ok(None)` where the file cannot be read (a failed answer); `NoMemory`
/// ends the reading.
fn read_file<R, T>(
    files: &dyn Files,
    name: &str,
    mut reader: R,
    feed: fn(&mut R, &[u8]) -> Result<(), Error>,
    finish: fn(R) -> Result<T, Error>,
) -> Result<Option<T>, Error> {
    let mut piece = buffer(PIECE)?;
    let mut offset = 0u64;
    loop {
        let read = match files.read_at(name, offset, &mut piece) {
            Ok(0) => break,
            Ok(read) => read,
            Err(_) => return Ok(None),
        };
        feed(&mut reader, piece.get(..read).ok_or(Error::Invalid)?)?;
        offset += read as u64;
    }
    finish(reader).map(Some)
}
