//! The to-do list as kept in `todo.txt` in the plugin's data folder (as
//! todo.txt has them: an item a line, `x 2026-10-19 ` before those done, a date
//! before open ones that would read as done). Items done before today go. The
//! list is written back only once it was read: a file that cannot be read is
//! left as it is, and the widget says so. Items past the first [`MAX_ITEMS`]
//! are not shown and kept as they are.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{Error, Host, storage::Root};
use paperpaw_ui_kit::{files, mem, text::copy, widget::settings::Stack};

use crate::native_dates::{civil_from_days, days_from_civil};

/// The list's file, in the data folder.
pub const FILE: &str = "todo.txt";
/// The most bytes of the file read (a larger one is left as it is).
const READ_LIMIT: usize = 32 * 1024;
/// Items shown and changed at most (those after them are kept as they are).
pub const MAX_ITEMS: usize = 100;
/// The length of an item, in characters.
pub const MAX_TEXT: usize = 200;
/// The date before an open item whose text would read as one done.
const MADE: &str = "1970-01-01";

/// A thing to do, and the day it was done (days since 1970).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub text: String,
    pub done: Option<i64>,
}

/// The list as kept: its first items, and the lines after them as they are,
/// written back after them.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct List {
    pub items: Vec<Item>,
    pub rest: String,
}

/// What reading the file gave.
#[derive(Debug, PartialEq, Eq)]
pub enum Loaded {
    /// The list (empty where there is no file).
    List(List),
    /// The file is there but cannot be read (too large, not text): it is not
    /// written over.
    Unreadable,
}

/// `2026-10-19` of a day since 1970.
///
/// # Errors
/// [`Error::NoSpace`] never for a day of the calendar the SDK counts.
pub fn date_of(day: i64) -> Result<Stack<24>, Error> {
    let (year, month, day) = civil_from_days(day);
    Stack::from_args(format_args!("{year:04}-{month:02}-{day:02}"))
}

/// The day since 1970 of `2026-10-19`.
#[must_use]
pub fn day_of(date: &str) -> Option<i64> {
    let mut parts = date.splitn(3, '-');
    let year = parts.next()?.parse::<i32>().ok()?;
    let month = parts.next()?.parse::<u8>().ok()?;
    let day = parts.next()?.parse::<u8>().ok()?;
    ((1..=12).contains(&month) && (1..=31).contains(&day))
        .then(|| days_from_civil(year, month, day))
}

/// Whether an open item's text would read as one done ("x ray at 3"), also
/// once dates before it are dropped: such an item is kept with a date in front
/// ([`MADE`]), where todo.txt has the day an item was made, so that it reads
/// as open there and here, and loses nothing.
fn dated(text: &str) -> bool {
    let mut rest = text;
    loop {
        if rest.starts_with("x ") {
            return true;
        }
        match rest.split_once(' ') {
            Some((word, after)) if day_of(word).is_some() => rest = after.trim_start(),
            _ => return false,
        }
    }
}

/// The items of a list as kept, but those done before `today`; past
/// [`MAX_ITEMS`], the rest of the lines as they are.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn parse(text: &str, today: i64) -> Result<List, Error> {
    let mut items = Vec::new();
    let mut lines = text.lines();
    while items.len() < MAX_ITEMS {
        let Some(line) = lines.next() else {
            break;
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (text, done) = match line.strip_prefix("x ") {
            Some(rest) => {
                let rest = rest.trim_start();
                let (date, text) = rest.split_once(' ').unwrap_or((rest, ""));
                match day_of(date) {
                    Some(day) => (text.trim(), Some(day)),
                    // Done, but when is not said: long ago.
                    None => (rest, Some(i64::MIN)),
                }
            }
            // An open item kept with a date in front (see `dated`): the date
            // dropped.
            None => match line.split_once(' ') {
                Some((date, text)) if dated(line) && day_of(date).is_some() => {
                    (text.trim_start(), None)
                }
                _ => (line, None),
            },
        };
        let gone = done.is_some_and(|day| day < today);
        if !gone && !text.is_empty() {
            mem::push(
                &mut items,
                Item {
                    text: copy(text)?,
                    done,
                },
            )?;
        }
    }
    let mut rest = String::new();
    for line in lines {
        rest.try_reserve(line.len().saturating_add(1))
            .map_err(|_| Error::NoMemory)?;
        rest.push_str(line);
        rest.push('\n');
    }
    Ok(List { items, rest })
}

/// Appends `parts` to `out`.
fn put(out: &mut String, parts: &[&str]) -> Result<(), Error> {
    let more = parts.iter().map(|part| part.len()).sum::<usize>();
    out.try_reserve(more).map_err(|_| Error::NoMemory)?;
    for part in parts {
        out.push_str(part);
    }
    Ok(())
}

/// The list as kept: an item a line, those done after `x` and the day, then
/// the lines after the first items as they were.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn format(list: &List) -> Result<String, Error> {
    let mut text = String::new();
    for item in &list.items {
        // Breaks in a text would make new items.
        let mut one = String::new();
        one.try_reserve(item.text.len())
            .map_err(|_| Error::NoMemory)?;
        one.extend(
            item.text
                .chars()
                .map(|c| if matches!(c, '\n' | '\r') { ' ' } else { c }),
        );
        match item.done {
            Some(day) => put(&mut text, &["x ", date_of(day)?.as_str(), " ", &one, "\n"])?,
            None if dated(&one) => put(&mut text, &[MADE, " ", &one, "\n"])?,
            None => put(&mut text, &[&one, "\n"])?,
        }
    }
    put(&mut text, &[&list.rest])?;
    Ok(text)
}

/// The list kept, empty where there is none; [`Loaded::Unreadable`] where it
/// is there but cannot be read (too large, not text), so that it is not
/// written over.
///
/// # Errors
/// The host's refusals, [`Error::NoMemory`].
pub fn load(host: &Host<'_>, today: i64) -> Result<Loaded, Error> {
    let bytes = match files::read_all(host, Root::Data, FILE, READ_LIMIT) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Ok(Loaded::List(List::default())),
        Err(Error::NoSpace) => return Ok(Loaded::Unreadable),
        Err(error) => return Err(error),
    };
    Ok(match read(&bytes, today)? {
        Some(list) => Loaded::List(list),
        None => Loaded::Unreadable,
    })
}

/// The list in `bytes` (a mark of UTF-8 at the start is ignored); `None` for
/// bytes that are no text.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn read(bytes: &[u8], today: i64) -> Result<Option<List>, Error> {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let Ok(text) = core::str::from_utf8(bytes) else {
        return Ok(None);
    };
    parse(text, today).map(Some)
}

/// Writes `list` as the file.
///
/// # Errors
/// The host's refusals, [`Error::NoMemory`]; the old file is intact then.
pub fn save(host: &Host<'_>, list: &List) -> Result<(), Error> {
    files::replace(host, Root::Data, FILE, format(list)?.as_bytes())
}

/// `text` as an item's: one line, not too long.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn cleaned(text: &str) -> Result<String, Error> {
    let text = text.trim();
    let mut out = String::new();
    out.try_reserve(text.len().min(MAX_TEXT.saturating_mul(4)))
        .map_err(|_| Error::NoMemory)?;
    out.extend(
        text.chars()
            .map(|c| {
                if matches!(c, '\n' | '\r' | '\t') {
                    ' '
                } else {
                    c
                }
            })
            .take(MAX_TEXT),
    );
    Ok(out)
}
