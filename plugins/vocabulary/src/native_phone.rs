//! *Edit with your phone* on interface 7, host-free: the page the phone gets
//! and the answers to its requests. The rules are those of the interface-6
//! `phone.rs` (the page's own code is the same file, `data/phone.txt`):
//!
//! - `GET /`: the page;
//! - `GET /api/decks`: `{"decks": [{"id", "name", "cards"}]}`;
//! - `GET /api/cards?deck=&from=`: `{"cards": [{"id", "v", "front", "back",
//!   "example", "due", "reps"}], "total"}`, [`PAGE`] at a time (`v` the card's
//!   [`version`](crate::cards::Card::version));
//! - `POST /api/deck` (form: `name`, and `deck` to rename one): `{"id"}`;
//!   `POST /api/deck/delete?deck=`;
//! - `POST /api/card?deck=` (form: `front`, `back`, `example`, and `id` and `v`
//!   to change one); `POST /api/card/delete?deck=&id=&v=`: a card changed since
//!   `v` was sent is left as it is (409);
//! - `POST /api/import?deck=`: a card a line, `front<TAB>back<TAB>example`.
//!
//! Every request carries the key shown on the device (`k=`): without it *403*,
//! and *429* for a minute after five wrong ones (the SDK's `served::Guard`). A
//! form longer than [`MAX_FORM`] or an import longer than [`MAX_IMPORT`] is
//! refused (*413*) before it is read; what a request sends is read from the file
//! the host saved it in. Allocation is fallible: a request that cannot be made
//! is answered *503* and never fails the plugin.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::served::{Access, Guard};
use paperpaw_sdk::{Instant, Storage, StorageError};
use paperpaw_ui_kit::text::{copy, write_to};

use crate::cards::{self, Card, Problem, Texts};

/// The page, as the plugin writes it into its data folder.
pub(crate) const PAGE_FILE: &str = "phone.html";
/// The plugin's number for the phone's server.
pub(crate) const SERVER: u32 = 1;
/// The port asked for; the host takes the next free one if it is in use.
pub(crate) const PORT: u16 = 8080;
/// Cards listed at a time, at most.
pub(crate) const PAGE: usize = 50;
/// Bytes of a page of cards, about: fewer cards where they are long.
const PAGE_BYTES: usize = 12 * 1024;
/// The longest form taken (a card's texts at their longest).
pub(crate) const MAX_FORM: u64 = 16 * 1024;
/// The longest import taken, in bytes (the server's `max_body` is this).
pub(crate) const MAX_IMPORT: u64 = 4 << 20;
/// Bytes of an answer sent from memory (`docs/abi/network.md` 5.3), with room
/// to spare: a longer one is written to a file and sent from it.
pub(crate) const INLINE: usize = 2_000;

/// The web page; its texts, and its limits, are filled in where it marks them.
const HTML: &str = include_str!("../data/phone.txt");
const STRINGS_MARK: &str = "/*STRINGS*/{}";
const IMPORT_MARK: &str = "/*MAX_IMPORT*/0";
const TEXT_MARK: &str = "/*MAX_TEXT*/0";

/// The keys of the page's texts, as it names them.
pub(crate) const TEXTS: [(&str, &str); 36] = [
    ("title", "vocabulary.web.title"),
    ("decks", "vocabulary.web.decks"),
    ("newDeck", "vocabulary.web.new-deck"),
    ("deckName", "vocabulary.web.deck-name"),
    ("add", "vocabulary.web.add"),
    ("save", "vocabulary.web.save"),
    ("cancel", "vocabulary.web.cancel"),
    ("rename", "vocabulary.web.rename"),
    ("delete", "vocabulary.web.delete"),
    ("deleteDeck", "vocabulary.web.delete-deck"),
    ("deleteCard", "vocabulary.web.delete-card"),
    ("cards", "vocabulary.web.cards"),
    ("noCards", "vocabulary.web.no-cards"),
    ("noDecks", "vocabulary.web.no-decks"),
    ("front", "vocabulary.web.front"),
    ("back", "vocabulary.web.back"),
    ("example", "vocabulary.web.example"),
    ("newCard", "vocabulary.web.new-card"),
    ("edit", "vocabulary.web.edit"),
    ("more", "vocabulary.web.more"),
    ("import", "vocabulary.web.import"),
    ("importHelp", "vocabulary.web.import-help"),
    ("paste", "vocabulary.web.paste"),
    ("header", "vocabulary.web.header"),
    ("file", "vocabulary.web.file"),
    ("send", "vocabulary.web.send"),
    ("read", "vocabulary.web.read"),
    ("imported", "vocabulary.web.imported"),
    ("failed", "vocabulary.web.failed"),
    ("newerAnki", "vocabulary.web.newer-anki"),
    ("noNotes", "vocabulary.web.no-notes"),
    ("tooLarge", "vocabulary.web.too-large"),
    ("wrongType", "vocabulary.web.wrong-type"),
    ("notUtf8", "vocabulary.web.not-utf8"),
    ("partial", "vocabulary.web.partial"),
    ("full", "vocabulary.web.full"),
];

