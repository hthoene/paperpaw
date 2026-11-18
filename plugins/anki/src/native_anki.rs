//! What is asked of AnkiConnect and what it answers: the requests of its
//! classic API (version 6, a JSON body `{"action", "version", "key", "params"}`
//! to the root of the add-on's address) and the readers of its answers,
//! `{"result": …, "error": …}`, as they stream (nothing larger than a card is
//! ever in memory). Only the protocol is used, as documented by its authors.
//!
//! The classic API is used (not the add-on's newer JSON-RPC one at
//! `/api/v1`) because every version of the add-on answers it: the current one
//! keeps it for existing clients.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::json::stream::{Event, Parser};
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::mem::{push, wipe};
use paperpaw_ui_kit::text::copy;

/// The API version asked for.
pub const VERSION: u32 = 6;
/// The most cards one sync fetches.
pub const MAX_CARDS: usize = 50;
/// The most decks the deck choice lists.
pub const MAX_DECKS: usize = 60;
/// The most answers one request carries (its body stays under the 2,048 bytes
/// a request sends inline).
pub const ANSWERS_PER_REQUEST: usize = 40;
/// The cards asked for in one request at first; halved where the answer is
/// larger than a request may hold.
pub const INFO_BATCH: usize = 3;
/// The longest key that is kept and sent.
pub const MAX_KEY: usize = 256;
/// The longest front and back of a card that are kept, in bytes.
pub const FRONT_MAX: usize = 400;
pub const BACK_MAX: usize = 800;
/// The longest message of Anki that is kept.
const SAID_MAX: usize = 120;

/// A request body, built in one growing buffer that is wiped when it is
/// dropped (it carries the key).
#[derive(Default)]
pub struct Body(Vec<u8>);

impl Body {
    /// The bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    fn put(&mut self, text: &str) -> Result<(), Error> {
        self.0
            .try_reserve(text.len())
            .map_err(|_| Error::NoMemory)?;
        self.0.extend_from_slice(text.as_bytes());
        Ok(())
    }

    fn number(&mut self, value: u64) -> Result<(), Error> {
        let mut digits = [0u8; 20];
        self.put(paperpaw_ui_kit::text::decimal_u64(value, &mut digits))
    }

    /// `text` as a JSON string with its quotes.
    fn string(&mut self, text: &str) -> Result<(), Error> {
        self.put("\"")?;
        for c in text.chars() {
            match c {
                '"' => self.put("\\\"")?,
                '\\' => self.put("\\\\")?,
                c if (c as u32) < 0x20 => {
                    const HEX: &[u8; 16] = b"0123456789abcdef";
                    let n = c as usize;
                    let escape = [b'\\', b'u', b'0', b'0', HEX[(n >> 4) & 15], HEX[n & 15]];
                    self.put(core::str::from_utf8(&escape).map_err(|_| Error::Invalid)?)?;
                }
                c => {
                    let mut utf8 = [0u8; 4];
                    self.put(c.encode_utf8(&mut utf8))?;
                }
            }
        }
        self.put("\"")
    }

    /// The head of a request: the action, the version and the key (where one
    /// is set), the body open at `"params":{`.
    fn head(action: &str, key: Option<&str>) -> Result<Self, Error> {
        let mut body = Self::default();
        body.put("{\"action\":")?;
        body.string(action)?;
        body.put(",\"version\":")?;
        body.number(u64::from(VERSION))?;
        if let Some(key) = key.filter(|k| !k.is_empty()) {
            body.put(",\"key\":")?;
            body.string(key)?;
        }
        body.put(",\"params\":{")?;
        Ok(body)
    }

    fn tail(mut self) -> Result<Self, Error> {
        self.put("}}")?;
        Ok(self)
    }

    /// `deckNames`.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn deck_names(key: Option<&str>) -> Result<Self, Error> {
        Self::head("deckNames", key)?.tail()
    }

    /// `findCards` with `query` (Anki's search syntax).
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn find_cards(key: Option<&str>, query: &str) -> Result<Self, Error> {
        let mut body = Self::head("findCards", key)?;
        body.put("\"query\":")?;
        body.string(query)?;
        body.tail()
    }

    /// `cardsInfo` of `ids`.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn cards_info(key: Option<&str>, ids: &[u64]) -> Result<Self, Error> {
        let mut body = Self::head("cardsInfo", key)?;
        body.put("\"cards\":[")?;
        for (i, id) in ids.iter().enumerate() {
            if i > 0 {
                body.put(",")?;
            }
            body.number(*id)?;
        }
        body.put("]")?;
        body.tail()
    }

    /// `answerCards`: each card with its ease (1 Again to 4 Easy).
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn answer_cards(key: Option<&str>, answers: &[(u64, u8)]) -> Result<Self, Error> {
        let mut body = Self::head("answerCards", key)?;
        body.put("\"answers\":[")?;
        for (i, (id, ease)) in answers.iter().enumerate() {
            if i > 0 {
                body.put(",")?;
            }
            body.put("{\"cardId\":")?;
            body.number(*id)?;
            body.put(",\"ease\":")?;
            body.number(u64::from(*ease))?;
            body.put("}")?;
        }
        body.put("]")?;
        body.tail()
    }
}

