//! Calibre's wireless device protocol for the native plugin, without host
//! calls: messages are JSON arrays `[opcode, {arguments}]`, each written after
//! its length in decimal digits. It is the interface-6 `protocol` made
//! fallible (a refused allocation is `NoMemory`, never an abort) and free of
//! slicing that could panic; each part is tested to give the same answers.
//!
//! Members of the arguments are found with the kit's `raw_member` (it allocates
//! nothing) and their text made with its fallible reader
//! (`paperpaw_ui_kit::json`), so a hostile message costs no more than the
//! framing's bound ([`max_message`]) and a refused allocation is `NoMemory`.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{digest::sha1::Sha1, files::is_plain_name, native::Error};
use paperpaw_ui_kit::{
    mem::{push, wipe},
    text::{concat, copy, decimal},
};

/// The ports Calibre listens on for a device looking for it.
pub const BROADCAST_PORTS: [u16; 5] = [54982, 48123, 39001, 44044, 59678];
/// The port Calibre takes for devices unless set otherwise.
pub const DEFAULT_PORT: u16 = 9090;

/// The opcodes PaperPaw knows.
pub mod op {
    pub const OK: u8 = 0;
    pub const SET_CALIBRE_DEVICE_INFO: u8 = 1;
    pub const SET_CALIBRE_DEVICE_NAME: u8 = 2;
    pub const GET_DEVICE_INFORMATION: u8 = 3;
    pub const TOTAL_SPACE: u8 = 4;
    pub const FREE_SPACE: u8 = 5;
    pub const GET_BOOK_COUNT: u8 = 6;
    pub const SEND_BOOKLISTS: u8 = 7;
    pub const SEND_BOOK: u8 = 8;
    pub const GET_INITIALIZATION_INFO: u8 = 9;
    pub const BOOK_DONE: u8 = 11;
    pub const NOOP: u8 = 12;
    pub const DELETE_BOOK: u8 = 13;
    pub const GET_BOOK_FILE_SEGMENT: u8 = 14;
    pub const GET_BOOK_METADATA: u8 = 15;
    pub const SEND_BOOK_METADATA: u8 = 16;
    pub const DISPLAY_MESSAGE: u8 = 17;
    pub const CALIBRE_BUSY: u8 = 18;
    pub const SET_LIBRARY_INFO: u8 = 19;
    pub const ERROR: u8 = 20;
}

/// A message: its opcode and its arguments as JSON (an object).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub op: u8,
    pub args: String,
}

/// Longest message read on a device of the small class (an X4) or any other: a
/// book's metadata with its description and cover thumbnail is the largest, a
/// few dozen kilobytes. A message is held whole.
#[must_use]
pub const fn max_message(small: bool) -> usize {
    if small { 64 * 1024 } else { 1 << 20 }
}

/// `op` with `args` (a JSON object) as it goes on the connection.
///
/// # Errors
/// `NoMemory` when the frame cannot be kept.
pub fn frame(op: u8, args: &str) -> Result<Vec<u8>, Error> {
    let mut digits = [0; 20];
    let body = concat(&["[", decimal(usize::from(op), &mut digits), ", ", args, "]"])?;
    let mut digits = [0; 20];
    let framed = concat(&[decimal(body.len(), &mut digits), &body])?;
    Ok(framed.into_bytes())
}

/// Why bytes that came are not Calibre's protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// More than eight digits of length.
    LengthTooLong,
    /// A `[` without a length.
    NoLength,
    /// A length of 0 or above the framing's bound.
    BadLength,
    /// Something other than a digit or `[` where the length is.
    NotAMessage,
    /// A body that is not `[op, ...]`.
    BadMessage,
    /// A refused allocation.
    NoMemory,
}

impl Refused {
    /// The words the interface-6 framing gave for it.
    #[must_use]
    pub const fn words(self) -> &'static str {
        match self {
            Self::LengthTooLong => "length too long",
            Self::NoLength => "no length",
            Self::BadLength => "bad length",
            Self::NotAMessage => "not a message",
            Self::BadMessage => "bad message",
            Self::NoMemory => "no memory",
        }
    }
}