/// What an answer carries.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Body {
    /// Words kept in the plugin.
    Text(&'static str),
    /// Words made for this request.
    Owned(String),
    /// The page, [`PAGE_FILE`] in the data folder, sent whole.
    Page,
}

/// The answer to one request.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Answer {
    pub status: u16,
    pub kind: &'static str,
    pub body: Body,
}

/// A request to answer.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Request<'a> {
    pub method: &'a str,
    pub target: &'a str,
    /// Where its body was saved (a file of the data folder), and how long it is.
    pub body: Option<&'a str>,
    pub body_len: u64,
    /// When it came.
    pub at: Instant,
}

/// What a request changed, to tell on the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Done {
    Deck(String),
    Card(String),
    Deleted,
    Imported(usize),
}

/// Why a request is refused: the status and the words.
#[derive(Debug)]
struct Fail(u16, &'static str);

const MISSING: Fail = Fail(404, "No such deck or card: reload the page.");
const BUSY: Fail = Fail(503, "failed, try again");

impl From<Error> for Fail {
    fn from(error: Error) -> Self {
        match error {
            Error::NoMemory => BUSY,
            Error::NotFound => MISSING,
            Error::Utf8 => Fail(400, "The text is not valid text."),
            _ => Fail(500, "The reader could not do it."),
        }
    }
}

impl From<StorageError> for Fail {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::OutOfMemory | StorageError::TooLarge => BUSY,
            StorageError::NotFound => MISSING,
            _ => Fail(500, "The reader could not save it."),
        }
    }
}

impl From<Problem> for Fail {
    fn from(problem: Problem) -> Self {
        match problem {
            Problem::Missing => MISSING,
            Problem::Full => Fail(409, "This deck or the reader is full: make another deck."),
            Problem::Changed => Fail(409, "The card was changed meanwhile: reload the page."),
            Problem::Empty => Fail(422, "A name or a front is needed."),
            Problem::Storage(error) => Fail::from(error),
        }
    }
}

const JSON: &str = "application/json";
const PLAIN: &str = "text/plain; charset=utf-8";

fn text(status: u16, words: &'static str) -> Answer {
    Answer {
        status,
        kind: PLAIN,
        body: Body::Text(words),
    }
}

fn json(body: String) -> Answer {
    Answer {
        status: 200,
        kind: JSON,
        body: Body::Owned(body),
    }
}

/// The answer to `request`, if `guard` lets it in, and what it changed.
pub(crate) fn answer(
    guard: &mut Guard,
    storage: &dyn Storage,
    request: &Request<'_>,
) -> (Answer, Option<Done>) {
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
    match route(storage, request, path, query) {
        Ok(done) => done,
        Err(Fail(status, words)) => (text(status, words), None),
    }
}

/// Lets a request in if it carries the key shown on the device: *403* for a
/// missing or wrong one, *429* while the guard is locked.
fn gate(guard: &mut Guard, query: &str, at: Instant) -> Result<(), Answer> {
    let given = match param(query, "k") {
        Ok(given) => given,
        Err(error) => {
            let Fail(status, words) = Fail::from(error);
            return Err(text(status, words));
        }
    };
    match guard.check(given.as_deref(), at) {
        Access::Granted => Ok(()),
        Access::Locked => Err(text(429, "Too many wrong keys: try again in a minute.")),
        // Denied, and a state to come: refused as a wrong key is.
        _ => Err(text(
            403,
            "The key is missing or wrong: scan the code shown on the reader.",
        )),
    }
}

type Outcome = Result<(Answer, Option<Done>), Fail>;

