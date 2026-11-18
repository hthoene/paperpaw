//! iCalendar (RFC 5545) as far as tasks and calendars need it: content lines
//! unfolded as they stream, a property's parts, text escaping, and a to-do
//! (`VTODO`) read and written.
use alloc::{string::String, vec::Vec};

use crate::{Error, date::DateTime, push_str};

/// The most bytes kept of one content line; the rest of a longer one is
/// dropped (a description of pages, an inline attachment).
pub const LINE_MAX: usize = 1024;
/// The most bytes of a `UID` kept; a to-do with a longer one is not told.
pub const UID_MAX: usize = 200;

/// Content lines, unfolded, as their bytes stream by in pieces of any size.
#[derive(Debug, Default)]
pub struct Lines {
    line: Vec<u8>,
    /// A line break was read: the next byte says whether the line goes on.
    pending: bool,
}

impl Lines {
    /// A reader at the start of a text.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            line: Vec::new(),
            pending: false,
        }
    }

    fn push(&mut self, byte: u8) -> Result<(), Error> {
        if self.line.capacity() == 0 {
            self.line
                .try_reserve_exact(LINE_MAX)
                .map_err(|_| Error::NoMemory)?;
        }
        if self.line.len() < LINE_MAX {
            self.line.push(byte);
        }
        Ok(())
    }

    /// Takes `bytes`; `visit` gets each line that is whole, without its line
    /// break and with its folds removed, cut at [`LINE_MAX`] bytes.
    ///
    /// # Errors
    /// [`Error::NoMemory`], or what `visit` says.
    pub fn feed<E: From<Error>>(
        &mut self,
        bytes: &[u8],
        visit: &mut dyn FnMut(&[u8]) -> Result<(), E>,
    ) -> Result<(), E> {
        for byte in bytes {
            if self.pending {
                self.pending = false;
                if matches!(byte, b' ' | b'\t') {
                    // A fold: the line goes on after this blank.
                    continue;
                }
                visit(&self.line)?;
                self.line.clear();
            }
            match byte {
                b'\n' => self.pending = true,
                b'\r' => {}
                other => self.push(*other)?,
            }
        }
        Ok(())
    }

    /// The text has ended: the last line, if it had no line break. The reader
    /// is then at the start of a next text.
    ///
    /// # Errors
    /// What `visit` says.
    pub fn finish<E: From<Error>>(
        &mut self,
        visit: &mut dyn FnMut(&[u8]) -> Result<(), E>,
    ) -> Result<(), E> {
        let pending = core::mem::take(&mut self.pending);
        let told = if pending || !self.line.is_empty() {
            visit(&self.line)
        } else {
            Ok(())
        };
        // Its memory is kept for the next text.
        self.line.clear();
        told
    }
}

/// A content line's parts: `NAME;PARAMS:VALUE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Property<'a> {
    /// The name, as written (compare with [`Property::is`]).
    pub name: &'a str,
    /// The parameters between the name and the value, without the first `;`.
    pub params: &'a str,
    /// The value, still escaped.
    pub value: &'a str,
}

/// The longest valid UTF-8 text `bytes` begins with (a cut line may end in
/// the middle of a character).
fn valid(bytes: &[u8]) -> &str {
    bytes.utf8_chunks().next().map_or("", |chunk| chunk.valid())
}

impl<'a> Property<'a> {
    /// The parts of a content line; `None` for a line that has no `:`.
    #[must_use]
    pub fn parse(line: &'a [u8]) -> Option<Self> {
        let line = valid(line);
        let mut quoted = false;
        let mut name_end = None;
        for (at, c) in line.char_indices() {
            match c {
                '"' => quoted = !quoted,
                ';' if !quoted && name_end.is_none() => name_end = Some(at),
                ':' if !quoted => {
                    let name_end = name_end.unwrap_or(at);
                    return Some(Self {
                        name: line.get(..name_end)?,
                        params: line.get(name_end..at)?.trim_start_matches(';'),
                        value: line.get(at + 1..)?,
                    });
                }
                _ => {}
            }
        }
        None
    }

