//! IMAP (RFC 3501) as text, for the native plugin: the interface-6 `imap`
//! made fallible and bounded. What a server's line says, and the commands to
//! send; no connection here (the bytes come from and go to whoever holds the
//! socket).
//!
//! Where it differs from interface 6, on purpose:
//!
//! - a refused allocation is [`Fault::NoMemory`], never an abort;
//! - what is read is bounded: a line is at most [`MAX_LINE`] bytes (also for
//!   [`response`] and [`tokens`], which refuse a longer one), and the buffer
//!   of a socket's bytes holds at most [`MAX_BUFFER`], a line makes at most
//!   [`MAX_TOKENS`] tokens and a list of capabilities, arguments or flags
//!   at most [`MAX_WORDS`]: past a bound the answer is [`Fault::TooLong`],
//!   never a larger allocation;
//! - a FETCH line is read from its own bytes: interface 6 counted from the
//!   end of the line, which cut the first token off a line that ended in
//!   white space or held bytes that are no UTF-8;
//! - the password of a login is quoted into a [`Secret`] that wipes itself
//!   ([`quoted_secret`]).
use super::codec::decode_base64;
use super::mem::{
    Error, Secret, copy, lossy, push, push_bytes, push_char, push_str, string_with, vec_with,
};
use alloc::string::String;
use alloc::vec::Vec;

/// The longest response line taken; a server's longer line is an error.
pub const MAX_LINE: usize = 8 * 1024;
/// The most bytes a [`LineBuffer`] holds: a line and what came after it in
/// the same read. The session takes what it has between two pushes; the
/// buffer grows by exactly what is pushed, so a push that makes it larger
/// holds the old and the new block at once (32 KiB at the most).
pub const MAX_BUFFER: usize = 2 * MAX_LINE;
/// The most tokens one line makes (lists and what is in them all count): a
/// real response has some twenty, and a line of single-letter atoms would
/// make some 4,100, which the X4's heap cannot hold.
pub const MAX_TOKENS: usize = 256;
/// The most words in a CAPABILITY line, in a response code's arguments and
/// in a list of flags.
pub const MAX_WORDS: usize = 64;
/// Tokens nested in lists, at most.
const MAX_DEPTH: usize = 6;
/// The most digits (and a `+`) a literal's size has between its braces.
const MAX_LITERAL_DIGITS: usize = 12;

/// Why a line could not be taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Fault {
    /// A line or a buffer grew past its bound.
    TooLong,
    /// An allocation was refused.
    NoMemory,
}

impl From<Error> for Fault {
    fn from(_: Error) -> Self {
        Self::NoMemory
    }
}

/// Bytes from a socket, cut into lines and literals.
#[derive(Debug, Default)]
pub struct LineBuffer {
    data: Vec<u8>,
}

