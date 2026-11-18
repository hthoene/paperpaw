//! A to-do changed in place as its calendar text streams through: the
//! properties a change names are replaced, every other byte is passed on as
//! it came (alarms, descriptions, repetitions, what other programs wrote), so
//! nothing a server keeps for a to-do is lost by ticking it off on a reader.
//! It needs no memory beyond the start of one line, however long the text.
use alloc::string::String;

use crate::{
    Error,
    date::DateTime,
    ical::{push_state, push_text, push_time},
};

/// How much of the start of a line is looked at to know its property.
const HEAD: usize = 96;

/// What to change of a to-do: `None` leaves a thing as it is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Change<'a> {
    /// What is to do.
    pub summary: Option<&'a str>,
    /// The day it is due (days since 1970), or no day.
    pub due: Option<Option<i32>>,
    /// 0 for none, 1 the highest, 9 the lowest.
    pub priority: Option<u8>,
    /// Done or open again.
    pub done: Option<bool>,
    /// When it was done, in seconds since 1970, if known.
    pub completed: Option<i64>,
    /// The time now, in seconds since 1970, if the clock is set: the to-do's
    /// stamps are set to it.
    pub now: Option<i64>,
}

impl Change<'_> {
    /// Whether the property `name` of a to-do is replaced by this change.
    fn owns(&self, name: &[u8]) -> bool {
        let is = |wanted: &str| name.eq_ignore_ascii_case(wanted.as_bytes());
        (self.now.is_some() && (is("LAST-MODIFIED") || is("DTSTAMP")))
            || (self.summary.is_some() && is("SUMMARY"))
            || (self.due.is_some() && (is("DUE") || is("DURATION")))
            || (self.priority.is_some() && is("PRIORITY"))
            || (self.done.is_some() && (is("STATUS") || is("COMPLETED") || is("PERCENT-COMPLETE")))
    }

    /// The lines that replace what was left out.
    fn lines(&self) -> Result<String, Error> {
        let mut out = String::new();
        if let Some(now) = self.now {
            let now = DateTime::from_unix(now);
            push_time(&mut out, "DTSTAMP:", now)?;
            push_time(&mut out, "LAST-MODIFIED:", now)?;
        }
        if let Some(summary) = self.summary {
            push_text(&mut out, "SUMMARY:", summary)?;
        }
        push_state(
            &mut out,
            self.due,
            self.priority,
            self.done,
            self.completed.or(self.now),
        )?;
        Ok(out)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// At the start of a line: its first bytes are being kept.
    Head,
    /// In the rest of a long line, which is passed on or left out.
    Rest(bool),
}

/// Applies a [`Change`] to the first to-do of a calendar text that streams
/// through in pieces of any size.
#[derive(Debug)]
pub struct Patcher<'a> {
    change: Change<'a>,
    head: [u8; HEAD],
    len: usize,
    mode: Mode,
    /// Whether the property the last line began is passed on: its folded
    /// lines go the same way.
    keeps: bool,
    depth: u16,
    /// The depth of the to-do being changed.
    todo: Option<u16>,
    patched: bool,
}

impl<'a> Patcher<'a> {
    /// A patcher at the start of a text.
    #[must_use]
    pub const fn new(change: Change<'a>) -> Self {
        Self {
            change,
            head: [0; HEAD],
            len: 0,
            mode: Mode::Head,
            keeps: true,
            depth: 0,
            todo: None,
            patched: false,
        }
    }

    /// Whether a to-do was found and changed.
    #[must_use]
    pub const fn patched(&self) -> bool {
        self.patched
    }

    /// The name of the property the kept start of a line begins, and what
    /// follows the name on that line.
    fn name(&self) -> (&[u8], &[u8]) {
        let line = self.head.get(..self.len).unwrap_or(&[]);
        let end = line
            .iter()
            .position(|b| matches!(b, b':' | b';' | b'\r' | b'\n'))
            .unwrap_or(line.len());
        line.split_at(end)
    }