    /// Whether this is the property `name` (names are in any case).
    #[must_use]
    pub fn is(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }

    /// The value of the parameter `name` (`VALUE`, `TZID`), without quotes.
    #[must_use]
    pub fn param(&self, name: &str) -> Option<&'a str> {
        let mut rest = self.params;
        while !rest.is_empty() {
            let mut quoted = false;
            let end = rest
                .char_indices()
                .find(|(_, c)| {
                    if *c == '"' {
                        quoted = !quoted;
                    }
                    *c == ';' && !quoted
                })
                .map_or(rest.len(), |(at, _)| at);
            let (one, after) = rest.split_at(end);
            rest = after.trim_start_matches(';');
            if let Some((key, value)) = one.split_once('=')
                && key.eq_ignore_ascii_case(name)
            {
                return Some(value.trim_matches('"'));
            }
        }
        None
    }
}

/// Appends the text a `TEXT` value stands for, at most `max` bytes of it:
/// `\n` is a line break (given as a blank where `one_line`), `\,` `\;` `\\`
/// are the character.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn unescape_into(
    out: &mut String,
    value: &str,
    max: usize,
    one_line: bool,
) -> Result<(), Error> {
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        let c = match c {
            '\\' => match chars.next() {
                Some('n' | 'N') if one_line => ' ',
                Some('n' | 'N') => '\n',
                Some(other) => other,
                None => break,
            },
            c if c.is_control() && one_line => ' ',
            c => c,
        };
        if out.len() + c.len_utf8() > max {
            break;
        }
        out.try_reserve(c.len_utf8()).map_err(|_| Error::NoMemory)?;
        out.push(c);
    }
    Ok(())
}

/// Appends `text` as a `TEXT` value: a line break as `\n`, `,` `;` `\` with a
/// `\` before them; other control characters are left out.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn escape_into(out: &mut String, text: &str) -> Result<(), Error> {
    for c in text.chars() {
        let mut one = [0u8; 4];
        let piece: &str = match c {
            '\n' => "\\n",
            ',' => "\\,",
            ';' => "\\;",
            '\\' => "\\\\",
            c if c.is_control() => "",
            c => c.encode_utf8(&mut one),
        };
        push_str(out, piece)?;
    }
    Ok(())
}

/// Appends the content line `line` folded at 75 bytes, with its line break.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn push_line(out: &mut String, line: &str) -> Result<(), Error> {
    let mut room = 75;
    for c in line.chars() {
        if c.len_utf8() > room {
            push_str(out, "\r\n ")?;
            room = 74;
        }
        room -= c.len_utf8();
        out.try_reserve(c.len_utf8()).map_err(|_| Error::NoMemory)?;
        out.push(c);
    }
    push_str(out, "\r\n")
}

/// Appends the line `NAME:value`, the value a date or time.
pub(crate) fn push_time(out: &mut String, name: &str, at: DateTime) -> Result<(), Error> {
    let mut line = String::new();
    push_str(&mut line, name)?;
    at.write(&mut line)?;
    push_line(out, &line)
}

/// Appends the line `NAME:text`, the text escaped.
pub(crate) fn push_text(out: &mut String, name: &str, text: &str) -> Result<(), Error> {
    let mut line = String::new();
    push_str(&mut line, name)?;
    escape_into(&mut line, text)?;
    push_line(out, &line)
}

/// What is read of a to-do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Todo {
    /// Its `UID`.
    pub uid: String,
    /// Its `SUMMARY` on one line, as much as the reader keeps.
    pub summary: String,
    /// When it is due.
    pub due: Option<DateTime>,
    /// Its `PRIORITY`: 0 for none, 1 the highest, 9 the lowest.
    pub priority: u8,
    /// Done (`STATUS:COMPLETED`, a `COMPLETED` time or all of it done) or
    /// given up (`STATUS:CANCELLED`).
    pub done: bool,
    /// When it was done, in seconds since 1970, if it says.
    pub completed: Option<i64>,
    /// It repeats (`RRULE`, `RDATE`) or is one time of a to-do that does.
    pub recurring: bool,
}