impl LineBuffer {
    /// Adds `bytes` from the socket.
    ///
    /// # Errors
    /// [`Fault::TooLong`] when the buffer would hold more than
    /// [`MAX_BUFFER`], [`Fault::NoMemory`]; the buffer is then unchanged.
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), Fault> {
        if self.data.len().saturating_add(bytes.len()) > MAX_BUFFER {
            return Err(Fault::TooLong);
        }
        // Exactly what is pushed: no doubling past the bound.
        self.data
            .try_reserve_exact(bytes.len())
            .map_err(|_| Fault::NoMemory)?;
        self.data.extend_from_slice(bytes);
        Ok(())
    }

    /// The next line, without its line break; `Ok(None)` while none is
    /// complete.
    ///
    /// # Errors
    /// [`Fault::TooLong`] when the line grows past [`MAX_LINE`] (a complete
    /// line is dropped then), [`Fault::NoMemory`] (the buffer is unchanged).
    pub fn line(&mut self) -> Result<Option<Vec<u8>>, Fault> {
        let Some(end) = self.data.iter().position(|&b| b == b'\n') else {
            return if self.data.len() > MAX_LINE {
                Err(Fault::TooLong)
            } else {
                Ok(None)
            };
        };
        let stop = if end > 0 && self.data.get(end - 1) == Some(&b'\r') {
            end - 1
        } else {
            end
        };
        if stop > MAX_LINE {
            self.data.drain(..=end);
            return Err(Fault::TooLong);
        }
        let mut line = vec_with(stop)?;
        push_bytes(&mut line, self.data.get(..stop).unwrap_or(&[]))?;
        self.data.drain(..=end);
        Ok(Some(line))
    }

    /// Up to `n` bytes from the front (a literal).
    ///
    /// # Errors
    /// [`Error::NoMemory`]; the buffer is unchanged.
    pub fn take(&mut self, n: usize) -> Result<Vec<u8>, Error> {
        let n = n.min(self.data.len());
        let mut out = vec_with(n)?;
        push_bytes(&mut out, self.data.get(..n).unwrap_or(&[]))?;
        self.data.drain(..n);
        Ok(out)
    }

    /// How many bytes are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Whether nothing is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// One piece of a response.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Token {
    Atom(String),
    Quoted(String),
    List(Vec<Token>),
    /// `{n}` or `{n+}` at the end of a line: n bytes follow.
    Literal(usize),
    Nil,
}

impl Token {
    /// The text of an atom or a quoted string.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match self {
            Token::Atom(s) | Token::Quoted(s) => Some(s),
            _ => None,
        }
    }

    /// The tokens of a list.
    #[must_use]
    pub fn list(&self) -> Option<&[Token]> {
        match self {
            Token::List(list) => Some(list),
            _ => None,
        }
    }

    /// The number an atom or a quoted string holds.
    #[must_use]
    pub fn number(&self) -> Option<u64> {
        self.text()?.parse().ok()
    }
}

/// The tokens of `line` (what is no token is skipped, an open list is
/// closed). Read as bytes: every character that parts tokens is ASCII, so
/// only the tokens themselves are made text.
///
/// # Errors
/// [`Fault::TooLong`] for a line over [`MAX_LINE`], [`Fault::NoMemory`].
pub fn tokens(line: &[u8]) -> Result<Vec<Token>, Fault> {
    if line.len() > MAX_LINE {
        return Err(Fault::TooLong);
    }
    let (mut at, mut count) = (0, 0);
    tokens_in(line, &mut at, 0, false, &mut count)
}

/// Counts one more token of the line.
fn counted(count: &mut usize) -> Result<(), Fault> {
    *count += 1;
    if *count > MAX_TOKENS {
        return Err(Fault::TooLong);
    }
    Ok(())
}

fn tokens_in(
    line: &[u8],
    at: &mut usize,
    depth: usize,
    in_list: bool,
    count: &mut usize,
) -> Result<Vec<Token>, Fault> {
    let mut out = Vec::new();
    while let Some(&byte) = line.get(*at) {
        match byte {
            b' ' | b'\t' => *at += 1,
            b')' => {
                *at += 1;
                if in_list {
                    return Ok(out);
                }
            }
            b'(' => {
                *at += 1;
                if depth >= MAX_DEPTH {
                    continue;
                }
                counted(count)?;
                let inner = tokens_in(line, at, depth + 1, true, count)?;
                push(&mut out, Token::List(inner))?;
            }
            b'"' => {
                *at += 1;
                counted(count)?;
                let text = quoted_token(line, at)?;
                push(&mut out, Token::Quoted(text))?;
            }
            b'{' => {
                // Digits (and a `+`) and `}`, close by: no search to the end
                // of the line for a brace that is not there.
                let rest = line.get(*at + 1..).unwrap_or(&[]);
                let close = rest
                    .iter()
                    .take(MAX_LITERAL_DIGITS + 2)
                    .position(|&c| !(c.is_ascii_digit() || c == b'+'))
                    .filter(|&e| rest.get(e) == Some(&b'}'));
                let size = close.and_then(|e| {
                    core::str::from_utf8(rest.get(..e)?)
                        .ok()?
                        .trim_end_matches('+')
                        .parse::<usize>()
                        .ok()
                });
                match (close, size) {
                    (Some(e), Some(n)) => {
                        *at += e + 2;
                        counted(count)?;
                        push(&mut out, Token::Literal(n))?;
                    }
                    _ => *at += 1,
                }
            }
            _ => {
                if let Some(token) = atom(line, at)? {
                    counted(count)?;
                    push(&mut out, token)?;
                }
            }
        }
    }
    Ok(out)
}

