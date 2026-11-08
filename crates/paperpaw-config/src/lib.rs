//! A tiny, human-editable configuration format.
//!
//! PaperPaw stores settings, key maps, themes and translations in the same
//! simple INI dialect, so users can edit everything with a text editor on
//! the device's storage. The parser is zero-copy, `no_std` and only allocates
//! when a quoted value contains escape sequences.
//!
//! # Format
//!
//! ```ini
//! # Comments start with '#' or ';' at the beginning of a line.
//! top-level-key = values before the first section have an empty section name
//!
//! [display]
//! full-refresh-interval = 6
//! night-mode = off
//!
//! [messages]
//! greeting = Hello, {name}!
//! padded = "  quoted values keep spaces, and support \"escapes\"\n"
//! ```
//!
//! - Keys and section names consist of ASCII letters, digits, `_`, `-`, `.`
//!   and `@`; a section name may also be words of those, one space
//!   between each (`[menu core.reader-menu]`).
//! - Unquoted values are taken literally (trimmed). There are no inline
//!   comments, so values may contain `#` and `;`.
//! - Quoted values support the escapes `\\`, `\"`, `\n`, `\t` and `\u{…}`.
//! - Later entries override earlier ones; that is up to the consumer.

#![no_std]

extern crate alloc;

use alloc::borrow::Cow;
use alloc::collections::{BTreeMap, TryReserveError};
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

/// Parses `source` into an iterator of entries.
pub fn parse(source: &str) -> Parser<'_> {
    Parser {
        lines: source.lines(),
        line: 0,
        section: "",
    }
}

/// A `key = value` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry<'a> {
    /// The enclosing section, `""` before the first section header.
    pub section: &'a str,
    pub key: &'a str,
    pub value: Value<'a>,
    /// 1-based line number, for error messages.
    pub line: u32,
}

/// A value as written in the source. Its escapes have already been validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Value<'a> {
    raw: &'a str,
    quoted: bool,
}

impl<'a> Value<'a> {
    paperpaw_table::shared! {
    /// The value as text, with escape sequences resolved.
    pub fn as_str(&self) -> Cow<'a, str> {
        if !self.quoted || !self.raw.contains('\\') {
            return Cow::Borrowed(self.raw);
        }
        let mut out = String::with_capacity(self.raw.len());
        let mut chars = self.raw.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            // Escapes were validated by the parser.
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('u') => {
                    let rest = chars.as_str();
                    if let Some((c, len)) = parse_unicode_escape(rest) {
                        out.push(c);
                        chars = rest.get(len..).unwrap_or_default().chars();
                    }
                }
                Some(other) => out.push(other),
                None => {}
            }
        }
        Cow::Owned(out)
    } @ data::CONFIG_AS_STR
    }

    /// Whether [`Value::as_str`] has to allocate: a quoted value with escape
    /// sequences. Its text with the escapes resolved is never longer than
    /// [`Value::raw`].
    pub fn is_escaped(&self) -> bool {
        self.quoted && self.raw.contains('\\')
    }

    /// [`Value::as_str`] without an infallible allocation: borrowed where
    /// there is no escape, else one fallible reservation of [`Value::raw`]'s
    /// length (the text with the escapes resolved never needs more), so a
    /// device short of memory gets the error instead of an abort.
    ///
    /// # Errors
    /// [`TryReserveError`] where the room for the resolved text cannot be had.
    pub fn try_as_str(&self) -> Result<Cow<'a, str>, TryReserveError> {
        if !self.is_escaped() {
            return Ok(Cow::Borrowed(self.raw));
        }
        let mut out = String::new();
        out.try_reserve_exact(self.raw.len())?;
        unescape_into(self.raw, &mut out);
        Ok(Cow::Owned(out))
    }

    /// Interprets `true/false`, `yes/no`, `on/off` and `1/0`.
    pub fn as_bool(&self) -> Option<bool> {
        match self.raw {
            "true" | "yes" | "on" | "1" => Some(true),
            "false" | "no" | "off" | "0" => Some(false),
            _ => None,
        }
    }

    paperpaw_table::shared! {
    pub fn as_i64(&self) -> Option<i64> {
        self.raw.parse().ok()
    } @ data::CONFIG_AS_I64
    }

    paperpaw_table::shared! {
    pub fn as_u32(&self) -> Option<u32> {
        self.raw.parse().ok()
    } @ data::CONFIG_AS_U32
    }

    /// The value exactly as written, without surrounding quotes.
    pub fn raw(&self) -> &'a str {
        self.raw
    }
}

