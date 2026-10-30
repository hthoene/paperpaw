//! Cards from a CSV, TSV or TXT file on the card (see [`sync`](crate::sync)).
//!
//! One card is a record; its columns are the front, the back, an example
//! and, if the file has them, the `id` the reader gave the card. The file
//! is read a chunk at a time, never whole, and nothing in it can make the
//! reader panic or hold more than one record:
//!
//! - UTF-8, with or without a byte order mark; a record that is not valid
//!   UTF-8 is a problem with its line number, the rest is still read.
//! - The delimiter is a tab if the first line has one (or the `#separator:`
//!   line of an Anki text export says so), else `;` or `,`, whichever is
//!   there more outside quotes.
//! - Fields in quotes as RFC 4180 has them (`""` is a quote, a line break
//!   inside quotes stays), so a record may span lines.
//! - A first line that names columns (`front`, `back`, `example`, `tags`,
//!   `id`, or German `vorderseite`, `rückseite`, `beispiel`) says which
//!   column is which, in any order; else they are the first three.
//! - A record over [`MAX_RECORD`] bytes, or without a front, is skipped
//!   and reported. A quote that is opened and not closed within that is
//!   reported with its line, and reading goes on at the next line.
//! - The `id` column is the number the reader gave a card, so it counts
//!   only in a file that says it is the reader's (a line `#paperpaw:`
//!   before the header); in any other file it is an ordinary column.
//! - A file that is not UTF-8 as a whole, with no NUL byte (a spreadsheet
//!   "CSV" in Windows-1252), is read as Windows-1252. UTF-16 (a
//!   spreadsheet's "Unicode text") is told, not read.

use alloc::string::String;
use alloc::vec::Vec;
use core::ops::ControlFlow;

use paperpaw_sdk::{Storage, StorageError};

/// The most a record may take, in bytes: a card's three texts at their
/// longest, with room for quotes.
pub(crate) const MAX_RECORD: usize = 8 * 1024;
/// How much of a field is kept, in bytes (more than a text may be: the
/// text is cut to its characters later).
const MAX_FIELD: usize = 2 * 1024;
/// Columns looked at.
const MAX_COLUMNS: usize = 8;
/// How much of the file's start tells its delimiter and header.
const SNIFF: usize = 4096;
const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// Which column holds what (the position of a column, from 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Columns {
    pub front: usize,
    pub back: Option<usize>,
    pub example: Option<usize>,
    pub id: Option<usize>,
}

/// How a file is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Layout {
    pub separator: u8,
    pub columns: Columns,
    /// The texts are HTML (`#html:true` of an Anki export).
    pub html: bool,
    /// The file says it was made by the reader (`#paperpaw:`): its `id`
    /// column is the cards' numbers.
    pub paperpaw: bool,
    /// The file is UTF-16 (a byte order mark says so): it is not read.
    pub utf16: bool,
    /// The file is Windows-1252, not UTF-8.
    pub latin1: bool,
    /// Where the first card's record begins, and its line (1-based).
    pub start: u64,
    pub line: u32,
}

/// What is wrong with a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fault {
    /// No text in the column of the front.
    NoFront,
    /// Not UTF-8 text.
    NotUtf8,
    /// Longer than [`MAX_RECORD`].
    TooLong,
    /// A quote opened and not closed within [`MAX_RECORD`] bytes.
    OpenQuote,
}

/// A card read from a file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Record {
    /// Where the record begins in the file, for [`record_at`].
    pub offset: u64,
    /// The line it begins on (0 where read at an offset).
    pub line: u32,
    pub front: String,
    pub back: String,
    pub example: String,
    pub id: Option<u32>,
}

/// What [`each`] finds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Item {
    Record(Record),
    Skipped { line: u32, fault: Fault },
}

