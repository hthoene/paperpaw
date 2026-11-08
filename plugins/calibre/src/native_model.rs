//! Calibre's wireless device protocol for the native plugin, without host
//! calls: messages are JSON arrays `[opcode, {arguments}]`, each written after
//! its length in decimal digits. It is the interface-6 `protocol` made
//! fallible (a refused allocation is `NoMemory`, never an abort) and free of
//! slicing that could panic; each part is tested to give the same answers.
//!
//! Members of the arguments are read with `paperpaw_sdk::json` until Kernel's
//! fallible reader (#414) is on integration: it allocates without a refusal
//! path, so what it takes is bounded by the longest message the framing lets
//! through ([`max_message`]), not by this crate (`PORTING-7.md`).
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{MemoryClass, digest::sha1::Sha1, files::is_plain_name, json, native::Error};
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy, decimal},
};

pub use crate::protocol::{BROADCAST_PORTS, DEFAULT_PORT, op};

/// A message: its opcode and its arguments as JSON (an object).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub op: u8,
    pub args: String,
}

/// Longest message read on a device with `memory`: a book's metadata with
/// its description and cover thumbnail is the largest, a few dozen
/// kilobytes. A message is held whole.
#[must_use]
pub const fn max_message(memory: MemoryClass) -> usize {
    match memory {
        MemoryClass::Small => 64 * 1024,
        _ => 1 << 20,
    }
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
    json::raw_member(object, key)
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
    match json::unquote(raw) {
        Some(text) => Ok(Some(text)),
        None => copy(raw).map(Some),
    }
}

/// Member `key` of `object` as a number.
#[must_use]
pub fn number(object: &str, key: &str) -> Option<u64> {
    member(object, key)?.parse::<f64>().ok().map(|n| n as u64)
}

/// The strings of the array member `key` of `object`.
#[must_use]
pub fn strings(object: &str, key: &str) -> Vec<String> {
    member(object, key)
        .map(json::string_array)
        .unwrap_or_default()
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
#[derive(Debug)]
pub struct Device<'a> {
    pub name: &'a str,
    /// The file extensions PaperPaw reads.
    pub extensions: &'a [&'a str],
    /// Calibre's challenge and the password, if one is set.
    pub challenge: &'a str,
    pub password: Option<&'a str>,
    pub version: &'a str,
}

/// The answer to `GET_INITIALIZATION_INFO`.
///
/// # Errors
/// `NoMemory` when the answer cannot be kept.
pub fn initialization(device: &Device<'_>) -> Result<String, Error> {
    let mut extensions = String::new();
    for (at, extension) in device.extensions.iter().enumerate() {
        let literal = json_string(extension)?;
        extensions = concat(&[&extensions, if at == 0 { "" } else { ", " }, &literal])?;
    }
    // TODO(#421): wipe the SHA-1 state, the hex array and the hash with
    // `mem::wipe` once it lands; the password is secret.
    let mut digits = [0; 40];
    let hash = match device.password {
        Some(password) if !device.challenge.is_empty() => {
            let mut sha = Sha1::new();
            sha.update(password.as_bytes());
            sha.update(device.challenge.as_bytes());
            sha_hex(sha, &mut digits)
        }
        _ => "",
    };
    let (name, version, hash) = (
        json_string(device.name)?,
        json_string(device.version)?,
        json_string(hash)?,
    );
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
        &hash,
        "}",
    ])
}