/// A syntax error with its 1-based line number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseError {
    pub line: u32,
    pub kind: ErrorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// A line that is neither a comment, a section header nor `key = value`.
    InvalidLine,
    /// A section header without closing `]` or with invalid characters.
    InvalidSection,
    /// A key with invalid characters.
    InvalidKey,
    /// A quoted value without closing quote.
    UnterminatedString,
    /// An unknown or malformed escape sequence.
    InvalidEscape,
    /// A cause this build has no variant for, by its code (0: not said).
    /// Codes are listed here as they are given and never reused; a build
    /// that doesn't know one treats it as the most general failure of its
    /// kind.
    Other(u16),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self.kind {
            ErrorKind::InvalidLine => "expected `key = value`, `[section]` or a comment",
            ErrorKind::InvalidSection => "invalid section header",
            ErrorKind::InvalidKey => "invalid key",
            ErrorKind::UnterminatedString => "missing closing quote",
            ErrorKind::InvalidEscape => "invalid escape sequence",
            ErrorKind::Other(code) => return write!(f, "line {}: error {code}", self.line),
        };
        write!(f, "line {}: {}", self.line, message)
    }
}

impl core::error::Error for ParseError {}

/// Iterator over the entries of a configuration source.
#[derive(Debug, Clone)]
pub struct Parser<'a> {
    lines: core::str::Lines<'a>,
    line: u32,
    section: &'a str,
}

impl<'a> Iterator for Parser<'a> {
    type Item = Result<Entry<'a>, ParseError>;

    paperpaw_table::shared! {
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let raw_line = self.lines.next()?;
            self.line += 1;
            let line = raw_line.trim();
            let error = |kind| {
                Some(Err(ParseError {
                    line: self.line,
                    kind,
                }))
            };

            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }

            if let Some(header) = line.strip_prefix('[') {
                let Some(name) = header.strip_suffix(']') else {
                    return error(ErrorKind::InvalidSection);
                };
                let name = name.trim();
                if !is_section(name) {
                    return error(ErrorKind::InvalidSection);
                }
                self.section = name;
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                return error(ErrorKind::InvalidLine);
            };
            let key = key.trim();
            if !is_identifier(key) {
                return error(ErrorKind::InvalidKey);
            }
            let value = match parse_value(value.trim()) {
                Ok(value) => value,
                Err(kind) => return error(kind),
            };
            return Some(Ok(Entry {
                section: self.section,
                key,
                value,
                line: self.line,
            }));
        }
    } @ data::CONFIG_NEXT
    }
}

/// A section name: identifiers, one space between each, as
/// `[menu core.reader-menu]` names a menu of another plugin.
fn is_section(s: &str) -> bool {
    s.split(' ').all(is_identifier)
}

/// Letters, digits, `_`, `-`, `.`, and `@` (the settings of one
/// placement of something placed more than once, e.g. `clock@2.face`).
fn is_identifier(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b'@'))
}

fn parse_value(text: &str) -> Result<Value<'_>, ErrorKind> {
    let Some(inner) = text.strip_prefix('"') else {
        return Ok(Value {
            raw: text,
            quoted: false,
        });
    };

    // Find the closing quote, skipping escaped characters.
    let mut chars = inner.char_indices();
    while let Some((index, c)) = chars.next() {
        match c {
            '"' => {
                if !inner.get(index + 1..).unwrap_or_default().trim().is_empty() {
                    return Err(ErrorKind::InvalidLine);
                }
                return Ok(Value {
                    raw: inner.get(..index).unwrap_or_default(),
                    quoted: true,
                });
            }
            '\\' => match chars.next() {
                Some((_, '\\' | '"' | 'n' | 't')) => {}
                Some((u_index, 'u')) => {
                    let len = inner
                        .get(u_index + 1..)
                        .and_then(parse_unicode_escape)
                        .ok_or(ErrorKind::InvalidEscape)?
                        .1;
                    for _ in 0..len {
                        chars.next();
                    }
                }
                Some(_) => return Err(ErrorKind::InvalidEscape),
                None => return Err(ErrorKind::UnterminatedString),
            },
            _ => {}
        }
    }
    Err(ErrorKind::UnterminatedString)
}