/// A column name's meaning, in any case.
fn role(name: &str) -> Option<Role> {
    let name = name.trim().trim_start_matches('\u{feff}').to_lowercase();
    Some(match name.as_str() {
        "front" | "vorderseite" | "word" | "wort" | "question" | "frage" => Role::Front,
        "back" | "rückseite" | "rueckseite" | "answer" | "antwort" | "meaning" => Role::Back,
        "example" | "beispiel" | "sentence" | "satz" => Role::Example,
        "id" => Role::Id,
        "tags" | "tag" => Role::Other,
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Front,
    Back,
    Example,
    Id,
    Other,
}

/// Reads up to `limit` bytes from the start of `path`.
fn head(storage: &dyn Storage, path: &str, limit: usize) -> Result<Vec<u8>, StorageError> {
    let mut out = Vec::new();
    let mut chunk = [0u8; 512];
    while out.len() < limit {
        let n = storage.read_at(path, out.len() as u64, &mut chunk)?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&chunk[..n.min(limit - out.len())]);
    }
    Ok(out)
}

/// The separator by the first line's count outside quotes: a tab if it
/// has one, else `;` where it has more of them than commas, else `,`.
fn guess_separator(line: &[u8]) -> u8 {
    let (mut tabs, mut semis, mut commas, mut quoted) = (0, 0, 0, false);
    // A quote that is never closed on the line is no quoting.
    let quotes = line.iter().filter(|&&b| b == b'"').count() % 2 == 0;
    for &byte in line {
        match byte {
            b'"' if quotes => quoted = !quoted,
            b'\t' if !quoted => tabs += 1,
            b';' if !quoted => semis += 1,
            b',' if !quoted => commas += 1,
            _ => {}
        }
    }
    if tabs > 0 {
        b'\t'
    } else if semis > commas {
        b';'
    } else {
        b','
    }
}

/// How `path` is to be read, from its first bytes: a missing file or one
/// that cannot be read is the storage's error.
pub(crate) fn detect(storage: &dyn Storage, path: &str) -> Result<Layout, StorageError> {
    let bytes = head(storage, path, SNIFF)?;
    let utf16 = bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]);
    let latin1 = !utf16 && is_latin1(&bytes);
    let mut at = if bytes.starts_with(&BOM) {
        BOM.len()
    } else {
        0
    };
    let mut line = 1u32;
    let (mut separator, mut html, mut paperpaw) = (None, false, false);
    // The lines of an Anki text export that start with `#`.
    while bytes.get(at) == Some(&b'#') {
        let end = bytes[at..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(bytes.len(), |n| at + n);
        let text = String::from_utf8_lossy(&bytes[at..end]);
        let text = text.trim_end_matches('\r');
        if let Some(name) = text.strip_prefix("#separator:") {
            separator = match name.trim() {
                "tab" | "Tab" => Some(b'\t'),
                "comma" | "Comma" => Some(b','),
                "semicolon" | "Semicolon" => Some(b';'),
                other if other.len() == 1 => Some(other.as_bytes()[0]),
                _ => separator,
            };
        } else if let Some(value) = text.strip_prefix("#html:") {
            html = value.trim() == "true";
        } else if text.starts_with("#paperpaw:") {
            paperpaw = true;
        }
        at = (end + 1).min(bytes.len());
        line += 1;
    }
    let rest = &bytes[at..];
    let first_end = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
    let first = &rest[..first_end];
    let mut layout = Layout {
        separator: separator.unwrap_or_else(|| guess_separator(first)),
        columns: Columns {
            front: 0,
            back: Some(1),
            example: Some(2),
            id: None,
        },
        html,
        paperpaw,
        utf16,
        latin1,
        start: at as u64,
        line,
    };
    // A first record that names columns is not a card (a line too long
    // to be seen whole is no header).
    let seen_whole = first_end < rest.len() || bytes.len() < SNIFF;
    let roles: Vec<Option<Role>> = cells_of(first, layout.separator, latin1)
        .iter()
        .map(|c| role(c))
        .collect();
    if seen_whole && roles.contains(&Some(Role::Front)) {
        let find = |wanted: Role| roles.iter().position(|r| *r == Some(wanted));
        layout.columns = Columns {
            front: find(Role::Front).unwrap_or(0),
            back: find(Role::Back),
            example: find(Role::Example),
            // Only the reader's own files have the cards' numbers.
            id: find(Role::Id).filter(|_| paperpaw),
        };
        layout.start = (at + first_end + 1).min(at + rest.len()) as u64;
        layout.line += 1;
    }
    Ok(layout)
}

