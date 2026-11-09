//! The fetch, one request at a time over a [`Wire`] (the data folder, the
//! network, the token and the account) and a [`Shelf`] (the library): the
//! articles read to the end archived in Reader (and their books taken away
//! unless they carry the reader's marks), the list of articles in the place
//! chosen, those fetched earlier that are not in it asked about, and the new
//! ones downloaded as books. The same order and rules as the interface-6 fetch
//! (`Fetch` in `lib.rs`), so that the decisions are tested without a host.
//!
//! - Reader answers twenty list requests a minute: a fetch asks for at most
//!   nineteen (the list, [`MAX_CHECKS`] questions, [`MAX_DOWNLOADS`] books)
//!   and ends where it is told to wait (the next fetch goes on).
//! - The list and the answers about one article are saved to files of the
//!   data folder and read in pieces (`native_answers`); the text of an article
//!   is copied from its answer into the book in pieces too (`Copier`), so an
//!   article of any length needs the same few kilobytes. What Reader sends is
//!   hostile: ids are letters and digits, titles are cut, an answer that is
//!   not whole changes nothing, and one over its cap (`MAX_LIST`,
//!   `MAX_ANSWER`) ends the fetch.
//! - A refused allocation ends the fetch ([`Ended::Memory`]); any other
//!   failure of a step counts as one failed article and the fetch goes on.
use alloc::{string::String, vec::Vec};

use paperpaw_sdk::{
    native::{
        Error,
        net::{Reason, RequestId},
        storage::MAX_DATA,
    },
    web,
};
use paperpaw_ui_kit::{
    files::{Volume, replace_on},
    mem,
    text::{self, concat, copy},
};

use crate::native_answers::{Answer, Asked, Entry, Listed};
use crate::native_lists::{
    FINISHED_FILE, load_articles, load_finished, save_articles, save_finished, settle,
};
use crate::native_read_later::{
    Article, Copied, Copier, MAX_ANSWER, MAX_LIST, TAIL, account, fits, head, is_id, location,
};
use crate::native_secret::Secret;

/// The plugin's own number of its one request, in its `http_done` event.
pub const REQUEST_TOKEN: u32 = 1;
/// The list's answer, in the data folder.
pub const LIST_FILE: &str = "list.json";
/// The answer about one article (or with its text), in the data folder.
pub const DOC_FILE: &str = "doc.json";
/// The book being made, in the data folder.
pub const BOOK_FILE: &str = "article.tmp";
/// Articles asked for in the list. Reader answers twenty list requests a
/// minute (an article's text is one): the list, up to [`MAX_CHECKS`]
/// articles asked about and up to [`MAX_DOWNLOADS`] books come to nineteen.
pub const MAX_LISTED: usize = 20;
/// The most fetched articles asked about in one fetch.
pub const MAX_CHECKS: usize = 8;
/// The most books made in one fetch.
pub const MAX_DOWNLOADS: usize = 10;
/// The most of a name made of a title, in UTF-16 units.
const NAME_UNITS: usize = 80;
/// What the answers saved are limited to, as the request says it (`max_file`).
const LIST_FILE_MAX: u32 = 512 * 1024;
const ANSWER_FILE_MAX: u32 = 4 * 1024 * 1024;
const _: () = assert!(LIST_FILE_MAX as usize == MAX_LIST && ANSWER_FILE_MAX as usize == MAX_ANSWER);
/// Bytes of a saved answer read at a time.
const PIECE: usize = 1024;
/// What an archived article's `location` is changed to.
const ARCHIVE_BODY: &str = r#"{"location":"archive"}"#;

/// Where an answer is saved, and how much of it at most.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Save<'a> {
    /// The file, in the data folder.
    pub file: &'a str,
    /// The most bytes of it.
    pub max: u32,
}

/// A request to Reader: `GET` or `PATCH`, its path on the server (the server,
/// the token and the user agent are the wire's), a body that is JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Call<'a> {
    pub method: &'static str,
    pub path: &'a str,
    pub body: Option<&'static str>,
    /// Where the answer goes; where it is not saved, the wire reads at most a
    /// few bytes of it.
    pub save: Option<Save<'a>>,
}

