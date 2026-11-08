//! KOReader's files for the native plugin, without host calls: the Lua values
//! its sidecar files hold, read and written back in the same form, the names
//! of the files beside a book, and the file of what both sides had at the last
//! exchange. They are the interface-6 `lua` module and the helpers of `lib.rs`
//! made fallible: a refused allocation is `NoMemory`, never an abort, and
//! nothing indexes text or bytes in a way that can panic. Tested to read and
//! write the same as the old code.
//!
//! What a hostile file costs is bounded by the caller: it reads at most
//! [`crate::max_file`] bytes of KOReader's file, and a table nests at most
//! [`MAX_DEPTH`] deep.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::annotations::Kind;
use paperpaw_sdk::document::Locator;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy, decimal_u64},
};

/// A Lua value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    /// As written, e.g. `12` or `0.5`.
    Number(String),
    Str(String),
    Table(Table),
}

/// A table: its fields in order, with their keys.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Table {
    pub fields: Vec<(Value, Value)>,
}

impl Table {
    /// The value at string key `key`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.fields
            .iter()
            .find(|(k, _)| matches!(k, Value::Str(s) if s == key))
            .map(|(_, v)| v)
    }

    /// The string at `key`, if it is one.
    #[must_use]
    pub fn str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Sets string key `key` to `value`, in place or at the end; `Nil`
    /// removes it.
    ///
    /// # Errors
    /// `NoMemory` when the field cannot be kept; the table is unchanged.
    pub fn set(&mut self, key: &str, value: Value) -> Result<(), Error> {
        let at = self
            .fields
            .iter()
            .position(|(k, _)| matches!(k, Value::Str(s) if s == key));
        match (at, value) {
            (Some(at), Value::Nil) => {
                self.fields.remove(at);
            }
            (Some(at), value) => {
                if let Some(field) = self.fields.get_mut(at) {
                    field.1 = value;
                }
            }
            (None, Value::Nil) => {}
            (None, value) => {
                let key = Value::Str(copy(key)?);
                push(&mut self.fields, (key, value))?;
            }
        }
        Ok(())
    }

    /// The values at keys 1, 2, … (a list), in key order.
    ///
    /// # Errors
    /// `NoMemory` when the list cannot be made.
    pub fn list(&self) -> Result<Vec<&Value>, Error> {
        let mut items: Vec<(u64, usize, &Value)> = Vec::new();
        for (at, (k, v)) in self.fields.iter().enumerate() {
            if let Value::Number(n) = k
                && let Ok(n) = n.parse::<u64>()
            {
                push(&mut items, (n, at, v))?;
            }
        }
        // In place (a stable sort asks for a buffer): equal keys stay in the
        // order of the file, as a stable sort would leave them.
        items.sort_unstable_by_key(|(n, at, _)| (*n, *at));
        let mut out = Vec::new();
        out.try_reserve_exact(items.len())
            .map_err(|_| Error::NoMemory)?;
        out.extend(items.into_iter().map(|(_, _, v)| v));
        Ok(out)
    }

    /// A list of `items` (keys 1, 2, …).
    ///
    /// # Errors
    /// `NoMemory` when the table cannot be made.
    pub fn from_list(items: impl IntoIterator<Item = Value>) -> Result<Self, Error> {
        let mut fields = Vec::new();
        for (i, v) in items.into_iter().enumerate() {
            let mut digits = [0; 20];
            let key = Value::Number(copy(decimal_u64(i as u64 + 1, &mut digits))?);
            push(&mut fields, (key, v))?;
        }
        Ok(Self { fields })
    }
}

/// Deepest nesting read, against files made to exhaust the stack.
pub const MAX_DEPTH: u32 = 64;

/// Why a value was not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fail {
    /// Not the form KOReader writes.
    Bad,
    NoMemory,
}

impl From<Error> for Fail {
    fn from(_: Error) -> Self {
        Self::NoMemory
    }
}

type Read<T> = Result<T, Fail>;

/// Reads a settings file: comments, then `return` and a value. `None` where
/// the text is not that.
///
/// # Errors
/// `NoMemory` when the value cannot be kept.
pub fn parse(text: &str) -> Result<Option<Value>, Error> {
    let mut parser = Parser {
        text: text.as_bytes(),
        at: 0,
        depth: 0,
    };
    match parser.file() {
        Ok(value) => Ok(Some(value)),
        Err(Fail::Bad) => Ok(None),
        Err(Fail::NoMemory) => Err(Error::NoMemory),
    }
}

