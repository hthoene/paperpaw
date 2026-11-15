//! Reading JSON as it streams, e.g. from a file, with little memory: a
//! [`Parser`] reports each value with its path ([`Event`]), and the
//! caller keeps only what it needs. Strings and other plain values are
//! cut at [`MAX_STRING`] bytes, keys at 256, and objects and arrays nested
//! deeper than [`MAX_DEPTH`] are skipped.
//!
//! [`Parser::try_feed`] and [`Parser::try_finish`] allocate fallibly: memory
//! refused is [`OutOfMemory`], after which the parser is dropped, never an
//! abort. [`Parser::feed`] and [`Parser::finish`] abort as other infallible
//! allocations do.
//!
//! ```
//! use paperpaw_json::stream::{Event, Parser};
//!
//! let mut titles = Vec::new();
//! let mut parser = Parser::default();
//! parser.feed(br#"{"items": [{"title": "A"}, {"title": "B"}]}"#, &mut |event| {
//!     if let Event::Value(path, text) = event
//!         && path == ["items", "[]", "title"]
//!     {
//!         titles.push(text.to_string());
//!     }
//! });
//! assert_eq!(titles, ["A", "B"]);
//! ```

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_platform::Storage;

/// Pieces a file is read in: three of a card's sectors, on the stack of
/// [`read_file`], whose frame stays under the firmware's limit of 2,048
/// bytes with it.
const CHUNK: usize = 1536;
/// Strings and other plain values are cut at this many bytes.
pub const MAX_STRING: usize = 4096;
/// Objects and arrays nested deeper than this are skipped whole, without
/// events.
pub const MAX_DEPTH: usize = 64;
/// Keys are cut at this many bytes: the path keeps one for each level.
const MAX_KEY: usize = 256;
/// What a broken `\u` escape or a lone surrogate stands for.
const REPLACEMENT: &[u8] = "\u{fffd}".as_bytes();

/// Where a value is: the keys from the top, `[]` for array items.
pub type Path = Vec<String>;

/// Memory for the parser's path or text was refused
/// ([`Parser::try_feed`], [`Parser::try_finish`], [`try_read_file`]). The
/// parser is in no state to go on: drop it. Events reported before were
/// whole; nothing cut or partial is reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct OutOfMemory;

impl core::fmt::Display for OutOfMemory {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("not enough memory to read the JSON")
    }
}

type Step = Result<(), OutOfMemory>;

/// Room for `more` items in `items`.
fn reserve<T>(items: &mut Vec<T>, more: usize) -> Step {
    items.try_reserve(more).map_err(|_| OutOfMemory)
}

/// `text` as an owned string, allocated fallibly.
fn owned(text: &str) -> Result<String, OutOfMemory> {
    let mut out = String::new();
    out.try_reserve_exact(text.len()).map_err(|_| OutOfMemory)?;
    out.push_str(text);
    Ok(out)
}

/// `bytes` as text, invalid UTF-8 replaced (as `String::from_utf8_lossy`),
/// reusing their buffer where they are valid and allocating fallibly where
/// not.
fn lossy(bytes: Vec<u8>) -> Result<String, OutOfMemory> {
    match String::from_utf8(bytes) {
        Ok(text) => Ok(text),
        Err(e) => lossy_copy(e.as_bytes()),
    }
}

/// `bytes` as text in a new string of exactly their length, invalid UTF-8
/// replaced (as `String::from_utf8_lossy`), allocated fallibly.
fn lossy_copy(bytes: &[u8]) -> Result<String, OutOfMemory> {
    if let Ok(text) = core::str::from_utf8(bytes) {
        return owned(text);
    }
    let replacement = char::REPLACEMENT_CHARACTER.len_utf8();
    let len = bytes
        .utf8_chunks()
        .map(|c| {
            c.valid().len()
                + if c.invalid().is_empty() {
                    0
                } else {
                    replacement
                }
        })
        .sum();
    let mut text = String::new();
    text.try_reserve_exact(len).map_err(|_| OutOfMemory)?;
    for chunk in bytes.utf8_chunks() {
        text.push_str(chunk.valid());
        if !chunk.invalid().is_empty() {
            text.push(char::REPLACEMENT_CHARACTER);
        }
    }
    Ok(text)
}