/// What the fetch needs of the world: the data folder, the network, the token
/// and the account. The host's in the plugin; a stand-in in tests.
pub trait Wire {
    /// The plugin's data folder.
    fn volume(&self) -> &dyn Volume;
    /// Whether the network can be used at all.
    ///
    /// # Errors
    /// `Unsupported` or `Denied` without it.
    fn network(&mut self) -> Result<(), Error>;
    /// The token kept, if there is one.
    ///
    /// # Errors
    /// `NoMemory`; what the secrets service says, except that there is none.
    fn token(&mut self) -> Result<Option<Secret>, Error>;
    /// What tells the accounts apart, as kept beside the token (the secret
    /// `account`, which the user cannot change).
    ///
    /// # Errors
    /// `NoMemory`; what the secrets service says, except that there is none.
    fn account(&mut self) -> Result<Option<String>, Error>;
    /// Keeps what tells the accounts apart.
    ///
    /// # Errors
    /// What the secrets service says.
    fn keep_account(&mut self, account: &str) -> Result<(), Error>;
    /// Sends `call` with `token`, the end told with token [`REQUEST_TOKEN`].
    ///
    /// # Errors
    /// `Unsupported` or `Denied` without the network; what the host says.
    fn send(&mut self, token: &Secret, call: &Call<'_>) -> Result<RequestId, Error>;
    /// Ends the request in flight; one that cannot be ended ends by itself
    /// and is ignored.
    fn cancel(&mut self);
}

/// What the library is to the fetch: where a book goes in, and where it comes
/// out. The host's in the plugin (`core.library`); a stand-in in tests.
pub trait Shelf {
    /// Puts the file `from` (in the data folder) into the folder `Reader` of
    /// the library as `file`, and says where it went. Where `free_name` is
    /// not set a name that is taken is refused, never replaced.
    ///
    /// # Errors
    /// `NoMemory`; any other error says the book is not there.
    fn put(&mut self, from: &str, file: &str, free_name: bool) -> Result<String, Error>;
    /// Takes the book at `path` out of the library; one with the reader's
    /// marks, or one that cannot be told about, stays.
    ///
    /// # Errors
    /// `NoMemory`; every other refusal means the book stays and is let go of.
    fn remove(&mut self, path: &str) -> Result<(), Error>;
}

/// A request that ended, as far as the fetch reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Done {
    /// The request.
    pub id: RequestId,
    /// The plugin's own number of it.
    pub token: u32,
    /// Why it ended.
    pub reason: Reason,
    /// The HTTP status; 0 when none came.
    pub status: u32,
}

impl Done {
    /// The end of the request `id`.
    #[must_use]
    pub const fn new(id: RequestId, token: u32, reason: Reason, status: u32) -> Self {
        Self {
            id,
            token,
            reason,
            status,
        }
    }

    /// An answer with a 2xx status came.
    const fn is_ok(&self) -> bool {
        matches!(self.reason, Reason::Ok) && self.status >= 200 && self.status < 300
    }
}

/// What a start came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Started {
    /// The first request is sent.
    Fetching,
    /// A fetch is under way: nothing happens.
    Running,
    /// There is no network for it.
    NoNetwork,
    /// There is no token.
    SetupNeeded,
    /// The lists could not be read (and are not to be changed).
    Unreadable,
    /// The first request could not be made.
    Unreachable,
}

/// Why a fetch ended before its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Stop {
    /// No network (the Wi-Fi off, the offline mode, the network off for the
    /// plugin).
    NoNetwork,
    /// Reader cannot be reached.
    Unreachable,
    /// Reader does not know the token (401, 403).
    BadToken,
    /// Reader asks to wait (429).
    Wait,
    /// Reader sent more than the device keeps.
    TooBig,
}

/// What the end of a request came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Ended {
    /// Not a request under way (cancelled, replaced, not ours).
    Ignored,
    /// The next request is sent.
    Next,
    /// The fetch is over: the books made, the articles archived and the steps
    /// that failed.
    Finished {
        fetched: u32,
        archived: u32,
        failed: u32,
    },
    /// The fetch is over before its end, asking on would not help.
    Stopped(Stop),
    /// The heap had no room: the fetch is stopped.
    Memory,
}

#[derive(Debug)]
enum Phase {
    Archive(String),
    List,
    Check(String),
    Download(Entry),
}