/// The text of a quoted string whose opening quote has been read.
fn quoted_token(line: &[u8], at: &mut usize) -> Result<String, Fault> {
    let mut text = vec_with(0)?;
    while let Some(&byte) = line.get(*at) {
        match byte {
            b'"' => {
                *at += 1;
                break;
            }
            b'\\' if *at + 1 < line.len() => {
                if let Some(&escaped) = line.get(*at + 1) {
                    push(&mut text, escaped)?;
                }
                *at += 2;
            }
            c => {
                push(&mut text, c)?;
                *at += 1;
            }
        }
    }
    Ok(lossy(&text)?)
}

/// An atom (`NIL` is [`Token::Nil`]); `None` where a byte parts tokens and
/// there is no atom to take.
fn atom(line: &[u8], at: &mut usize) -> Result<Option<Token>, Fault> {
    let start = *at;
    let mut depth_square = 0u32;
    while let Some(&byte) = line.get(*at) {
        match byte {
            b'[' => depth_square += 1,
            b']' => depth_square = depth_square.saturating_sub(1),
            b' ' | b'(' | b')' | b'"' | b'{' if depth_square == 0 && *at > start => break,
            b' ' | b')' | b'(' | b'"' if depth_square == 0 => break,
            _ => {}
        }
        // `BODY[HEADER.FIELDS (FROM TO)]` is one atom.
        *at += 1;
    }
    if *at == start {
        *at += 1;
        return Ok(None);
    }
    let atom = lossy(line.get(start..*at).unwrap_or(&[]))?;
    Ok(Some(if atom.eq_ignore_ascii_case("NIL") {
        Token::Nil
    } else {
        Token::Atom(atom)
    }))
}

/// How a command ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Status {
    Ok,
    No,
    Bad,
}

/// What one line of a server says.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Response {
    /// `A3 OK Done`
    Tagged {
        tag: String,
        status: Status,
        text: String,
    },
    /// `+ go ahead`
    Continue(String),
    /// `* …`
    Untagged(Untagged),
}

/// An untagged response.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Untagged {
    Capability(Vec<String>),
    /// `(flags) "delimiter" name`, the name as the server wrote it.
    List {
        flags: Vec<String>,
        delimiter: Option<char>,
        name: String,
    },
    Exists(u32),
    Search(Vec<u32>),
    Status {
        name: String,
        items: Vec<(String, u64)>,
    },
    Fetch {
        sequence: u32,
        items: Vec<Token>,
    },
    /// `* OK [UIDVALIDITY 3] …`, `* OK [CAPABILITY …]`
    Code {
        code: String,
        args: Vec<String>,
    },
    Bye(String),
    /// Anything else (flags, recent, expunge, text).
    Other,
}