/// The infallible forms: memory refused aborts, as an infallible
/// allocation does.
fn or_abort(step: Step) {
    if step.is_err() {
        alloc::alloc::handle_alloc_error(core::alloc::Layout::new::<u8>());
    }
}

/// What the parser reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event<'a> {
    /// An object or array starts at this path.
    Open(&'a [String]),
    /// It ends.
    Close(&'a [String]),
    /// A string (or other plain value, as text) at this path.
    Value(&'a [String], &'a str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Container {
    Object,
    Array,
}

#[derive(Debug, Default)]
enum State {
    #[default]
    Between,
    /// In a string: whether it is a key, and an escape seen.
    Text {
        key: bool,
        escape: bool,
        /// A `\u` escape so far: how many hex digits, and their value.
        unicode: Option<(u8, u32)>,
    },
    /// In a number or `true`/`false`/`null`.
    Plain,
}

/// A streaming JSON parser that reports values with their paths.
#[derive(Debug, Default)]
pub struct Parser {
    /// The path of the innermost open object or array.
    path: Path,
    containers: Vec<Container>,
    /// The key of the next value in an object.
    key: Option<String>,
    state: State,
    /// The text of the string or plain value being read.
    bytes: Vec<u8>,
    /// A high surrogate waiting for its low half.
    surrogate: Option<u32>,
    /// How deep the parser is in objects and arrays past [`MAX_DEPTH`].
    skipped: usize,
}

impl Parser {
    paperpaw_table::shared! {
    /// Reads `data`, the next piece of the text, reporting what it finds.
    pub fn feed(&mut self, data: &[u8], emit: &mut dyn FnMut(Event<'_>)) {
        or_abort(self.try_feed(data, emit));
    } @ data::JSON_STREAM_FEED
    }

    paperpaw_table::shared! {
    /// Ends the text: reports a number or `true`/`false`/`null` it ends
    /// with, which only what follows it could end otherwise (e.g. `42` as
    /// the whole text). A string left open is not reported.
    pub fn finish(&mut self, emit: &mut dyn FnMut(Event<'_>)) {
        or_abort(self.try_finish(emit));
    } @ data::JSON_STREAM_FINISH
    }

    /// As [`Parser::feed`], allocating fallibly.
    ///
    /// # Errors
    /// [`OutOfMemory`] where memory for the path or a text is refused; the
    /// parser is then dropped.
    pub fn try_feed(
        &mut self,
        data: &[u8],
        emit: &mut dyn FnMut(Event<'_>),
    ) -> Result<(), OutOfMemory> {
        for &b in data {
            self.byte(b, emit)?;
        }
        Ok(())
    }

    /// As [`Parser::finish`], allocating fallibly.
    ///
    /// # Errors
    /// [`OutOfMemory`] where memory for the last value is refused.
    pub fn try_finish(&mut self, emit: &mut dyn FnMut(Event<'_>)) -> Result<(), OutOfMemory> {
        if matches!(self.state, State::Plain) {
            self.end_plain(emit)?;
        }
        Ok(())
    }

    /// Adds `bytes` to the text being read.
    fn push_bytes(&mut self, bytes: &[u8]) -> Step {
        self.flush_surrogate()?;
        if self.bytes.len() < MAX_STRING {
            reserve(&mut self.bytes, bytes.len())?;
            self.bytes.extend_from_slice(bytes);
        }
        Ok(())
    }

    fn push_char(&mut self, c: char) -> Step {
        let mut buf = [0u8; 4];
        self.push_bytes(c.encode_utf8(&mut buf).as_bytes())
    }

    /// Adds the UTF-16 code unit of a `\u` escape.
    fn push_unit(&mut self, unit: u32) -> Step {
        match (self.surrogate, unit) {
            (Some(high), 0xdc00..=0xdfff) => {
                self.surrogate = None;
                let code = 0x10000 + ((high - 0xd800) << 10) + (unit - 0xdc00);
                self.push_char(char::from_u32(code).unwrap_or('\u{fffd}'))
            }
            (_, 0xd800..=0xdbff) => {
                self.flush_surrogate()?;
                self.surrogate = Some(unit);
                Ok(())
            }
            _ => self.push_char(char::from_u32(unit).unwrap_or('\u{fffd}')),
        }
    }

    /// A high surrogate not followed by its low half stands for U+FFFD.
    fn flush_surrogate(&mut self) -> Step {
        if self.surrogate.take().is_some() && self.bytes.len() < MAX_STRING {
            reserve(&mut self.bytes, REPLACEMENT.len())?;
            self.bytes.extend_from_slice(REPLACEMENT);
        }
        Ok(())
    }

    /// Reports `text` as the next value of the open object or array.
    fn value(&mut self, text: &str, emit: &mut dyn FnMut(Event<'_>)) -> Result<(), OutOfMemory> {
        match self.containers.last() {
            Some(Container::Array) => {
                reserve(&mut self.path, 1)?;
                self.path.push(owned("[]")?);
                emit(Event::Value(&self.path, text));
                self.path.pop();
            }
            Some(Container::Object) => {
                reserve(&mut self.path, 1)?;
                let key = self.key.take();
                let has_key = key.is_some();
                self.path.push(key.unwrap_or_default());
                emit(Event::Value(&self.path, text));
                // The key stays until the next `,`.
                self.key = self.path.pop().filter(|_| has_key);
            }
            None => emit(Event::Value(&self.path, text)),
        }
        Ok(())
    }

    fn open(
        &mut self,
        container: Container,
        emit: &mut dyn FnMut(Event<'_>),
    ) -> Result<(), OutOfMemory> {
        if self.skipped > 0 || self.containers.len() >= MAX_DEPTH {
            self.skipped = self.skipped.saturating_add(1);
            return Ok(());
        }
        reserve(&mut self.containers, 1)?;
        match self.containers.last() {
            Some(Container::Array) => {
                reserve(&mut self.path, 1)?;
                self.path.push(owned("[]")?);
            }
            Some(Container::Object) => {
                reserve(&mut self.path, 1)?;
                self.path.push(self.key.take().unwrap_or_default());
            }
            None => {}
        }
        emit(Event::Open(&self.path));
        self.containers.push(container);
        self.key = None;
        Ok(())
    }

    fn close(&mut self, emit: &mut dyn FnMut(Event<'_>)) {
        if self.skipped > 0 {
            self.skipped -= 1;
            return;
        }
        self.containers.pop();
        emit(Event::Close(&self.path));
        let last = self.path.pop();
        // Back in an object: the key it was under.
        self.key = match self.containers.last() {
            Some(Container::Object) => last,
            _ => None,
        };
    }

    fn end_string(
        &mut self,
        key: bool,
        emit: &mut dyn FnMut(Event<'_>),
    ) -> Result<(), OutOfMemory> {
        self.flush_surrogate()?;
        self.state = State::Between;
        if self.skipped == 0 {
            if key {
                self.bytes.truncate(MAX_KEY);
                // An exact copy: the path keeps every key, so a key must
                // not keep the (up to `MAX_STRING`) text buffer.
                self.key = Some(lossy_copy(&self.bytes)?);
                self.bytes.clear();
                return Ok(());
            }
            self.emit_text(emit)?;
        }
        self.bytes.clear();
        Ok(())
    }

    fn end_plain(&mut self, emit: &mut dyn FnMut(Event<'_>)) -> Result<(), OutOfMemory> {
        self.state = State::Between;
        if self.skipped == 0 {
            self.emit_text(emit)?;
        }
        self.bytes.clear();
        Ok(())
    }

    /// Reports the text read as a value; its buffer is kept for the next.
    fn emit_text(&mut self, emit: &mut dyn FnMut(Event<'_>)) -> Result<(), OutOfMemory> {
        let text = lossy(core::mem::take(&mut self.bytes))?;
        let reported = self.value(&text, emit);
        self.bytes = text.into_bytes();
        reported
    }

    fn byte(&mut self, b: u8, emit: &mut dyn FnMut(Event<'_>)) -> Result<(), OutOfMemory> {
        match &mut self.state {
            State::Text {
                key,
                escape,
                unicode,
            } => {
                if let Some((digits, value)) = unicode {
                    let Some(digit) = char::from(b).to_digit(16) else {
                        // A cut escape: the byte is read as it is.
                        *unicode = None;
                        self.push_bytes(REPLACEMENT)?;
                        return self.byte(b, emit);
                    };
                    *value = *value * 16 + digit;
                    *digits += 1;
                    if *digits == 4 {
                        let unit = *value;
                        *unicode = None;
                        self.push_unit(unit)?;
                    }
                    return Ok(());
                }
                if *escape {
                    *escape = false;
                    let b = match b {
                        b'n' => b'\n',
                        b't' => b'\t',
                        b'r' => b'\r',
                        b'b' | b'f' => b' ',
                        b'u' => {
                            *unicode = Some((0, 0));
                            return Ok(());
                        }
                        other => other,
                    };
                    return self.push_bytes(&[b]);
                }
                match b {
                    b'\\' => {
                        *escape = true;
                        Ok(())
                    }
                    b'"' => {
                        let key = *key;
                        self.end_string(key, emit)
                    }
                    _ => self.push_bytes(&[b]),
                }
            }
            State::Plain => {
                if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+') {
                    self.push_bytes(&[b])
                } else {
                    self.end_plain(emit)?;
                    self.byte(b, emit)
                }
            }
            State::Between => match b {
                b'{' => self.open(Container::Object, emit),
                b'[' => self.open(Container::Array, emit),
                b'}' | b']' => {
                    self.close(emit);
                    Ok(())
                }
                b'"' => {
                    let key = self.skipped == 0
                        && self.containers.last() == Some(&Container::Object)
                        && self.key.is_none();
                    self.state = State::Text {
                        key,
                        escape: false,
                        unicode: None,
                    };
                    Ok(())
                }
                b',' => {
                    if self.skipped == 0 && self.containers.last() == Some(&Container::Object) {
                        self.key = None;
                    }
                    Ok(())
                }
                b':' => Ok(()),
                b if b.is_ascii_whitespace() => Ok(()),
                b => {
                    self.state = State::Plain;
                    self.push_bytes(&[b])
                }
            },
        }
    }
}

paperpaw_table::shared! {
/// Reads the JSON file at `path`, a piece at a time, reporting what it
/// finds to `emit`.
pub fn read_file(storage: &dyn Storage, path: &str, emit: &mut dyn FnMut(Event<'_>)) {
    let mut parser = Parser::default();
    let mut buf = [0u8; CHUNK];
    let mut offset = 0;
    while let Ok(n) = storage.read_at(path, offset, &mut buf) {
        if n == 0 {
            break;
        }
        parser.feed(&buf[..n], emit);
        offset += n as u64;
    }
    parser.finish(emit);
} @ data::JSON_READ_FILE
}

/// As [`read_file`], allocating fallibly: reading ends at the first refusal.
///
/// # Errors
/// [`OutOfMemory`] where memory for the path or a text is refused.
pub fn try_read_file(
    storage: &dyn Storage,
    path: &str,
    emit: &mut dyn FnMut(Event<'_>),
) -> Result<(), OutOfMemory> {
    let mut parser = Parser::default();
    let mut buf = [0u8; CHUNK];
    let mut offset = 0;
    while let Ok(n) = storage.read_at(path, offset, &mut buf) {
        if n == 0 {
            break;
        }
        parser.try_feed(&buf[..n], emit)?;
        offset += n as u64;
    }
    parser.try_finish(emit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::ToString;

    /// The values of `json` with their paths joined by `/`.
    fn values(json: &[u8]) -> Vec<(String, String)> {
        let mut out = Vec::new();
        Parser::default().feed(json, &mut |event| {
            if let Event::Value(path, text) = event {
                out.push((path.join("/"), text.to_string()));
            }
        });
        out
    }

    fn pair(path: &str, text: &str) -> (String, String) {
        (path.to_string(), text.to_string())
    }

    #[test]
    fn reports_paths() {
        let mut events = Vec::new();
        Parser::default().feed(br#"{"a": [1, {"b": "x"}], "c": true}"#, &mut |event| {
            events.push(match event {
                Event::Open(path) => format!("open {}", path.join("/")),
                Event::Close(path) => format!("close {}", path.join("/")),
                Event::Value(path, text) => format!("{} = {text}", path.join("/")),
            });
        });
        assert_eq!(
            events,
            [
                "open ",
                "open a",
                "a/[] = 1",
                "open a/[]",
                "a/[]/b = x",
                "close a/[]",
                "close a",
                "c = true",
                "close ",
            ]
        );
    }

    #[test]
    fn cut_unicode_escapes_end() {
        // A non-hex byte, multibyte or not, ends the escape.
        let json = r#"{"a": "\u0éz", "b": "\u12", "c": "x"}"#;
        assert_eq!(
            values(json.as_bytes()),
            [
                pair("a", "\u{fffd}\u{e9}z"),
                pair("b", "\u{fffd}"),
                pair("c", "x")
            ]
        );
    }

    #[test]
    fn surrogates_pair_only_with_low_halves() {
        let json =
            br#"["\ud83d\ude00", "\ud800A", "\ud800", "\udc00", "\ud800\ud83d\ude00", "\ud800\n"]"#;
        let texts: Vec<String> = values(json).into_iter().map(|(_, t)| t).collect();
        assert_eq!(
            texts,
            [
                "\u{1f600}",
                "\u{fffd}A",
                "\u{fffd}",
                "\u{fffd}",
                "\u{fffd}\u{1f600}",
                "\u{fffd}\n"
            ]
        );
    }

    #[test]
    fn plain_values_are_cut() {
        let mut json = String::from("[");
        json.push_str(&"1".repeat(3 * MAX_STRING));
        json.push_str(", 2]");
        let out = values(json.as_bytes());
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].1.len(), MAX_STRING);
        assert_eq!(out[1], pair("[]", "2"));
    }

    #[test]
    fn long_keys_are_cut() {
        let json = format!("{{\"{}\": 1}}", "k".repeat(MAX_STRING));
        let out = values(json.as_bytes());
        assert_eq!(out[0].0.len(), MAX_KEY);
    }

    #[test]
    fn deep_nesting_is_skipped() {
        let depth = 10_000;
        let json = format!(
            "{{\"deep\": {}\"x]\", {{\"k\": 1}}{}, \"after\": \"y\"}}",
            "[".repeat(depth),
            "]".repeat(depth)
        );
        let mut deepest = 0;
        let mut out = Vec::new();
        Parser::default().feed(json.as_bytes(), &mut |event| {
            let (Event::Open(path) | Event::Close(path) | Event::Value(path, _)) = event;
            deepest = deepest.max(path.len());
            if let Event::Value(path, text) = event {
                out.push((path.join("/"), text.to_string()));
            }
        });
        assert!(deepest <= MAX_DEPTH);
        assert_eq!(out, [pair("after", "y")]);
    }
}