/// A fetch under way.
#[derive(Debug)]
struct Run {
    token: Secret,
    location: &'static str,
    pending: Option<RequestId>,
    phase: Option<Phase>,
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

impl Run {
    const fn finished(&self) -> Ended {
        Ended::Finished {
            fetched: self.fetched,
            archived: self.archived,
            failed: self.failed,
        }
    }
}

/// The fetch: at most one at a time.
#[derive(Debug, Default)]
pub struct Fetcher {
    run: Option<Run>,
}

/// What a fetch cut short left: up to 4 MiB.
fn clear_files(volume: &dyn Volume) {
    for name in [LIST_FILE, DOC_FILE, BOOK_FILE] {
        volume.remove(name).ok();
    }
}

/// Reads the saved answer `name` in pieces into `feed`, which says whether it
/// has what it needs; an answer over `cap` bytes is not read (`NoSpace`).
fn pieces(
    volume: &dyn Volume,
    name: &str,
    cap: usize,
    feed: &mut dyn FnMut(&[u8]) -> Result<bool, Error>,
) -> Result<(), Error> {
    fits(volume.size(name)?, cap)?;
    let mut piece = mem::buffer(PIECE)?;
    let mut at = 0u64;
    loop {
        let read = volume.read_at(name, at, &mut piece)?;
        if read == 0 {
            return Ok(());
        }
        let got = piece.get(..read).ok_or(Error::Io)?;
        if feed(got)? {
            return Ok(());
        }
        at += read as u64;
    }
}

/// The articles of the list answered, `None` for an answer that is not a
/// whole list.
fn read_list(volume: &dyn Volume) -> Result<Option<Vec<Entry>>, Error> {
    let mut reading = Listed::new(MAX_LISTED);
    pieces(volume, LIST_FILE, MAX_LIST, &mut |p| {
        reading.feed(p).map(|()| false)
    })?;
    reading.finish()
}

/// What the answer in `doc.json` says of article `id`.
fn read_asked(volume: &dyn Volume, id: &str, cap: usize) -> Result<Option<Answer>, Error> {
    let mut reading = Asked::with_cap(id, cap);
    pieces(volume, DOC_FILE, cap, &mut |p| {
        reading.feed(p).map(|()| false)
    })?;
    reading.finish()
}

/// Whether the saved answer is larger than it can be.
fn too_big(volume: &dyn Volume, file: &str, cap: usize) -> bool {
    volume.size(file).is_ok_and(|size| fits(size, cap).is_err())
}

/// Writes `bytes` to a file anew, in as many calls as the storage takes.
fn write_all(volume: &dyn Volume, name: &str, bytes: &[u8]) -> Result<(), Error> {
    let mut parts = bytes.chunks(MAX_DATA);
    volume.write(name, parts.next().unwrap_or_default())?;
    for part in parts {
        volume.append(name, part)?;
    }
    Ok(())
}

fn append_all(volume: &dyn Volume, name: &str, bytes: &[u8]) -> Result<(), Error> {
    for part in bytes.chunks(MAX_DATA) {
        volume.append(name, part)?;
    }
    Ok(())
}

/// The articles fetched and those read to the end, for this token. Those of
/// another token (another account) are let go of first: their books stay in
/// the library, never asked about at a Reader that does not know them.
fn lists_of_account(
    wire: &mut dyn Wire,
    token: &Secret,
) -> Result<(Vec<Article>, Vec<String>), Error> {
    let fetched = load_articles(wire.volume())?;
    let finished = load_finished(wire.volume())?;
    let account = account(token.as_str())?;
    if wire.account()?.as_deref() == Some(account.as_str()) {
        return Ok((fetched, finished));
    }
    if !fetched.is_empty() || !finished.is_empty() {
        save_articles(wire.volume(), &[])?;
        replace_on(wire.volume(), FINISHED_FILE, b"")?;
    }
    wire.keep_account(&account)?;
    Ok((Vec::new(), Vec::new()))
}

/// Removes an article's book and what is noted about it (nothing from a list
/// that cannot be read). A book with the reader's marks stays, let go of (the
/// library says so); so does one another article is at too.
fn remove_article(volume: &dyn Volume, shelf: &mut dyn Shelf, id: &str) -> Result<(), Error> {
    match load_articles(volume) {
        Ok(mut list) => {
            if let Some(at) = list.iter().position(|a| a.id == id) {
                let article = list.remove(at);
                if !list.iter().any(|a| a.path == article.path) {
                    settle(shelf.remove(&article.path))?;
                }
                settle(save_articles(volume, &list))?;
            }
        }
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => {}
    }
    match load_finished(volume) {
        Ok(mut ids) => {
            if let Some(at) = ids.iter().position(|f| f == id) {
                ids.remove(at);
                settle(save_finished(volume, &ids))?;
            }
        }
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => {}
    }
    Ok(())
}

/// The start of an article's book is written, then its text copied from the
/// answer in `doc.json`, then the end: `false` where the answer has no whole
/// text (nothing is a book then).
fn book_file(volume: &dyn Volume, entry: &Entry, author: &str) -> Result<bool, Error> {
    write_all(volume, BOOK_FILE, head(&entry.title, author)?.as_bytes())?;
    let mut copier = Copier::default();
    pieces(volume, DOC_FILE, MAX_ANSWER, &mut |p| {
        copier.feed(p)?;
        append_all(volume, BOOK_FILE, copier.taken())?;
        copier.clear();
        Ok(copier.is_done())
    })?;
    if copier.outcome() != Copied::Whole {
        return Ok(false);
    }
    append_all(volume, BOOK_FILE, TAIL.as_bytes())?;
    Ok(true)
}

/// Puts the book in `article.tmp` into the library as `name.html`, or as
/// `name (id).html` where that is taken, by a book or by an article fetched
/// before (whose book the reader may have deleted); a free name in any case,
/// so no book is replaced; and notes it. Where it is not noted, the book stays
/// as any other.
fn keep_book(volume: &dyn Volume, shelf: &mut dyn Shelf, entry: &Entry) -> Result<bool, Error> {
    let mut list = match load_articles(volume) {
        Ok(list) => list,
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        Err(_) => return Ok(false),
    };
    let cleaned = web::try_clean_name(&entry.title, NAME_UNITS).map_err(|_| Error::NoMemory)?;
    let name = cleaned
        .as_deref()
        .filter(|n| !n.is_empty())
        .unwrap_or(&entry.id);
    let plain = concat(&[name, ".html"])?;
    let with_id = concat(&[name, " (", &entry.id, ").html"])?;
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
        match shelf.put(BOOK_FILE, &plain, false) {
            Ok(path) => Some(path),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            // Taken or refused: the name with the id.
            Err(_) => None,
        }
    };
    let path = match first {
        Some(path) => path,
        None if listed(&with_id) => return Ok(false),
        None => match shelf.put(BOOK_FILE, &with_id, true) {
            Ok(path) => path,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(false),
        },
    };
    mem::push(
        &mut list,
        Article {
            id: copy(&entry.id)?,
            path,
        },
    )?;
    settle(save_articles(volume, &list))?;
    Ok(true)
}

