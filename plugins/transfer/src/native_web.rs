//! The requests of the web page on interface 7: the page itself, listing,
//! downloading, uploading, making folders, moving, renaming and deleting. The rules of `web.rs`
//! over the card as `core.files` shows it (root 1: the host leaves out
//! `plugins` and `.paperpaw` and checks every path by identity), host-free: the
//! card is a [`Card`], allocation is fallible, and an answer is small data the
//! plugin sends with `core.net` `respond`.
//!
//! Every request carries the key shown on the device (`k=`); without it the
//! answer is *403*, and after a few wrong keys *429* for a while (the SDK's
//! `served::Guard`). Paths are plain names ([`files::is_plain_name`]: nothing
//! hidden, no `..`); the settings file can be fetched but not changed
//! ([`read_only`]). Installing plugins from the computer is `native_install`.
//! Moving, deleting and putting books are the library's where the file is a
//! book (the card does it: `native_files`), so what the library keeps for a
//! book goes along (`PORTING-7.md`).
use alloc::string::String;
use paperpaw_sdk::Instant;
use paperpaw_sdk::files;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::served::{Access, Guard};
use paperpaw_ui_kit::text::{concat, copy, decimal_u64, write_to};

use crate::native_install::{Installs, Plugins};
use crate::native_transfer::read_only;

/// The page, as the plugin writes it into its data folder.
pub const PAGE_FILE: &str = "page.html";
/// Bytes of an inline answer at most (`docs/abi/network.md` 5.3), with room to
/// spare: a folder with more entries is listed in parts.
pub const INLINE: usize = 2_000;
/// Numbered names tried for a name that is taken (`name (2)`, ...).
const FREE_NAMES: u32 = 1_000;

/// What the transfer needs of the card (`core.files` root 1 and the request
/// bodies in the data folder).
pub trait Card {
    /// What is at `path`: whether a folder, and its size; `None` where there
    /// is nothing.
    ///
    /// # Errors
    /// The host's refusal (`Invalid` for a path it does not take).
    fn stat(&mut self, path: &str) -> Result<Option<(bool, u64)>, Error>;
    /// Hands the entries of `dir` that can be named (hidden ones included) to
    /// `take` in the storage's order, until it says `false`.
    ///
    /// # Errors
    /// What `take` or the host refuses.
    fn list(
        &mut self,
        dir: &str,
        take: &mut dyn FnMut(&str, bool, u64) -> Result<bool, Error>,
    ) -> Result<(), Error>;
    /// Makes the folder `path`.
    ///
    /// # Errors
    /// The host's refusal.
    fn make_dir(&mut self, path: &str) -> Result<(), Error>;
    /// Removes the file or folder at `path` and all in it (`is_dir`): a book
    /// with all that is kept for it, a folder with the books in it (the host
    /// forgets them).
    ///
    /// # Errors
    /// The host's refusal (`Busy` for a path it uses).
    fn remove(&mut self, path: &str, is_dir: bool) -> Result<(), Error>;
    /// Moves the file or folder `from` to `to` (`is_dir`: a folder), never over
    /// what is there; a book keeps what is kept for it. The folder of `to` is
    /// there.
    ///
    /// # Errors
    /// The host's refusal: `State` where `to` is there, `Busy` while the host
    /// uses a path or moves another, `Denied`, `Full`.
    fn move_to(&mut self, from: &str, to: &str, is_dir: bool) -> Result<(), Error>;
    /// Bytes free for new files; `None` while not known.
    ///
    /// # Errors
    /// The host's refusal.
    fn free(&mut self) -> Result<Option<u64>, Error>;
    /// Puts what a request sent (`body`, a file of the data folder; none for an
    /// empty one) at `path`, never over what is there; a book into the
    /// library's folder is catalogued at once. Gives the path it was put at
    /// (the card takes the next free name if `path` was taken meanwhile).
    ///
    /// # Errors
    /// The host's refusal (`Full` for a full card).
    fn put(&mut self, body: Option<&str>, path: &str) -> Result<String, Error>;
}