/// Reads messages from what comes, never asking for more than the current
/// message holds, so a book's bytes after it are not taken as text.
#[derive(Debug)]
pub struct Framing {
    /// The digits of the length so far.
    digits: [u8; 8],
    used: usize,
    /// The message's text so far, once its length is known.
    body: Option<(usize, String)>,
    /// The longest message taken, in bytes.
    max: usize,
}

impl Default for Framing {
    /// A framing for messages up to a megabyte.
    fn default() -> Self {
        Self::new(1 << 20)
    }
}

impl Framing {
    /// A framing for messages up to `max` bytes ([`max_message`]).
    #[must_use]
    pub const fn new(max: usize) -> Self {
        Self {
            digits: [0; 8],
            used: 0,
            body: None,
            max,
        }
    }

    /// How many bytes to read next.
    #[must_use]
    pub fn wanted(&self) -> u32 {
        match &self.body {
            // A digit or the `[` at a time.
            None => 1,
            Some((len, text)) => {
                u32::try_from(len.saturating_sub(text.len()).max(1)).unwrap_or(u32::MAX)
            }
        }
    }

    fn length(&self) -> Option<usize> {
        let mut len: usize = 0;
        for digit in self.digits.get(..self.used)? {
            len = len
                .checked_mul(10)?
                .checked_add(usize::from(digit - b'0'))?;
        }
        (self.used > 0).then_some(len)
    }

    /// Takes what came; a message when it is complete.
    ///
    /// # Errors
    /// [`Refused`] for what is not Calibre's protocol, or a refused allocation.
    pub fn feed(&mut self, text: &str) -> Result<Option<Message>, Refused> {
        for c in text.chars() {
            match &mut self.body {
                None if c.is_ascii_digit() => {
                    if self.used >= self.digits.len() {
                        return Err(Refused::LengthTooLong);
                    }
                    // An ASCII digit is one byte.
                    if let Some(slot) = self.digits.get_mut(self.used) {
                        *slot = c as u8;
                    }
                    self.used += 1;
                }
                None if c == '[' => {
                    let len = self.length().ok_or(Refused::NoLength)?;
                    if len == 0 || len > self.max {
                        return Err(Refused::BadLength);
                    }
                    self.used = 0;
                    // The whole message at once: pushing then never grows it.
                    let mut body = String::new();
                    body.try_reserve_exact(len).map_err(|_| Refused::NoMemory)?;
                    body.push('[');
                    self.body = Some((len, body));
                }
                None => return Err(Refused::NotAMessage),
                Some((_, body)) => {
                    body.try_reserve(c.len_utf8())
                        .map_err(|_| Refused::NoMemory)?;
                    body.push(c);
                }
            }
        }
        match &self.body {
            Some((len, body)) if body.len() >= *len => {
                let (_, body) = self.body.take().unwrap_or_default();
                match parse(&body) {
                    Ok(Some(message)) => Ok(Some(message)),
                    Ok(None) => Err(Refused::BadMessage),
                    Err(_) => Err(Refused::NoMemory),
                }
            }
            _ => Ok(None),
        }
    }
}

/// `[op, {...}]` as a message; `None` for anything else.
///
/// # Errors
/// `NoMemory` when the arguments cannot be kept.
pub fn parse(body: &str) -> Result<Option<Message>, Error> {
    let Some(inner) = body
        .trim()
        .strip_prefix('[')
        .and_then(|b| b.strip_suffix(']'))
    else {
        return Ok(None);
    };
    let Some((op, args)) = inner.split_once(',') else {
        return Ok(None);
    };
    let Ok(op) = op.trim().parse() else {
        return Ok(None);
    };
    Ok(Some(Message {
        op,
        args: copy(args.trim())?,
    }))
}

/// Member `key` of `object`, as it is written.
#[must_use]
pub fn member<'a>(object: &'a str, key: &str) -> Option<&'a str> {
    paperpaw_ui_kit::json::raw_member(object, key)
}

/// Member `key` of `object` as text: a JSON string decoded, other values as
/// written.
///
/// # Errors
/// `NoMemory` when the text cannot be kept.
pub fn text(object: &str, key: &str) -> Result<Option<String>, Error> {
    let Some(raw) = member(object, key) else {
        return Ok(None);
    };
    match paperpaw_ui_kit::json::unquote(raw)? {
        Some(text) => Ok(Some(text)),
        None => copy(raw).map(Some),
    }
}