/// Reads the to-dos of a calendar text from its content lines. Only what a
/// to-do says itself counts: its alarms and what follows it do not.
#[derive(Debug)]
pub struct TodoReader {
    depth: u16,
    /// The depth of the to-do being read.
    todo: Option<u16>,
    current: Todo,
    summary_max: usize,
    /// Its `STATUS` said it is open, whatever else says.
    open: bool,
}

impl TodoReader {
    /// A reader that keeps `summary_max` bytes of a summary.
    #[must_use]
    pub fn new(summary_max: usize) -> Self {
        Self {
            depth: 0,
            todo: None,
            current: Todo::default(),
            summary_max,
            open: false,
        }
    }

    /// Back to the start of a text, whatever was read.
    pub fn reset(&mut self) {
        *self = Self::new(self.summary_max);
    }

    /// Takes a content line; gives a to-do when the line ends one that has a
    /// `UID` of at most [`UID_MAX`] bytes.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn line(&mut self, line: &[u8]) -> Result<Option<Todo>, Error> {
        let Some(property) = Property::parse(line) else {
            return Ok(None);
        };
        if property.is("BEGIN") {
            self.depth = self.depth.saturating_add(1);
            if self.todo.is_none() && property.value.trim().eq_ignore_ascii_case("VTODO") {
                self.todo = Some(self.depth);
                self.current = Todo::default();
                self.open = false;
            }
            return Ok(None);
        }
        if property.is("END") {
            let ended = self.todo == Some(self.depth);
            self.depth = self.depth.saturating_sub(1);
            if !ended {
                return Ok(None);
            }
            self.todo = None;
            let mut todo = core::mem::take(&mut self.current);
            todo.done &= !self.open;
            let told = !todo.uid.is_empty() && todo.uid.len() <= UID_MAX;
            return Ok(told.then_some(todo));
        }
        if self.todo == Some(self.depth) {
            self.property(&property)?;
        }
        Ok(None)
    }

    fn property(&mut self, property: &Property<'_>) -> Result<(), Error> {
        let todo = &mut self.current;
        let value = property.value.trim();
        match property.name.to_ascii_uppercase_buf().as_str() {
            "UID" => {
                todo.uid.clear();
                // One byte more than is taken, so that a longer one is seen.
                unescape_into(&mut todo.uid, value, UID_MAX + 1, true)?;
            }
            "SUMMARY" => {
                todo.summary.clear();
                unescape_into(&mut todo.summary, property.value, self.summary_max, true)?;
            }
            "DUE" => todo.due = DateTime::parse(value),
            "PRIORITY" => todo.priority = value.parse::<u8>().unwrap_or(0).min(9),
            "STATUS" => {
                let done = value.eq_ignore_ascii_case("COMPLETED")
                    || value.eq_ignore_ascii_case("CANCELLED");
                todo.done |= done;
                self.open = !done;
            }
            "COMPLETED" => {
                todo.completed = DateTime::parse(value).map(|at| at.unix());
                todo.done = true;
            }
            "PERCENT-COMPLETE" => todo.done |= value == "100",
            "RRULE" | "RDATE" | "RECURRENCE-ID" => todo.recurring = true,
            _ => {}
        }
        Ok(())
    }
}

/// A short name in capitals without the heap (names are compared so).
struct Upper {
    bytes: [u8; 24],
    len: usize,
}

impl Upper {
    fn as_str(&self) -> &str {
        core::str::from_utf8(self.bytes.get(..self.len).unwrap_or(&[])).unwrap_or("")
    }
}

trait UpperName {
    fn to_ascii_uppercase_buf(&self) -> Upper;
}

impl UpperName for str {
    /// The name in capitals; empty for one that is longer than any known.
    fn to_ascii_uppercase_buf(&self) -> Upper {
        let mut upper = Upper {
            bytes: [0; 24],
            len: 0,
        };
        if self.len() <= upper.bytes.len() && self.is_ascii() {
            for (slot, byte) in upper.bytes.iter_mut().zip(self.bytes()) {
                *slot = byte.to_ascii_uppercase();
            }
            upper.len = self.len();
        }
        upper
    }
}