/// What the server's `line` is; `None` for an empty line.
///
/// # Errors
/// [`Fault::TooLong`] for a line over [`MAX_LINE`], [`Fault::NoMemory`].
pub fn response(line: &[u8]) -> Result<Option<Response>, Fault> {
    if line.len() > MAX_LINE {
        return Err(Fault::TooLong);
    }
    let text = lossy(line)?;
    let text = text.trim_end();
    if text.is_empty() {
        return Ok(None);
    }
    if let Some(rest) = text.strip_prefix('+') {
        return Ok(Some(Response::Continue(copy(rest.trim())?)));
    }
    if let Some(rest) = text.strip_prefix("* ") {
        let raw = line.get(2..).unwrap_or(&[]);
        return Ok(Some(Response::Untagged(untagged(rest, raw)?)));
    }
    let Some((tag, rest)) = text.split_once(' ') else {
        return Ok(None);
    };
    let (status, rest) = rest.split_once(' ').unwrap_or((rest, ""));
    let status = if status.eq_ignore_ascii_case("OK") {
        Status::Ok
    } else if status.eq_ignore_ascii_case("NO") {
        Status::No
    } else if status.eq_ignore_ascii_case("BAD") {
        Status::Bad
    } else {
        return Ok(None);
    };
    Ok(Some(Response::Tagged {
        tag: copy(tag)?,
        status,
        text: copy(rest)?,
    }))
}

/// `raw` is the line's bytes after `* `, `rest` the same as text.
fn untagged(rest: &str, raw: &[u8]) -> Result<Untagged, Fault> {
    let (first, tail) = rest.split_once(' ').unwrap_or((rest, ""));
    let is = |word: &str| first.eq_ignore_ascii_case(word);
    if let Ok(number) = first.parse::<u32>() {
        let (word, _) = tail.split_once(' ').unwrap_or((tail, ""));
        if word.eq_ignore_ascii_case("EXISTS") {
            return Ok(Untagged::Exists(number));
        }
        if word.eq_ignore_ascii_case("FETCH") {
            // After `n FETCH `, in the bytes.
            let (_, after_number) = split_space(raw);
            let (_, after) = split_space(after_number);
            let items = match tokens(after)?.into_iter().next() {
                Some(Token::List(items)) => items,
                _ => Vec::new(),
            };
            return Ok(Untagged::Fetch {
                sequence: number,
                items,
            });
        }
        return Ok(Untagged::Other);
    }
    if is("CAPABILITY") {
        Ok(Untagged::Capability(words(tail)?))
    } else if is("LIST") || is("LSUB") {
        list(tail)
    } else if is("SEARCH") {
        let mut found = Vec::new();
        for number in tail.split_whitespace().filter_map(|n| n.parse().ok()) {
            push(&mut found, number)?;
        }
        Ok(Untagged::Search(found))
    } else if is("STATUS") {
        status_line(tail)
    } else if is("BYE") {
        Ok(Untagged::Bye(copy(tail.trim())?))
    } else if is("OK") || is("NO") || is("BAD") || is("PREAUTH") {
        code(tail)
    } else {
        Ok(Untagged::Other)
    }
}

/// The bytes up to the first space, and those after it.
fn split_space(bytes: &[u8]) -> (&[u8], &[u8]) {
    match bytes.iter().position(|&b| b == b' ') {
        Some(at) => (
            bytes.get(..at).unwrap_or(&[]),
            bytes.get(at + 1..).unwrap_or(&[]),
        ),
        None => (bytes, &[]),
    }
}

/// Adds a copy of `word` to `found`, which holds at most [`MAX_WORDS`].
fn push_word(found: &mut Vec<String>, word: &str) -> Result<(), Fault> {
    if found.len() >= MAX_WORDS {
        return Err(Fault::TooLong);
    }
    push(found, copy(word)?)?;
    Ok(())
}

/// The words of `text`, each a copy, at most [`MAX_WORDS`].
fn words(text: &str) -> Result<Vec<String>, Fault> {
    let mut found = Vec::new();
    for word in text.split_whitespace() {
        push_word(&mut found, word)?;
    }
    Ok(found)
}

/// `text` with its ASCII letters in capitals.
fn upper(text: &str) -> Result<String, Fault> {
    let mut out = string_with(text.len())?;
    for c in text.chars() {
        push_char(&mut out, c.to_ascii_uppercase())?;
    }
    Ok(out)
}