/// Makes the book of `entry` from the answer in `doc.json`: `false` where
/// that fails (nothing is left of it).
fn make_book(volume: &dyn Volume, shelf: &mut dyn Shelf, entry: &Entry) -> Result<bool, Error> {
    // The id goes into a file's name and a list: letters and digits only.
    if !is_id(&entry.id) {
        return Ok(false);
    }
    let author = match read_asked(volume, &entry.id, MAX_ANSWER) {
        Ok(Some(Answer::Known { author, .. })) => author,
        Err(Error::NoMemory) => return Err(Error::NoMemory),
        _ => return Ok(false),
    };
    let made = match book_file(volume, entry, &author) {
        Ok(true) => keep_book(volume, shelf, entry),
        Ok(false) => Ok(false),
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Err(_) => Ok(false),
    };
    // A book put in the library is gone from here; what is left is not a book.
    volume.remove(BOOK_FILE).ok();
    made
}

/// What a request that is not a 2xx answer says of the fetch, if it ends it.
fn stopped(done: &Done, run: &Run) -> Option<Ended> {
    match done.reason {
        Reason::Cancelled | Reason::Replaced => Some(Ended::Ignored),
        // The run's minutes are over: no request is to follow outside it.
        Reason::WorkEnded => Some(run.finished()),
        Reason::Ok => match done.status {
            401 | 403 => Some(Ended::Stopped(Stop::BadToken)),
            429 => Some(Ended::Stopped(Stop::Wait)),
            _ => None,
        },
        Reason::TooLarge => Some(Ended::Stopped(Stop::TooBig)),
        Reason::NoNetwork | Reason::Offline | Reason::Saving | Reason::NetworkOff => {
            Some(Ended::Stopped(Stop::NoNetwork))
        }
        _ => Some(Ended::Stopped(Stop::Unreachable)),
    }
}

impl Fetcher {
    /// A new one, with nothing under way.
    #[must_use]
    pub const fn new() -> Self {
        Self { run: None }
    }

    /// Whether a fetch is under way.
    #[must_use]
    pub const fn busy(&self) -> bool {
        self.run.is_some()
    }