/// Parses `{XXXX}` (1 to 6 hex digits) and returns the char and consumed bytes.
/// Appends `raw` (escapes validated by the parser) with its escapes resolved.
/// `out` must have room for `raw.len()` more bytes, so that it never grows:
/// every escape is at least as long as what it stands for.
fn unescape_into(raw: &str, out: &mut String) {
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('u') => {
                let rest = chars.as_str();
                if let Some((c, len)) = parse_unicode_escape(rest) {
                    out.push(c);
                    chars = rest.get(len..).unwrap_or_default().chars();
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
}

fn parse_unicode_escape(s: &str) -> Option<(char, usize)> {
    let body = s.strip_prefix('{')?;
    let end = body.find('}')?;
    let hex = body.get(..end)?;
    if hex.is_empty() || hex.len() > 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let code = u32::from_str_radix(hex, 16).ok()?;
    Some((char::from_u32(code)?, end + 2))
}

paperpaw_table::shared! {
/// Returns `source` with `key` in `section` set to `value`.
///
/// Edits as little as possible, so hand-written files keep their comments,
/// order and formatting: an existing entry's line is replaced (the last
/// one, if the key is there more than once: it is the one read), a new
/// entry goes after the section's last entry, and a missing section is
/// appended. Entries of section `""` go before the first section header.
/// Values that need it are quoted and escaped.
///
/// A key or section name that [`parse`] would reject leaves `source` as it
/// is. For more than a few keys, [`set_all`] edits the file only once.
pub fn set(source: &str, section: &str, key: &str, value: &str) -> String {
    set_all(source, [(section, key, value)])
} @ data::CONFIG_SET
}

/// Returns `source` with each `(section, key, value)` of `entries` set, as
/// calls of [`set`] one after another would, in a single pass over the file.
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
pub fn set_all<S, K, V>(source: &str, entries: impl IntoIterator<Item = (S, K, V)>) -> String
where
    S: AsRef<str>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let entries: Vec<(S, K, V)> = entries.into_iter().collect();

    // The new lines: one per key, in the order the keys first come, with
    // the last value given.
    let mut texts: Vec<String> = Vec::new();
    let mut keys: Vec<(&str, &str)> = Vec::new();
    let mut index: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for (section, key, value) in &entries {
        let (section, key) = (section.as_ref(), key.as_ref());
        if !(section.is_empty() || is_section(section)) || !is_identifier(key) {
            continue;
        }
        let text = alloc::format!("{key} = {}", quote(value.as_ref()));
        if let Some(&t) = index.get(&(section, key)) {
            texts[t] = text;
        } else {
            index.insert((section, key), texts.len());
            texts.push(text);
            keys.push((section, key));
        }
    }
    if texts.is_empty() {
        return String::from(source);
    }

    // Where `set` would put the next entry of each of these sections
    // (before which line: after its last entry, else after its header),
    // and the line of each key (the last one, as it is the one read).
    // Inserted one by one: collecting into a map sorts with `sort_by`,
    // whose scratch takes 4 KiB of stack (too much on the X4). So below,
    // where an unstable sort orders alike.
    let mut anchors: BTreeMap<&str, (Option<usize>, Option<usize>)> = BTreeMap::new();
    for &(section, _) in &keys {
        anchors.insert(section, (None, None));
    }
    let mut line_of: Vec<Option<usize>> = alloc::vec![None; texts.len()];
    let mut current = "";
    let mut line_count = 0;
    for (i, raw) in source.lines().enumerate() {
        line_count = i + 1;
        let text = raw.trim();
        if let Some(name) = text.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
            current = name.trim();
            if let Some((after_header, _)) = anchors.get_mut(current) {
                *after_header = Some(i + 1);
            }
            continue;
        }
        if text.is_empty() || text.starts_with(['#', ';']) {
            continue;
        }
        let (Some((key, _)), Some((_, after_entry))) =
            (text.split_once('='), anchors.get_mut(current))
        else {
            continue;
        };
        *after_entry = Some(i + 1);
        if let Some(&t) = index.get(&(current, key.trim())) {
            line_of[t] = Some(i);
        }
    }

    // Each new line goes in place of a line, before a line, or into a
    // section appended at the end.
    let mut replaced: BTreeMap<usize, usize> = BTreeMap::new();
    let mut inserted: Vec<(usize, usize)> = Vec::new();
    let mut appended: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (t, &(section, _)) in keys.iter().enumerate() {
        let (after_header, after_entry) = anchors[section];
        if let Some(line) = line_of[t] {
            replaced.insert(line, t);
        } else if let Some(at) = after_entry
            .or(after_header)
            .or(section.is_empty().then_some(0))
        {
            inserted.push((at, t));
        } else {
            appended.entry(section).or_default().push(t);
        }
    }
    // In the order `set` would append them (each line in one section only).
    let mut appended: Vec<(&str, Vec<usize>)> = appended.into_iter().collect();
    paperpaw_sort::sort_by_key(&mut appended, |(_, lines)| lines[0]);

    let mut out = if replaced.is_empty() && inserted.is_empty() {
        String::from(source)
    } else {
        // Lines inserted at the same place keep their order: the second
        // of each pair counts up as they were pushed.
        paperpaw_sort::sort(&mut inserted);
        let mut inserted = inserted.iter().peekable();
        let mut insert_before = |i: usize, out: &mut String| {
            while let Some(&(_, t)) = inserted.next_if(|&&(at, _)| at == i) {
                out.push_str(&texts[t]);
                out.push('\n');
            }
        };
        let mut out = String::with_capacity(source.len() + 64);
        for (i, raw) in source.lines().enumerate() {
            insert_before(i, &mut out);
            out.push_str(replaced.get(&i).map_or(raw, |&t| &texts[t]));
            out.push('\n');
        }
        insert_before(line_count, &mut out);
        out
    };
    for (section, lines) in appended {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        // A blank line before it, unless the file was empty when `set`
        // would have appended it (lines placed earlier made it longer).
        if !source.is_empty() || lines[0] > 0 {
            out.push('\n');
        }
        out.push('[');
        out.push_str(section);
        out.push_str("]\n");
        for t in lines {
            out.push_str(&texts[t]);
            out.push('\n');
        }
    }
    out
}