/// `* OK [CODE args] text`.
fn code(tail: &str) -> Result<Untagged, Fault> {
    let Some(inner) = tail.strip_prefix('[') else {
        return Ok(Untagged::Other);
    };
    let Some((inner, _)) = inner.split_once(']') else {
        return Ok(Untagged::Other);
    };
    let mut parts = inner.split_whitespace();
    let code = upper(parts.next().unwrap_or(""))?;
    let mut args = Vec::new();
    for word in parts {
        push_word(&mut args, word)?;
    }
    Ok(Untagged::Code { code, args })
}

fn list(tail: &str) -> Result<Untagged, Fault> {
    let tokens = tokens(tail.as_bytes())?;
    let mut it = tokens.iter();
    let mut flags = Vec::new();
    match it.next() {
        Some(Token::List(found)) => {
            for text in found.iter().filter_map(Token::text) {
                push_word(&mut flags, text)?;
            }
        }
        _ => return Ok(Untagged::Other),
    }
    let delimiter = match it.next() {
        Some(Token::Quoted(d)) => d.chars().next(),
        _ => None,
    };
    let name = match it.next().and_then(Token::text) {
        Some(name) => copy(name)?,
        None => return Ok(Untagged::Other),
    };
    Ok(Untagged::List {
        flags,
        delimiter,
        name,
    })
}

fn status_line(tail: &str) -> Result<Untagged, Fault> {
    let tokens = tokens(tail.as_bytes())?;
    let name = match tokens.first().and_then(Token::text) {
        Some(name) => copy(name)?,
        None => String::new(),
    };
    let mut items = Vec::new();
    if let Some(Token::List(list)) = tokens.get(1) {
        for pair in list.chunks(2) {
            if let [key, value] = pair
                && let (Some(key), Some(value)) = (key.text(), value.number())
            {
                push(&mut items, (upper(key)?, value))?;
            }
        }
    }
    Ok(Untagged::Status { name, items })
}

/// The value after `key` in the items of a FETCH (`UID 42`, `RFC822.SIZE
/// 1000`).
#[must_use]
pub fn fetch_number(items: &[Token], key: &str) -> Option<u64> {
    let at = items
        .iter()
        .position(|t| t.text().is_some_and(|k| k.eq_ignore_ascii_case(key)))?;
    items.get(at + 1)?.number()
}

/// The text after `key` in the items of a FETCH (`INTERNALDATE "…"`).
#[must_use]
pub fn fetch_text<'a>(items: &'a [Token], key: &str) -> Option<&'a str> {
    let at = items
        .iter()
        .position(|t| t.text().is_some_and(|k| k.eq_ignore_ascii_case(key)))?;
    items.get(at + 1)?.text()
}

/// The flags of a FETCH (`\Seen`, …), at most [`MAX_WORDS`].
///
/// # Errors
/// [`Fault::TooLong`] for more flags, [`Fault::NoMemory`].
pub fn fetch_flags(items: &[Token]) -> Result<Vec<String>, Fault> {
    let mut out = Vec::new();
    let flags = items
        .iter()
        .position(|t| t.text().is_some_and(|k| k.eq_ignore_ascii_case("FLAGS")))
        .and_then(|at| items.get(at + 1))
        .and_then(Token::list);
    for text in flags.into_iter().flatten().filter_map(Token::text) {
        push_word(&mut out, text)?;
    }
    Ok(out)
}

/// The size of the literal a FETCH's body part ends with: the line's last
/// token is `{n}`.
#[must_use]
pub fn literal_at_end(line: &[u8]) -> Option<usize> {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let line = line.strip_suffix(b"}")?;
    let open = line.iter().rposition(|&b| b == b'{')?;
    let inner = line.get(open + 1..)?;
    let inner = inner.strip_suffix(b"+").unwrap_or(inner);
    if inner.is_empty() || !inner.iter().all(u8::is_ascii_digit) {
        return None;
    }
    core::str::from_utf8(inner).ok()?.parse().ok()
}

