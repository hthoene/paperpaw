//! What the plugin keeps in its folder, a line each (see `docs/data-files.md`),
//! read line by line so their length does not matter: the same files and
//! lines as interface 6 (`kept.rs`), over a [`Volume`] (the plugin's data
//! folder) and with every step fallible.
//!
//! - `sent.txt`: the books whose notes are in Joplin: the notes file, the
//!   note's id and the MD5 of its text as written last (`path<TAB>id<TAB>md5`).
//! - `sending.txt`: the notes sent whose answer did not come (yet), as
//!   `sent.txt` has them: Joplin may have them all the same, so the next
//!   sending looks for them there rather than making the note again.
//! - `waiting.txt`: the notes files still to send, at most [`MOST_WAITING`] at
//!   a time (the rest stay in the file and come with the next sending).
//! - `notebook.txt`: the notebook made for the notes: its name and id.
use alloc::{string::String, vec::Vec};
use core::ops::ControlFlow;

use paperpaw_sdk::native::{Error, storage::MAX_DATA};
use paperpaw_ui_kit::{
    files::{Volume, for_each_line_on},
    text,
};

pub const SENT: &str = "sent.txt";
pub const SENDING: &str = "sending.txt";
pub const WAITING: &str = "waiting.txt";
pub const NOTEBOOK: &str = "notebook.txt";
/// Where a list is written anew before it replaces the old.
const NEW: &str = ".new";
/// The most notes files held at a time: the list is read in pieces, so a long
/// one costs a few sendings, not the heap.
pub const MOST_WAITING: usize = 32;

/// A book's note in Joplin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    pub id: String,
    /// The MD5 of its text as written (see `native_note::text_hash`).
    pub text: String,
}

/// The line `fields` joined by tabs and ended by a line break.
fn line_of(fields: &[&str]) -> Result<String, Error> {
    let size = fields.iter().map(|f| f.len() + 1).sum();
    let mut out = String::new();
    out.try_reserve_exact(size).map_err(|_| Error::NoMemory)?;
    for (i, field) in fields.iter().enumerate() {
        if i > 0 {
            out.push('\t');
        }
        out.push_str(field);
    }
    out.push('\n');
    Ok(out)
}

/// The first line of `name` whose first field is `key`: its other fields.
/// `None` where there is no such line or no file.
fn find(v: &dyn Volume, name: &str, key: &str) -> Result<Option<Vec<String>>, Error> {
    let mut found = None;
    let read = for_each_line_on(v, name, &mut |line| {
        let mut fields = line.split('\t');
        if fields.next() != Some(key) {
            return Ok(ControlFlow::Continue(()));
        }
        let mut rest = Vec::new();
        for field in fields {
            rest.try_reserve(1).map_err(|_| Error::NoMemory)?;
            rest.push(text::copy(field)?);
        }
        found = Some(rest);
        Ok(ControlFlow::Break(()))
    });
    match read {
        Ok(()) | Err(Error::NotFound) => Ok(found),
        Err(error) => Err(error),
    }
}

/// Appends `line` in as many writes as the storage takes.
fn append(v: &dyn Volume, name: &str, line: &str) -> Result<(), Error> {
    for part in line.as_bytes().chunks(MAX_DATA) {
        v.append(name, part)?;
    }
    Ok(())
}

/// Writes `name` anew without the lines whose first field is `key`, with
/// `line` (ended by its break) at the end if given: through a copy, line by
/// line. On any error the file is left as it was.
fn rewrite(v: &dyn Volume, name: &str, key: &str, line: Option<&str>) -> Result<(), Error> {
    let copy = text::format(format_args!("{name}{NEW}"))?;
    v.write(&copy, &[])?;
    let mut result = Ok(());
    let read = for_each_line_on(v, name, &mut |old| {
        if old.is_empty() || old.split('\t').next() == Some(key) {
            return Ok(ControlFlow::Continue(()));
        }
        result = text::format(format_args!("{old}\n")).and_then(|l| append(v, &copy, &l));
        Ok(if result.is_ok() {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        })
    });
    match read {
        Ok(()) | Err(Error::NotFound) => {}
        Err(error) => result = Err(error),
    }
    if let Some(line) = line {
        result = result.and_then(|()| append(v, &copy, line));
    }
    match result {
        Ok(()) => v.rename(&copy, name),
        Err(error) => {
            let _ = v.remove(&copy);
            Err(error)
        }
    }
}