fn route(storage: &dyn Storage, request: &Request<'_>, path: &str, query: &str) -> Outcome {
    let number_of = |name: &str| -> Result<Option<u32>, Fail> {
        Ok(param(query, name)?.and_then(|n| n.parse::<u32>().ok()))
    };
    match (request.method, path) {
        ("GET", "/") => Ok((
            Answer {
                status: 200,
                kind: "text/html; charset=utf-8",
                body: Body::Page,
            },
            None,
        )),
        ("GET", "/api/decks") => Ok((json(decks_json(storage)?), None)),
        ("GET", "/api/cards") => {
            let deck = number_of("deck")?.ok_or(MISSING)?;
            let from = number_of("from")?.unwrap_or(0) as usize;
            Ok((json(cards_json(storage, deck, from)?), None))
        }
        ("POST", "/api/deck") => deck(storage, request),
        ("POST", "/api/deck/delete") => {
            let deck = number_of("deck")?.ok_or(MISSING)?;
            cards::delete_deck(storage, deck)?;
            Ok((json(copy("{}")?), Some(Done::Deleted)))
        }
        ("POST", "/api/card") => {
            let deck = number_of("deck")?.ok_or(MISSING)?;
            card(storage, request, deck)
        }
        ("POST", "/api/card/delete") => {
            let (deck, id) = (number_of("deck")?, number_of("id")?);
            let (Some(deck), Some(id)) = (deck, id) else {
                return Err(MISSING);
            };
            cards::delete(storage, deck, id, number_of("v")?)?;
            Ok((json(copy("{}")?), Some(Done::Deleted)))
        }
        ("POST", "/api/import") => {
            let deck = number_of("deck")?.ok_or(MISSING)?;
            import(storage, request, deck)
        }
        _ => Ok((text(404, "not found"), None)),
    }
}

/// The form a request sent: `None` for none; too long is refused before it
/// is read (*413*).
fn form(storage: &dyn Storage, request: &Request<'_>) -> Result<Option<String>, Fail> {
    let Some(file) = request.body else {
        return Ok(None);
    };
    if request.body_len > MAX_FORM {
        return Err(Fail(
            413,
            "That is too much for the reader: send less at once.",
        ));
    }
    let bytes = paperpaw_sdk::read_file(storage, file, MAX_FORM as usize)?;
    // Text that is not valid is refused as a request that is not understood.
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| Fail(400, "The text is not valid text."))
}

/// A field of the form, empty where it has none.
fn field(form: &str, name: &str) -> Result<String, Fail> {
    Ok(param(form, name)?.unwrap_or_default())
}

fn deck(storage: &dyn Storage, request: &Request<'_>) -> Outcome {
    let form = form(storage, request)?.unwrap_or_default();
    let name = field(&form, "name")?;
    let renamed = param(&form, "deck")?.and_then(|d| d.parse::<u32>().ok());
    let id = match renamed {
        Some(deck) => cards::rename_deck(storage, deck, &name).map(|()| deck)?,
        None => cards::add_deck(storage, &name)?,
    };
    let shown = cards::clean_or_empty(&name, cards::MAX_NAME);
    let mut body = String::new();
    write_to(&mut body, format_args!("{{\"id\":{id}}}"))?;
    Ok((json(body), Some(Done::Deck(shown))))
}

fn card(storage: &dyn Storage, request: &Request<'_>, deck: u32) -> Outcome {
    let form = form(storage, request)?.unwrap_or_default();
    let texts = Texts {
        front: field(&form, "front")?,
        back: field(&form, "back")?,
        example: field(&form, "example")?,
    };
    let front = cards::clean_or_empty(&texts.front, cards::MAX_TEXT);
    let seen = param(&form, "v")?.and_then(|v| v.parse::<u32>().ok());
    match param(&form, "id")?.and_then(|i| i.parse::<u32>().ok()) {
        Some(id) => cards::edit(storage, deck, id, seen, &texts)?,
        None => {
            if cards::add(storage, deck, [texts])? == 0 {
                return Err(Problem::Empty.into());
            }
        }
    }
    Ok((json(copy("{}")?), Some(Done::Card(front))))
}