/// What quoting `text` puts out, character by character: `"`, each
/// character (a quote or a backslash after a backslash, a control character
/// as a space), `"`.
fn write_quoted(text: &str, mut put: impl FnMut(char) -> Result<(), Error>) -> Result<(), Error> {
    put('"')?;
    for c in text.chars() {
        match c {
            '"' | '\\' => {
                put('\\')?;
                put(c)?;
            }
            c if c.is_control() => put(' ')?,
            c => put(c)?,
        }
    }
    put('"')
}

/// How many bytes quoting `text` makes.
fn quoted_len(text: &str) -> usize {
    let mut len = 0;
    let _ = write_quoted(text, |c| {
        len += c.len_utf8();
        Ok(())
    });
    len
}

/// `text` as an IMAP quoted string (control characters become spaces).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn quoted(text: &str) -> Result<String, Error> {
    let mut out = string_with(quoted_len(text))?;
    write_quoted(text, |c| push_char(&mut out, c))?;
    Ok(out)
}

/// `text` (a password) as an IMAP quoted string in a buffer that wipes
/// itself. The buffer is made at its size and never grows.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn quoted_secret(text: &str) -> Result<Secret, Error> {
    let mut out = Secret::with_capacity(quoted_len(text))?;
    write_quoted(text, |c| {
        let mut one = [0u8; 4];
        let done = out.push_str(c.encode_utf8(&mut one));
        super::mem::wipe(&mut one);
        done
    })?;
    Ok(out)
}

/// The name of a mailbox as the reader sees it: Modified UTF-7 (RFC 3501
/// 5.1.3) decoded (`&AOQ-` is ä); a name that is not valid stays as it is.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn mailbox_display(name: &str) -> Result<String, Error> {
    if !name.contains('&') {
        return copy(name);
    }
    let mut out = string_with(name.len())?;
    let mut rest = name;
    while let Some(start) = rest.find('&') {
        push_str(&mut out, rest.get(..start).unwrap_or(""))?;
        let after = rest.get(start + 1..).unwrap_or("");
        let Some(end) = after.find('-') else {
            return copy(name);
        };
        let body = after.get(..end).unwrap_or("");
        if body.is_empty() {
            push_char(&mut out, '&')?;
        } else {
            // The modified alphabet has `,` where base64 has `/`.
            let mut standard = string_with(body.len())?;
            for c in body.chars() {
                push_char(&mut standard, if c == ',' { '/' } else { c })?;
            }
            let bytes = decode_base64(standard.as_bytes())?;
            if !bytes.len().is_multiple_of(2) {
                return copy(name);
            }
            let (pairs, _) = bytes.as_chunks::<2>();
            let units = pairs.iter().map(|pair| u16::from_be_bytes(*pair));
            for c in char::decode_utf16(units) {
                push_char(&mut out, c.unwrap_or('\u{fffd}'))?;
            }
        }
        rest = after.get(end + 1..).unwrap_or("");
    }
    push_str(&mut out, rest)?;
    Ok(out)
}

/// The flags of a message as one letter each: S seen, F flagged, A
/// answered, D deleted, R draft.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn flag_letters(flags: &[String]) -> Result<String, Error> {
    let mut out = string_with(5)?;
    for flag in flags {
        let letter = if flag.eq_ignore_ascii_case("\\seen") {
            'S'
        } else if flag.eq_ignore_ascii_case("\\flagged") {
            'F'
        } else if flag.eq_ignore_ascii_case("\\answered") {
            'A'
        } else if flag.eq_ignore_ascii_case("\\deleted") {
            'D'
        } else if flag.eq_ignore_ascii_case("\\draft") {
            'R'
        } else {
            continue;
        };
        if !out.contains(letter) {
            push_char(&mut out, letter)?;
        }
    }
    Ok(out)
}