/// What an answer carries.
#[derive(Debug, PartialEq, Eq)]
pub enum Body {
    /// Words kept in the plugin.
    Text(&'static str),
    /// Words made for this request.
    Owned(String),
    /// A file of the plugin's data folder (the page, [`PAGE_FILE`]; the list of
    /// the plugins; the phone's page), sent whole.
    Data(&'static str),
    /// A file of the card, sent whole.
    File(String),
}

/// The answer to one request.
#[derive(Debug, PartialEq, Eq)]
pub struct Answer {
    pub status: u16,
    pub kind: &'static str,
    pub body: Body,
}

/// What a request changed, to tell on the device: the kind and the name of
/// what was received, sent, made or deleted (or not received).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Done {
    Received,
    Sent,
    Folder,
    Deleted,
    /// Moved or renamed: the name it has now.
    Moved,
    NotReceived,
}

/// A request to answer.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    pub method: &'a str,
    pub target: &'a str,
    /// Where its body was saved (a file of the data folder), and how long it is.
    pub body: Option<&'a str>,
    pub body_len: u64,
    /// When it came.
    pub at: Instant,
}

/// What is done about a request besides its answer.
pub type Outcome = (Answer, Option<(Done, String)>);

const NOT_FOUND: Fail = Fail(404, "not found");
const BAD_PATH: Fail = Fail(400, "bad path");
const READ_ONLY: Fail = Fail(403, "PaperPaw's settings cannot be changed from here.");
/// Fetching the settings is refused too: defence in depth (interface 7 keeps
/// secrets in `core.secrets`, but a card from Beta 3 can still hold plain ones
/// until its first load moves them out).
const NO_FETCH: Fail = Fail(403, "PaperPaw's settings cannot be fetched from here.");
const BAD_NAME: Fail = Fail(400, "bad name");
const INTO_ITSELF: Fail = Fail(400, "a folder cannot go into itself");

/// Why a request is refused: the status and the words.
#[derive(Debug)]
struct Fail(u16, &'static str);

impl From<Error> for Fail {
    fn from(error: Error) -> Self {
        match error {
            Error::NotFound => NOT_FOUND,
            Error::Invalid | Error::NoSpace | Error::Utf8 => BAD_PATH,
            Error::Full => Fail(507, "no room on the card"),
            // The host uses the path (a book being read, a move going on).
            Error::Busy => Fail(409, "busy, try again"),
            Error::State => Fail(409, "exists"),
            Error::Denied => Fail(403, "not allowed"),
            Error::NoMemory => Fail(503, "busy, try again"),
            _ => Fail(500, "failed"),
        }
    }
}

fn text(status: u16, words: &'static str) -> Answer {
    Answer {
        status,
        kind: "text/plain; charset=utf-8",
        body: Body::Text(words),
    }
}

fn ok(done: Option<(Done, String)>) -> Outcome {
    let answer = Answer {
        status: 200,
        kind: "application/json",
        body: Body::Text("{\"ok\":true}"),
    };
    (answer, done)
}

/// The host's parts of answering: the card, and the plugins sent from the
/// computer with what is known of them.
pub struct Parts<'a> {
    pub card: &'a mut dyn Card,
    pub plugins: &'a mut dyn Plugins,
    pub installs: &'a mut Installs,
}

/// Answers `request`, if `guard` lets it in, and says what it changed.
pub fn answer(parts: &mut Parts<'_>, guard: &mut Guard, request: &Request<'_>) -> Outcome {
    let (path, query) = request
        .target
        .split_once('?')
        .unwrap_or((request.target, ""));
    if path == "/favicon.ico" {
        return (text(404, "not found"), None);
    }
    if let Err(refused) = gate(guard, query, request.at) {
        return (refused, None);
    }
    match route(parts, request, path, query) {
        Ok(done) => done,
        Err(refused) => {
            // An upload that failed is said on the device too, so the last
            // success is not taken for it.
            let upload = request.method == "PUT" && path == "/api/file";
            let done = upload.then(|| not_received(query)).flatten();
            (fail_answer(refused), done)
        }
    }
}

/// Lets a request in if it carries the key shown on the device: *403* for a
/// missing or wrong one, *429* while the guard is locked.
pub(crate) fn gate(guard: &mut Guard, query: &str, at: Instant) -> Result<(), Answer> {
    let given = match param(query, "k") {
        Ok(given) => given,
        Err(error) => return Err(fail_answer(Fail::from(error))),
    };
    match guard.check(given.as_deref(), at) {
        Access::Granted => Ok(()),
        Access::Locked => Err(text(429, "Too many wrong keys: try again in a minute.")),
        // Denied, and a state to come: refused as a wrong key is.
        _ => Err(text(
            403,
            "The key is missing or wrong: open the address shown on the device.",
        )),
    }
}