/// Member `key` of `object` as a number.
#[must_use]
pub fn number(object: &str, key: &str) -> Option<u64> {
    member(object, key)?.parse::<f64>().ok().map(|n| n as u64)
}

/// The strings of the array member `key` of `object` (other elements are
/// left out, as are the strings after one that is cut short).
///
/// # Errors
/// `NoMemory` when the strings cannot be kept.
pub fn strings(object: &str, key: &str) -> Result<Vec<String>, Error> {
    let mut out = Vec::new();
    let Some(array) = member(object, key) else {
        return Ok(out);
    };
    let Some(mut rest) = array.trim().strip_prefix('[') else {
        return Ok(out);
    };
    loop {
        rest = rest.trim_start_matches([' ', '\t', '\r', '\n', ',']);
        if !rest.starts_with('"') {
            // Not a string: skipped up to the next comma at this level.
            match skip_value(rest) {
                Some(after) if !after.is_empty() && !after.starts_with(']') => rest = after,
                _ => return Ok(out),
            }
            continue;
        }
        let Some(end) = string_end(rest) else {
            return Ok(out);
        };
        let (literal, after) = rest.split_at(end);
        if let Some(text) = paperpaw_ui_kit::json::unquote(literal)? {
            push(&mut out, text)?;
        }
        rest = after;
    }
}

/// The byte just after the string literal `text` starts with.
fn string_end(text: &str) -> Option<usize> {
    let mut escaped = false;
    for (at, c) in text.char_indices().skip(1) {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '"' => return Some(at + 1),
            _ => {}
        }
    }
    None
}

/// What follows the value (a number, a literal, a nested array or object)
/// `text` starts with, up to the next element: the rest from its comma or
/// closing bracket.
fn skip_value(text: &str) -> Option<&str> {
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (at, c) in text.char_indices() {
        if quoted {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => quoted = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            '[' | '{' => depth += 1,
            ']' | '}' if depth > 0 => depth -= 1,
            ',' | ']' if depth == 0 => return text.get(at..),
            _ => {}
        }
    }
    None
}

/// `text` as a JSON string literal.
///
/// # Errors
/// `NoMemory` when the literal cannot be kept.
pub fn json_string(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve(text.len().saturating_add(8))
        .map_err(|_| Error::NoMemory)?;
    out.push('"');
    for c in text.chars() {
        let escaped: Option<&str> = match c {
            '"' => Some("\\\""),
            '\\' => Some("\\\\"),
            '\n' => Some("\\n"),
            '\r' => Some("\\r"),
            '\t' => Some("\\t"),
            _ => None,
        };
        out.try_reserve(escaped.map_or(c.len_utf8().max(6), str::len))
            .map_err(|_| Error::NoMemory)?;
        match escaped {
            Some(text) => out.push_str(text),
            None if (c as u32) < 0x20 => {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                out.push_str("\\u00");
                out.push(char::from(HEX[(c as usize >> 4) & 15]));
                out.push(char::from(HEX[c as usize & 15]));
            }
            None => out.push(c),
        }
    }
    out.try_reserve(1).map_err(|_| Error::NoMemory)?;
    out.push('"');
    Ok(out)
}

/// What the device says of itself when Calibre asks
/// (`GET_INITIALIZATION_INFO`).
pub struct Device<'a> {
    pub name: &'a str,
    /// The file extensions PaperPaw reads.
    pub extensions: &'a [&'a str],
    /// Calibre's challenge and the password, if one is set.
    pub challenge: &'a str,
    pub password: Option<&'a str>,
    pub version: &'a str,
}

/// The password is never shown: `Debug` writes `<set>` or `<none>`.
impl core::fmt::Debug for Device<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Device")
            .field("name", &self.name)
            .field("extensions", &self.extensions)
            .field("challenge", &self.challenge)
            .field(
                "password",
                &if self.password.is_some() {
                    "<set>"
                } else {
                    "<none>"
                },
            )
            .field("version", &self.version)
            .finish()
    }
}