fn import(storage: &dyn Storage, request: &Request<'_>, deck: u32) -> Outcome {
    let Some(file) = request.body else {
        return Ok((json(copy("{\"added\":0}")?), None));
    };
    if request.body_len > MAX_IMPORT {
        return Ok((
            text(
                413,
                "Too many cards at once for the reader: send them in parts.",
            ),
            None,
        ));
    }
    let added = cards::add_from(storage, deck, file)?;
    let mut body = String::new();
    write_to(&mut body, format_args!("{{\"added\":{added}}}"))?;
    Ok((json(body), Some(Done::Imported(added))))
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

fn decks_json(storage: &dyn Storage) -> Result<String, Fail> {
    let sizes = cards::sizes(storage)?;
    let mut out = String::new();
    push(&mut out, "{\"decks\":[")?;
    for (i, deck) in cards::decks(storage)?.iter().enumerate() {
        if i > 0 {
            push(&mut out, ",")?;
        }
        write_to(&mut out, format_args!("{{\"id\":{},\"name\":", deck.id))?;
        push_json(&mut out, &deck.name)?;
        let cards = sizes.get(&deck.id).copied().unwrap_or(0);
        write_to(&mut out, format_args!(",\"cards\":{cards}}}"))?;
    }
    push(&mut out, "]}")?;
    Ok(out)
}

fn push_card(out: &mut String, card: &Card) -> Result<(), Error> {
    write_to(
        out,
        format_args!(
            "{{\"id\":{},\"v\":{},\"front\":",
            card.state.id,
            card.version()
        ),
    )?;
    push_json(out, &card.front)?;
    push(out, ",\"back\":")?;
    push_json(out, &card.back)?;
    push(out, ",\"example\":")?;
    push_json(out, &card.example)?;
    write_to(
        out,
        format_args!(",\"due\":{},\"reps\":{}}}", card.state.due, card.state.reps),
    )
}

/// A page of `deck`'s cards from number `from`: at most [`PAGE`] cards and
/// about [`PAGE_BYTES`], written straight into the answer.
fn cards_json(storage: &dyn Storage, deck: u32, from: usize) -> Result<String, Fail> {
    let mut out = String::new();
    push(&mut out, "{\"cards\":[")?;
    let (mut shown, mut failed) = (0, None);
    let total = cards::visit_page(storage, deck, from, |card| {
        let made = (if shown > 0 {
            push(&mut out, ",")
        } else {
            Ok(())
        })
        .and_then(|()| push_card(&mut out, card));
        if let Err(error) = made {
            failed = Some(error);
            return false;
        }
        shown += 1;
        shown < PAGE && out.len() < PAGE_BYTES
    });
    if let Some(error) = failed {
        return Err(error.into());
    }
    write_to(&mut out, format_args!("],\"total\":{total}}}"))?;
    Ok(out)
}

/// Parameter `name` of a query (`a=1&b=x%20y`), decoded, as `served::param`
/// reads it; `None` where there is none. Bytes that are not UTF-8 are refused
/// as a text that is not valid.
///
/// # Errors
/// `NoMemory`, `Utf8`.
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
    let mut out = Vec::new();
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
    String::from_utf8(out).map(Some).map_err(|_| Error::Utf8)
}

/// The page with its texts (`strings`, a JSON object of [`TEXTS`]) and its
/// limits, made in one pass.
///
/// # Errors
/// `NoMemory`, `Invalid` for a page that lacks the mark of its texts.
pub(crate) fn assemble(strings: &str) -> Result<Vec<u8>, Error> {
    let (mut import, mut text) = (String::new(), String::new());
    write_to(&mut import, format_args!("{MAX_IMPORT}"))?;
    write_to(&mut text, format_args!("{}", cards::MAX_TEXT))?;
    let filled = [
        (STRINGS_MARK, strings),
        (IMPORT_MARK, import.as_str()),
        (TEXT_MARK, text.as_str()),
    ];
    let room: usize = filled.iter().map(|(_, value)| value.len()).sum();
    let mut page = Vec::new();
    page.try_reserve_exact(HTML.len() + room)
        .map_err(|_| Error::NoMemory)?;
    let mut rest = HTML;
    for (mark, value) in filled {
        // Each search starts where the last value ended: a value may hold a mark.
        let Some(at) = rest.find(mark) else {
            if mark == STRINGS_MARK {
                return Err(Error::Invalid);
            }
            continue;
        };
        let (head, tail) = rest.split_at(at);
        page.extend_from_slice(head.as_bytes());
        page.extend_from_slice(value.as_bytes());
        rest = tail.split_at(mark.len()).1;
    }
    page.extend_from_slice(rest.as_bytes());
    Ok(page)
}

#[cfg(test)]
mod tests;