struct Parser<'a> {
    text: &'a [u8],
    at: usize,
    depth: u32,
}

/// `bytes` as text, invalid sequences as U+FFFD, as `from_utf8_lossy`.
fn lossy(bytes: &[u8]) -> Read<String> {
    let mut out = String::new();
    out.try_reserve(bytes.len()).map_err(|_| Fail::NoMemory)?;
    for chunk in bytes.utf8_chunks() {
        let more = chunk.valid().len() + if chunk.invalid().is_empty() { 0 } else { 3 };
        out.try_reserve(more).map_err(|_| Fail::NoMemory)?;
        out.push_str(chunk.valid());
        if !chunk.invalid().is_empty() {
            out.push('\u{fffd}');
        }
    }
    Ok(out)
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.get(self.at).copied()
    }

    fn rest(&self) -> &[u8] {
        self.text.get(self.at..).unwrap_or_default()
    }

    fn file(&mut self) -> Read<Value> {
        self.blank();
        if self.rest().starts_with(b"return") {
            self.at += 6;
        }
        let value = self.value()?;
        self.blank();
        if self.at == self.text.len() {
            Ok(value)
        } else {
            Err(Fail::Bad)
        }
    }

    /// Skips white space and comments.
    fn blank(&mut self) {
        loop {
            while self.peek().is_some_and(|b| b.is_ascii_whitespace()) {
                self.at += 1;
            }
            if !self.rest().starts_with(b"--") {
                return;
            }
            self.at += 2;
            if let Some((_, after)) = self.long_open() {
                self.at = after;
                continue;
            }
            while self.peek().is_some_and(|b| b != b'\n') {
                self.at += 1;
            }
        }
    }

    /// At `[[` or `[==[`: the end of the long bracket's content and the place
    /// after its closing, as `(content end, after)`; moves past the opening.
    /// Nothing is allocated: the closing is looked for by its parts.
    fn long_open(&mut self) -> Option<(usize, usize)> {
        let rest = self.rest();
        let level = rest.iter().skip(1).take_while(|&&b| b == b'=').count();
        if rest.first() != Some(&b'[') || rest.get(level + 1) != Some(&b'[') {
            return None;
        }
        let start = self.at + level + 2;
        let body = self.text.get(start..)?;
        let mut from = 0;
        while let Some(found) = body.get(from..)?.iter().position(|&b| b == b']') {
            let close = from + found;
            let equals = body.get(close + 1..)?;
            if equals.iter().take(level).filter(|&&b| b == b'=').count() == level
                && equals.get(level) == Some(&b']')
            {
                self.at = start;
                return Some((start + close, start + close + level + 2));
            }
            from = close + 1;
        }
        None
    }

    fn value(&mut self) -> Read<Value> {
        self.blank();
        match self.peek().ok_or(Fail::Bad)? {
            b'{' => {
                self.depth += 1;
                if self.depth > MAX_DEPTH {
                    return Err(Fail::Bad);
                }
                let table = self.table();
                self.depth -= 1;
                table.map(Value::Table)
            }
            b'"' | b'\'' => self.string().map(Value::Str),
            b'[' => {
                let (end, after) = self.long_open().ok_or(Fail::Bad)?;
                let mut text = self.text.get(self.at..end).ok_or(Fail::Bad)?;
                // A line break right after the opening is not part of it.
                if text.first() == Some(&b'\n') {
                    text = text.get(1..).unwrap_or_default();
                }
                let value = lossy(text)?;
                self.at = after;
                Ok(Value::Str(value))
            }
            b'-' | b'.' | b'0'..=b'9' => self.number(),
            _ => {
                let (start, end) = self.word();
                match self.text.get(start..end).ok_or(Fail::Bad)? {
                    b"nil" => Ok(Value::Nil),
                    b"true" => Ok(Value::Bool(true)),
                    b"false" => Ok(Value::Bool(false)),
                    _ => Err(Fail::Bad),
                }
            }
        }
    }

    /// A word (letters, digits, `_`) from here, as its start and end.
    fn word(&mut self) -> (usize, usize) {
        let start = self.at;
        while self
            .peek()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            self.at += 1;
        }
        (start, self.at)
    }

    fn number(&mut self) -> Read<Value> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        while self.peek().is_some_and(|b| {
            b.is_ascii_hexdigit() || matches!(b, b'.' | b'x' | b'X' | b'+' | b'-' | b'p' | b'P')
        }) {
            // A sign only after an exponent.
            if matches!(self.peek(), Some(b'+' | b'-'))
                && !matches!(
                    self.text.get(self.at.wrapping_sub(1)),
                    Some(b'e' | b'E' | b'p' | b'P')
                )
            {
                break;
            }
            self.at += 1;
        }
        let bytes = self.text.get(start..self.at).ok_or(Fail::Bad)?;
        let text = core::str::from_utf8(bytes).map_err(|_| Fail::Bad)?;
        if text == "-" || text.is_empty() {
            return Err(Fail::Bad);
        }
        Ok(Value::Number(copy(text)?))
    }

    fn string(&mut self) -> Read<String> {
        let quote = self.peek().ok_or(Fail::Bad)?;
        self.at += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let b = self.peek().ok_or(Fail::Bad)?;
            self.at += 1;
            match b {
                b'\\' => {
                    let e = self.peek().ok_or(Fail::Bad)?;
                    self.at += 1;
                    match e {
                        b'n' | b'\n' => push(&mut out, b'\n')?,
                        b't' => push(&mut out, b'\t')?,
                        b'r' => push(&mut out, b'\r')?,
                        b'a' => push(&mut out, 7)?,
                        b'b' => push(&mut out, 8)?,
                        b'f' => push(&mut out, 12)?,
                        b'v' => push(&mut out, 11)?,
                        b'x' => {
                            let hex = self.text.get(self.at..self.at + 2).ok_or(Fail::Bad)?;
                            let hex = core::str::from_utf8(hex).map_err(|_| Fail::Bad)?;
                            push(
                                &mut out,
                                u8::from_str_radix(hex, 16).map_err(|_| Fail::Bad)?,
                            )?;
                            self.at += 2;
                        }
                        b'z' => {
                            while self.peek().is_some_and(|b| b.is_ascii_whitespace()) {
                                self.at += 1;
                            }
                        }
                        b'0'..=b'9' => {
                            let mut n = u32::from(e - b'0');
                            for _ in 0..2 {
                                match self.peek() {
                                    Some(d @ b'0'..=b'9') => {
                                        n = n * 10 + u32::from(d - b'0');
                                        self.at += 1;
                                    }
                                    _ => break,
                                }
                            }
                            push(&mut out, u8::try_from(n).map_err(|_| Fail::Bad)?)?;
                        }
                        other => push(&mut out, other)?,
                    }
                }
                b'\n' => return Err(Fail::Bad),
                b if b == quote => break,
                b => push(&mut out, b)?,
            }
        }
        lossy(&out)
    }

    fn table(&mut self) -> Read<Table> {
        self.at += 1;
        let mut table = Table::default();
        let mut next = 1u64;
        loop {
            self.blank();
            match self.peek().ok_or(Fail::Bad)? {
                b'}' => {
                    self.at += 1;
                    return Ok(table);
                }
                b',' | b';' => {
                    self.at += 1;
                    continue;
                }
                _ => {}
            }
            let key;
            if self.peek() == Some(b'[') && self.text.get(self.at + 1) != Some(&b'[') {
                self.at += 1;
                key = self.value()?;
                self.blank();
                if self.peek().ok_or(Fail::Bad)? != b']' {
                    return Err(Fail::Bad);
                }
                self.at += 1;
                self.blank();
                if self.peek().ok_or(Fail::Bad)? != b'=' {
                    return Err(Fail::Bad);
                }
                self.at += 1;
            } else {
                // `name = value`, or a value in the list.
                let start = self.at;
                let (word_start, word_end) = self.word();
                let word = self.text.get(word_start..word_end).ok_or(Fail::Bad)?;
                self.blank();
                if !word.is_empty()
                    && !matches!(word, b"nil" | b"true" | b"false")
                    && self.peek() == Some(b'=')
                {
                    self.at += 1;
                    key = Value::Str(lossy(word)?);
                } else {
                    self.at = start;
                    let mut digits = [0; 20];
                    key = Value::Number(copy(decimal_u64(next, &mut digits))?);
                    next += 1;
                }
            }
            let value = self.value()?;
            if value != Value::Nil {
                push(&mut table.fields, (key, value))?;
            }
        }
    }
}

