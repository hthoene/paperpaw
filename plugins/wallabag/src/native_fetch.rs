//! What the plugin reads of Wallabag's answers, as they stream: the list of
//! unread articles (`entries.json`) and whether one article is archived
//! (`entry.json`). The same rules as the interface-6 readers (`unread` and
//! `is_archived` in `lib.rs`), with fallible allocation: the JSON is read by
//! `paperpaw_sdk::json::stream::Parser::try_feed`/`try_finish`, what the
//! plugin keeps of it is allocated with `try_reserve`, and a refused
//! allocation ends the reading in [`Error::NoMemory`] with nothing returned
//! in part.
//!
//! The caller gives the file in pieces as it reads it (`feed`), then asks
//! for the result (`finish`); [`Unread::read`] and [`Archived::read`] do both
//! for a text already in memory.
use crate::native_wallabag::{MAX_ARTICLES, is_id};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::json::stream::{Event, Parser};
use paperpaw_sdk::native::Error;

/// Pieces a text is fed in by [`Unread::read`] and [`Archived::read`] (as a
/// file is read).
const PIECE: usize = 2048;

/// What a reading takes of the parser's events.
trait Sink: core::fmt::Debug {
    fn event(&mut self, event: Event<'_>);
    /// Why the reading cannot go on, if it cannot.
    fn failed(&self) -> Option<Error>;
}

/// A parser and what takes its events.
#[derive(Debug)]
struct Feed<S: Sink> {
    parser: Parser,
    sink: S,
    /// The parser ran out of memory: it is no more (its state is lost).
    spent: bool,
}

impl<S: Sink> Feed<S> {
    fn new(sink: S) -> Self {
        Self {
            parser: Parser::default(),
            sink,
            spent: false,
        }
    }

    fn feed(&mut self, piece: &[u8]) -> Result<(), Error> {
        self.check()?;
        let Self { parser, sink, .. } = self;
        if parser.try_feed(piece, &mut |e| sink.event(e)).is_err() {
            self.spent = true;
        }
        self.check()
    }

    fn finish(mut self) -> Result<S, Error> {
        self.check()?;
        let Self { parser, sink, .. } = &mut self;
        if parser.try_finish(&mut |e| sink.event(e)).is_err() {
            self.spent = true;
        }
        self.check()?;
        Ok(self.sink)
    }

    fn check(&self) -> Result<(), Error> {
        if self.spent {
            return Err(Error::NoMemory);
        }
        self.sink.failed().map_or(Ok(()), Err)
    }
}

/// An unread article of the list: its id and title.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub title: String,
}

fn copy(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(text.len())
        .map_err(|_| Error::NoMemory)?;
    out.push_str(text);
    Ok(out)
}

const LIST: [&str; 2] = ["_embedded", "items"];
const ITEM: [&str; 3] = ["_embedded", "items", "[]"];

#[derive(Debug, Default)]
struct UnreadSink {
    items: Vec<Entry>,
    /// The list ended: it is a whole list.
    whole: bool,
    /// Items are kept: the one open is among the first [`MAX_ARTICLES`]
    /// with an id.
    keeping: bool,
    failed: Option<Error>,
}

impl UnreadSink {
    fn take(&mut self, event: Event<'_>) -> Result<(), Error> {
        match event {
            Event::Open(path) if path == ITEM => {
                self.keeping = self.items.len() < MAX_ARTICLES;
                if self.keeping {
                    self.items.try_reserve(1).map_err(|_| Error::NoMemory)?;
                    self.items.push(Entry::default());
                }
            }
            Event::Close(path) if path == ITEM => {
                // Ids are numbers in Wallabag; others are left out.
                if self.keeping && self.items.last().is_some_and(|e| !is_id(&e.id)) {
                    self.items.pop();
                }
                self.keeping = false;
            }
            Event::Close(path) if path == LIST => self.whole = true,
            Event::Value(path, value) if self.keeping && path.len() == 4 && path[..3] == ITEM => {
                if let Some(last) = self.items.last_mut() {
                    match path[3].as_str() {
                        "id" => last.id = copy(value)?,
                        "title" => last.title = copy(value)?,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl Sink for UnreadSink {
    fn event(&mut self, event: Event<'_>) {
        if self.failed.is_none()
            && let Err(error) = self.take(event)
        {
            self.failed = Some(error);
        }
    }

    fn failed(&self) -> Option<Error> {
        self.failed
    }
}

/// A reading of the unread articles in the list Wallabag sent
/// (`entries.json`).
#[derive(Debug)]
pub struct Unread(Feed<UnreadSink>);

impl Default for Unread {
    fn default() -> Self {
        Self::new()
    }
}

impl Unread {
    pub fn new() -> Self {
        Self(Feed::new(UnreadSink::default()))
    }

    /// The next piece of the file.
    ///
    /// # Errors
    /// `NoMemory`; the reading is over then.
    pub fn feed(&mut self, piece: &[u8]) -> Result<(), Error> {
        self.0.feed(piece)
    }

    /// The first [`MAX_ARTICLES`] articles with an id (a number), in the
    /// list's order; `None` when the answer is not a whole list (its items
    /// never end).
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn finish(self) -> Result<Option<Vec<Entry>>, Error> {
        let sink = self.0.finish()?;
        Ok(sink.whole.then_some(sink.items))
    }

    /// Reads `text`, the whole file, in pieces as a file is read.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn read(text: &[u8]) -> Result<Option<Vec<Entry>>, Error> {
        let mut reading = Self::new();
        for piece in text.chunks(PIECE) {
            reading.feed(piece)?;
        }
        reading.finish()
    }
}

#[derive(Debug)]
struct ArchivedSink<'a> {
    id: &'a str,
    same: bool,
    archived: Option<bool>,
}

impl Sink for ArchivedSink<'_> {
    fn event(&mut self, event: Event<'_>) {
        if let Event::Value([key], value) = event {
            match key.as_str() {
                "id" => self.same = value == self.id,
                "is_archived" => {
                    self.archived = match value {
                        "1" | "true" => Some(true),
                        "0" | "false" => Some(false),
                        _ => None,
                    }
                }
                _ => {}
            }
        }
    }

    fn failed(&self) -> Option<Error> {
        None
    }
}

/// A reading of whether article `id`, as Wallabag sent it (`entry.json`),
/// is archived.
#[derive(Debug)]
pub struct Archived<'a>(Feed<ArchivedSink<'a>>);

impl<'a> Archived<'a> {
    pub fn new(id: &'a str) -> Self {
        Self(Feed::new(ArchivedSink {
            id,
            same: false,
            archived: None,
        }))
    }

    /// The next piece of the file.
    ///
    /// # Errors
    /// `NoMemory`; the reading is over then.
    pub fn feed(&mut self, piece: &[u8]) -> Result<(), Error> {
        self.0.feed(piece)
    }

    /// Whether the article is archived; `None` when the answer is not that
    /// article, or says nothing of it.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn finish(self) -> Result<Option<bool>, Error> {
        let sink = self.0.finish()?;
        Ok(sink.archived.filter(|_| sink.same))
    }

    /// Reads `text`, the whole file, in pieces as a file is read.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn read(id: &'a str, text: &[u8]) -> Result<Option<bool>, Error> {
        let mut reading = Self::new(id);
        for piece in text.chunks(PIECE) {
            reading.feed(piece)?;
        }
        reading.finish()
    }
}
