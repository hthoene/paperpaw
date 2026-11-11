//! Plugins another plugin asks for (slice 5, host-free): the open request kept
//! on the card (`request.txt`: who asked, then the ids; `request-result.txt`:
//! `+id` installed, `-id` not, one a line as each is done), what the store takes
//! of an ask, whether an ask is the same as one just shown, and what became of
//! it. The interface-6 rules of `request.rs` and the same files (a Beta 3
//! request is read as it is), as a model: the dialog, the event that asks
//! (`event.plugins_requested`, 77), the answer (78) and the fetch are the
//! plugin's and come with their host pieces (`PORTING-7.md`).
//!
//! Who asks is what a plugin names itself: nothing here trusts it more than
//! that. At most [`MAX_WANTED`] ids, each once, none already on the device, and
//! the dialog names the plugins by the store's own listing ([`entries`]), never
//! by what the request says.
use crate::native_index::Index;
use crate::native_model::Entry;
use crate::native_queue::{join_names, read_text};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::Storage;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{mem::push, text::concat, text::copy};

/// The open request, and what has become of its plugins.
pub const REQUEST: &str = "request.txt";
pub const RESULT: &str = "request-result.txt";
const MAX_FILE: usize = 4 * 1024;
/// Ids a request names at most (the kernel's own cap too, which a plugin that
/// sends the event itself skips).
pub const MAX_WANTED: usize = paperpaw_sdk::MAX_REQUESTED_PLUGINS;
/// How long a request the reader declined is not asked again (the one who
/// asked is told *Not now* at once meanwhile), in milliseconds.
pub const DECLINED_MS: u64 = 60_000;

/// The ids asked for as the store takes them: each once, none on the device
/// already, at most [`MAX_WANTED`]; at most four times as many are looked at.
///
/// # Errors
/// `NoMemory`.
pub fn wanted<'a>(
    ids: impl IntoIterator<Item = &'a str>,
    on_device: &dyn Fn(&str) -> bool,
) -> Result<Vec<String>, Error> {
    let mut wanted: Vec<String> = Vec::new();
    for id in ids.into_iter().take(MAX_WANTED.saturating_mul(4)) {
        if wanted.len() >= MAX_WANTED {
            break;
        }
        if !on_device(id) && !wanted.iter().any(|w| w == id) {
            push(&mut wanted, copy(id)?)?;
        }
    }
    Ok(wanted)
}

/// A request: who asked (a plugin's id) and the ids wanted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Request {
    pub by: String,
    pub ids: Vec<String>,
}

impl Request {
    /// The request open, if there is one that reads.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn open(storage: &dyn Storage) -> Result<Option<Request>, Error> {
        let Some(text) = read_text(storage, REQUEST, MAX_FILE)? else {
            return Ok(None);
        };
        let mut lines = text.lines();
        let Some(by) = lines.next() else {
            return Ok(None);
        };
        // A refused allocation is not "no request".
        match paperpaw_sdk::Id::try_parse(by) {
            Ok(_) => {}
            Err(paperpaw_sdk::IdError::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(None),
        }
        let mut ids = Vec::new();
        for id in lines.take(MAX_WANTED) {
            push(&mut ids, copy(id)?)?;
        }
        Ok(Some(Request { by: copy(by)?, ids }))
    }

    /// Keeps a request open: `by` asked for `ids`. A request not kept is not
    /// answered when its dialog is, so the caller answers at once.
    ///
    /// # Errors
    /// `NoMemory`; `Io` where the card refuses.
    pub fn start(storage: &dyn Storage, by: &str, ids: &[String]) -> Result<(), Error> {
        let mut text = String::new();
        let more = ids.iter().map(|id| id.len() + 1).sum::<usize>() + by.len() + 1;
        text.try_reserve(more).map_err(|_| Error::NoMemory)?;
        text.push_str(by);
        text.push('\n');
        for id in ids {
            text.push_str(id);
            text.push('\n');
        }
        storage
            .write(REQUEST, text.as_bytes())
            .map_err(|_| Error::Io)
    }

    /// Fetches `ids` again for `by` after a download that did not end well (the
    /// screen's *Try again*): what was noted goes, the request is kept anew.
    ///
    /// # Errors
    /// As [`start`](Self::start).
    pub fn again(storage: &dyn Storage, by: &str, ids: &[String]) -> Result<(), Error> {
        match storage.remove(RESULT) {
            Ok(()) | Err(_) => {}
        }
        Self::start(storage, by, ids)
    }

    /// Closes the request: what is kept of it goes.
    pub fn close(storage: &dyn Storage) {
        for name in [REQUEST, RESULT] {
            match storage.remove(name) {
                Ok(()) | Err(_) => {}
            }
        }
    }
}

