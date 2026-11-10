//! The store's queue of plugins to fetch, kept on the card (slice 5,
//! host-free): *Update all*, the plugins another plugin asked for and the
//! first-run setup's plugins are lines of `queue.txt`, the one whose turn it
//! is first and marked, so that one cut off by a restart is begun anew. The
//! interface-6 rules of `queue.rs` with the same files and format (a Beta 3
//! queue is read as it is), as a model: what to fetch next is the answer of
//! [`Queue::advance`]; asking the network, the install and the notices are the
//! plugin's and come with their host pieces (`PORTING-7.md`).
//!
//! Made fallible: memory refused is `NoMemory`; a queue that cannot be read is
//! an empty one (as before: the queue is advice, rebuilt by the next *Update
//! all*); nothing indexes text in a way that can panic.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::Storage;
use paperpaw_sdk::install::is_plugin_name;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy},
};

/// The queue and what became of the plugins put in quietly.
pub const QUEUE: &str = "queue.txt";
pub const QUIET: &str = "queue-quiet.txt";
/// The mark of the plugin whose turn it is, and of the two kinds put in
/// without a notice each.
const TURN: &str = "> ";
const QUIET_MARK: &str = "~";
const REQUESTED_MARK: &str = "&";
/// The most of either file read: more is no queue of ours.
pub const MAX_FILE: usize = 16 * 1024;
/// Plugins in the queue at most (a line each, so a file of the cap holds more
/// than anyone asks for).
pub const MAX_LINES: usize = 512;

/// The text of the file `name`, at most `cap` bytes: `None` where there is no
/// file, it is over `cap`, cannot be read or is not text.
///
/// # Errors
/// `NoMemory`.
pub fn read_text(storage: &dyn Storage, name: &str, cap: usize) -> Result<Option<String>, Error> {
    let Ok(size) = storage.size(name) else {
        return Ok(None);
    };
    let Ok(len) = usize::try_from(size) else {
        return Ok(None);
    };
    if len > cap {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
    bytes.resize(len, 0);
    let mut read = 0;
    while read < len {
        let Some(part) = bytes.get_mut(read..) else {
            return Ok(None);
        };
        match storage.read_at(name, read as u64, part) {
            Ok(0) | Err(_) => return Ok(None),
            Ok(n) => read += n,
        }
    }
    Ok(String::from_utf8(bytes).ok())
}

/// Joins `parts` with `, ` (or any separator), asking for the room first.
///
/// # Errors
/// `NoMemory`.
pub fn join_names<'a>(
    parts: impl IntoIterator<Item = &'a str>,
    separator: &str,
) -> Result<String, Error> {
    let mut text = String::new();
    for (n, part) in parts.into_iter().enumerate() {
        let more = part.len() + if n == 0 { 0 } else { separator.len() };
        text.try_reserve(more).map_err(|_| Error::NoMemory)?;
        if n > 0 {
            text.push_str(separator);
        }
        text.push_str(part);
    }
    Ok(text)
}

/// Why a plugin is in the queue.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Mark {
    /// *Update all*: said as it comes.
    #[default]
    Said,
    /// The first-run setup's: not said one by one but once, at the end.
    Quiet,
    /// Another plugin asked for it (see `native_request`): fetched quietly too,
    /// and told to the one who asked.
    Requested,
}

impl Mark {
    const fn prefix(self) -> &'static str {
        match self {
            Mark::Said => "",
            Mark::Quiet => QUIET_MARK,
            Mark::Requested => REQUESTED_MARK,
        }
    }

    /// Put in without a notice of its own.
    #[must_use]
    pub const fn is_quiet(self) -> bool {
        !matches!(self, Mark::Said)
    }
}

/// A plugin of the queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub id: String,
    pub mark: Mark,
    /// Its turn is on: its page is fetched or its download goes.
    pub turn: bool,
}

impl Line {
    fn parse(text: &str) -> Result<Self, Error> {
        let (turn, rest) = match text.strip_prefix(TURN) {
            Some(rest) => (true, rest),
            None => (false, text),
        };
        let (mark, id) = if let Some(id) = rest.strip_prefix(QUIET_MARK) {
            (Mark::Quiet, id)
        } else if let Some(id) = rest.strip_prefix(REQUESTED_MARK) {
            (Mark::Requested, id)
        } else {
            (Mark::Said, rest)
        };
        Ok(Self {
            id: copy(id)?,
            mark,
            turn,
        })
    }

    fn text(&self) -> Result<String, Error> {
        concat(&[
            if self.turn { TURN } else { "" },
            self.mark.prefix(),
            &self.id,
        ])
    }
}

/// The queue: the plugins to fetch in turn, the one whose turn it is first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Queue {
    lines: Vec<Line>,
}