/// The cells of one line.
fn cells_of(line: &[u8], separator: u8, latin1: bool) -> Vec<String> {
    let mut parser = Parser::new(separator);
    line.iter()
        .chain(b"\n")
        .find_map(|&byte| parser.feed(byte))
        .map_or_else(Vec::new, |cells| {
            cells
                .fields
                .iter()
                .map(|f| {
                    if latin1 {
                        windows_1252(f)
                    } else {
                        String::from_utf8_lossy(f).into_owned()
                    }
                })
                .collect()
        })
}

/// Whether the start of a file is Windows-1252 text: not UTF-8 (an end cut
/// in the middle of a character does not count), and no NUL byte, which
/// binary files have.
fn is_latin1(bytes: &[u8]) -> bool {
    if bytes.contains(&0) {
        return false;
    }
    match core::str::from_utf8(bytes) {
        Ok(_) => false,
        Err(e) => e.error_len().is_some(),
    }
}

/// Windows-1252 as text: Latin-1, but for the 32 characters of 0x80-0x9F.
fn windows_1252(bytes: &[u8]) -> String {
    const HIGH: [char; 32] = [
        '\u{20AC}', '\u{81}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}',
        '\u{2021}', '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{8D}',
        '\u{017D}', '\u{8F}', '\u{90}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}',
        '\u{2013}', '\u{2014}', '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}',
        '\u{9D}', '\u{017E}', '\u{0178}',
    ];
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0x9F => HIGH[usize::from(b - 0x80)],
            _ => char::from(b),
        })
        .collect()
}

/// A field's bytes as text. One cut at [`MAX_FIELD`] may end in the middle of
/// a character: that end is dropped.
fn decode(bytes: &[u8], layout: &Layout) -> Result<String, ()> {
    if layout.latin1 {
        return Ok(windows_1252(bytes));
    }
    match core::str::from_utf8(bytes) {
        Ok(text) => Ok(String::from(text)),
        Err(e) if e.error_len().is_none() && bytes.len() >= MAX_FIELD => Ok(String::from(
            core::str::from_utf8(&bytes[..e.valid_up_to()]).map_err(|_| ())?,
        )),
        Err(_) => Err(()),
    }
}

/// A record's fields as they were cut, and what it took.
struct Cells {
    fields: Vec<Vec<u8>>,
    too_long: bool,
}

/// The state of reading records byte by byte.
struct Parser {
    separator: u8,
    fields: Vec<Vec<u8>>,
    field: Vec<u8>,
    in_quotes: bool,
    /// A quote inside quotes was just read: a second one is a quote, any
    /// other byte ends the quotes.
    quote_pending: bool,
    /// The field began with a quote.
    quoted_field: bool,
    size: usize,
    too_long: bool,
    /// A byte of this record was seen.
    started: bool,
}

impl Parser {
    fn new(separator: u8) -> Self {
        Self {
            separator,
            fields: Vec::new(),
            field: Vec::new(),
            in_quotes: false,
            quote_pending: false,
            quoted_field: false,
            size: 0,
            too_long: false,
            started: false,
        }
    }

    /// Whether a byte of the record in hand was seen.
    fn started(&self) -> bool {
        self.started
    }

    /// Whether a quote was opened and the record is over its size without
    /// it being closed: a lone quote, not a quoted text.
    fn open_quote(&self) -> bool {
        (self.in_quotes || self.quote_pending) && self.size > MAX_RECORD
    }

    fn push(&mut self, byte: u8) {
        self.size += 1;
        if self.size > MAX_RECORD {
            self.too_long = true;
        } else if self.field.len() < MAX_FIELD {
            self.field.push(byte);
        }
    }

    fn end_field(&mut self) {
        let field = core::mem::take(&mut self.field);
        if self.fields.len() < MAX_COLUMNS {
            self.fields.push(field);
        }
        self.quoted_field = false;
    }