fn fail_answer(Fail(status, words): Fail) -> Answer {
    text(status, words)
}

fn not_received(query: &str) -> Option<(Done, String)> {
    let wanted = param(query, "path").ok()??;
    let name = copy(files::name(wanted.trim_matches('/'))).ok()?;
    Some((Done::NotReceived, name))
}

fn route(
    parts: &mut Parts<'_>,
    request: &Request<'_>,
    path: &str,
    query: &str,
) -> Result<Outcome, Fail> {
    if Installs::is_install_request(path) {
        let answer = parts
            .installs
            .answer(&mut *parts.plugins, request, path, query)?;
        return Ok((answer, None));
    }
    let card = &mut *parts.card;
    let wanted = param(query, "path")?.unwrap_or_default();
    match (request.method, path) {
        ("GET", "/") => Ok((
            Answer {
                status: 200,
                kind: "text/html; charset=utf-8",
                body: Body::Data(PAGE_FILE),
            },
            None,
        )),
        ("GET", "/api/list") => {
            let from = param(query, "from")?.and_then(|from| from.parse().ok());
            listing(card, &wanted, from.unwrap_or(0)).map(|answer| (answer, None))
        }
        ("GET", "/api/file") => {
            let file = locate(card, &wanted, false)?;
            if file.is_dir != Some(false) {
                return Err(NOT_FOUND);
            }
            if read_only(&file.path) {
                return Err(NO_FETCH);
            }
            let name = copy(files::name(&file.path))?;
            let answer = Answer {
                status: 200,
                kind: "application/octet-stream",
                body: Body::File(file.path),
            };
            Ok((answer, Some((Done::Sent, name))))
        }
        ("PUT", "/api/file") => upload(card, request, &wanted),
        ("POST", "/api/mkdir") => {
            let at = free_place(card, &wanted)?;
            let name = copy(files::name(&at))?;
            card.make_dir(&at)?;
            Ok(ok(Some((Done::Folder, name))))
        }
        ("POST", "/api/delete") => {
            let found = locate(card, &wanted, false)?;
            // Not everything at once.
            if found.path.is_empty() {
                return Err(NOT_FOUND);
            }
            if read_only(&found.path) {
                return Err(READ_ONLY);
            }
            let name = copy(files::name(&found.path))?;
            card.remove(&found.path, found.is_dir == Some(true))?;
            Ok(ok(Some((Done::Deleted, name))))
        }
        ("POST", "/api/move") => relocate(card, query, &wanted),
        _ => Err(NOT_FOUND),
    }
}

/// A free place for what `wanted` names, in the folder it names there (which
/// must be there), under its name or `name (2)`; not the top, not the settings.
fn free_place(card: &mut dyn Card, wanted: &str) -> Result<String, Fail> {
    let target = locate(card, wanted, true)?;
    if target.path.is_empty() {
        return Err(BAD_PATH);
    }
    let at = free_path(card, files::parent(&target.path), files::name(&target.path))?;
    if read_only(&at) {
        return Err(READ_ONLY);
    }
    Ok(at)
}

/// One file put on the card: in a free place, if the card has room.
fn upload(card: &mut dyn Card, request: &Request<'_>, wanted: &str) -> Result<Outcome, Fail> {
    // The folder is not there: a bad path, as the page names it.
    let at = match free_place(card, wanted) {
        Err(Fail(404, _)) => return Err(Fail(400, "bad path: the folder is not there")),
        other => other?,
    };
    if card.free()?.is_some_and(|free| request.body_len > free) {
        return Err(Fail(507, "no room on the card"));
    }
    let put = card.put(request.body, &at)?;
    // Done: a line that cannot be kept is not a failed upload.
    let done = copy(files::name(&put))
        .ok()
        .map(|name| (Done::Received, name));
    Ok(ok(done))
}