/// A to-do to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewTodo<'a> {
    /// Who writes it (`-//PaperPaw//Tasks//EN`).
    pub prodid: &'a str,
    /// Its `UID`.
    pub uid: &'a str,
    /// What is to do.
    pub summary: &'a str,
    /// The day it is due, in days since 1970.
    pub due: Option<i32>,
    /// 0 for none, 1 the highest, 9 the lowest.
    pub priority: u8,
    /// Whether it is done.
    pub done: bool,
    /// When it was done, in seconds since 1970, if known.
    pub completed: Option<i64>,
    /// The time now, in seconds since 1970, if the clock is set.
    pub now: Option<i64>,
}

/// The calendar text of one to-do.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn write_todo(todo: &NewTodo<'_>) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve(320 + todo.summary.len() + todo.uid.len() + todo.prodid.len())
        .map_err(|_| Error::NoMemory)?;
    push_line(&mut out, "BEGIN:VCALENDAR")?;
    push_line(&mut out, "VERSION:2.0")?;
    push_text(&mut out, "PRODID:", todo.prodid)?;
    push_line(&mut out, "BEGIN:VTODO")?;
    push_text(&mut out, "UID:", todo.uid)?;
    // A to-do needs its stamp: without a clock it is the start of time.
    let now = DateTime::from_unix(todo.now.unwrap_or(0));
    push_time(&mut out, "DTSTAMP:", now)?;
    if todo.now.is_some() {
        push_time(&mut out, "CREATED:", now)?;
        push_time(&mut out, "LAST-MODIFIED:", now)?;
    }
    push_text(&mut out, "SUMMARY:", todo.summary)?;
    push_state(
        &mut out,
        Some(todo.due),
        Some(todo.priority),
        Some(todo.done),
        todo.completed.or(todo.now),
    )?;
    push_line(&mut out, "END:VTODO")?;
    push_line(&mut out, "END:VCALENDAR")?;
    Ok(out)
}

/// Appends the lines of what is given: the due day, the priority, and whether
/// it is done (with the time it was, where known).
pub(crate) fn push_state(
    out: &mut String,
    due: Option<Option<i32>>,
    priority: Option<u8>,
    done: Option<bool>,
    completed: Option<i64>,
) -> Result<(), Error> {
    if let Some(Some(day)) = due {
        push_time(out, "DUE;VALUE=DATE:", DateTime::date(day))?;
    }
    if let Some(priority) = priority.filter(|p| (1..=9).contains(p)) {
        let digit = [b'0' + priority];
        let mut line = String::new();
        push_str(&mut line, "PRIORITY:")?;
        push_str(&mut line, core::str::from_utf8(&digit).unwrap_or("0"))?;
        push_line(out, &line)?;
    }
    match done {
        Some(true) => {
            push_line(out, "STATUS:COMPLETED")?;
            push_line(out, "PERCENT-COMPLETE:100")?;
            if let Some(at) = completed {
                push_time(out, "COMPLETED:", DateTime::from_unix(at))?;
            }
        }
        Some(false) => push_line(out, "STATUS:NEEDS-ACTION")?,
        None => {}
    }
    Ok(())
}