impl Queue {
    /// The queue kept on the card; an empty one where there is none or it
    /// cannot be read.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn read(storage: &dyn Storage) -> Result<Self, Error> {
        let mut lines = Vec::new();
        if let Some(text) = read_text(storage, QUEUE, MAX_FILE)? {
            for line in text.lines().take(MAX_LINES) {
                push(&mut lines, Line::parse(line)?)?;
            }
        }
        Ok(Self { lines })
    }

    /// Keeps the queue on the card; an empty queue is no file.
    ///
    /// # Errors
    /// `NoMemory`; `Io` where the card refuses.
    pub fn keep(&self, storage: &dyn Storage) -> Result<(), Error> {
        if self.lines.is_empty() {
            // No file is as good as a removed one.
            return match storage.remove(QUEUE) {
                Ok(()) | Err(paperpaw_sdk::StorageError::NotFound) => Ok(()),
                Err(_) => Err(Error::Io),
            };
        }
        let mut text = String::new();
        for line in &self.lines {
            let one = line.text()?;
            text.try_reserve(one.len() + 1)
                .map_err(|_| Error::NoMemory)?;
            text.push_str(&one);
            text.push('\n');
        }
        storage.write(QUEUE, text.as_bytes()).map_err(|_| Error::Io)
    }

    /// The plugins in it, in order.
    #[must_use]
    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    /// Whether the plugin whose turn it is was asked for by a plugin.
    #[must_use]
    pub fn requested(&self) -> bool {
        self.lines
            .first()
            .is_some_and(|l| l.turn && l.mark == Mark::Requested)
    }

    /// Whether plugins asked for by a plugin are in the queue still.
    #[must_use]
    pub fn requested_waiting(&self) -> bool {
        self.lines.iter().any(|l| l.mark == Mark::Requested)
    }

    /// Whether the plugin whose turn it is was put in quietly (what becomes of
    /// it is kept with [`note`], not said).
    #[must_use]
    pub fn quiet(&self) -> bool {
        self.lines
            .first()
            .is_some_and(|l| l.turn && l.mark.is_quiet())
    }

    /// Whether plugins put in quietly wait: they go on at the start.
    #[must_use]
    pub fn has_quiet(&self) -> bool {
        self.lines.iter().any(|l| l.mark.is_quiet())
    }

    /// Puts `ids` behind those waiting, each once, with `mark`.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn add<'a>(
        &mut self,
        ids: impl IntoIterator<Item = &'a str>,
        mark: Mark,
    ) -> Result<(), Error> {
        for id in ids {
            if self.lines.iter().any(|l| l.id == id) || self.lines.len() >= MAX_LINES {
                continue;
            }
            push(
                &mut self.lines,
                Line {
                    id: copy(id)?,
                    mark,
                    turn: false,
                },
            )?;
        }
        Ok(())
    }

    /// A download cut off by a restart: its turn again, first, when the queue
    /// goes on.
    pub fn interrupted(&mut self) {
        if let Some(first) = self.lines.first_mut() {
            first.turn = false;
        }
    }

    /// Gives the next plugin its turn: the one before is done, stopped or set
    /// aside, so it goes; what is no plugin's name goes too. The id whose turn
    /// it is now, or `None` once the queue is empty.
    pub fn advance(&mut self) -> Option<&str> {
        if self.lines.first().is_some_and(|l| l.turn) {
            self.lines.remove(0);
        }
        self.lines.retain(|l| is_plugin_name(&l.id));
        let first = self.lines.first_mut()?;
        first.turn = true;
        Some(first.id.as_str())
    }
}

/// Keeps that the quiet plugin `name` was installed (`done`) or not.
///
/// # Errors
/// `NoMemory`; `Io` where the card refuses.
pub fn note(storage: &dyn Storage, name: &str, done: bool) -> Result<(), Error> {
    let mut line = String::new();
    line.try_reserve(name.len() + 2)
        .map_err(|_| Error::NoMemory)?;
    line.push(if done { '+' } else { '-' });
    line.extend(name.chars().map(|c| if c == '\n' { ' ' } else { c }));
    line.push('\n');
    storage
        .append(QUIET, line.as_bytes())
        .map_err(|_| Error::Io)
}

/// What the plugins put in quietly became, as the names of those installed
/// and of those not.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub done: String,
    pub not: String,
}

/// Once no quiet plugin waits any more, what they became (and the notes go):
/// `None` while one waits or where nothing was noted.
///
/// # Errors
/// `NoMemory`.
pub fn report(storage: &dyn Storage, queue: &Queue) -> Result<Option<Report>, Error> {
    if queue.has_quiet() {
        return Ok(None);
    }
    let Some(text) = read_text(storage, QUIET, MAX_FILE)? else {
        return Ok(None);
    };
    match storage.remove(QUIET) {
        Ok(()) | Err(_) => {}
    }
    let names = |mark: char| join_names(text.lines().filter_map(|l| l.strip_prefix(mark)), ", ");
    Ok(Some(Report {
        done: names('+')?,
        not: names('-')?,
    }))
}