/// One file or folder moved or renamed: from where it is to a folder there
/// (`to`, which must be there), under its name or `name`; never over what is
/// there (`name (2)` then), a folder never into itself, the settings neither
/// moved nor a target, and the same place no change.
fn relocate(card: &mut dyn Card, query: &str, wanted: &str) -> Result<Outcome, Fail> {
    let from = locate(card, wanted, false)?;
    // Not everything at once.
    if from.path.is_empty() {
        return Err(NOT_FOUND);
    }
    if read_only(&from.path) {
        return Err(READ_ONLY);
    }
    let to = param(query, "to")?.unwrap_or_default();
    let folder = locate(card, &to, false)?;
    if folder.is_dir != Some(true) {
        return Err(NOT_FOUND);
    }
    let name = match param(query, "name")? {
        Some(name) => name,
        None => copy(files::name(&from.path))?,
    };
    if !files::is_plain_name(&name) {
        return Err(BAD_NAME);
    }
    if same_name(&join(&folder.path, &name)?, &from.path) {
        return Ok(ok(None));
    }
    if from.is_dir == Some(true) && is_within(&folder.path, &from.path) {
        return Err(INTO_ITSELF);
    }
    let at = free_path(card, &folder.path, &name)?;
    if read_only(&at) {
        return Err(READ_ONLY);
    }
    let name = copy(files::name(&at))?;
    card.move_to(&from.path, &at, from.is_dir == Some(true))?;
    Ok(ok(Some((Done::Moved, name))))
}

/// Whether `path` is the folder `dir` or lies in it, as the card names things
/// (case is ignored); every path lies in `""`.
fn is_within(path: &str, dir: &str) -> bool {
    let mut parts = path.split('/');
    dir.is_empty()
        || dir
            .split('/')
            .all(|d| parts.next().is_some_and(|p| same_name(p, d)))
}

/// Whether `a` and `b` are one name for the card, which ignores case.
fn same_name(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(char::to_lowercase)
        .eq(b.chars().flat_map(char::to_lowercase))
}

/// The entries of the folder `wanted` from the `from`th on (hidden ones left
/// out), as JSON that fits an inline answer: `{"entries":[{"name":…,"dir":…,
/// "size":…},…]}` with `"next":n` where more follow from the `n`th. In the
/// storage's order; the page sorts them.
fn listing(card: &mut dyn Card, wanted: &str, from: usize) -> Result<Answer, Fail> {
    let dir = locate(card, wanted, false)?;
    if dir.is_dir != Some(true) {
        return Err(NOT_FOUND);
    }
    let mut json = String::new();
    push(&mut json, "{\"entries\":[")?;
    let (mut index, mut taken, mut next) = (0usize, 0usize, None);
    card.list(&dir.path, &mut |name, is_dir, size| {
        if name.starts_with('.') {
            return Ok(true);
        }
        let at = index;
        index += 1;
        if at < from {
            return Ok(true);
        }
        let mut entry = String::new();
        push(&mut entry, "{\"name\":")?;
        push_json(&mut entry, name)?;
        push(
            &mut entry,
            if is_dir {
                ",\"dir\":true"
            } else {
                ",\"dir\":false"
            },
        )?;
        push(&mut entry, ",\"size\":")?;
        push(&mut entry, decimal_u64(size, &mut [0; 20]))?;
        push(&mut entry, "}")?;
        // The end and `next` need up to 32 bytes.
        if json.len() + 1 + entry.len() + 32 > INLINE {
            if taken > 0 {
                next = Some(at);
                return Ok(false);
            }
            // Not even one fits (a name of many escapes): left out.
            return Ok(true);
        }
        if taken > 0 {
            push(&mut json, ",")?;
        }
        taken += 1;
        push(&mut json, &entry)?;
        Ok(true)
    })?;
    push(&mut json, "]")?;
    if let Some(next) = next {
        push(&mut json, ",\"next\":")?;
        push(&mut json, decimal_u64(next as u64, &mut [0; 20]))?;
    }
    push(&mut json, "}")?;
    Ok(Answer {
        status: 200,
        kind: "application/json",
        body: Body::Owned(json),
    })
}

/// Where a request's path leads.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Found {
    /// The path on the card; `""` for the top.
    path: String,
    /// Whether a folder is there; `None` where nothing is yet.
    is_dir: Option<bool>,
}