/// The first to-do of a whole calendar text, keeping `summary_max` bytes of
/// its summary.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn read_todo(text: &[u8], summary_max: usize) -> Result<Option<Todo>, Error> {
    let mut lines = Lines::new();
    let mut reader = TodoReader::new(summary_max);
    let mut found = None;
    let mut visit = |line: &[u8]| -> Result<(), Error> {
        if found.is_none() {
            found = reader.line(line)?;
        }
        Ok(())
    };
    lines.feed(text, &mut visit)?;
    lines.finish(&mut visit)?;
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use std::string::ToString;

    fn lines_of(text: &[u8], piece: usize) -> Vec<String> {
        let mut lines = Lines::new();
        let mut found = Vec::new();
        let mut visit = |line: &[u8]| -> Result<(), Error> {
            found.push(String::from_utf8_lossy(line).to_string());
            Ok(())
        };
        for part in text.chunks(piece.max(1)) {
            lines.feed(part, &mut visit).unwrap();
        }
        lines.finish(&mut visit).unwrap();
        found
    }

    #[test]
    fn lines_are_unfolded_whatever_the_pieces() {
        let text = b"BEGIN:VTODO\r\nSUMMARY:Buy mi\r\n lk and\r\n\t bread\nUID:1\r\nEND:VTODO";
        let whole = lines_of(text, text.len());
        assert_eq!(
            whole,
            [
                "BEGIN:VTODO",
                "SUMMARY:Buy milk and bread",
                "UID:1",
                "END:VTODO"
            ]
        );
        for piece in 1..8 {
            assert_eq!(lines_of(text, piece), whole, "pieces of {piece}");
        }
    }

    #[test]
    fn a_line_longer_than_the_most_is_cut_not_refused() {
        let mut text = b"DESCRIPTION:".to_vec();
        text.extend(vec![b'x'; 3 * LINE_MAX]);
        text.extend(b"\r\nUID:after\r\n");
        let lines = lines_of(&text, 100);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].len(), LINE_MAX);
        assert_eq!(lines[1], "UID:after");
    }

    #[test]
    fn a_property_has_a_name_parameters_and_a_value() {
        let p =
            Property::parse(b"DUE;TZID=\"Europe:Berlin\";VALUE=DATE-TIME:20261120T170000").unwrap();
        assert!(p.is("due"));
        assert_eq!(p.param("tzid"), Some("Europe:Berlin"));
        assert_eq!(p.param("VALUE"), Some("DATE-TIME"));
        assert_eq!(p.param("X"), None);
        assert_eq!(p.value, "20261120T170000");
        let plain = Property::parse(b"SUMMARY:a: b; c").unwrap();
        assert_eq!(
            (plain.name, plain.params, plain.value),
            ("SUMMARY", "", "a: b; c")
        );
        assert_eq!(Property::parse(b"no colon here"), None);
        // A line cut in the middle of a character keeps what is whole.
        let cut = Property::parse(&"SUMMARY:caf\u{e9}".as_bytes()[..12]).unwrap();
        assert_eq!(cut.value, "caf");
    }

    #[test]
    fn text_is_escaped_and_read_back() {
        for text in ["plain", "a, b; c\\d", "two\nlines", "Größe: 5 €"] {
            let mut escaped = String::new();
            escape_into(&mut escaped, text).unwrap();
            assert!(!escaped.contains('\n'));
            let mut back = String::new();
            unescape_into(&mut back, &escaped, 1000, false).unwrap();
            assert_eq!(back, text);
        }
        let mut one = String::new();
        unescape_into(&mut one, "two\\nlines\tx", 1000, true).unwrap();
        assert_eq!(one, "two lines x");
        let mut short = String::new();
        unescape_into(&mut short, "caf\u{e9}s", 4, true).unwrap();
        assert_eq!(short, "caf", "never half a character");
    }

    #[test]
    fn a_long_line_is_folded_at_75_bytes_between_characters() {
        let mut out = String::new();
        let line = ["SUMMARY:", &"ä".repeat(60)].concat();
        push_line(&mut out, &line).unwrap();
        for physical in out.split("\r\n") {
            assert!(physical.len() <= 75, "{}", physical.len());
        }
        assert_eq!(lines_of(out.as_bytes(), 7), [line]);
    }

    const NEXTCLOUD: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Nextcloud Tasks v0.16\r\n\