/// Text under construction that never grows without asking.
struct Out(String);

impl Out {
    fn text(&mut self, text: &str) -> Result<(), Error> {
        self.0
            .try_reserve(text.len())
            .map_err(|_| Error::NoMemory)?;
        self.0.push_str(text);
        Ok(())
    }

    fn ch(&mut self, c: char) -> Result<(), Error> {
        self.0
            .try_reserve(c.len_utf8())
            .map_err(|_| Error::NoMemory)?;
        self.0.push(c);
        Ok(())
    }
}

/// `value` as KOReader writes settings, ready to be read back: `return` and
/// the value, tables with a field a line.
///
/// # Errors
/// `NoMemory` when the text cannot be kept.
pub fn write(value: &Value) -> Result<String, Error> {
    let mut out = Out(String::new());
    out.text("-- we can read Lua syntax here!\nreturn ")?;
    write_value(value, 0, &mut out)?;
    out.ch('\n')?;
    Ok(out.0)
}

fn write_value(value: &Value, indent: usize, out: &mut Out) -> Result<(), Error> {
    match value {
        Value::Nil => out.text("nil"),
        Value::Bool(b) => out.text(if *b { "true" } else { "false" }),
        Value::Number(n) => out.text(n),
        Value::Str(s) => quote(s, out),
        Value::Table(table) => {
            out.ch('{')?;
            for (key, value) in &table.fields {
                out.ch('\n')?;
                pad(indent + 1, out)?;
                out.ch('[')?;
                write_value(key, indent + 1, out)?;
                out.text("] = ")?;
                write_value(value, indent + 1, out)?;
                out.ch(',')?;
            }
            if !table.fields.is_empty() {
                out.ch('\n')?;
                pad(indent, out)?;
            }
            out.ch('}')
        }
    }
}