/// The answer to `GET_INITIALIZATION_INFO`.
///
/// # Errors
/// `NoMemory` when the answer cannot be kept.
pub fn initialization(device: &Device<'_>) -> Result<String, Error> {
    let mut digits = [0; 40];
    let mut quoted = String::new();
    initialization_with(device, &mut digits, &mut quoted)
}

/// [`initialization`] with the hex digits of the password hash kept in
/// `digits` and its quoted copy in `quoted`, both zeroed before it returns
/// (`quoted` keeps its length, all zero bytes); only the answer keeps the
/// hash.
///
/// The SHA-1 state (it holds the password's last block) wipes itself when
/// the digest is dropped (`paperpaw_digest`'s `Drop`, #451).
fn initialization_with(
    device: &Device<'_>,
    digits: &mut [u8; 40],
    quoted: &mut String,
) -> Result<String, Error> {
    let answer = initialization_inner(device, digits, quoted);
    wipe(digits);
    wipe_string(quoted);
    answer
}

/// Zeroes `text` in place: it keeps its length, all zero bytes (valid text).
fn wipe_string(text: &mut String) {
    let mut bytes = core::mem::take(text).into_bytes();
    wipe(&mut bytes);
    *text = String::from_utf8(bytes).unwrap_or_default();
}

fn initialization_inner(
    device: &Device<'_>,
    digits: &mut [u8; 40],
    quoted: &mut String,
) -> Result<String, Error> {
    let mut extensions = String::new();
    for (at, extension) in device.extensions.iter().enumerate() {
        let literal = json_string(extension)?;
        extensions = concat(&[&extensions, if at == 0 { "" } else { ", " }, &literal])?;
    }
    let hash = match device.password {
        Some(password) if !device.challenge.is_empty() => {
            let mut sha = Sha1::new();
            sha.update(password.as_bytes());
            sha.update(device.challenge.as_bytes());
            sha_hex(sha, digits)
        }
        _ => "",
    };
    let (name, version) = (json_string(device.name)?, json_string(device.version)?);
    *quoted = json_string(hash)?;
    concat(&[
        "{\"versionOK\": true, \"maxBookContentPacketLen\": 65536, \"acceptedExtensions\": [",
        &extensions,
        "], \"extensionPathLengths\": {}, \
         \"canStreamBooks\": true, \"canStreamMetadata\": true, \
         \"canReceiveBookBinary\": true, \"canDeleteMultipleBooks\": true, \
         \"canUseCachedMetadata\": false, \"cacheUsesLpaths\": true, \
         \"canSendOkToSendbook\": false, \"canAcceptLibraryInfo\": true, \
         \"willAskForUpdateBooks\": false, \"useUuidFileNames\": false, \
         \"coverHeight\": 240, \"deviceKind\": \"PaperPaw\", \"deviceName\": ",
        &name,
        ", \"appName\": \"PaperPaw\", \"ccVersionNumber\": ",
        &version,
        ", \"passwordHash\": ",
        quoted,
        "}",
    ])
}

/// The 40 lower-case hex digits of a SHA-1, written into `out`.
fn sha_hex(sha: Sha1, out: &mut [u8; 40]) -> &str {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut raw = sha.finish();
    for (pair, byte) in out.as_chunks_mut::<2>().0.iter_mut().zip(raw.iter()) {
        pair[0] = HEX[usize::from(byte >> 4)];
        pair[1] = HEX[usize::from(byte & 15)];
    }
    wipe(&mut raw);
    core::str::from_utf8(out).unwrap_or_default()
}

/// The reply to Calibre's `GET_DEVICE_INFORMATION`: what Calibre told the
/// device about itself last (`SET_CALIBRE_DEVICE_INFO`), if anything.
///
/// # Errors
/// `NoMemory` when the reply cannot be kept.
pub fn device_information(
    stored: Option<&str>,
    name: &str,
    version: &str,
) -> Result<String, Error> {
    let own;
    let info = match stored.filter(|s| s.trim_start().starts_with('{')) {
        Some(stored) => stored,
        None => {
            own = concat(&["{\"device_name\": ", &json_string(name)?, "}"])?;
            &own
        }
    };
    let v = json_string(version)?;
    concat(&[
        "{\"device_info\": ",
        info,
        ", \"version\": ",
        &v,
        ", \"device_version\": ",
        &v,
        "}",
    ])
}