    /// The record this byte ends, if it does.
    fn feed(&mut self, byte: u8) -> Option<Cells> {
        self.started = true;
        if self.quote_pending {
            self.quote_pending = false;
            if byte == b'"' {
                self.push(b'"');
                return None;
            }
            self.in_quotes = false;
        }
        if self.in_quotes {
            if byte == b'"' {
                self.quote_pending = true;
            } else {
                self.push(byte);
            }
            return None;
        }
        match byte {
            b'"' if self.field.is_empty() && !self.quoted_field => {
                self.in_quotes = true;
                self.quoted_field = true;
            }
            b if b == self.separator => self.end_field(),
            b'\n' => {
                if self.field.last() == Some(&b'\r') {
                    self.field.pop();
                }
                self.end_field();
                let done = Cells {
                    fields: core::mem::take(&mut self.fields),
                    too_long: self.too_long,
                };
                self.size = 0;
                self.too_long = false;
                self.started = false;
                return Some(done);
            }
            b => self.push(b),
        }
        None
    }

    /// The record the file's end leaves, if there is one.
    fn finish(&mut self) -> Option<Cells> {
        if !self.started {
            return None;
        }
        self.quote_pending = false;
        self.in_quotes = false;
        self.feed(b'\n')
    }
}

/// Turns a record's cells into a card, or what is wrong with it.
fn card(cells: Cells, layout: &Layout, offset: u64, line: u32) -> Option<Item> {
    // A line with nothing on it is no card, and no problem.
    if cells
        .fields
        .iter()
        .all(|f| f.iter().all(u8::is_ascii_whitespace))
    {
        return None;
    }
    let skipped = |fault| Some(Item::Skipped { line, fault });
    if cells.too_long {
        return skipped(Fault::TooLong);
    }
    let text = |at: Option<usize>| -> Result<String, ()> {
        let Some(bytes) = at.and_then(|i| cells.fields.get(i)) else {
            return Ok(String::new());
        };
        let text = decode(bytes, layout)?;
        Ok(if layout.html { plain(&text) } else { text })
    };
    let (Ok(front), Ok(back), Ok(example), Ok(id)) = (
        text(Some(layout.columns.front)),
        text(layout.columns.back),
        text(layout.columns.example),
        text(layout.columns.id),
    ) else {
        return skipped(Fault::NotUtf8);
    };
    if front.trim().is_empty() {
        return skipped(Fault::NoFront);
    }
    Some(Item::Record(Record {
        offset,
        line,
        front,
        back,
        example,
        id: id.trim().parse().ok(),
    }))
}

/// Where reading a file goes on: a record's start and its line (1-based; 0
/// where it is not known).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Resume {
    pub at: u64,
    pub line: u32,
}

/// Calls `visit` with each card of `path`, and each record skipped, from
/// the first card (or from `from`, a record's start) to the end.
pub(crate) fn each(
    storage: &dyn Storage,
    path: &str,
    layout: &Layout,
    from: Option<u64>,
    visit: impl FnMut(Item) -> ControlFlow<()>,
) -> Result<(), StorageError> {
    let from = match from {
        Some(at) => Resume { at, line: 0 },
        None => Resume {
            at: layout.start,
            line: layout.line,
        },
    };
    scan(storage, path, layout, from, None, visit).map(|_| ())
}

/// As [`each`] from `from`, but stops at the first record's end after
/// `budget` bytes were read and says where to go on (`None`: the end of the
/// file, or `visit` broke off). A file is read a step at a time with it.
pub(crate) fn scan(
    storage: &dyn Storage,
    path: &str,
    layout: &Layout,
    from: Resume,
    budget: Option<u64>,
    mut visit: impl FnMut(Item) -> ControlFlow<()>,
) -> Result<Option<Resume>, StorageError> {
    let (mut at, mut line) = (from.at, from.line);
    let mut parser = Parser::new(layout.separator);
    let (mut start, mut start_line) = (at, line);
    let mut chunk = [0u8; 512];
    'file: loop {
        let n = storage.read_at(path, at, &mut chunk)?;
        if n == 0 {
            break;
        }
        for (i, &byte) in chunk[..n].iter().enumerate() {
            if !parser.started() {
                start = at + i as u64;
                start_line = line;
            }
            if byte == b'\n' {
                line = line.saturating_add(1);
            }
            let ended = parser.feed(byte);
            if let Some(cells) = ended {
                if let Some(item) = card(cells, layout, start, start_line)
                    && visit(item).is_break()
                {
                    return Ok(None);
                }
                let next = at + i as u64 + 1;
                if budget.is_some_and(|b| next - from.at >= b) {
                    return Ok(Some(Resume { at: next, line }));
                }
            } else if parser.open_quote() {
                // A lone quote: the record is told, and reading goes on at
                // the line after the one the quote was on.
                let item = Item::Skipped {
                    line: start_line,
                    fault: Fault::OpenQuote,
                };
                if visit(item).is_break() {
                    return Ok(None);
                }
                at = line_after(storage, path, start)?;
                line = start_line.saturating_add(1);
                parser = Parser::new(layout.separator);
                (start, start_line) = (at, line);
                continue 'file;
            }
        }
        at += n as u64;
    }
    if let Some(cells) = parser.finish()
        && let Some(item) = card(cells, layout, start, start_line)
    {
        let _ = visit(item);
    }
    Ok(None)
}