    /// Starts a fetch of the articles of the place `from` names (the setting
    /// `read-later-cloud.from`).
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn start(&mut self, wire: &mut dyn Wire, from: &str) -> Result<Started, Error> {
        if self.run.is_some() {
            return Ok(Started::Running);
        }
        match wire.network() {
            Ok(()) => {}
            Err(Error::Unsupported | Error::Denied) => return Ok(Started::NoNetwork),
            Err(error) => return Err(error),
        }
        let Some(token) = wire.token()?.filter(|t| !t.as_str().is_empty()) else {
            return Ok(Started::SetupNeeded);
        };
        let (fetched, finished) = match lists_of_account(wire, &token) {
            Ok(lists) => lists,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(Started::Unreadable),
        };
        clear_files(wire.volume());
        let mut archive = Vec::new();
        for id in finished {
            if fetched.iter().any(|a| a.id == id) {
                mem::push(&mut archive, id)?;
            }
        }
        let mut run = Run {
            token,
            location: location(from),
            pending: None,
            phase: None,
            archive,
            checks: Vec::new(),
            downloads: Vec::new(),
            listed: false,
            fetched: 0,
            archived: 0,
            failed: 0,
        };
        let sent = Self::next(wire, &mut run);
        match sent {
            Ok(Ended::Next) => {
                self.run = Some(run);
                Ok(Started::Fetching)
            }
            Ok(Ended::Stopped(Stop::NoNetwork)) => Ok(Started::NoNetwork),
            Ok(_) => Ok(Started::Unreachable),
            Err(error) => Err(error),
        }
    }

    /// Cancels the fetch (its end is ignored) and clears what it left.
    pub fn cancel(&mut self, wire: &mut dyn Wire) {
        if self.run.take().is_some() {
            wire.cancel();
            clear_files(wire.volume());
        }
    }

    /// A request ended: its answer is taken and the next request is sent, or
    /// the fetch is over. `Ignored` for one that is not the fetch's.
    pub fn ended(&mut self, wire: &mut dyn Wire, shelf: &mut dyn Shelf, done: &Done) -> Ended {
        let Some(run) = self.run.as_ref() else {
            return Ended::Ignored;
        };
        if done.token != REQUEST_TOKEN || run.pending != Some(done.id) {
            return Ended::Ignored;
        }
        let Some(mut run) = self.run.take() else {
            return Ended::Ignored;
        };
        run.pending = None;
        let Some(phase) = run.phase.take() else {
            return Ended::Ignored;
        };
        let ended = match Self::step(wire, shelf, &mut run, phase, done) {
            Ok(ended) => ended,
            Err(Error::NoMemory) => Ended::Memory,
            Err(_) => Ended::Stopped(Stop::Unreachable),
        };
        if ended == Ended::Next {
            self.run = Some(run);
        } else {
            clear_files(wire.volume());
        }
        ended
    }

    /// Takes the answer to the request of `phase`. Only `NoMemory` is an
    /// error.
    fn step(
        wire: &mut dyn Wire,
        shelf: &mut dyn Shelf,
        run: &mut Run,
        phase: Phase,
        done: &Done,
    ) -> Result<Ended, Error> {
        if let Some(ended) = stopped(done, run) {
            return Ok(ended);
        }
        let ok = done.is_ok();
        let volume = wire.volume();
        match phase {
            Phase::Archive(id) => {
                // Not there any more (404): as good as archived.
                if ok || done.status == 404 {
                    run.archived += 1;
                    remove_article(volume, shelf, &id)?;
                } else {
                    run.failed += 1;
                }
            }
            Phase::List => {
                if ok && too_big(volume, LIST_FILE, MAX_LIST) {
                    return Ok(Ended::Stopped(Stop::TooBig));
                }
                run.listed = true;
                Self::listed(volume, run, ok)?;
            }
            Phase::Check(id) => {
                if ok && too_big(volume, DOC_FILE, MAX_LIST) {
                    return Ok(Ended::Stopped(Stop::TooBig));
                }
                Self::checked(volume, shelf, run, &id, ok)?;
            }
            Phase::Download(entry) => {
                if ok && too_big(volume, DOC_FILE, MAX_ANSWER) {
                    return Ok(Ended::Stopped(Stop::TooBig));
                }
                if ok && make_book(volume, shelf, &entry)? {
                    run.fetched += 1;
                } else {
                    run.failed += 1;
                }
                volume.remove(DOC_FILE).ok();
            }
        }
        Self::next(wire, run)
    }