/// The path a book is to have below the library folder: Calibre's `lpath`,
/// with no way out of the folder and none into what is hidden there
/// (PaperPaw's own data): plain names alone.
///
/// # Errors
/// `NoMemory` when the path cannot be kept.
pub fn safe_path(lpath: &str) -> Result<Option<String>, Error> {
    let mut path = String::new();
    for part in lpath
        .split(['/', '\\'])
        .filter(|p| !p.is_empty() && *p != ".")
    {
        if !is_plain_name(part) {
            return Ok(None);
        }
        path.try_reserve(part.len() + 1)
            .map_err(|_| Error::NoMemory)?;
        if !path.is_empty() {
            path.push('/');
        }
        path.push_str(part);
    }
    Ok((!path.is_empty()).then_some(path))
}

/// The path a book Calibre sends is to have below the library folder:
/// [`safe_path`], and a book PaperPaw reads (`extensions`, lowercase, e.g.
/// `epub` or `fb2.zip`), so nothing else on the storage is written.
///
/// # Errors
/// `NoMemory` when the path cannot be kept.
pub fn book_path(lpath: &str, extensions: &[&str]) -> Result<Option<String>, Error> {
    let Some(path) = safe_path(lpath)? else {
        return Ok(None);
    };
    let name = path.rsplit('/').next().unwrap_or_default();
    let reads = extensions.iter().any(|ext| {
        let (Some(at), true) = (
            name.len().checked_sub(ext.len()),
            name.is_char_boundary(name.len().saturating_sub(ext.len())),
        ) else {
            return false;
        };
        name.get(at..)
            .is_some_and(|tail| tail.eq_ignore_ascii_case(ext))
            && at > 1
            && name.get(..at).is_some_and(|stem| stem.ends_with('.'))
    });
    Ok(reads.then_some(path))
}

/// `object` without its member `key`, e.g. a book's metadata without the
/// cover thumbnail Calibre sends with it, which nothing here needs.
///
/// # Errors
/// `NoMemory` when the result cannot be kept.
pub fn without_member(object: &str, key: &str) -> Result<String, Error> {
    let Some(value) = member(object, key) else {
        return copy(object);
    };
    // Where the value is in `object`, and the key before it.
    let Some(start) = (value.as_ptr() as usize).checked_sub(object.as_ptr() as usize) else {
        return copy(object);
    };
    let end = start + value.len();
    let quoted = json_string(key)?;
    let Some(key_at) = object.get(..start).and_then(|head| head.rfind(&quoted)) else {
        return copy(object);
    };
    if object
        .get(key_at + quoted.len()..start)
        .is_none_or(|between| between.trim() != ":")
    {
        return copy(object);
    }
    let (Some(head), Some(after)) = (object.get(..key_at), object.get(end..)) else {
        return copy(object);
    };
    match after.trim_start().strip_prefix(',') {
        // With the comma after it.
        Some(rest) => concat(&[head, rest.trim_start()]),
        // The last member: with the comma before it.
        None => {
            let before = head.trim_end();
            concat(&[before.strip_suffix(',').unwrap_or(before), after])
        }
    }
}

/// Calibre's answer to a broadcast: the port to connect to.
#[must_use]
pub fn port_of_answer(answer: &str) -> Option<u16> {
    if !answer.starts_with("calibre wireless device client") {
        return None;
    }
    let (_, ports) = answer.rsplit_once(';')?;
    ports.rsplit(',').next()?.trim().parse().ok()
}

/// The name Calibre gives in its answer, e.g. `pc`.
#[must_use]
pub fn name_of_answer(answer: &str) -> Option<&str> {
    let start = answer.find("(on ")? + 4;
    let tail = answer.get(start..)?;
    tail.get(..tail.find(')')?)
}