fn pad(indent: usize, out: &mut Out) -> Result<(), Error> {
    for _ in 0..indent {
        out.text("    ")?;
    }
    Ok(())
}

/// A Lua string literal.
fn quote(s: &str, out: &mut Out) -> Result<(), Error> {
    out.ch('"')?;
    for c in s.chars() {
        match c {
            '"' => out.text("\\\"")?,
            '\\' => out.text("\\\\")?,
            '\n' => out.text("\\n")?,
            '\r' => out.text("\\r")?,
            c if (c as u32) < 0x20 || c == '\u{7f}' => {
                // `\` and three digits.
                let code = c as u32;
                out.ch('\\')?;
                for divisor in [100, 10, 1] {
                    let digit = u8::try_from(code / divisor % 10).unwrap_or(0);
                    out.ch(char::from(b'0' + digit))?;
                }
            }
            c => out.ch(c)?,
        }
    }
    out.ch('"')
}

/// A table value, for building them.
///
/// # Errors
/// `NoMemory` when the table cannot be made.
pub fn table(fields: impl IntoIterator<Item = (&'static str, Value)>) -> Result<Value, Error> {
    let mut out = Vec::new();
    for (k, v) in fields {
        if v != Value::Nil {
            push(&mut out, (Value::Str(copy(k)?), v))?;
        }
    }
    Ok(Value::Table(Table { fields: out }))
}

/// A string value.
///
/// # Errors
/// `NoMemory` when the text cannot be kept.
pub fn string(s: &str) -> Result<Value, Error> {
    Ok(Value::Str(copy(s)?))
}

/// KOReader's folder for the book at `book`: `Book.sdr` beside it.
///
/// # Errors
/// `NoMemory` when the name cannot be kept.
pub fn sidecar(book: &str) -> Result<String, Error> {
    let stem = book.rsplit_once('.').map_or(book, |(stem, _)| stem);
    concat(&[stem, ".sdr"])
}

/// KOReader's own file in it: `metadata.<suffix>.lua`.
///
/// # Errors
/// `NoMemory` when the name cannot be kept.
pub fn metadata_file(book: &str) -> Result<String, Error> {
    let suffix = book.rsplit_once('.').map_or("_", |(_, suffix)| suffix);
    concat(&[&sidecar(book)?, "/metadata.", suffix, ".lua"])
}

/// Annotations for KOReader to take over: `<file name>.annotations.lua`.
///
/// # Errors
/// `NoMemory` when the name cannot be kept.
pub fn exchange_file(book: &str) -> Result<String, Error> {
    concat(&[
        &sidecar(book)?,
        "/",
        paperpaw_sdk::files::name(book),
        ".annotations.lua",
    ])
}

/// Where the annotations both sides had at the last exchange are kept, in
/// `plugin_dir` (the plugin's folder): named by the book's key.
///
/// # Errors
/// `NoMemory` when the name cannot be kept.
pub fn known_file(plugin_dir: &str, book: &str) -> Result<String, Error> {
    // The book's key (`library::key`): 16 hex digits of the FNV-1a hash of its
    // path, whose values never change; made here without an allocation.
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let hash = paperpaw_sdk::digest::fnv1a64(book);
    let mut digits = [0u8; 16];
    for (at, digit) in digits.iter_mut().enumerate() {
        let nibble = usize::try_from((hash >> ((15 - at) * 4)) & 15).unwrap_or(0);
        *digit = HEX.get(nibble).copied().unwrap_or(b'0');
    }
    let key = core::str::from_utf8(&digits).unwrap_or_default();
    concat(&[plugin_dir, "/", key, ".txt"])
}

/// A fingerprint of `note` (64-bit FNV-1a): whether it changed since.
#[must_use]
pub fn fingerprint(note: &str) -> u64 {
    note.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, b| {
        (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// What an annotation is known by: its kind and place.
pub type Key = (Kind, Locator, Locator);

/// An annotation both sides had at the last exchange: where it is, and a
/// fingerprint of the note both had on it then (`None` where their notes
/// differed, or in what an older version kept).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Known {
    pub key: Key,
    pub note: Option<u64>,
}

/// The line in what is known telling that every annotation here was removed
/// since the last exchange.
pub const EMPTIED: &str = "emptied";
/// The start of the line in what is known telling how many annotations
/// KOReader's own file had at the last exchange that read it.
pub const THEIRS: &str = "theirs ";

/// What is known of the last exchange.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Last {
    /// What both sides had.
    pub known: Vec<Known>,
    /// Whether every annotation here was removed since.
    pub emptied: bool,
    /// How many annotations KOReader's own file had at the last exchange that
    /// read it (not the file for KOReader); 0 if not known.
    pub seen: u32,
}

/// What both sides had at the last exchange, a line each: kind, start, end and
/// the note's fingerprint (`b 3 120 3 120 5d2c…`). And whether every
/// annotation here was removed since ([`EMPTIED`]), and how many KOReader's
/// own file had ([`THEIRS`]).
///
/// # Errors
/// `NoMemory` when the list cannot be kept.
pub fn parse_known(text: &str) -> Result<Last, Error> {
    let emptied = text.lines().any(|line| line == EMPTIED);
    let seen = text
        .lines()
        .find_map(|line| line.strip_prefix(THEIRS)?.parse().ok())
        .unwrap_or(0);
    let mut known = Vec::new();
    for line in text.lines() {
        if let Some(one) = known_line(line) {
            push(&mut known, one)?;
        }
    }
    Ok(Last {
        known,
        emptied,
        seen,
    })
}

fn known_line(line: &str) -> Option<Known> {
    let mut fields = line.split(' ');
    let kind = match fields.next()? {
        "b" => Kind::Bookmark,
        "h" => Kind::Highlight,
        _ => return None,
    };
    let mut number = || fields.next()?.parse::<u32>().ok();
    let start = Locator::new(number()?, number()?);
    let end = Locator::new(number()?, number()?);
    let note = fields
        .next()
        .and_then(|hex| u64::from_str_radix(hex, 16).ok());
    Some(Known {
        key: (kind, start, end),
        note,
    })
}

/// What is known, as [`parse_known`] reads it.
///
/// # Errors
/// `NoMemory` when the text cannot be kept.
pub fn write_known(known: &[Known], seen: u32) -> Result<String, Error> {
    let mut out = Out(String::new());
    if seen > 0 {
        let mut digits = [0; 20];
        out.text(THEIRS)?;
        out.text(decimal_u64(u64::from(seen), &mut digits))?;
        out.ch('\n')?;
    }
    for Known {
        key: (kind, start, end),
        note,
    } in known
    {
        out.ch(if *kind == Kind::Bookmark { 'b' } else { 'h' })?;
        for number in [start.section, start.offset, end.section, end.offset] {
            let mut digits = [0; 20];
            out.ch(' ')?;
            out.text(decimal_u64(u64::from(number), &mut digits))?;
        }
        if let Some(note) = note {
            const HEX: &[u8; 16] = b"0123456789abcdef";
            out.ch(' ')?;
            for shift in (0..16).rev() {
                let nibble = usize::try_from((note >> (shift * 4)) & 15).unwrap_or(0);
                out.ch(char::from(HEX.get(nibble).copied().unwrap_or(b'0')))?;
            }
        }
        out.ch('\n')?;
    }
    Ok(out.0)
}