/// Notes what became of plugin `id`, which a request asked for: installed
/// (`done`) or not. One not kept counts as not installed: the one who asked is
/// told so, and asks again later.
///
/// # Errors
/// `NoMemory`; `Io` where the card refuses.
pub fn outcome(storage: &dyn Storage, id: &str, done: bool) -> Result<(), Error> {
    let line = concat(&[if done { "+" } else { "-" }, id, "\n"])?;
    storage
        .append(RESULT, line.as_bytes())
        .map_err(|_| Error::Io)
}

/// What became of a request whose queue is empty: every plugin asked for is
/// installed or not.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Finished {
    pub by: String,
    pub installed: Vec<String>,
    pub missing: Vec<String>,
}

/// The queue is empty: what became of the request open (it is closed), or
/// `None` where none is open.
///
/// # Errors
/// `NoMemory`.
pub fn finished(storage: &dyn Storage) -> Result<Option<Finished>, Error> {
    let Some(request) = Request::open(storage)? else {
        return Ok(None);
    };
    let results = read_text(storage, RESULT, MAX_FILE)?.unwrap_or_default();
    let mut installed = Vec::new();
    let mut missing = Vec::new();
    for id in &request.ids {
        let done = results
            .lines()
            .any(|l| l.strip_prefix('+') == Some(id.as_str()));
        push(if done { &mut installed } else { &mut missing }, copy(id)?)?;
    }
    Request::close(storage);
    Ok(Some(Finished {
        by: request.by,
        installed,
        missing,
    }))
}

/// The plugins `ids` as the library lists them (with their names and sizes),
/// each with its entry where the index has it, in the order of `ids`.
///
/// # Errors
/// `NoMemory`.
pub fn entries(
    storage: &dyn Storage,
    ids: &[String],
) -> Result<Vec<(String, Option<Entry>)>, Error> {
    let mut found: Vec<Entry> = Vec::new();
    if let Some(index) = Index::kept(storage)? {
        index.scan(storage, &mut |_, entry| {
            if ids.contains(&entry.id) {
                push(&mut found, entry)?;
            }
            Ok(())
        })?;
    }
    let mut list = Vec::new();
    for id in ids {
        let entry = found
            .iter()
            .position(|entry| entry.id == *id)
            .map(|at| found.swap_remove(at));
        push(&mut list, (copy(id)?, entry))?;
    }
    Ok(list)
}

/// The plugins' names (the library's, else their ids), joined for a notice.
///
/// # Errors
/// `NoMemory`.
pub fn names(list: &[(String, Option<Entry>)]) -> Result<String, Error> {
    join_names(
        list.iter()
            .map(|(id, entry)| entry.as_ref().map_or(id.as_str(), |e| e.name.as_str())),
        ", ",
    )
}

/// The last request shown: who asked, for what, when, and whether the reader
/// declined it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Last {
    pub by: String,
    pub ids: Vec<String>,
    pub at_ms: u64,
    pub declined: bool,
}

/// What the store does with an ask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Verdict {
    /// Everything asked for is on the device: tell the asker nothing is
    /// missing.
    NothingMissing,
    /// The same ask the reader declined a moment ago: *Not now* at once, no new
    /// dialog.
    DeclinedAgain,
    /// The same ask whose dialog is still open or download going on: it is the
    /// one, answered once.
    SameOpen,
    /// Another request is under way: *Not now* for this one.
    Busy,
    /// Show the dialog; a request left by a dialog closed without an answer is
    /// closed first (`close_stale`).
    Ask { close_stale: bool },
}

/// Decides an ask by `by` for `wanted` (as [`wanted`] took it) at `now_ms`,
/// given the last request shown, whether one is open and whether plugins
/// asked for are waiting in the queue.
#[must_use]
pub fn decide(
    by: &str,
    wanted: &[String],
    last: Option<&Last>,
    now_ms: u64,
    open: bool,
    requested_waiting: bool,
) -> Verdict {
    if wanted.is_empty() {
        return Verdict::NothingMissing;
    }
    let same = last.and_then(|l| {
        (l.by == by && l.ids == wanted && now_ms.saturating_sub(l.at_ms) < DECLINED_MS)
            .then_some(l.declined)
    });
    match same {
        Some(true) => return Verdict::DeclinedAgain,
        Some(false) if open => return Verdict::SameOpen,
        _ => {}
    }
    if open {
        if requested_waiting {
            return Verdict::Busy;
        }
        return Verdict::Ask { close_stale: true };
    }
    Verdict::Ask { close_stale: false }
}