/// The 40 lower-case hex digits of a SHA-1, written into `out`.
fn sha_hex(sha: Sha1, out: &mut [u8; 40]) -> &str {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for (pair, byte) in out.as_chunks_mut::<2>().0.iter_mut().zip(sha.finish()) {
        pair[0] = HEX[usize::from(byte >> 4)];
        pair[1] = HEX[usize::from(byte & 15)];
    }
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
    use crate::protocol as old;
    use alloc::{format, string::ToString};

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
    fn frames_are_what_interface_6_wrote() {
        for op in [0u8, 7, 9, 20, 255] {
            for args in ARGS {
                assert_eq!(frame(op, args).unwrap(), old::frame(op, args));
            }
        }
    }

    #[test]
    fn messages_are_read_as_before_in_any_chunking_and_hostile_bytes_are_refused() {
        let mut wire = String::new();
        for (n, args) in ARGS.iter().enumerate() {
            wire.push_str(core::str::from_utf8(&frame(n as u8, args).unwrap()).unwrap());
        }
        for chunk in [1usize, 2, 3, 7, 64, usize::MAX] {
            let (mut new, mut oldf) = (Framing::default(), old::Framing::default());
            let mut rest = wire.as_str();
            while !rest.is_empty() {
                let n = (new.wanted() as usize).min(chunk).min(rest.len());
                let mut n = n;
                while !rest.is_char_boundary(n) {
                    n += 1;
                }
                assert_eq!(new.wanted(), oldf.wanted());
                let (part, tail) = rest.split_at(n);
                rest = tail;
                let got = new.feed(part).map_err(Refused::words);
                let was = oldf.feed(part);
                match (got, was) {
                    (Ok(Some(a)), Ok(Some(b))) => assert_eq!((a.op, a.args), (b.op, b.args)),
                    (a, b) => assert_eq!(a.map(|m| m.is_some()), b.map(|m| m.is_some())),
                }
            }
        }
        for hostile in [
            "x",
            "[",
            "0[",
            "123456789[",
            "9999999[",
            "5[0]",
            "7[a, {}]",
            "7 [0, {}]",
            "0",
            "12",
            "3[]]",
        ] {
            let (mut new, mut oldf) = (Framing::new(1000), old::Framing::new(1000));
            let a = new
                .feed(hostile)
                .map_err(Refused::words)
                .map(|m| m.map(|m| (m.op, m.args)));
            let b = oldf.feed(hostile).map(|m| m.map(|m| (m.op, m.args)));
            assert_eq!(a, b, "{hostile:?}");
        }
    }

    #[test]
    fn the_members_are_read_as_before() {
        for args in ARGS {
            for key in [
                "title",
                "authors",
                "thumbnail",
                "n",
                "a",
                "b",
                "x",
                "y",
                "none",
            ] {
                assert_eq!(member(args, key), old::member(args, key));
                assert_eq!(text(args, key).unwrap(), old::text(args, key));
                assert_eq!(number(args, key), old::number(args, key));
                assert_eq!(strings(args, key), old::strings(args, key));
                assert_eq!(
                    without_member(args, key).unwrap(),
                    old::without_member(args, key),
                    "{args:?} {key}"
                );
            }
        }
        for body in [
            "[0, {}]",
            " [3,{\"a\":1}] ",
            "[x, {}]",
            "0, {}",
            "[1]",
            "[]",
            "[300, {}]",
        ] {
            assert_eq!(
                parse(body).unwrap(),
                old::parse(body).map(|m| Message {
                    op: m.op,
                    args: m.args
                })
            );
        }
    }

    #[test]
    fn the_device_speaks_as_before() {
        let extensions = ["epub", "fb2", "fb2.zip", "txt"];
        for (challenge, password) in [
            ("", None),
            ("abc", None),
            ("abc", Some("pw")),
            ("", Some("pw")),
        ] {
            let new = Device {
                name: "My \"Reader\"",
                extensions: &extensions,
                challenge,
                password,
                version: "1.0.9",
            };
            let oldd = old::Device {
                name: new.name,
                extensions: new.extensions,
                challenge,
                password,
                version: new.version,
            };
            assert_eq!(initialization(&new).unwrap(), old::initialization(&oldd));
        }
        for stored in [
            None,
            Some("{\"k\": 1}"),
            Some("  {\"k\": 1}"),
            Some("nonsense"),
        ] {
            assert_eq!(
                device_information(stored, "Name \"x\"", "1.0.9").unwrap(),
                old::device_information(stored, "Name \"x\"", "1.0.9")
            );
        }
        assert_eq!(
            initialization(&Device {
                name: "n",
                extensions: &[],
                challenge: "",
                password: None,
                version: "v"
            })
            .unwrap(),
            old::initialization(&old::Device {
                name: "n",
                extensions: &[],
                challenge: "",
                password: None,
                version: "v"
            })
        );
    }

    #[test]
    fn paths_are_as_before_and_never_leave_the_folder() {
        let extensions = ["epub", "fb2.zip", "txt"];
        for lpath in [
            "a.epub",
            "dir/sub/b.EPUB",
            "dir\\sub\\c.txt",
            "./x/./y.txt",
            "../evil.epub",
            "a/../b.txt",
            ".paperpaw/x.epub",
            "x/.hidden.epub",
            " lead.epub",
            "trail .epub",
            "a:b.epub",
            "x.epub.",
            "",
            "/",
            "///",
            ".epub",
            "a.fb2.zip",
            "a.zip",
            "ünï/ö.TXT",
            "con\u{1}trol.txt",
            "é.txt",
            "x.t\u{e9}xt",
        ] {
            assert_eq!(
                safe_path(lpath).unwrap(),
                old::safe_path(lpath),
                "{lpath:?}"
            );
            assert_eq!(
                book_path(lpath, &extensions).unwrap(),
                old::book_path(lpath, &extensions),
                "{lpath:?}"
            );
        }
    }

    #[test]
    fn calibres_answers_to_the_broadcast_are_read_as_before() {
        for answer in [
            "calibre wireless device client (on pc);59678,9090",
            "calibre wireless device client (on pc);9090",
            "calibre wireless device client",
            "calibre wireless device client (on ",
            "calibre wireless device client (on pc",
            "calibre wireless device client (on é);1,2,x",
            "other",
            "",
            "calibre wireless device client (on );70000",
        ] {
            assert_eq!(
                port_of_answer(answer),
                old::port_of_answer(answer),
                "{answer:?}"
            );
            assert_eq!(
                name_of_answer(answer),
                old::name_of_answer(answer),
                "{answer:?}"
            );
        }
    }

    #[test]
    fn json_strings_are_as_before() {
        for text in [
            "",
            "plain",
            "q\"uote",
            "back\\slash",
            "a\nb\rc\td",
            "\u{1}\u{1f}",
            "ünï 😀",
        ] {
            assert_eq!(json_string(text).unwrap(), json::string(text), "{text:?}");
        }
        assert_eq!(format!("{}", 1), 1.to_string());
    }
}
