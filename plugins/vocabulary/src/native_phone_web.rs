//! The requests of the phone page on interface 7 (interface 6: `phone::answer`):
//! the page, the decks and a page of cards as JSON, and making, changing and
//! deleting decks and cards, many at once from a pasted list or a file. Over a
//! [`Storage`] and host-free: the shell gives the data folder, and the answer is
//! small data it sends with `core.net` `respond`; allocation is fallible, so
//! a heap that refuses answers *503* and the callback ends well.
//!
//! Every request carries the key shown on the device (`k=`); without it the
//! answer is *403*, and after a few wrong keys *429* for a while (the SDK's
//! `served::Guard`).
use alloc::string::String;

use paperpaw_sdk::native::Error;
use paperpaw_sdk::served::{Access, Guard};
use paperpaw_sdk::{Instant, Storage};
use paperpaw_ui_kit::mem::buffer;
use paperpaw_ui_kit::text::{copy, write_to};

use crate::cards::{self, Problem, Texts};

/// Cards listed at a time, at most, and the bytes of a page of cards, about.
const PAGE: usize = 50;
const PAGE_BYTES: usize = 12 * 1024;
/// The longest form taken (a card's texts at their longest).
pub(crate) const MAX_FORM: usize = 16 * 1024;
/// The longest import taken, in bytes.
pub(crate) const MAX_IMPORT: u64 = 4 << 20;
/// The page's file in the data folder, for `core.net` to send.
pub(crate) const PAGE_FILE: &str = "phone-page.html";
/// The files of long answers, in the data folder, for the same. The host reads
/// the file in parts after the callback and up to two requests of the server
/// are open at once, so each long answer has a file of its own, in turn: the
/// one written four answers ago is long sent.
pub(crate) const ANSWER_FILES: [&str; 4] = [
    "phone-answer-0.json",
    "phone-answer-1.json",
    "phone-answer-2.json",
    "phone-answer-3.json",
];

const JSON: &str = "application/json";
const PLAIN: &str = "text/plain; charset=utf-8";
const HTML: &str = "text/html; charset=utf-8";

/// What an answer carries.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Body {
    /// Words kept in the plugin.
    Text(&'static str),
    /// Words (JSON) made for this request.
    Owned(String),
    /// The page, from the plugin's data folder ([`PAGE_FILE`]).
    Page,
}

/// The answer to one request.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Answer {
    pub(crate) status: u16,
    pub(crate) kind: &'static str,
    pub(crate) body: Body,
}

/// What a request changed, to tell on the reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Done {
    Deck(String),
    Card(String),
    Deleted,
    Imported(usize),
}

/// A request to answer.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Request<'a> {
    pub(crate) method: &'a str,
    pub(crate) target: &'a str,
    /// Where its body was saved (a name the storage takes), if it had one.
    pub(crate) body: Option<&'a str>,
    /// When it came.
    pub(crate) at: Instant,
}

/// What is done about a request besides its answer.
pub(crate) type Outcome = (Answer, Option<Done>);