impl Drop for Body {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

impl core::fmt::Debug for Body {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Body(..)")
    }
}

/// Which cards a sync asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    /// Cards due now (review and learning).
    Due,
    /// Cards never seen.
    New,
}

/// The search for `which` cards of `deck` (all decks where it is empty), in
/// Anki's syntax: a deck's name has `\`, `"`, `*` and `_` escaped, and
/// suspended and buried cards are left out.
///
/// # Errors
/// `NoMemory`.
pub fn query(deck: &str, which: Which) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve(deck.len() + 64)
        .map_err(|_| Error::NoMemory)?;
    if !deck.is_empty() {
        out.push_str("deck:\"");
        for c in deck.chars() {
            if matches!(c, '\\' | '"' | '*' | '_') {
                out.push('\\');
            }
            out.push(c);
        }
        out.push_str("\" ");
    }
    out.push_str(match which {
        Which::Due => "is:due",
        Which::New => "is:new",
    });
    out.push_str(" -is:suspended -is:buried");
    Ok(out)
}

/// Why an answer of Anki is no success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// The key is missing or wrong.
    BadKey,
    /// Anki said something else (cut at 120 bytes).
    Said(String),
}

/// What every answer carries: whether it ended, and its `error`.
#[derive(Debug, Default)]
struct Status {
    whole: bool,
    fault: Option<Fault>,
    spent: Option<Error>,
}

impl Status {
    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Value(path, value) if path.len() == 1 && path[0] == "error" => {
                if value != "null" && !value.is_empty() {
                    self.fault = Some(fault_of(value));
                }
            }
            Event::Close([]) => self.whole = true,
            _ => {}
        }
    }
}

fn fault_of(message: &str) -> Fault {
    if message.contains("api key") {
        return Fault::BadKey;
    }
    let mut end = message.len().min(SAID_MAX);
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    match copy(&message[..end]) {
        Ok(text) => Fault::Said(text),
        Err(_) => Fault::Said(String::new()),
    }
}

/// What one reading of an answer collects.
trait Sink {
    fn status(&mut self) -> &mut Status;
    fn take(&mut self, event: Event<'_>) -> Result<(), Error>;
}

/// A parser and its sink.
#[derive(Debug)]
struct Reading<S> {
    parser: Parser,
    sink: S,
    spent: bool,
}

impl<S: Sink + core::fmt::Debug> Reading<S> {
    fn new(sink: S) -> Self {
        Self {
            parser: Parser::default(),
            sink,
            spent: false,
        }
    }

    fn feed(&mut self, piece: &[u8]) -> Result<(), Error> {
        if self.spent {
            return Err(Error::NoMemory);
        }
        let Self { parser, sink, .. } = self;
        if parser
            .try_feed(piece, &mut |e| {
                sink.status().event(e);
                if let Err(error) = sink.take(e) {
                    sink.status().spent = Some(error);
                }
            })
            .is_err()
        {
            self.spent = true;
            return Err(Error::NoMemory);
        }
        self.check()
    }

    fn finish(mut self) -> Result<S, Error> {
        let Self { parser, sink, .. } = &mut self;
        if parser
            .try_finish(&mut |e| {
                sink.status().event(e);
                if let Err(error) = sink.take(e) {
                    sink.status().spent = Some(error);
                }
            })
            .is_err()
        {
            return Err(Error::NoMemory);
        }
        self.check()?;
        Ok(self.sink)
    }