    /// Decides the line whose start is kept (`whole`: all of it is), writes
    /// what belongs before it, and says whether the line is passed on.
    fn decide<E: From<Error>>(
        &mut self,
        whole: bool,
        out: &mut dyn FnMut(&[u8]) -> Result<(), E>,
    ) -> Result<bool, E> {
        let (name, rest) = self.name();
        let value = rest.strip_prefix(b":").unwrap_or(&[]).trim_ascii();
        let is_todo = value.eq_ignore_ascii_case(b"VTODO");
        let begins = whole && name.eq_ignore_ascii_case(b"BEGIN");
        let ends = whole && name.eq_ignore_ascii_case(b"END");
        let keeps = !(self.todo == Some(self.depth) && self.change.owns(name));
        if begins {
            self.depth = self.depth.saturating_add(1);
            if is_todo && self.todo.is_none() && !self.patched {
                self.todo = Some(self.depth);
            }
        } else if ends {
            if self.todo == Some(self.depth) {
                self.todo = None;
                self.patched = true;
                out(self.change.lines()?.as_bytes())?;
            }
            self.depth = self.depth.saturating_sub(1);
        }
        self.keeps = keeps || begins || ends;
        Ok(self.keeps)
    }

    /// Takes `bytes` and gives `out` what the changed text has for them.
    ///
    /// # Errors
    /// [`Error::NoMemory`], or what `out` says.
    pub fn feed<E: From<Error>>(
        &mut self,
        mut bytes: &[u8],
        out: &mut dyn FnMut(&[u8]) -> Result<(), E>,
    ) -> Result<(), E> {
        while let Some((first, after)) = bytes.split_first() {
            match self.mode {
                Mode::Rest(keeps) => {
                    let end = bytes
                        .iter()
                        .position(|b| *b == b'\n')
                        .map_or(bytes.len(), |at| at + 1);
                    let (run, after) = bytes.split_at(end);
                    if keeps {
                        out(run)?;
                    }
                    if run.last() == Some(&b'\n') {
                        self.mode = Mode::Head;
                    }
                    bytes = after;
                }
                Mode::Head if self.len == 0 && matches!(first, b' ' | b'\t') => {
                    // A folded line: it goes the way of the line it belongs to.
                    self.mode = Mode::Rest(self.keeps);
                }
                Mode::Head => {
                    if let Some(slot) = self.head.get_mut(self.len) {
                        *slot = *first;
                        self.len += 1;
                    }
                    bytes = after;
                    let whole = *first == b'\n';
                    if whole || self.len == HEAD {
                        let keeps = self.decide(whole, out)?;
                        if keeps {
                            out(self.head.get(..self.len).unwrap_or(&[]))?;
                        }
                        self.len = 0;
                        if !whole {
                            self.mode = Mode::Rest(keeps);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// The text has ended: what was kept of a last line without a line break.
    ///
    /// # Errors
    /// [`Error::NoMemory`], or what `out` says.
    pub fn finish<E: From<Error>>(
        &mut self,
        out: &mut dyn FnMut(&[u8]) -> Result<(), E>,
    ) -> Result<(), E> {
        if self.mode == Mode::Head && self.len > 0 {
            if self.decide(true, out)? {
                out(self.head.get(..self.len).unwrap_or(&[]))?;
            }
            self.len = 0;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ical::read_todo;
    use alloc::vec::Vec;
    use std::string::ToString;

    fn patch(text: &[u8], change: Change<'_>, piece: usize) -> (String, bool) {
        let mut patcher = Patcher::new(change);
        let mut made = Vec::new();
        let mut out = |bytes: &[u8]| -> Result<(), Error> {
            made.extend_from_slice(bytes);
            Ok(())
        };
        for part in text.chunks(piece.max(1)) {
            patcher.feed(part, &mut out).unwrap();
        }
        patcher.finish(&mut out).unwrap();
        (
            String::from_utf8_lossy(&made).to_string(),
            patcher.patched(),
        )
    }

    const TODO: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Other//EN\r\nBEGIN:VTODO\r\n\
UID:abc\r\nDTSTAMP:20261101T080000Z\r\nLAST-MODIFIED:20261101T080000Z\r\n\
SUMMARY:An old title that is so long that the program which wrote it had to\r\n  fold it\r\n\
DESCRIPTION:Kept\\, with\r\n its fold\r\nDUE;TZID=Europe/Berlin:20261120T170000\r\nPRIORITY:9\r\n\
STATUS:NEEDS-ACTION\r\nX-OTHER;A=b:c\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nSUMMARY:alarm\r\n\
DUE:not a property of ours\r\nEND:VALARM\r\nEND:VTODO\r\nBEGIN:VTODO\r\nUID:abc\r\n\
RECURRENCE-ID:20261121\r\nSTATUS:NEEDS-ACTION\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";

    #[test]
    fn ticking_off_keeps_everything_else_byte_for_byte() {
        let change = Change {
            done: Some(true),
            completed: Some(1_795_000_000),
            ..Change::default()
        };
        let (made, patched) = patch(TODO.as_bytes(), change, TODO.len());
        assert!(patched);
        let added = "STATUS:COMPLETED\r\nPERCENT-COMPLETE:100\r\nCOMPLETED:20261118T110640Z\r\n";
        let expected = TODO.replacen("STATUS:NEEDS-ACTION\r\n", "", 1).replacen(
            "END:VALARM\r\nEND:VTODO",
            &["END:VALARM\r\n", added, "END:VTODO"].concat(),
            1,
        );
        assert_eq!(made, expected);
        // The same whatever the size of the pieces.
        for piece in [1, 2, 3, 7, 64, 97, 200] {
            assert_eq!(
                patch(TODO.as_bytes(), change, piece).0,
                expected,
                "pieces of {piece}"
            );
        }
        let todo = read_todo(made.as_bytes(), 200).unwrap().unwrap();
        assert!(todo.done);
        assert_eq!(todo.completed, Some(1_795_000_000));
    }

    #[test]
    fn every_field_is_replaced_with_its_folds() {
        let change = Change {
            summary: Some("New, short"),
            due: Some(Some(20_800)),
            priority: Some(1),
            done: Some(false),
            completed: None,
            now: Some(1_795_000_000),
        };
        let (made, _) = patch(TODO.as_bytes(), change, 5);
        let todo = read_todo(made.as_bytes(), 200).unwrap().unwrap();
        assert_eq!(todo.summary, "New, short");
        assert_eq!(todo.due, Some(DateTime::date(20_800)));
        assert_eq!((todo.priority, todo.done), (1, false));
        assert!(
            !made.contains("fold it"),
            "the old title's folded line went with it"
        );
        assert!(made.contains("DESCRIPTION:Kept\\, with\r\n its fold\r\n"));
        assert!(made.contains("X-OTHER;A=b:c\r\n"));
        // The alarm's own lines and the second to-do are as they were.
        assert!(made.contains("SUMMARY:alarm\r\nDUE:not a property of ours\r\n"));
        assert!(made.contains("RECURRENCE-ID:20261121\r\nSTATUS:NEEDS-ACTION\r\n"));
        assert_eq!(made.matches("DTSTAMP:").count(), 1);
        assert!(made.contains("LAST-MODIFIED:20261118T110640Z\r\n"));
    }

    #[test]
    fn no_day_takes_the_day_away() {
        let change = Change {
            due: Some(None),
            priority: Some(0),
            ..Change::default()
        };
        let (made, _) = patch(TODO.as_bytes(), change, 11);
        let todo = read_todo(made.as_bytes(), 200).unwrap().unwrap();
        assert_eq!((todo.due, todo.priority), (None, 0));
        // Nothing of the clock is touched without a clock.
        assert!(made.contains("DTSTAMP:20261101T080000Z\r\n"));
    }

    #[test]
    fn a_very_long_line_passes_or_goes_whole() {
        let long = "x".repeat(5000);
        let text = [
            "BEGIN:VTODO\nUID:u\nSUMMARY:",
            &long,
            "\nDESCRIPTION:",
            &long,
            "\nEND:VTODO",
        ]
        .concat();
        let change = Change {
            summary: Some("short"),
            ..Change::default()
        };
        for piece in [1, 50, 96, 97, 4096] {
            let (made, patched) = patch(text.as_bytes(), change, piece);
            assert!(patched);
            let expected = [
                "BEGIN:VTODO\nUID:u\nDESCRIPTION:",
                &long,
                "\nSUMMARY:short\r\nEND:VTODO",
            ]
            .concat();
            assert_eq!(made, expected, "pieces of {piece}");
        }
    }

    #[test]
    fn a_text_without_a_todo_is_passed_on_and_said_so() {
        let event = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nSUMMARY:x\nEND:VEVENT\nEND:VCALENDAR\n";
        let change = Change {
            summary: Some("y"),
            ..Change::default()
        };
        let (made, patched) = patch(event.as_bytes(), change, 9);
        assert_eq!(made, event);
        assert!(!patched);
    }
}