/// The offset after the first line break at or after `from` (the file's
/// end if there is none).
fn line_after(storage: &dyn Storage, path: &str, from: u64) -> Result<u64, StorageError> {
    let (mut at, mut chunk) = (from, [0u8; 512]);
    loop {
        let n = storage.read_at(path, at, &mut chunk)?;
        if n == 0 {
            return Ok(at);
        }
        if let Some(i) = chunk[..n].iter().position(|&b| b == b'\n') {
            return Ok(at + i as u64 + 1);
        }
        at += n as u64;
    }
}

/// The card whose record begins at `offset` (as [`Record::offset`] says).
pub(crate) fn record_at(
    storage: &dyn Storage,
    path: &str,
    layout: &Layout,
    offset: u64,
) -> Option<Record> {
    let mut found = None;
    let _ = each(storage, path, layout, Some(offset), |item| {
        if let Item::Record(record) = item {
            found = Some(record);
        }
        ControlFlow::Break(())
    });
    found
}

/// `html` as plain text: tags out (a line break is a space), the usual
/// character references turned.
fn plain(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut tag = String::new();
    for c in html.chars() {
        match (in_tag, c) {
            (false, '<') => {
                in_tag = true;
                tag.clear();
            }
            (true, '>') => {
                in_tag = false;
                let name = tag.trim_start_matches('/').to_lowercase();
                if name.starts_with("br") || name == "div" || name == "p" {
                    out.push(' ');
                }
            }
            (true, c) => tag.push(c),
            (false, c) => out.push(c),
        }
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    fn read(file: &[u8]) -> (Layout, Vec<Item>) {
        let storage = MemoryStorage::new();
        storage.write("f", file).unwrap();
        let layout = detect(&storage, "f").unwrap();
        let mut items = Vec::new();
        each(&storage, "f", &layout, None, |item| {
            items.push(item);
            ControlFlow::Continue(())
        })
        .unwrap();
        (layout, items)
    }

    fn cards(items: &[Item]) -> Vec<(&str, &str, &str)> {
        items
            .iter()
            .filter_map(|i| match i {
                Item::Record(r) => Some((r.front.as_str(), r.back.as_str(), r.example.as_str())),
                Item::Skipped { .. } => None,
            })
            .collect()
    }

    #[test]
    fn tabs_commas_and_semicolons_are_told_apart() {
        for (text, separator) in [
            ("a\tb\tc\n", b'\t'),
            ("a;b;c\n", b';'),
            ("a,b,c\n", b','),
            ("\"a,b\";c\n", b';'),
            ("one\n", b','),
        ] {
            assert_eq!(read(text.as_bytes()).0.separator, separator, "{text}");
        }
    }

    #[test]
    fn a_file_with_a_header_names_its_columns() {
        // The reader's own file (it says so) has the cards' numbers.
        let (layout, items) = read(b"#paperpaw:\nBeispiel;Back;Front;id\nDie Katze.;cat;Katze;7\n");
        assert_eq!(layout.line, 3);
        assert_eq!(cards(&items), [("Katze", "cat", "Die Katze.")]);
        let Item::Record(record) = &items[0] else {
            panic!()
        };
        assert_eq!(record.id, Some(7));
        assert_eq!(record.line, 3);
        // Any other file with an id column: the column is an ordinary one.
        let (layout, items) = read(b"Beispiel;Back;Front;id\nDie Katze.;cat;Katze;7\n");
        assert_eq!(layout.line, 2);
        assert_eq!(layout.columns.id, None);
        let Item::Record(record) = &items[0] else {
            panic!()
        };
        assert_eq!((record.id, record.front.as_str()), (None, "Katze"));
        // No header: the first three columns.
        let (layout, items) = read(b"Katze,cat\n");
        assert_eq!(layout.line, 1);
        assert_eq!(cards(&items), [("Katze", "cat", "")]);
    }

    #[test]
    fn a_byte_order_mark_crlf_and_quotes_are_read() {
        let file = "\u{feff}front,back\r\n\"a, b\",\"say \"\"hi\"\"\"\r\n\"two\nlines\",x\r\n";
        let (_, items) = read(file.as_bytes());
        assert_eq!(
            cards(&items),
            [("a, b", "say \"hi\"", ""), ("two\nlines", "x", "")]
        );
        let Item::Record(second) = &items[1] else {
            panic!()
        };
        assert_eq!(second.line, 3);
    }

    #[test]
    fn an_anki_export_says_its_separator_and_has_html() {
        let file = "#separator:tab\n#html:true\nhaus\tthe <b>house</b><br>big&amp;old\n";
        let (layout, items) = read(file.as_bytes());
        assert_eq!(layout.separator, b'\t');
        assert_eq!(cards(&items), [("haus", "the house big&old", "")]);
    }

    #[test]
    fn bad_records_are_told_with_their_line_and_the_rest_is_read() {
        let mut file = b"a\tb\n\tno front\n".to_vec();
        file.extend_from_slice(b"too\t");
        file.extend_from_slice(&[b'x'; MAX_RECORD + 10]);
        file.extend_from_slice(b"\nlast\tone");
        let (_, items) = read(&file);
        assert_eq!(cards(&items), [("a", "b", ""), ("last", "one", "")]);
        let skipped: Vec<(u32, Fault)> = items
            .iter()
            .filter_map(|i| match i {
                Item::Skipped { line, fault } => Some((*line, *fault)),
                Item::Record(_) => None,
            })
            .collect();
        assert_eq!(skipped, [(2, Fault::NoFront), (3, Fault::TooLong)]);
    }

    #[test]
    fn a_record_that_is_not_utf8_is_told_where_the_file_is_utf8() {
        // Seen whole, the start of the file is UTF-8 (padded past the part
        // that tells the encoding): a bad record is a problem, not text.
        let mut file = b"a\tb\n".repeat(SNIFF / 4);
        file.extend_from_slice(b"bad\t\xff\xfe\nlast\tone\n");
        let (layout, items) = read(&file);
        assert!(!layout.latin1);
        let last = items.last().unwrap();
        assert_eq!(cards(&items[items.len() - 2..]), [("last", "one", "")]);
        assert!(matches!(last, Item::Record(_)));
        assert!(items.iter().any(|i| matches!(
            i,
            Item::Skipped {
                fault: Fault::NotUtf8,
                ..
            }
        )));
    }

    #[test]
    fn a_windows_1252_file_is_read_as_such() {
        // "Rückseite", "Müll", a euro sign and a curly quote, as Excel saves.
        let file = b"Front;R\xfcckseite\nM\xfcll;\x80 \x93ok\x94\n";
        let (layout, items) = read(file);
        assert!(layout.latin1 && !layout.utf16);
        assert_eq!(cards(&items), [("Müll", "€ \u{201c}ok\u{201d}", "")]);
        // UTF-8 is not mistaken for it, nor is a binary file.
        assert!(!read("Müll;x\n".as_bytes()).0.latin1);
        assert!(!read(b"a\0\xff;b\n").0.latin1);
    }

    #[test]
    fn utf16_is_told_and_not_read_as_text() {
        assert!(read(b"\xff\xfea\0;\0b\0\n\0").0.utf16);
        assert!(read(b"\xfe\xff\0a\0;\0b").0.utf16);
        assert!(!read(b"a;b\n").0.utf16);
    }

    #[test]
    fn a_lone_quote_does_not_eat_the_rest_of_the_file() {
        let mut file = b"\"to be\tsein\nfine\tgut\n".to_vec();
        // Past the size a record may be, with no closing quote in sight.
        for _ in 0..MAX_RECORD / 6 {
            file.extend_from_slice(b"ab\tcd\n");
        }
        file.extend_from_slice(b"end\tende\n");
        let (_, items) = read(&file);
        assert!(matches!(
            items[0],
            Item::Skipped {
                line: 1,
                fault: Fault::OpenQuote
            }
        ));
        // The lines after the one with the quote are read again.
        assert_eq!(cards(&items[1..2]), [("fine", "gut", "")]);
        assert_eq!(cards(&items[items.len() - 1..]), [("end", "ende", "")]);
        // A quoted text that does close, of any lines, is one record.
        let (_, items) = read(b"front\tback\n\"two\nlines\"\tz\n");
        assert_eq!(cards(&items), [("two\nlines", "z", "")]);
    }

    #[test]
    fn a_field_cut_at_its_limit_does_not_break_a_character() {
        let mut file = "ä".repeat(MAX_FIELD).into_bytes();
        file.extend_from_slice(b"\tb\n");
        let (_, items) = read(&file);
        let Item::Record(record) = &items[0] else {
            panic!("{items:?}")
        };
        assert!(record.front.chars().all(|c| c == 'ä'));
        assert_eq!(record.front.len(), MAX_FIELD);
    }

    #[test]
    fn a_file_is_scanned_in_steps_with_the_same_result() {
        let storage = MemoryStorage::new();
        let file: Vec<u8> = (0..500)
            .flat_map(|i| alloc::format!("w{i}\tb{i}\n").into_bytes())
            .collect();
        storage.write("f", &file).unwrap();
        let layout = detect(&storage, "f").unwrap();
        let (mut fronts, mut steps) = (Vec::new(), 0);
        let mut at = Resume {
            at: layout.start,
            line: layout.line,
        };
        loop {
            steps += 1;
            let next = scan(&storage, "f", &layout, at, Some(100), |item| {
                if let Item::Record(r) = item {
                    fronts.push(r.front);
                }
                ControlFlow::Continue(())
            })
            .unwrap();
            match next {
                Some(resume) => at = resume,
                None => break,
            }
        }
        assert!(steps > 20, "{steps}");
        let expected: Vec<String> = (0..500).map(|i| alloc::format!("w{i}")).collect();
        assert_eq!(fronts, expected);
    }

    #[test]
    fn a_record_can_be_read_again_where_it_began() {
        let storage = MemoryStorage::new();
        storage.write("f", b"x\ty\n\"q\nq\"\tz\nlast\tw").unwrap();
        let layout = detect(&storage, "f").unwrap();
        let mut offsets = Vec::new();
        each(&storage, "f", &layout, None, |item| {
            if let Item::Record(r) = item {
                offsets.push((r.offset, r.front));
            }
            ControlFlow::Continue(())
        })
        .unwrap();
        for (offset, front) in offsets {
            let again = record_at(&storage, "f", &layout, offset).unwrap();
            assert_eq!(again.front, front);
        }
    }

    #[test]
    fn empty_files_and_odd_endings_are_no_trouble() {
        for file in [
            &b""[..],
            b"\n\n",
            b"front\tback",
            b"\"open",
            b"a\tb\n\n\n",
            b"#only",
        ] {
            let _ = read(file);
        }
        let (_, items) = read(b"front\tback\n");
        assert!(items.is_empty());
    }

    proptest::proptest! {
        /// Any bytes: no panic, every card has a front, and a line of a
        /// card is never past the file's lines.
        #[test]
        fn any_bytes_read_without_a_panic(file in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..3000)) {
            let lines = file.iter().filter(|b| **b == b'\n').count() as u32 + 2;
            let (_, items) = read(&file);
            for item in items {
                match item {
                    Item::Record(r) => {
                        proptest::prop_assert!(!r.front.trim().is_empty());
                        proptest::prop_assert!(r.line <= lines);
                    }
                    Item::Skipped { line, .. } => proptest::prop_assert!(line <= lines),
                }
            }
        }
    }
}