BEGIN:VTODO\r\nUID:6b1f2c1e-0d0a-4f3e-9c55-1a2b3c4d5e6f\r\nCREATED:20261101T080000Z\r\n\
LAST-MODIFIED:20261117T101500Z\r\nDTSTAMP:20261117T101500Z\r\n\
SUMMARY:Call the plumber\\, then\r\n  pay the invoice\r\nDESCRIPTION:Ask about the boiler\\nand the tap\r\n\
DUE;TZID=Europe/Berlin:20261120T170000\r\nPRIORITY:1\r\nSTATUS:NEEDS-ACTION\r\n\
X-APPLE-SORT-ORDER:12\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nSUMMARY:not the task\r\n\
TRIGGER:-PT15M\r\nEND:VALARM\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";

    #[test]
    fn a_servers_todo_is_read() {
        let todo = read_todo(NEXTCLOUD.as_bytes(), 200).unwrap().unwrap();
        assert_eq!(todo.uid, "6b1f2c1e-0d0a-4f3e-9c55-1a2b3c4d5e6f");
        assert_eq!(todo.summary, "Call the plumber, then pay the invoice");
        assert_eq!(todo.due, DateTime::parse("20261120T170000"));
        assert_eq!(todo.priority, 1);
        assert!(!todo.done && !todo.recurring);
        assert_eq!(todo.completed, None);
    }

    #[test]
    fn done_and_repeating_are_seen() {
        let done = "BEGIN:VTODO\nUID:a\nSUMMARY:x\nCOMPLETED:20261117T101500Z\nEND:VTODO\n";
        let todo = read_todo(done.as_bytes(), 50).unwrap().unwrap();
        assert!(todo.done);
        assert_eq!(
            todo.completed,
            DateTime::parse("20261117T101500Z").map(|t| t.unix())
        );
        // Reopened elsewhere: the status counts, not a time left behind.
        let reopened =
            "BEGIN:VTODO\nUID:a\nCOMPLETED:20261117T101500Z\nSTATUS:NEEDS-ACTION\nEND:VTODO\n";
        assert!(!read_todo(reopened.as_bytes(), 50).unwrap().unwrap().done);
        let repeating = "BEGIN:VTODO\nUID:a\nRRULE:FREQ=DAILY\nSTATUS:CANCELLED\nEND:VTODO\n";
        let todo = read_todo(repeating.as_bytes(), 50).unwrap().unwrap();
        assert!(todo.recurring && todo.done);
        // No UID, or an event: nothing.
        assert_eq!(
            read_todo(b"BEGIN:VTODO\nSUMMARY:x\nEND:VTODO\n", 50).unwrap(),
            None
        );
        assert_eq!(
            read_todo(b"BEGIN:VEVENT\nUID:e\nEND:VEVENT\n", 50).unwrap(),
            None
        );
    }

    #[test]
    fn a_todo_written_reads_back_the_same() {
        for (summary, due, priority, done) in [
            ("Buy milk, bread; eggs", Some(20_777), 5, false),
            (
                "Größere Aufgabe mit einem sehr langen Titel, der über die Zeile von fünfundsiebzig Zeichen geht",
                None,
                0,
                true,
            ),
            ("back\\slash", Some(0), 9, true),
        ] {
            let new = NewTodo {
                prodid: "-//PaperPaw//Tasks//EN",
                uid: "pp-1-2-3",
                summary,
                due,
                priority,
                done,
                completed: done.then_some(1_795_000_000),
                now: Some(1_795_000_100),
            };
            let text = write_todo(&new).unwrap();
            assert!(text.starts_with("BEGIN:VCALENDAR\r\n") && text.ends_with("END:VCALENDAR\r\n"));
            assert!(text.split("\r\n").all(|line| line.len() <= 75));
            let todo = read_todo(text.as_bytes(), 400).unwrap().unwrap();
            assert_eq!(todo.uid, "pp-1-2-3");
            assert_eq!(todo.summary, summary);
            assert_eq!(todo.due, due.map(DateTime::date));
            assert_eq!((todo.priority, todo.done), (priority, done));
            assert_eq!(todo.completed, done.then_some(1_795_000_000));
            assert!(!todo.recurring);
        }
    }

    #[test]
    fn a_todo_without_a_clock_is_still_valid() {
        let text = write_todo(&NewTodo {
            prodid: "p",
            uid: "u",
            summary: "s",
            due: None,
            priority: 0,
            done: true,
            completed: None,
            now: None,
        })
        .unwrap();
        assert!(text.contains("DTSTAMP:19700101T000000Z\r\n"));
        assert!(!text.contains("COMPLETED:") && text.contains("STATUS:COMPLETED"));
    }
}