/// Why a request is refused: the status and the words.
#[derive(Debug)]
struct Fail(u16, &'static str);

impl From<Error> for Fail {
    fn from(error: Error) -> Self {
        match error {
            Error::NoMemory => Fail(503, "busy, try again"),
            _ => Fail(500, "The reader could not do it."),
        }
    }
}

impl From<Problem> for Fail {
    fn from(problem: Problem) -> Self {
        match problem {
            Problem::Missing => Fail(404, "No such deck or card: reload the page."),
            Problem::Full => Fail(409, "This deck or the reader is full: make another deck."),
            Problem::Changed => Fail(409, "The card was changed meanwhile: reload the page."),
            Problem::Empty => Fail(422, "A name or a front is needed."),
            Problem::Storage(_) => Fail(500, "The reader could not save it."),
        }
    }
}

impl Answer {
    /// Plain words as the answer.
    pub(crate) fn text(status: u16, words: &'static str) -> Self {
        Self {
            status,
            kind: PLAIN,
            body: Body::Text(words),
        }
    }
}

fn text(status: u16, words: &'static str) -> Answer {
    Answer::text(status, words)
}

fn json(body: Body) -> Answer {
    Answer {
        status: 200,
        kind: JSON,
        body,
    }
}

/// Answers `request` if `guard` lets it in, and says what it changed.
pub(crate) fn answer(storage: &dyn Storage, guard: &mut Guard, request: &Request<'_>) -> Outcome {
    let (path, query) = request
        .target
        .split_once('?')
        .unwrap_or((request.target, ""));
    let given = match param(query, "k") {
        Ok(given) => given,
        Err(error) => {
            let Fail(status, words) = Fail::from(error);
            return (text(status, words), None);
        }
    };
    match guard.check(given.as_deref(), request.at) {
        Access::Granted => {}
        Access::Locked => {
            return (
                text(429, "Too many wrong keys: try again in a minute."),
                None,
            );
        }
        // Denied, and a state to come: refused as a wrong key is.
        _ => {
            let words = "The key is missing or wrong: scan the code shown on the reader.";
            return (text(403, words), None);
        }
    }
    match route(storage, request, path, query) {
        Ok(done) => done,
        Err(Fail(status, words)) => (text(status, words), None),
    }
}

/// A number among the parameters.
fn number(query: &str, name: &str) -> Result<Option<u32>, Error> {
    Ok(param(query, name)?.and_then(|n| n.parse().ok()))
}

fn route(
    storage: &dyn Storage,
    request: &Request<'_>,
    path: &str,
    query: &str,
) -> Result<Outcome, Fail> {
    let done = |done: Done| -> Result<Outcome, Fail> { Ok((json(Body::Text("{}")), Some(done))) };
    match (request.method, path) {
        ("GET", "/") => Ok((
            Answer {
                status: 200,
                kind: HTML,
                body: Body::Page,
            },
            None,
        )),
        ("GET", "/api/decks") => Ok((json(Body::Owned(decks_json(storage)?)), None)),
        ("GET", "/api/cards") => {
            let deck = number(query, "deck")?.ok_or(Problem::Missing)?;
            let from = number(query, "from")?.unwrap_or(0) as usize;
            Ok((json(Body::Owned(cards_json(storage, deck, from)?)), None))
        }
        ("POST", "/api/deck") => {
            let form = form(storage, request)?;
            let name = param(&form, "name")?.unwrap_or_default();
            let id = match param(&form, "deck")?.and_then(|d| d.parse().ok()) {
                Some(deck) => cards::rename_deck(storage, deck, &name).map(|()| deck)?,
                None => cards::add_deck(storage, &name)?,
            };
            let name = copy(&cards::clean_or_empty(&name, cards::MAX_NAME))?;
            let mut body = String::new();
            write_to(&mut body, format_args!("{{\"id\":{id}}}"))?;
            Ok((json(Body::Owned(body)), Some(Done::Deck(name))))
        }
        ("POST", "/api/deck/delete") => {
            let deck = number(query, "deck")?.ok_or(Problem::Missing)?;
            cards::delete_deck(storage, deck)?;
            done(Done::Deleted)
        }
        ("POST", "/api/card") => {
            let deck = number(query, "deck")?.ok_or(Problem::Missing)?;
            done(Done::Card(change_card(storage, request, deck)?))
        }
        ("POST", "/api/card/delete") => {
            let (deck, id) = (number(query, "deck")?, number(query, "id")?);
            let (Some(deck), Some(id)) = (deck, id) else {
                return Err(Problem::Missing.into());
            };
            cards::delete(storage, deck, id, number(query, "v")?)?;
            done(Done::Deleted)
        }
        ("POST", "/api/import") => import(storage, request, number(query, "deck")?),
        _ => Ok((text(404, "not found"), None)),
    }
}

/// A card made or changed from the form; its front as said on the reader.
fn change_card(storage: &dyn Storage, request: &Request<'_>, deck: u32) -> Result<String, Fail> {
    let form = form(storage, request)?;
    let field =
        |name: &str| -> Result<String, Error> { Ok(param(&form, name)?.unwrap_or_default()) };
    let texts = Texts {
        front: field("front")?,
        back: field("back")?,
        example: field("example")?,
    };
    let front = copy(&cards::clean_or_empty(&texts.front, cards::MAX_TEXT))?;
    let seen = param(&form, "v")?.and_then(|v| v.parse().ok());
    match param(&form, "id")?.and_then(|i| i.parse().ok()) {
        Some(id) => cards::edit(storage, deck, id, seen, &texts)?,
        None => match cards::add(storage, deck, [texts]) {
            Ok(0) => return Err(Problem::Empty.into()),
            Ok(_) => {}
            Err(problem) => return Err(problem.into()),
        },
    }
    Ok(front)
}

/// Many cards at once: a card a line, `front<TAB>back<TAB>example`.
fn import(
    storage: &dyn Storage,
    request: &Request<'_>,
    deck: Option<u32>,
) -> Result<Outcome, Fail> {
    match (deck, request.body) {
        (Some(_), Some(file)) if storage.size(file).is_ok_and(|s| s > MAX_IMPORT) => Ok((
            text(
                413,
                "Too many cards at once for the reader: send them in parts.",
            ),
            None,
        )),
        (Some(deck), Some(file)) => {
            let added = cards::add_from(storage, deck, file)?;
            let mut body = String::new();
            write_to(&mut body, format_args!("{{\"added\":{added}}}"))?;
            Ok((json(Body::Owned(body)), Some(Done::Imported(added))))
        }
        (Some(_), None) => Ok((json(Body::Text("{\"added\":0}")), None)),
        _ => Err(Problem::Missing.into()),
    }
}

/// The form of a request: its body when it is text and at most [`MAX_FORM`]
/// long, else nothing (the missing parts are then refused as empty).
fn form(storage: &dyn Storage, request: &Request<'_>) -> Result<String, Error> {
    let Some(file) = request.body else {
        return Ok(String::new());
    };
    match storage.size(file) {
        Ok(size) if size <= MAX_FORM as u64 => {}
        _ => return Ok(String::new()),
    }
    let mut bytes = buffer(MAX_FORM)?;
    let read = storage.read_at(file, 0, &mut bytes).unwrap_or(0);
    match core::str::from_utf8(bytes.get(..read).unwrap_or(&[])) {
        Ok(text) => copy(text),
        Err(_) => Ok(String::new()),
    }
}

fn push(out: &mut String, more: &str) -> Result<(), Error> {
    out.try_reserve(more.len()).map_err(|_| Error::NoMemory)?;
    out.push_str(more);
    Ok(())
}

/// `text` as a JSON string, safe inside a page's `<script>` too.
pub(crate) fn push_json(out: &mut String, text: &str) -> Result<(), Error> {
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

/// `{"decks": [{"id", "name", "cards"}]}`; no decks where they cannot be read.
fn decks_json(storage: &dyn Storage) -> Result<String, Error> {
    let sizes = cards::sizes(storage).unwrap_or_default();
    let decks = cards::decks(storage).unwrap_or_default();
    let mut out = String::new();
    push(&mut out, "{\"decks\":[")?;
    for (i, deck) in decks.iter().enumerate() {
        if i > 0 {
            push(&mut out, ",")?;
        }
        write_to(&mut out, format_args!("{{\"id\":{},\"name\":", deck.id))?;
        push_json(&mut out, &deck.name)?;
        let n = sizes.get(&deck.id).copied().unwrap_or(0);
        write_to(&mut out, format_args!(",\"cards\":{n}}}"))?;
    }
    push(&mut out, "]}")?;
    Ok(out)
}

/// A page of `deck`'s cards from number `from`: at most [`PAGE`] cards and about
/// [`PAGE_BYTES`], written straight into the answer.
fn cards_json(storage: &dyn Storage, deck: u32, from: usize) -> Result<String, Error> {
    let mut out = String::new();
    push(&mut out, "{\"cards\":[")?;
    let (mut shown, mut failed) = (0, Ok(()));
    let total = cards::visit_page(storage, deck, from, |c| {
        let line = (|| {
            if shown > 0 {
                push(&mut out, ",")?;
            }
            write_to(
                &mut out,
                format_args!("{{\"id\":{},\"v\":{},\"front\":", c.state.id, c.version()),
            )?;
            push_json(&mut out, &c.front)?;
            push(&mut out, ",\"back\":")?;
            push_json(&mut out, &c.back)?;
            push(&mut out, ",\"example\":")?;
            push_json(&mut out, &c.example)?;
            write_to(
                &mut out,
                format_args!(",\"due\":{},\"reps\":{}}}", c.state.due, c.state.reps),
            )
        })();
        if let Err(error) = line {
            failed = Err(error);
            return false;
        }
        shown += 1;
        shown < PAGE && out.len() < PAGE_BYTES
    });
    failed?;
    write_to(&mut out, format_args!("],\"total\":{total}}}"))?;
    Ok(out)
}

/// Parameter `name` of a query (`a=1&b=x%20y`), decoded, as `served::param`
/// reads it; `None` where there is none.
///
/// # Errors
/// `NoMemory`.
fn param(query: &str, name: &str) -> Result<Option<String>, Error> {
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
    // Text sent by a browser is UTF-8; bytes that are not make no value.
    Ok(String::from_utf8(out).ok())
}

#[cfg(test)]
mod tests;
