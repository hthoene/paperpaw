//! What the plugin reads of Reader's answers, as they stream: the list of
//! articles (`list.json`) and the answer to a list asking for one article
//! (`doc.json`: is it still there, where is it, who wrote it). The same rules
//! as the interface-6 readers (`answers.rs`), with fallible allocation: the
//! JSON is read by `paperpaw_sdk::json::stream::Parser::try_feed` and
//! `try_finish`, what the plugin keeps of it is allocated with `try_reserve`,
//! and a refused allocation ends the reading in [`Error::NoMemory`] with
//! nothing returned in part.
//!
//! The caller gives the file in pieces as it reads it (`feed`), then asks
//! for the result (`finish`); [`Listed::read`] and [`Asked::read`] do both
//! for a text already in memory. Like interface 6's, these answers are the
//! list's or one article's data: at most [`MAX_LIST`] bytes (the caller asks
//! [`fits`] with the file's size first, and `feed` refuses with `NoSpace` the
//! bytes over it). After an error the reading is over: every later `feed`
//! says the same.
use crate::native_read_later::{MAX_LIST, fits, is_id};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::json::stream::{Event, Parser};
use paperpaw_sdk::native::Error;

/// Pieces a text is fed in by `read` (as a file is read).
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
    fed: usize,
    /// The reading is over: memory was refused or the answer is too long.
    spent: Option<Error>,
}

impl<S: Sink> Feed<S> {
    fn new(sink: S) -> Self {
        Self {
            parser: Parser::default(),
            sink,
            fed: 0,
            spent: None,
        }
    }

    fn feed(&mut self, piece: &[u8]) -> Result<(), Error> {
        self.check()?;
        self.fed = self.fed.saturating_add(piece.len());
        if self.fed > MAX_LIST {
            self.spent = Some(Error::NoSpace);
            return self.check();
        }
        let Self { parser, sink, .. } = self;
        if parser.try_feed(piece, &mut |e| sink.event(e)).is_err() {
            self.spent = Some(Error::NoMemory);
        }
        self.check()
    }

    fn finish(mut self) -> Result<S, Error> {
        self.check()?;
        let Self { parser, sink, .. } = &mut self;
        if parser.try_finish(&mut |e| sink.event(e)).is_err() {
            self.spent = Some(Error::NoMemory);
        }
        self.check()?;
        Ok(self.sink)
    }

    fn check(&self) -> Result<(), Error> {
        if let Some(error) = self.spent {
            return Err(error);
        }
        self.sink.failed().map_or(Ok(()), Err)
    }
}

/// The most bytes kept of a title, a location or an author (a new cap, at a
/// character's end: interface 6 kept them whole up to the parser's 4 KiB,
/// which 20 hostile titles would make 82 KiB).
pub const FIELD_CAP: usize = 256;

/// An article of the list: its id and title.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub title: String,
}

/// `text` cut to at most [`FIELD_CAP`] bytes, at a character's end, kept
/// fallibly.
fn copy_cut(text: &str) -> Result<String, Error> {
    let mut end = text.len().min(FIELD_CAP);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    copy(&text[..end])
}

fn copy(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(text.len())
        .map_err(|_| Error::NoMemory)?;
    out.push_str(text);
    Ok(out)
}

const LIST: [&str; 1] = ["results"];
const ITEM: [&str; 2] = ["results", "[]"];

#[derive(Debug)]
struct ListedSink {
    max: usize,
    items: Vec<Entry>,
    /// The list ended: it is a whole list.
    whole: bool,
    /// The item open is kept: it is among the first `max` with an id.
    keeping: bool,
    failed: Option<Error>,
}