paperpaw_table::shared! {
/// Returns `source` without `key` in `section`; the rest stays as it is.
pub fn remove(source: &str, section: &str, key: &str) -> String {
    let mut current = "";
    let mut out: Vec<&str> = Vec::new();
    let mut removed = false;
    for raw in source.lines() {
        let text = raw.trim();
        if let Some(name) = text.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
            current = name.trim();
        } else if current == section
            && !text.starts_with(['#', ';'])
            && text.split_once('=').is_some_and(|(k, _)| k.trim() == key)
        {
            removed = true;
            continue;
        }
        out.push(raw);
    }
    if !removed {
        return String::from(source);
    }
    join(&out)
} @ data::CONFIG_REMOVE
}

fn join(lines: &[&str]) -> String {
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

paperpaw_table::shared! {
/// A value as written in a file: quoted and escaped if it would not
/// survive unquoted (leading or trailing spaces, empty, starting with a
/// quote, or more than one line), as it is otherwise.
pub fn quote(value: &str) -> Cow<'_, str> {
    let plain = !value.is_empty()
        && value.trim() == value
        && !value.starts_with('"')
        && !value.contains(['\n', '\r']);
    if plain {
        return Cow::Borrowed(value);
    }
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out.push('"');
    Cow::Owned(out)
} @ data::CONFIG_QUOTE
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    fn entries(source: &str) -> Vec<Entry<'_>> {
        parse(source).collect::<Result<_, _>>().unwrap()
    }

    #[test]
    fn a_cause_of_a_later_build_says_its_code() {
        let error = ParseError {
            line: 3,
            kind: ErrorKind::Other(7),
        };
        assert_eq!(std::format!("{error}"), "line 3: error 7");
    }

    #[test]
    fn sections_keys_and_comments() {
        let parsed = entries("# comment\ntop = 1\n\n[display]\n; another\nnight-mode = on\n");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].section, "");
        assert_eq!(parsed[0].key, "top");
        assert_eq!(parsed[0].value.as_i64(), Some(1));
        assert_eq!(parsed[1].section, "display");
        assert_eq!(parsed[1].key, "night-mode");
        assert_eq!(parsed[1].value.as_bool(), Some(true));
        assert_eq!(parsed[1].line, 6);
    }

    #[test]
    fn unquoted_values_are_literal() {
        let parsed = entries("a = Hello # not a comment; \\n stays\n");
        assert_eq!(parsed[0].value.as_str(), "Hello # not a comment; \\n stays");
    }

    #[test]
    fn quoted_values_resolve_escapes() {
        let parsed = entries(r#"a = "  line\nnext \"q\" \\ \u{1F4D6}""#);
        assert_eq!(parsed[0].value.as_str(), "  line\nnext \"q\" \\ \u{1F4D6}");
    }

    #[test]
    fn try_as_str_matches_as_str_and_never_grows() {
        let source = r#"a = "  line\nnext \"q\" \\ \u{1F4D6} \t\u{41}"
b = plain
c = "quoted plain"
"#;
        for entry in entries(source) {
            let value = entry.value;
            let tried = value.try_as_str().unwrap();
            assert_eq!(tried, value.as_str());
            assert_eq!(value.is_escaped(), matches!(tried, Cow::Owned(_)));
            if let Cow::Owned(text) = &tried {
                // One reservation of the raw length, never a second one.
                assert!(text.capacity() >= value.raw().len());
                assert!(text.len() <= value.raw().len());
            }
        }
    }

    #[test]
    fn quoted_value_without_escapes_borrows() {
        let parsed = entries(r#"a = "plain""#);
        assert!(matches!(parsed[0].value.as_str(), Cow::Borrowed("plain")));
    }

    #[test]
    fn errors_report_line_numbers() {
        let error = |source| parse(source).find_map(Result::err).unwrap();
        assert_eq!(error("ok = 1\nbroken").kind, ErrorKind::InvalidLine);
        assert_eq!(error("ok = 1\nbroken").line, 2);
        assert_eq!(error("[open").kind, ErrorKind::InvalidSection);
        assert_eq!(error("bad key = 1").kind, ErrorKind::InvalidKey);
        assert_eq!(error(r#"a = "open"#).kind, ErrorKind::UnterminatedString);
        assert_eq!(error(r#"a = "\x""#).kind, ErrorKind::InvalidEscape);
        assert_eq!(error(r#"a = "\u{zz}""#).kind, ErrorKind::InvalidEscape);
        assert_eq!(error(r#"a = "x" trailing"#).kind, ErrorKind::InvalidLine);
        // Words of a section name, one space between each, no more.
        for header in ["[menu  core.menu]", "[ menu x ]x", "[menu core/menu]", "[]"] {
            assert_eq!(error(header).kind, ErrorKind::InvalidSection, "{header}");
        }
    }

    #[test]
    fn a_section_name_may_be_words() {
        let parsed: Vec<_> = parse("[menu core.reader-menu]\nadd = my.night\n[ menu x ]\nhide = a")
            .map(Result::unwrap)
            .collect();
        assert_eq!(parsed[0].section, "menu core.reader-menu");
        assert_eq!(parsed[1].section, "menu x", "trimmed as before");
        assert_eq!(
            set("", "menu core.reader-menu", "add", "my.night"),
            "[menu core.reader-menu]\nadd = my.night\n"
        );
    }

    #[test]
    fn handles_crlf_line_endings() {
        let parsed = entries("[s]\r\nkey = value\r\n");
        assert_eq!(parsed[0].value.as_str(), "value");
    }

    #[test]
    fn remove_keeps_everything_else() {
        let source = "# top
[menus]
a = x, y
# note
b = z
[other]
a = 1
";
        assert_eq!(
            remove(source, "menus", "a"),
            "# top
[menus]
# note
b = z
[other]
a = 1
"
        );
        assert_eq!(remove(source, "menus", "missing"), source);
    }

    #[test]
    fn set_replaces_inserts_and_appends() {
        let source =
            "# My settings\n[core]\nlanguage = en  \n\n# reading\n[reader]\nfont-size = 11\n";
        let replaced = set(source, "core", "language", "de");
        assert_eq!(
            replaced,
            "# My settings\n[core]\nlanguage = de\n\n# reading\n[reader]\nfont-size = 11\n"
        );
        let inserted = set(source, "core", "night-mode", "on");
        assert!(inserted.contains("language = en  \nnight-mode = on\n\n# reading"));
        let appended = set(source, "files", "root", " my books ");
        assert!(appended.ends_with("font-size = 11\n\n[files]\nroot = \" my books \"\n"));
        assert_eq!(set("", "a", "b", "c"), "[a]\nb = c\n");
        // A key there twice (by hand, or its section twice): the last one
        // is read, so it is the one changed.
        let twice = "[core]\nlanguage = en\n[reader]\nx = 1\n[core]\nlanguage = fr\n";
        let changed = set(twice, "core", "language", "de");
        assert_eq!(
            changed,
            "[core]\nlanguage = en\n[reader]\nx = 1\n[core]\nlanguage = de\n"
        );
        let read = parse(&changed)
            .filter_map(Result::ok)
            .filter(|e| e.key == "language")
            .last()
            .unwrap();
        assert_eq!(read.value.as_str(), "de");
        // The result parses back to the new values.
        let entry = parse(&appended).last().unwrap().unwrap();
        assert_eq!(entry.value.as_str(), " my books ");
    }

    #[test]
    fn set_before_the_first_section() {
        let source = "# My settings\n[core]\nlanguage = en\n";
        let changed = set(source, "", "theme", "dark");
        assert_eq!(
            changed,
            "theme = dark\n# My settings\n[core]\nlanguage = en\n"
        );
        let changed = set(&changed, "", "night", "on");
        assert_eq!(
            changed,
            "theme = dark\nnight = on\n# My settings\n[core]\nlanguage = en\n"
        );
        let parsed = entries(&changed);
        assert_eq!((parsed[1].section, parsed[1].key), ("", "night"));
        assert_eq!(set("", "", "a", "b"), "a = b\n");
    }

    #[test]
    fn set_refuses_what_parse_would_reject() {
        let source = "[core]\nlanguage = en\n";
        for (section, key) in [
            ("core", "a = b"),
            ("core", "a\nb"),
            ("core", "[x]"),
            ("core", ""),
            ("core", " padded"),
            ("x]\n[y", "a"),
            ("x=y", "a"),
            ("a  b", "a"),
            (" a", "a"),
        ] {
            assert_eq!(
                set(source, section, key, "1"),
                source,
                "{section:?} {key:?}"
            );
        }
        // Values are quoted, so they cannot add lines of their own.
        let changed = set(source, "core", "a", "1\n[evil]\nb = 2\r\n");
        let parsed = entries(&changed);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].value.as_str(), "1\n[evil]\nb = 2\n");
        // `@` is allowed, as in the parser.
        let changed = set(source, "clock@2", "face", "round");
        assert_eq!(entries(&changed)[1].section, "clock@2");
    }

    #[test]
    fn set_all_is_set_one_after_another() {
        let sources = [
            "",
            "\n",
            "top = 1\n",
            "# My settings\n[core]\nlanguage = en  \n\n# reading\n[reader]\nfont-size = 11",
            "[core]\nlanguage = en\n[reader]\nx = 1\n[core]\nlanguage = fr\n",
            "[core]\r\n\r\n[empty]\r\n",
        ];
        let edits = [
            ("core", "language", "de"),
            ("files", "root", " my books "),
            ("", "theme", "dark"),
            ("core", "night-mode", "on"),
            ("files", "sort", "name"),
            ("empty", "a", "1"),
            ("", "top", "2"),
            ("core", "language", "it"),
            ("core", "bad key", "x"),
            ("new", "a", "1"),
            ("reader", "x", "2"),
        ];
        for source in sources {
            for n in 0..=edits.len() {
                let mut one_by_one = String::from(source);
                for (section, key, value) in &edits[..n] {
                    one_by_one = set(&one_by_one, section, key, value);
                }
                assert_eq!(set_all(source, edits[..n].iter().copied()), one_by_one);
            }
        }
    }

    #[test]
    fn unicode_escapes_take_hex_digits_only() {
        let error = |source| parse(source).find_map(Result::err).map(|e| e.kind);
        assert_eq!(error(r#"a = "\u{+41}""#), Some(ErrorKind::InvalidEscape));
        assert_eq!(error(r#"a = "\u{41}""#), None);
    }
}