    /// The list was answered: what to ask about and what to download. Only a
    /// whole list counts: an answer that is not one (a page of the server's
    /// own, an error, a list cut short) changes nothing.
    fn listed(volume: &dyn Volume, run: &mut Run, ok: bool) -> Result<(), Error> {
        let unread = if ok {
            match read_list(volume) {
                Ok(unread) => unread,
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => None,
            }
        } else {
            None
        };
        volume.remove(LIST_FILE).ok();
        let fetched = match load_articles(volume) {
            Ok(list) => Some(list),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => None,
        };
        let (Some(mut unread), Some(fetched)) = (unread, fetched) else {
            run.failed += 1;
            return Ok(());
        };
        // Those not in the list may be archived or deleted elsewhere, or just
        // older than the newest: each is asked about before its book goes. The
        // oldest fetched are asked about first, so that none waits for ever.
        run.checks = mem::collect(
            fetched
                .iter()
                .filter(|a| !unread.iter().any(|e| e.id == a.id))
                .take(MAX_CHECKS)
                .map(|a| copy(&a.id)),
        )?;
        run.checks.reverse();
        // The list is newest first: popped from the end, the oldest of the
        // newest ten come first.
        unread.retain(|e| !fetched.iter().any(|a| a.id == e.id));
        unread.truncate(MAX_DOWNLOADS);
        run.downloads = unread;
        Ok(())
    }

    /// An article fetched but not among the newest in its place: its book
    /// goes only when Reader says it is archived or no longer there.
    fn checked(
        volume: &dyn Volume,
        shelf: &mut dyn Shelf,
        run: &mut Run,
        id: &str,
        ok: bool,
    ) -> Result<(), Error> {
        let answer = if ok {
            match read_asked(volume, id, MAX_LIST) {
                Ok(answer) => answer,
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => None,
            }
        } else {
            None
        };
        match answer {
            Some(Answer::Gone) => remove_article(volume, shelf, id)?,
            Some(Answer::Known { location, .. }) if location == "archive" => {
                remove_article(volume, shelf, id)?;
            }
            Some(Answer::Known { .. }) => {}
            None => run.failed += 1,
        }
        volume.remove(DOC_FILE).ok();
        Ok(())
    }

    /// The next step: archive, list, ask about articles, download, or done.
    fn next(wire: &mut dyn Wire, run: &mut Run) -> Result<Ended, Error> {
        let phase = if let Some(id) = run.archive.pop() {
            Phase::Archive(id)
        } else if !run.listed {
            Phase::List
        } else if let Some(id) = run.checks.pop() {
            Phase::Check(id)
        } else if let Some(entry) = run.downloads.pop() {
            Phase::Download(entry)
        } else {
            return Ok(run.finished());
        };
        match Self::launch(wire, run, &phase) {
            Ok(id) => {
                run.pending = Some(id);
                run.phase = Some(phase);
                Ok(Ended::Next)
            }
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(Error::Unsupported | Error::Denied) => Ok(Ended::Stopped(Stop::NoNetwork)),
            Err(_) => Ok(Ended::Stopped(Stop::Unreachable)),
        }
    }

    fn launch(wire: &mut dyn Wire, run: &Run, phase: &Phase) -> Result<RequestId, Error> {
        let (method, path, body, save) = match phase {
            Phase::Archive(id) => (
                "PATCH",
                concat(&["/api/v3/update/", id, "/"])?,
                Some(ARCHIVE_BODY),
                None,
            ),
            Phase::List => (
                "GET",
                text::format(format_args!(
                    "/api/v3/list/?location={}&category=article&limit={MAX_LISTED}",
                    run.location
                ))?,
                None,
                Some((LIST_FILE, LIST_FILE_MAX)),
            ),
            Phase::Check(id) => (
                "GET",
                concat(&["/api/v3/list/?id=", id])?,
                None,
                Some((DOC_FILE, LIST_FILE_MAX)),
            ),
            Phase::Download(entry) => (
                "GET",
                concat(&["/api/v3/list/?id=", &entry.id, "&withHtmlContent=true"])?,
                None,
                Some((DOC_FILE, ANSWER_FILE_MAX)),
            ),
        };
        let call = Call {
            method,
            path: &path,
            body,
            save: save.map(|(file, max)| Save { file, max }),
        };
        wire.send(&run.token, &call)
    }
}