/// Pushes `message` fallibly (a helper for the plugin's queue).
///
/// # Errors
/// `NoMemory`.
pub fn queue(messages: &mut Vec<Message>, message: Message) -> Result<(), Error> {
    push(messages, message)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;
    use alloc::{format, vec};

    const ARGS: [&str; 7] = [
        "{}",
        r#"{"passwordChallenge": "", "validExtensions": ["epub", "fb2.zip"]}"#,
        r#"{"title": "Bücher ✓", "authors": ["A", "B"], "thumbnail": [1, 2, "x,y"], "n": 1.0}"#,
        r#"{"a": "q\"uote\\", "b": null}"#,
        r#"{"thumbnail": "zzz"}"#,
        "not json",
        r#"{"x": 1, "thumbnail": [1,2,3], "y": 2}"#,
    ];

    #[test]
    fn frames_are_the_length_then_the_message() {
        for op in [0u8, 7, 9, 20, 255] {
            for args in ARGS {
                let body = format!("[{op}, {args}]");
                let expected = format!("{}{body}", body.len());
                assert_eq!(frame(op, args).unwrap(), expected.into_bytes());
            }
        }
    }

    #[test]
    fn messages_are_read_in_any_chunking_and_hostile_bytes_are_refused() {
        let mut wire = String::new();
        for (n, args) in ARGS.iter().enumerate() {
            wire.push_str(core::str::from_utf8(&frame(n as u8, args).unwrap()).unwrap());
        }
        for chunk in [1usize, 2, 3, 7, 64, usize::MAX] {
            let mut framing = Framing::default();
            let mut rest = wire.as_str();
            let mut got = Vec::new();
            while !rest.is_empty() {
                let mut n = (framing.wanted() as usize).min(chunk).min(rest.len());
                while !rest.is_char_boundary(n) {
                    n += 1;
                }
                let (part, tail) = rest.split_at(n);
                rest = tail;
                if let Some(message) = framing.feed(part).unwrap() {
                    got.push((message.op, message.args));
                }
            }
            // "not json" is not an object but is a message all the same.
            assert_eq!(got.len(), ARGS.len(), "chunk {chunk}");
            for (n, (op, args)) in got.iter().enumerate() {
                assert_eq!((*op, args.as_str()), (n as u8, ARGS[n]));
            }
        }
        for (hostile, why) in [
            ("x", Refused::NotAMessage),
            ("[", Refused::NoLength),
            ("0[", Refused::BadLength),
            ("123456789[", Refused::LengthTooLong),
            ("9999999[", Refused::BadLength),
        ] {
            let mut framing = Framing::new(1000);
            assert_eq!(framing.feed(hostile).unwrap_err(), why, "{hostile:?}");
        }
        for bad in ["3[0]", "7[a, {}]", "3[]]"] {
            let mut framing = Framing::new(1000);
            assert_eq!(
                framing.feed(bad).unwrap_err(),
                Refused::BadMessage,
                "{bad:?}"
            );
        }
        // What has not come yet is waited for, not refused.
        for partial in ["0", "12", "12[0, ", "7"] {
            let mut framing = Framing::new(1000);
            assert!(framing.feed(partial).unwrap().is_none(), "{partial:?}");
        }
    }

    #[test]
    fn the_members_are_read() {
        let book = ARGS[2];
        assert_eq!(text(book, "title").unwrap().as_deref(), Some("Bücher ✓"));
        assert_eq!(number(book, "n"), Some(1));
        assert_eq!(strings(book, "authors").unwrap(), vec!["A", "B"]);
        assert_eq!(text(book, "none").unwrap(), None);
        assert_eq!(number(book, "title"), None);
        assert_eq!(text(ARGS[3], "a").unwrap().as_deref(), Some("q\"uote\\"));
        // Written, not a string: as written.
        assert_eq!(text(ARGS[3], "b").unwrap().as_deref(), Some("null"));
        assert_eq!(member(ARGS[6], "y"), Some("2"));
        for args in ["not json", "{}", ARGS[6]] {
            assert!(strings(args, "authors").unwrap().is_empty());
        }
        // Other elements and hostile arrays give what is there, never a panic.
        assert_eq!(
            strings(r#"{"t": ["a", 1, [2, "x"], {"k": "v"}, "b"]}"#, "t").unwrap(),
            ["a", "b"]
        );
        for cut in [
            r#"{"t": ["a", "b"#,
            r#"{"t": ["a", "#,
            r#"{"t": [,,"a","#,
            r#"{"t": ["#,
        ] {
            let got = strings(cut, "t").unwrap();
            assert!(got.len() <= 1, "{cut:?} {got:?}");
        }
    }

    #[test]
    fn without_member_cuts_the_thumbnail() {
        let cut = |args: &str| without_member(args, "thumbnail").unwrap();
        assert_eq!(
            cut(ARGS[2]),
            r#"{"title": "Bücher ✓", "authors": ["A", "B"], "n": 1.0}"#
        );
        assert_eq!(cut(ARGS[6]), r#"{"x": 1, "y": 2}"#);
        assert_eq!(cut(ARGS[4]), "{}");
        assert_eq!(cut("not json"), "not json");
        assert_eq!(cut(ARGS[0]), "{}");
    }

    #[test]
    fn a_body_is_a_message_or_nothing() {
        let message = |body: &str| parse(body).unwrap().map(|m| (m.op, m.args));
        assert_eq!(message("[0, {}]"), Some((0, "{}".into())));
        assert_eq!(message(" [3,{\"a\":1}] "), Some((3, "{\"a\":1}".into())));
        for body in ["[x, {}]", "0, {}", "[1]", "[]", "[300, {}]"] {
            assert_eq!(message(body), None, "{body:?}");
        }
    }

    const INIT_HEAD: &str = "{\"versionOK\": true, \"maxBookContentPacketLen\": 65536, \"acceptedExtensions\": [\"epub\", \"fb2\", \"fb2.zip\", \"txt\"], \"extensionPathLengths\": {}, \"canStreamBooks\": true, \"canStreamMetadata\": true, \"canReceiveBookBinary\": true, \"canDeleteMultipleBooks\": true, \"canUseCachedMetadata\": false, \"cacheUsesLpaths\": true, \"canSendOkToSendbook\": false, \"canAcceptLibraryInfo\": true, \"willAskForUpdateBooks\": false, \"useUuidFileNames\": false, \"coverHeight\": 240, \"deviceKind\": \"PaperPaw\", \"deviceName\": \"My \\\"Reader\\\"\", \"appName\": \"PaperPaw\", \"ccVersionNumber\": \"1.0.9\", \"passwordHash\": \"";

    #[test]
    fn the_device_speaks_as_interface_6_did() {
        let extensions = ["epub", "fb2", "fb2.zip", "txt"];
        for (challenge, password, hash) in [
            ("", None, ""),
            ("abc", None, ""),
            (
                "abc",
                Some("pw"),
                "cf2b7e4e50a003a1a04361027f6a339c2dda6bb1",
            ),
        ] {
            let device = Device {
                name: "My \"Reader\"",
                extensions: &extensions,
                challenge,
                password,
                version: "1.0.9",
            };
            assert_eq!(
                initialization(&device).unwrap(),
                format!("{INIT_HEAD}{hash}\"}}")
            );
        }
        let version = "\"version\": \"1.0.9\", \"device_version\": \"1.0.9\"}";
        for (stored, info) in [
            (None, "{\"device_name\": \"Name \\\"x\\\"\"}"),
            (Some("{\"k\": 1}"), "{\"k\": 1}"),
            (Some("nonsense"), "{\"device_name\": \"Name \\\"x\\\"\"}"),
        ] {
            assert_eq!(
                device_information(stored, "Name \"x\"", "1.0.9").unwrap(),
                format!("{{\"device_info\": {info}, {version}")
            );
        }
    }

    #[test]
    fn the_password_is_never_in_the_debug_text() {
        let device = Device {
            name: "n",
            extensions: &[],
            challenge: "abc",
            password: Some("hunter2-secret"),
            version: "v",
        };
        let shown = alloc::format!("{device:?}");
        assert!(!shown.contains("hunter2"), "{shown}");
        assert!(shown.contains("<set>"));
    }

    #[test]
    fn the_hash_digits_are_zeroed_once_the_answer_is_made() {
        let device = Device {
            name: "n",
            extensions: &["epub"],
            challenge: "abc",
            password: Some("pw"),
            version: "v",
        };
        let mut digits = [0xAA; 40];
        let mut quoted = String::new();
        let answer = initialization_with(&device, &mut digits, &mut quoted).unwrap();
        assert_eq!(digits, [0; 40]);
        // The quoted hash copy (`"` + 40 digits + `"`) is zeroed, not freed.
        assert_eq!(quoted.len(), 42);
        assert!(quoted.bytes().all(|b| b == 0), "{quoted:?}");
        assert_eq!(answer, initialization(&device).unwrap());
        assert!(answer.contains("\"passwordHash\": \""));
        // Also with no extension in the answer.
        let mut digits = [0xAA; 40];
        let none = Device {
            extensions: &[],
            ..device
        };
        let mut quoted = String::new();
        initialization_with(&none, &mut digits, &mut quoted).unwrap();
        assert_eq!(digits, [0; 40]);
        assert!(quoted.bytes().all(|b| b == 0));
    }

    #[test]
    fn paths_never_leave_the_folder() {
        let extensions = ["epub", "fb2.zip", "txt"];
        // (lpath, the path below the library folder, the same for a book).
        let cases: [(&str, Option<&str>, Option<&str>); 21] = [
            ("a.epub", Some("a.epub"), Some("a.epub")),
            (
                "dir/sub/b.EPUB",
                Some("dir/sub/b.EPUB"),
                Some("dir/sub/b.EPUB"),
            ),
            (
                "dir\\sub\\c.txt",
                Some("dir/sub/c.txt"),
                Some("dir/sub/c.txt"),
            ),
            ("./x/./y.txt", Some("x/y.txt"), Some("x/y.txt")),
            ("../evil.epub", None, None),
            ("a/../b.txt", None, None),
            (".paperpaw/x.epub", None, None),
            ("x/.hidden.epub", None, None),
            (" lead.epub", None, None),
            ("trail .epub", Some("trail .epub"), Some("trail .epub")),
            ("a:b.epub", None, None),
            ("x.epub.", None, None),
            ("", None, None),
            ("/", None, None),
            ("///", None, None),
            (".epub", None, None),
            ("a.fb2.zip", Some("a.fb2.zip"), Some("a.fb2.zip")),
            ("a.zip", Some("a.zip"), None),
            ("ünï/ö.TXT", Some("ünï/ö.TXT"), Some("ünï/ö.TXT")),
            ("con\u{1}trol.txt", None, None),
            ("é.txt", Some("é.txt"), Some("é.txt")),
        ];
        for (lpath, safe, book) in cases {
            assert_eq!(safe_path(lpath).unwrap().as_deref(), safe, "{lpath:?}");
            assert_eq!(
                book_path(lpath, &extensions).unwrap().as_deref(),
                book,
                "{lpath:?}"
            );
        }
    }

    #[test]
    fn calibres_answers_to_the_broadcast_are_read() {
        for (answer, port, name) in [
            (
                "calibre wireless device client (on pc);59678,9090",
                Some(9090),
                Some("pc"),
            ),
            (
                "calibre wireless device client (on pc);9090",
                Some(9090),
                Some("pc"),
            ),
            ("calibre wireless device client", None, None),
            ("calibre wireless device client (on ", None, None),
            (
                "calibre wireless device client (on é);1,2,x",
                None,
                Some("é"),
            ),
            ("other", None, None),
            ("calibre wireless device client (on );70000", None, Some("")),
        ] {
            assert_eq!(port_of_answer(answer), port, "{answer:?}");
            assert_eq!(name_of_answer(answer), name, "{answer:?}");
        }
    }

    #[test]
    fn json_strings_are_quoted_and_escaped() {
        for (text, quoted) in [
            ("", "\"\""),
            ("plain", "\"plain\""),
            ("q\"uote", "\"q\\\"uote\""),
            ("back\\slash", "\"back\\\\slash\""),
            ("a\nb\rc\td", "\"a\\nb\\rc\\td\""),
            ("\u{1}\u{1f}", "\"\\u0001\\u001f\""),
            ("ünï 😀", "\"ünï 😀\""),
        ] {
            assert_eq!(json_string(text).unwrap(), quoted, "{text:?}");
            assert_eq!(
                paperpaw_ui_kit::json::unquote(quoted).unwrap().as_deref(),
                Some(text)
            );
        }
    }
}