fn note(v: &dyn Volume, name: &str, path: &str) -> Result<Option<Sent>, Error> {
    let Some(mut fields) = find(v, name, path)? else {
        return Ok(None);
    };
    if fields.len() != 2 {
        return Ok(None);
    }
    let text = fields.swap_remove(1);
    let id = fields.swap_remove(0);
    Ok(Some(Sent { id, text }))
}

fn keep_note(v: &dyn Volume, name: &str, path: &str, sent: &Sent) -> Result<(), Error> {
    rewrite(
        v,
        name,
        path,
        Some(&line_of(&[path, &sent.id, &sent.text])?),
    )
}

/// The note of the book whose notes are in `path`, as sent.
///
/// # Errors
/// `NoMemory`; what the card says, except that there is no list.
pub fn sent(v: &dyn Volume, path: &str) -> Result<Option<Sent>, Error> {
    note(v, SENT, path)
}

/// Keeps that the note of `path` is in Joplin as `sent`.
///
/// # Errors
/// What the card says; the list is as it was then.
pub fn keep_sent(v: &dyn Volume, path: &str, sent: &Sent) -> Result<(), Error> {
    keep_note(v, SENT, path, sent)
}

/// The note sent for `path` whose answer did not come.
///
/// # Errors
/// As [`sent`].
pub fn sending(v: &dyn Volume, path: &str) -> Result<Option<Sent>, Error> {
    note(v, SENDING, path)
}

/// Keeps that the note of `path` goes to Joplin as `sent`, before it goes.
///
/// # Errors
/// As [`keep_sent`].
pub fn keep_sending(v: &dyn Volume, path: &str, sent: &Sent) -> Result<(), Error> {
    keep_note(v, SENDING, path, sent)
}

/// The answer for the notes file `path` came.
///
/// # Errors
/// As [`keep_sent`].
pub fn remove_sending(v: &dyn Volume, path: &str) -> Result<(), Error> {
    rewrite(v, SENDING, path, None)
}

/// The notes files waiting to be sent, the oldest first, at most
/// [`MOST_WAITING`].
///
/// # Errors
/// `NoMemory`; what the card says, except that there is no list.
pub fn waiting(v: &dyn Volume) -> Result<Vec<String>, Error> {
    let mut paths: Vec<String> = Vec::new();
    let read = for_each_line_on(v, WAITING, &mut |line| {
        if !line.is_empty() && !paths.iter().any(|p| p == line) {
            paths.try_reserve(1).map_err(|_| Error::NoMemory)?;
            paths.push(text::copy(line)?);
        }
        Ok(if paths.len() >= MOST_WAITING {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        })
    });
    match read {
        Ok(()) | Err(Error::NotFound) => Ok(paths),
        Err(error) => Err(error),
    }
}

/// Adds the notes file `path` to those waiting, at the end.
///
/// # Errors
/// As [`keep_sent`].
pub fn add_waiting(v: &dyn Volume, path: &str) -> Result<(), Error> {
    rewrite(v, WAITING, path, Some(&line_of(&[path])?))
}

/// The notes file `path` waits no more.
///
/// # Errors
/// As [`keep_sent`].
pub fn remove_waiting(v: &dyn Volume, path: &str) -> Result<(), Error> {
    rewrite(v, WAITING, path, None)
}

/// The notebook made for the notes: its name and id.
///
/// # Errors
/// `NoMemory`; what the card says, except that there is no file.
pub fn notebook(v: &dyn Volume) -> Result<Option<(String, String)>, Error> {
    let mut kept = None;
    let read = for_each_line_on(v, NOTEBOOK, &mut |line| {
        if let Some((name, id)) = line.split_once('\t') {
            kept = Some((text::copy(name)?, text::copy(id)?));
        }
        Ok(ControlFlow::Break(()))
    });
    match read {
        Ok(()) | Err(Error::NotFound) => Ok(kept),
        Err(error) => Err(error),
    }
}

/// Keeps the notebook `name` made as `id`.
///
/// # Errors
/// What the card says.
pub fn keep_notebook(v: &dyn Volume, name: &str, id: &str) -> Result<(), Error> {
    let line = line_of(&[name, id])?;
    if line.len() > MAX_DATA {
        return Err(Error::NoSpace);
    }
    v.write(NOTEBOOK, line.as_bytes())
}