/// Finds `wanted`, from a request, part by part: each part is a plain name
/// ([`files::is_plain_name`]) of something there, a folder but for the last.
/// With `new`, the last may also be a name not there yet. The host checks each
/// path by identity (`plugins` and `.paperpaw` are never reached, by any spelling).
fn locate(card: &mut dyn Card, wanted: &str, new: bool) -> Result<Found, Fail> {
    let wanted = wanted.trim_matches('/');
    let mut found = Found {
        path: String::new(),
        is_dir: Some(true),
    };
    if wanted.is_empty() {
        return Ok(found);
    }
    if !wanted.split('/').all(files::is_plain_name) {
        return Err(BAD_PATH);
    }
    let mut parts = wanted.split('/').peekable();
    while let Some(part) = parts.next() {
        if found.is_dir != Some(true) {
            return Err(NOT_FOUND);
        }
        found.path = join(&found.path, part)?;
        found.is_dir = match card.stat(&found.path)? {
            Some((is_dir, _)) => Some(is_dir),
            None if new && parts.peek().is_none() => None,
            None => return Err(NOT_FOUND),
        };
    }
    Ok(found)
}

/// A free path for `name` in `dir`: `name`, else `name (2)`, `name (3)`, …
/// (before the extension).
fn free_path(card: &mut dyn Card, dir: &str, name: &str) -> Result<String, Fail> {
    let first = join(dir, name)?;
    if card.stat(&first)?.is_none() {
        return Ok(first);
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, ext),
        _ => (name, ""),
    };
    for n in 2..=FREE_NAMES {
        let mut numbered = String::new();
        write_to(&mut numbered, format_args!("{stem} ({n})"))?;
        if !ext.is_empty() {
            push(&mut numbered, ".")?;
            push(&mut numbered, ext)?;
        }
        let path = join(dir, &numbered)?;
        if card.stat(&path)?.is_none() {
            return Ok(path);
        }
    }
    Err(Fail(507, "no free name"))
}

/// `dir/name`, or `name` at the top.
fn join(dir: &str, name: &str) -> Result<String, Error> {
    if dir.is_empty() {
        copy(name)
    } else {
        concat(&[dir, "/", name])
    }
}

fn push(out: &mut String, more: &str) -> Result<(), Error> {
    out.try_reserve(more.len()).map_err(|_| Error::NoMemory)?;
    out.push_str(more);
    Ok(())
}

/// `text` as a JSON string, safe inside a page's `<script>` too.
pub fn push_json(out: &mut String, text: &str) -> Result<(), Error> {
    push(out, "\"")?;
    for c in text.chars() {
        match c {
            '"' => push(out, "\\\"")?,
            '\\' => push(out, "\\\\")?,
            '<' => push(out, "\\u003c")?,
            c if (c as u32) < 0x20 => write_to(out, format_args!("\\u{:04x}", c as u32))?,
            c => {
                let mut bytes = [0; 4];
                push(out, c.encode_utf8(&mut bytes))?;
            }
        }
    }
    push(out, "\"")
}

/// Parameter `name` of a query (`a=1&b=x%20y`), decoded, as `served::param`
/// reads it; `None` where there is none.
///
/// # Errors
/// `NoMemory`.
pub(crate) fn param(query: &str, name: &str) -> Result<Option<String>, Error> {
    let Some(value) = query
        .split('&')
        .filter_map(|pair| pair.split_once('=').or(Some((pair, ""))))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
    else {
        return Ok(None);
    };
    let bytes = value.as_bytes();
    let mut out = alloc::vec::Vec::new();
    out.try_reserve_exact(bytes.len())
        .map_err(|_| Error::NoMemory)?;
    let hex = |b: u8| char::from(b).to_digit(16).map(|d| d as u8);
    let mut i = 0;
    while i < bytes.len() {
        let escaped = (bytes[i] == b'%')
            .then(|| Some((hex(*bytes.get(i + 1)?)?, hex(*bytes.get(i + 2)?)?)))
            .flatten();
        match (bytes[i], escaped) {
            (_, Some((high, low))) => {
                out.push(high << 4 | low);
                i += 3;
                continue;
            }
            (b'+', _) => out.push(b' '),
            (byte, _) => out.push(byte),
        }
        i += 1;
    }
    // Bytes that are not UTF-8 are not a path or a key: refused, as a bad name.
    String::from_utf8(out).map(Some).map_err(|_| Error::Utf8)
}

#[cfg(test)]
mod tests;