    fn check(&mut self) -> Result<(), Error> {
        match self.sink.status().spent {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

/// What a reading ended in.
#[derive(Debug, PartialEq, Eq)]
pub enum Reply<T> {
    /// A whole answer without an error.
    Ok(T),
    /// Anki's error.
    Fault(Fault),
    /// Not an answer of AnkiConnect (cut short, or not JSON of that shape).
    Unknown,
}

fn reply<S: Sink, T>(mut sink: S, value: impl FnOnce(S) -> T) -> Reply<T> {
    let status = sink.status();
    if let Some(fault) = status.fault.take() {
        return Reply::Fault(fault);
    }
    if !status.whole {
        return Reply::Unknown;
    }
    Reply::Ok(value(sink))
}

/// The list of numbers a `findCards` answers with: the first `limit`.
#[derive(Debug, Default)]
struct IdsSink {
    status: Status,
    ids: Vec<u64>,
    limit: usize,
    listed: bool,
}

impl Sink for IdsSink {
    fn status(&mut self) -> &mut Status {
        &mut self.status
    }
    fn take(&mut self, event: Event<'_>) -> Result<(), Error> {
        match event {
            Event::Open(path) if path == ["result"] => self.listed = true,
            Event::Value(path, value)
                if path == ["result", "[]"] && self.ids.len() < self.limit =>
            {
                if let Ok(id) = value.parse::<u64>() {
                    self.ids.try_reserve(1).map_err(|_| Error::NoMemory)?;
                    self.ids.push(id);
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// A reading of the card ids of an answer.
#[derive(Debug)]
pub struct Ids(Reading<IdsSink>);

impl Ids {
    /// A reading that keeps the first `limit` ids.
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self(Reading::new(IdsSink {
            limit,
            ..IdsSink::default()
        }))
    }

    /// The next piece of the answer.
    ///
    /// # Errors
    /// `NoMemory`; the reading is over then.
    pub fn feed(&mut self, piece: &[u8]) -> Result<(), Error> {
        self.0.feed(piece)
    }

    /// The ids, once the answer ended.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn finish(self) -> Result<Reply<Vec<u64>>, Error> {
        let sink = self.0.finish()?;
        let listed = sink.listed;
        Ok(match reply(sink, |s| s.ids) {
            Reply::Ok(_) if !listed => Reply::Unknown,
            other => other,
        })
    }
}

/// The names `deckNames` answers with.
#[derive(Debug, Default)]
struct NamesSink {
    status: Status,
    names: Vec<String>,
    listed: bool,
}

impl Sink for NamesSink {
    fn status(&mut self) -> &mut Status {
        &mut self.status
    }
    fn take(&mut self, event: Event<'_>) -> Result<(), Error> {
        match event {
            Event::Open(path) if path == ["result"] => self.listed = true,
            Event::Value(path, value)
                if path == ["result", "[]"]
                    && self.names.len() < MAX_DECKS
                    && !value.is_empty() =>
            {
                push(&mut self.names, copy(value)?)?;
            }
            _ => {}
        }
        Ok(())
    }
}

/// A reading of the deck names of an answer.
#[derive(Debug)]
pub struct Names(Reading<NamesSink>);

impl Default for Names {
    fn default() -> Self {
        Self::new()
    }
}

impl Names {
    /// A reading.
    #[must_use]
    pub fn new() -> Self {
        Self(Reading::new(NamesSink::default()))
    }

    /// The next piece of the answer.
    ///
    /// # Errors
    /// `NoMemory`; the reading is over then.
    pub fn feed(&mut self, piece: &[u8]) -> Result<(), Error> {
        self.0.feed(piece)
    }

    /// The names, sorted, once the answer ended.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn finish(self) -> Result<Reply<Vec<String>>, Error> {
        let sink = self.0.finish()?;
        let listed = sink.listed;
        Ok(
            match reply(sink, |mut s| {
                s.names.sort_unstable();
                s.names
            }) {
                Reply::Ok(_) if !listed => Reply::Unknown,
                other => other,
            },
        )
    }
}

/// A card as it is kept: Anki's id and the two sides as text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Card {
    pub id: u64,
    pub front: String,
    pub back: String,
}

impl Card {
    /// The card as a line of `cards.txt` (with its line break): the id, the
    /// front and the back, separated by tabs, with `\`, tab and line breaks
    /// written as `\\`, `\t` and `\n`.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn push_line(&self, out: &mut Vec<u8>) -> Result<(), Error> {
        let mut digits = [0u8; 20];
        let id = paperpaw_ui_kit::text::decimal_u64(self.id, &mut digits);
        out.try_reserve(id.len() + self.front.len() + self.back.len() + 8)
            .map_err(|_| Error::NoMemory)?;
        out.extend_from_slice(id.as_bytes());
        for text in [&self.front, &self.back] {
            out.push(b'\t');
            for b in text.bytes() {
                match b {
                    b'\\' => out.extend_from_slice(b"\\\\"),
                    b'\t' => out.extend_from_slice(b"\\t"),
                    b'\n' => out.extend_from_slice(b"\\n"),
                    b'\r' => {}
                    b => out.push(b),
                }
            }
        }
        out.push(b'\n');
        Ok(())
    }

    /// The card of a line of `cards.txt`; `None` for a line that is none.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn parse_line(line: &str) -> Result<Option<Self>, Error> {
        let mut parts = line.split('\t');
        let (Some(id), Some(front), Some(back)) = (parts.next(), parts.next(), parts.next()) else {
            return Ok(None);
        };
        let Ok(id) = id.parse::<u64>() else {
            return Ok(None);
        };
        Ok(Some(Self {
            id,
            front: unescape(front)?,
            back: unescape(back)?,
        }))
    }
}

fn unescape(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(text.len())
        .map_err(|_| Error::NoMemory)?;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(c) => out.push(c),
            None => {}
        }
    }
    Ok(out)
}

/// The cards of a `cardsInfo` answer.
#[derive(Debug, Default)]
struct CardsSink {
    status: Status,
    cards: Vec<Card>,
    id: Option<u64>,
    question: Option<String>,
    answer: Option<String>,
    listed: bool,
}

impl Sink for CardsSink {
    fn status(&mut self) -> &mut Status {
        &mut self.status
    }
    fn take(&mut self, event: Event<'_>) -> Result<(), Error> {
        match event {
            Event::Open(path) if path == ["result"] => self.listed = true,
            Event::Open(path) if path == ["result", "[]"] => {
                self.id = None;
                self.question = None;
                self.answer = None;
            }
            Event::Value(path, value) if path.len() == 3 && path[..2] == ["result", "[]"] => {
                match path[2].as_str() {
                    "cardId" => self.id = value.parse().ok(),
                    "question" => self.question = Some(text_of(value, FRONT_MAX)?),
                    "answer" => self.answer = Some(back_of(value)?),
                    _ => {}
                }
            }
            Event::Close(path) if path == ["result", "[]"] => {
                // A card Anki does not have is `{}`: nothing to keep.
                if let (Some(id), Some(front), Some(back)) =
                    (self.id.take(), self.question.take(), self.answer.take())
                    && !front.is_empty()
                {
                    push(&mut self.cards, Card { id, front, back })?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// A reading of the cards of an answer.
#[derive(Debug)]
pub struct Cards(Reading<CardsSink>);

impl Default for Cards {
    fn default() -> Self {
        Self::new()
    }
}

impl Cards {
    /// A reading.
    #[must_use]
    pub fn new() -> Self {
        Self(Reading::new(CardsSink::default()))
    }

    /// The next piece of the answer.
    ///
    /// # Errors
    /// `NoMemory`; the reading is over then.
    pub fn feed(&mut self, piece: &[u8]) -> Result<(), Error> {
        self.0.feed(piece)
    }

    /// The cards, once the answer ended.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn finish(self) -> Result<Reply<Vec<Card>>, Error> {
        let sink = self.0.finish()?;
        let listed = sink.listed;
        Ok(match reply(sink, |s| s.cards) {
            Reply::Ok(_) if !listed => Reply::Unknown,
            other => other,
        })
    }
}

/// How `answerCards` went: the answers it took and the ones it did not (a card
/// that is gone from Anki).
#[derive(Debug, Default)]
struct AnsweredSink {
    status: Status,
    taken: usize,
    missed: usize,
    listed: bool,
}

impl Sink for AnsweredSink {
    fn status(&mut self) -> &mut Status {
        &mut self.status
    }
    fn take(&mut self, event: Event<'_>) -> Result<(), Error> {
        match event {
            Event::Open(path) if path == ["result"] => self.listed = true,
            Event::Value(path, value) if path == ["result", "[]"] => {
                if value == "true" {
                    self.taken += 1;
                } else {
                    self.missed += 1;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// A reading of the answer to `answerCards`.
#[derive(Debug)]
pub struct Answered(Reading<AnsweredSink>);

impl Default for Answered {
    fn default() -> Self {
        Self::new()
    }
}

impl Answered {
    /// A reading.
    #[must_use]
    pub fn new() -> Self {
        Self(Reading::new(AnsweredSink::default()))
    }

    /// The next piece of the answer.
    ///
    /// # Errors
    /// `NoMemory`; the reading is over then.
    pub fn feed(&mut self, piece: &[u8]) -> Result<(), Error> {
        self.0.feed(piece)
    }

    /// How many answers Anki took, once the answer ended.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn finish(self) -> Result<Reply<usize>, Error> {
        let sink = self.0.finish()?;
        let listed = sink.listed;
        Ok(match reply(sink, |s| s.taken + s.missed) {
            Reply::Ok(_) if !listed => Reply::Unknown,
            other => other,
        })
    }
}

/// The text of the back of a card: what follows the rule (`<hr id=answer>`)
/// in the answer, which repeats the front above it.
fn back_of(html: &str) -> Result<String, Error> {
    let lower_at = |needle: &str| {
        html.as_bytes()
            .windows(needle.len())
            .position(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
    };
    let marker = lower_at("id=answer").or_else(|| lower_at("id=\"answer\""));
    let rest = match marker {
        Some(at) => {
            let from = &html[at..];
            from.find('>').map_or(from, |close| &from[close + 1..])
        }
        None => html,
    };
    text_of(rest, BACK_MAX)
}

/// Plain text of `html` of at most `max` bytes: styles, scripts, tags, sounds
/// and pictures out, breaks as line breaks, the common character references
/// turned, white space tidied.
///
/// # Errors
/// `NoMemory`.
pub fn text_of(html: &str, max: usize) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve(html.len().min(max + 4))
        .map_err(|_| Error::NoMemory)?;
    let bytes = html.as_bytes();
    let mut i = 0;
    let mut last_space = true;
    let mut breaks = 0usize;
    while i < bytes.len() && out.len() < max {
        let rest = &html[i..];
        let c = match rest.chars().next() {
            Some(c) => c,
            None => break,
        };
        if c == '<' {
            let Some(close) = rest.find('>') else { break };
            let tag = &rest[1..close];
            let name_end = tag
                .find(|ch: char| ch.is_whitespace() || ch == '/')
                .unwrap_or(tag.len());
            let name = &tag[..name_end];
            i += close + 1;
            if name.eq_ignore_ascii_case("style") || name.eq_ignore_ascii_case("script") {
                // Skip to the closing tag.
                let closing = if name.eq_ignore_ascii_case("style") {
                    "</style"
                } else {
                    "</script"
                };
                let found = html.as_bytes()[i..]
                    .windows(closing.len())
                    .position(|w| w.eq_ignore_ascii_case(closing.as_bytes()));
                match found {
                    Some(at) => {
                        i += at;
                        match html[i..].find('>') {
                            Some(end) => i += end + 1,
                            None => break,
                        }
                    }
                    None => break,
                }
            } else if matches!(
                name.to_ascii_lowercase().as_str(),
                "br" | "p" | "/p" | "div" | "/div" | "li" | "tr" | "hr"
            ) {
                breaks += 1;
            }
            continue;
        }
        if c == '[' && rest.starts_with("[sound:") {
            match rest.find(']') {
                Some(close) => {
                    i += close + 1;
                    continue;
                }
                None => break,
            }
        }
        let (shown, used) = if c == '&' {
            reference(rest).unwrap_or(('&', 1))
        } else {
            (c, c.len_utf8())
        };
        i += used;
        if shown.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
            continue;
        }
        if breaks > 0 && !out.is_empty() {
            while out.ends_with(' ') {
                out.pop();
            }
            out.push('\n');
        }
        breaks = 0;
        out.push(shown);
        last_space = false;
    }
    while out.ends_with([' ', '\n']) {
        out.pop();
    }
    if out.len() > max {
        let mut end = max;
        while !out.is_char_boundary(end) {
            end -= 1;
        }
        out.truncate(end);
    }
    Ok(out)
}

/// The character a reference at the start of `text` stands for, and its length.
fn reference(text: &str) -> Option<(char, usize)> {
    let end = text.find(';').filter(|e| *e <= 8)?;
    let name = &text[1..end];
    let c = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        _ => {
            let number = name.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code)?
        }
    };
    Some((c, end + 1))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;

    #[test]
    fn a_request_is_the_classic_form() {
        let body = Body::answer_cards(Some("k\"1"), &[(10, 3), (11, 1)]).unwrap();
        assert_eq!(
            core::str::from_utf8(body.as_bytes()).unwrap(),
            "{\"action\":\"answerCards\",\"version\":6,\"key\":\"k\\\"1\",\"params\":{\"answers\":[{\"cardId\":10,\"ease\":3},{\"cardId\":11,\"ease\":1}]}}"
        );
        let body = Body::deck_names(None).unwrap();
        assert_eq!(
            core::str::from_utf8(body.as_bytes()).unwrap(),
            "{\"action\":\"deckNames\",\"version\":6,\"params\":{}}"
        );
    }

    #[test]
    fn a_search_escapes_the_deck_name() {
        assert_eq!(
            query("A \"b\"_c*", Which::Due).unwrap(),
            "deck:\"A \\\"b\\\"\\_c\\*\" is:due -is:suspended -is:buried"
        );
        assert_eq!(
            query("", Which::New).unwrap(),
            "is:new -is:suspended -is:buried"
        );
    }

    #[test]
    fn html_becomes_text() {
        let html = "<style>.card { color: red }</style>Hel&amp;lo<br>w&#246;rld [sound:a.mp3]<img src=x>  &nbsp;end";
        assert_eq!(text_of(html, 100).unwrap(), "Hel&lo\nwörld end");
        let answer = "<style>x{}</style>Front<hr id=answer>Back<div>more</div>";
        assert_eq!(back_of(answer).unwrap(), "Back\nmore");
        assert_eq!(text_of("abcdef", 3).unwrap(), "abc");
    }

    #[test]
    fn a_card_line_round_trips() {
        let card = Card {
            id: 1_498_938_915_662,
            front: "a\tb\\c".into(),
            back: "x\ny".into(),
        };
        let mut line = Vec::new();
        card.push_line(&mut line).unwrap();
        let text = core::str::from_utf8(&line).unwrap().trim_end_matches('\n');
        assert!(!text.contains('\n'));
        assert_eq!(Card::parse_line(text).unwrap(), Some(card));
        assert_eq!(Card::parse_line("nope").unwrap(), None);
    }

    fn read<T: Sink + core::fmt::Debug>(sink: T, json: &str, piece: usize) -> T {
        let mut reading = Reading::new(sink);
        for part in json.as_bytes().chunks(piece) {
            reading.feed(part).unwrap();
        }
        reading.finish().unwrap()
    }

    #[test]
    fn the_answers_are_read_in_pieces() {
        let mut ids = Ids::new(2);
        for part in br#"{"result": [5, 6, 7], "error": null}"#.chunks(5) {
            ids.feed(part).unwrap();
        }
        assert_eq!(ids.finish().unwrap(), Reply::Ok(alloc::vec![5, 6]));

        let mut ids = Ids::new(9);
        ids.feed(br#"{"result": null, "error": "valid api key must be provided"}"#)
            .unwrap();
        assert_eq!(ids.finish().unwrap(), Reply::Fault(Fault::BadKey));

        let mut ids = Ids::new(9);
        ids.feed(br#"{"result": [1, 2"#).unwrap();
        assert_eq!(ids.finish().unwrap(), Reply::Unknown);

        let mut names = Names::new();
        names
            .feed(br#"{"result": ["B", "A::x"], "error": null}"#)
            .unwrap();
        assert_eq!(
            names.finish().unwrap(),
            Reply::Ok(alloc::vec!["A::x".into(), "B".into()])
        );

        let mut answered = Answered::new();
        answered
            .feed(br#"{"result": [true, false], "error": null}"#)
            .unwrap();
        assert_eq!(answered.finish().unwrap(), Reply::Ok(2));
    }

    #[test]
    fn cards_are_read_with_their_sides() {
        let json = r#"{"result": [
            {"cardId": 11, "question": "<style>.c{}</style>Hund", "answer": "<style>.c{}</style>Hund<hr id=answer>dog", "css": "x", "deckName": "D", "fields": {"Front": {"value": "Hund", "order": 0}}},
            {},
            {"cardId": 12, "question": "Katze", "answer": "Katze<hr id=\"answer\">cat"}
        ], "error": null}"#;
        let sink = read(CardsSink::default(), json, 7);
        assert!(sink.status.whole && sink.status.fault.is_none());
        assert_eq!(
            sink.cards,
            alloc::vec![
                Card {
                    id: 11,
                    front: "Hund".into(),
                    back: "dog".into()
                },
                Card {
                    id: 12,
                    front: "Katze".into(),
                    back: "cat".into()
                },
            ]
        );
    }
}