impl ListedSink {
    fn take(&mut self, event: Event<'_>) -> Result<(), Error> {
        match event {
            Event::Open(path) if path == ITEM => {
                self.keeping = self.items.len() < self.max;
                if self.keeping {
                    self.items.try_reserve(1).map_err(|_| Error::NoMemory)?;
                    self.items.push(Entry::default());
                }
            }
            Event::Close(path) if path == ITEM => {
                // Ids go into addresses, lists and names of files: others
                // are left out.
                if self.keeping && self.items.last().is_some_and(|e| !is_id(&e.id)) {
                    self.items.pop();
                }
                self.keeping = false;
            }
            Event::Close(path) if path == LIST => self.whole = true,
            Event::Value(path, value) if self.keeping && path.len() == 3 && path[..2] == ITEM => {
                if let Some(last) = self.items.last_mut() {
                    match path[2].as_str() {
                        "id" => last.id = copy(value)?,
                        "title" => last.title = copy_cut(value)?,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl Sink for ListedSink {
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

/// A reading of the articles in the list Reader sent (`list.json`).
#[derive(Debug)]
pub struct Listed(Feed<ListedSink>);

impl Listed {
    /// A reading that keeps the first `max` articles with an id.
    pub fn new(max: usize) -> Self {
        Self(Feed::new(ListedSink {
            max,
            items: Vec::new(),
            whole: false,
            keeping: false,
            failed: None,
        }))
    }

    /// The next piece of the file.
    ///
    /// # Errors
    /// `NoMemory`, `NoSpace` over [`MAX_LIST`] bytes; the reading is over
    /// then.
    pub fn feed(&mut self, piece: &[u8]) -> Result<(), Error> {
        self.0.feed(piece)
    }

    /// The articles, in the list's order; `None` when the answer is not a
    /// whole list (its results never end).
    ///
    /// # Errors
    /// `NoMemory`, or the error of an earlier `feed`.
    pub fn finish(self) -> Result<Option<Vec<Entry>>, Error> {
        let sink = self.0.finish()?;
        Ok(sink.whole.then_some(sink.items))
    }

    /// Reads `text`, the whole file, in pieces as a file is read.
    ///
    /// # Errors
    /// `NoSpace` when `text` is over [`MAX_LIST`] bytes; `NoMemory`.
    pub fn read(max: usize, text: &[u8]) -> Result<Option<Vec<Entry>>, Error> {
        fits(text.len() as u64, MAX_LIST)?;
        let mut reading = Self::new(max);
        for piece in text.chunks(PIECE) {
            reading.feed(piece)?;
        }
        reading.finish()
    }
}

/// An article asked about by its id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// Reader does not know it any more.
    Gone,
    /// Where it is (`new`, `later`, `archive` …), and who wrote it.
    Known { location: String, author: String },
}

#[derive(Debug)]
struct AskedSink<'a> {
    id: &'a str,
    whole: bool,
    count: u32,
    same: bool,
    location: String,
    author: String,
    failed: Option<Error>,
}

impl AskedSink<'_> {
    fn take(&mut self, event: Event<'_>) -> Result<(), Error> {
        match event {
            Event::Open(path) if path == ITEM => self.count = self.count.saturating_add(1),
            Event::Close(path) if path == LIST => self.whole = true,
            Event::Value(path, value)
                if path.len() == 3 && path[..2] == ITEM && self.count == 1 =>
            {
                match path[2].as_str() {
                    "id" => self.same = value == self.id,
                    "location" => self.location = copy_cut(value)?,
                    "author" => self.author = copy_cut(value)?,
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl Sink for AskedSink<'_> {
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

/// A reading of the answer to a list asking for article `id`
/// (`doc.json`).
#[derive(Debug)]
pub struct Asked<'a>(Feed<AskedSink<'a>>);

impl<'a> Asked<'a> {
    pub fn new(id: &'a str) -> Self {
        Self(Feed::new(AskedSink {
            id,
            whole: false,
            count: 0,
            same: false,
            location: String::new(),
            author: String::new(),
            failed: None,
        }))
    }

    /// The next piece of the file.
    ///
    /// # Errors
    /// `NoMemory`, `NoSpace` over [`MAX_LIST`] bytes; the reading is over
    /// then.
    pub fn feed(&mut self, piece: &[u8]) -> Result<(), Error> {
        self.0.feed(piece)
    }

    /// What the answer says; `None` when it is not a whole answer, or about
    /// another article.
    ///
    /// # Errors
    /// `NoMemory`, or the error of an earlier `feed`.
    pub fn finish(self) -> Result<Option<Answer>, Error> {
        let sink = self.0.finish()?;
        Ok(match (sink.whole, sink.count) {
            (true, 0) => Some(Answer::Gone),
            (true, _) if sink.same => Some(Answer::Known {
                location: sink.location,
                author: sink.author,
            }),
            _ => None,
        })
    }

    /// Reads `text`, the whole file, in pieces as a file is read.
    ///
    /// # Errors
    /// `NoSpace` when `text` is over [`MAX_LIST`] bytes; `NoMemory`.
    pub fn read(id: &'a str, text: &[u8]) -> Result<Option<Answer>, Error> {
        fits(text.len() as u64, MAX_LIST)?;
        let mut reading = Self::new(id);
        for piece in text.chunks(PIECE) {
            reading.feed(piece)?;
        }
        reading.finish()
    }
}
