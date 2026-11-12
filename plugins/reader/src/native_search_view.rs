//! The search screen (`reader.search-results`): the hits of a text looked for
//! in the open book, in the order of the book, each as its snippet and how far
//! into the book it is; a choice goes to the reading screen's place and closes
//! the screen. The model (the text looked for, the snippets, the hits kept, the
//! scan) is `native_search`; this is the part that meets the host.
//!
//! * **The text** is asked by the reading screen with a text question (the
//!   keyboard answers it); the answer reaches this screen as a hand-over
//!   param (so each text has an instance of its own) and the text itself is
//!   handed over by the plugin ([`SearchView::open`]).
//! * **The scan** reads [`BLOCKS_PER_STEP`](crate::native_search::BLOCKS_PER_STEP)
//!   blocks of the reading screen's document each time its work (`core.work`,
//!   key `search`) is due, and asks again until the book is read. The screen
//!   draws what it has.
//! * **No match mark:** choosing a hit goes to the place where the match
//!   begins; the page shows no mark on it (`PORTING-7.md`).
use crate::{
    native_document::HostDocument,
    native_open::Place,
    native_place::Progress,
    native_search::{Blocks, MAX_HITS, Measure, Note, Search},
    native_view::{Handover, read_handover},
};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::{
    Handled,
    document::{Block, Locator},
    native::{
        Error, Host,
        callbacks::{DrawFrame, Input},
        document::{DocId, Documents},
        fonts::{Font, FontRole},
        formatting::Arg,
        snapshot,
        ui::{HandoverId, Ui},
    },
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Style,
    mem::{SERVICE_BUFFER, buffer},
    text,
};

/// The room a row's snippet has where the font or the display cannot be asked
/// (pixels): a narrow screen's.
const ROW_ROOM: f32 = 300.0;
/// Steps that fail with the same error in a row before the search is ended.
const FAILS_ALLOWED: u8 = 1;

/// What a choice on the screen meant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing the screen handles.
    Ignored,
    /// Handled; the screen stays.
    Handled,
    /// Go to where a match begins and close the screen; the match ends at this
    /// offset of the same section, and the page marks it.
    Go(Place, u32),
}

/// What a step of the scan asks of the plugin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stepped {
    /// The page shown changed: draw it.
    pub redraw: bool,
    /// The book is not read to its end: step again.
    pub again: bool,
}

/// One open search.
#[derive(Debug)]
pub struct SearchView {
    /// The book's document: the reading screen's (the X4 holds one open
    /// document per plugin, so none is opened here).
    document: Option<DocId>,
    sections: u32,
    progress: Option<Progress>,
    search: Option<Search>,
    list: ItemList,
    /// Where each row of the list leads (its hit's start, and the offset where
    /// it ends in the same section): the snippets are in the rows only, not in
    /// the hits as well.
    places: Vec<(Place, u32)>,
    /// The size of the text and the room of a snippet in a row, read from the
    /// display once.
    metrics: Option<(f32, f32)>,
    /// The text looked for, for the note of a search that found nothing.
    query: String,
    /// Why there is nothing to list (the key of the text).
    message: Option<&'static str>,
    /// The scan stopped on an error: the key of the note.
    failed: Option<Error>,
    /// The error the last step ended in, and how often in a row.
    last: Option<(Error, u8)>,
}

/// The book as the scan reads it, over the document service.
struct HostBlocks<'a, 'h> {
    document: HostDocument<'h>,
    progress: Option<&'a Progress>,
    sections: u32,
}

impl Blocks for HostBlocks<'_, '_> {
    type Block = Block;

    fn sections(&self) -> u32 {
        self.sections
    }

    fn block_at(&mut self, at: Place) -> Result<Option<Block>, Error> {
        match self.document.read_at(Locator::new(at.section, at.offset)) {
            Ok(block) => Ok(Some(block)),
            Err(Error::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn text<'b>(&self, block: &'b Block) -> &'b str {
        &block.text
    }

    fn end(&self, block: &Block) -> u32 {
        block.end
    }

    fn source_offset(&self, block: &Block, byte: usize) -> u32 {
        block.source_offset(byte)
    }

    fn percent(&self, at: Place) -> u8 {
        percent_of(self.progress, self.sections, at)
    }
}

/// How far into the book `at` is, in percent: by the sections' lengths where
/// they are known, else by the section.
fn percent_of(progress: Option<&Progress>, sections: u32, at: Place) -> u8 {
    let percent = match progress {
        Some(progress) => progress.ppm(at.section, at.offset) / 10_000,
        None => at.section.saturating_mul(100) / sections.max(1),
    };
    u8::try_from(percent.min(100)).unwrap_or(100)
}

/// The font as the scan measures a snippet with.
struct Ruler<'f, 'h> {
    font: Option<&'f Font<'h>>,
    size: f32,
}

impl Ruler<'_, '_> {
    fn width_of(&self, text: &str) -> f32 {
        // Where the font cannot say, half the size is a letter's width.
        let guess = self.size * 0.5 * text.chars().count() as f32;
        self.font
            .and_then(|font| font.measure_text(self.size, text).ok())
            .unwrap_or(guess)
    }
}

impl Measure for Ruler<'_, '_> {
    fn advance(&mut self, c: char) -> f32 {
        self.width_of(c.encode_utf8(&mut [0; 4]))
    }

    fn ellipsis(&mut self) -> f32 {
        self.width_of("…")
    }
}

impl SearchView {
    fn none(message: &'static str) -> Self {
        Self {
            document: None,
            sections: 1,
            progress: None,
            search: None,
            list: ItemList::default(),
            places: Vec::new(),
            metrics: None,
            query: String::new(),
            message: Some(message),
            failed: None,
            last: None,
        }
    }

    /// The search for `query` in the book the hand-over names, which is the
    /// reading screen's document `shared`; the scan starts when its work is
    /// first due.
    ///
    /// # Errors
    /// `NoMemory`; the book's own refusals are a screen that says so.
    pub fn open(
        host: &Host<'_>,
        ui: &Ui,
        docs: &Documents,
        (handover, shared): (Option<HandoverId>, Option<DocId>),
        query: &str,
    ) -> Result<Self, Error> {
        let found = match handover {
            Some(handover) => read_handover(host, ui, handover).unwrap_or_default(),
            None => Handover::default(),
        };
        let (Some(id), false) = (shared, found.path.is_empty()) else {
            return Ok(Self::none("reader.no-book"));
        };
        let (mut input, mut out) = (buffer(256)?, buffer(SERVICE_BUFFER)?);
        let sections = match docs.metadata(host, id, &mut input, &mut out) {
            Ok(meta) => meta.sections().max(1),
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => return Ok(Self::none("reader.error")),
        };
        let progress = Progress::read(host, docs, id, sections, (&mut input, &mut out))?;
        let mut view = Self::none("reader.search.wait");
        view.document = Some(id);
        view.sections = sections;
        view.progress = progress;
        view.query = text::copy(query.trim())?;
        view.search = Some(Search::new(query, true)?);
        view.message = None;
        Ok(view)
    }

    /// Whether the book is still being read: its work is to be declared.
    #[must_use]
    pub fn searching(&self) -> bool {
        self.search.as_ref().is_some_and(Search::searching)
    }

    /// Reads the next blocks of the book and shows the hits they gave.
    ///
    /// # Errors
    /// `NoMemory` for the list of the hits; a refusal of the book ends the
    /// search with a note after a repeat, and is not an error.
    pub fn step(&mut self, host: &Host<'_>, docs: &Documents) -> Result<Stepped, Error> {
        let (Some(id), Some(search)) = (self.document, &self.search) else {
            return Ok(Stepped {
                redraw: false,
                again: false,
            });
        };
        let (before, shown) = (search.hits().len(), search.note());
        let (result, percent) = match self.scan(host, docs, id) {
            Ok(pair) => pair,
            Err(error) => (Err(error), 0),
        };
        let mut redraw = false;
        let mut again = false;
        if let Some(search) = self.search.as_mut() {
            let grew = search.hits().len() != before;
            redraw = grew || search.progressed(percent) || search.note() != shown;
            again = search.searching();
            match result {
                Ok(()) => self.last = None,
                Err(error) => {
                    let repeats = match self.last {
                        Some((last, n)) if last == error => n.saturating_add(1),
                        _ => 1,
                    };
                    self.last = Some((error, repeats));
                    if repeats > FAILS_ALLOWED {
                        // The same refusal twice: the book is not read to its end.
                        self.search = None;
                        self.failed = Some(error);
                        redraw = true;
                        again = false;
                    }
                }
            }
        }
        // The rows of the hits found; a list that cannot grow ends the search.
        if let Err(error) = self.extend_list() {
            self.fail(error);
            redraw = true;
            again = false;
        }
        Ok(Stepped { redraw, again })
    }

    /// Ends the search on `error`: the hits so far stay, the note says it
    /// stopped.
    pub fn fail(&mut self, error: Error) {
        self.search = None;
        self.failed = Some(error);
    }

    /// One step of the scan over the document `id`, and how far it has read.
    fn scan(
        &mut self,
        host: &Host<'_>,
        docs: &Documents,
        id: DocId,
    ) -> Result<(Result<(), Error>, u8), Error> {
        let (size, room) = match self.metrics {
            Some(metrics) => metrics,
            None => {
                let metrics = Display::read(host).map_or((16.0, ROW_ROOM), |d| (d.size, d.room()));
                self.metrics = Some(metrics);
                metrics
            }
        };
        let Some(search) = self.search.as_mut() else {
            return Ok((Ok(()), 100));
        };
        let font = Font::acquire_role(host, FontRole::Ui).ok();
        let mut ruler = Ruler {
            font: font.as_ref(),
            size,
        };
        let mut book = HostBlocks {
            document: HostDocument::new(host, docs, id, self.sections, false)?,
            progress: self.progress.as_ref(),
            sections: self.sections,
        };
        let result = search.step(&mut book, &mut ruler, room).map(|_| ());
        Ok((result, search.percent(&book)))
    }

    /// Adds the rows of the hits found since: each takes its hit's snippet
    /// (the hit keeps its place), so no snippet is held twice and no row is
    /// made again.
    fn extend_list(&mut self) -> Result<(), Error> {
        let Some(search) = self.search.as_mut() else {
            return Ok(());
        };
        while self.places.len() < search.hits().len() {
            let index = self.places.len();
            let Some(start) = search
                .hits()
                .get(index)
                .map(|hit| ((hit.start, hit.end.offset), hit.percent))
            else {
                break;
            };
            let Some(snippet) = search.take_snippet(index) else {
                break;
            };
            let value = text::format(format_args!("{} %", start.1))?;
            let row = Item::row(&snippet, &value)?;
            drop(snippet);
            self.places.try_reserve(1).map_err(|_| Error::NoMemory)?;
            self.list.push(row)?;
            self.places.push(start.0);
        }
        Ok(())
    }

    /// What an input did.
    ///
    /// # Errors
    /// The host's refusals.
    pub fn input(
        &mut self,
        host: &Host<'_>,
        input: &Input<'_>,
    ) -> Result<(Handled, Outcome), Error> {
        let step = self.list.input(host, input)?;
        let outcome = match step {
            ItemStep::Chosen(index) => self
                .places
                .get(index)
                .map_or(Outcome::Handled, |(start, end)| Outcome::Go(*start, *end)),
            ItemStep::Ignored => Outcome::Ignored,
            _ => Outcome::Handled,
        };
        Ok((ItemList::handled(step), outcome))
    }

    /// Draws the hits, or what says why there are none; the note under them
    /// says how far the search is or how many it found.
    ///
    /// # Errors
    /// Allocation, font and drawing errors.
    pub fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        let title = messages.copy(host, "reader.search")?;
        let note = match (self.message, &self.search, self.failed) {
            (Some(key), ..) => return self.empty(host, frame, &title, &messages.copy(host, key)?),
            (_, _, Some(_)) => messages.copy(host, "reader.search.failed")?,
            (_, Some(search), _) => self.note_of(host, &mut messages, search.note())?,
            _ => String::new(),
        };
        if self.places.is_empty() {
            return self.empty(host, frame, &title, &note);
        }
        self.list
            .draw_with(host, frame, &title, true, Footer::Note(&note))
    }

    /// What a list with no hits says.
    fn empty(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        title: &str,
        said: &str,
    ) -> Result<(), Error> {
        self.list.draw_with(
            host,
            frame,
            title,
            true,
            Footer::Empty {
                title: said,
                text: "",
            },
        )
    }

    /// The text of the header's note.
    fn note_of(
        &self,
        host: &Host<'_>,
        messages: &mut Messages,
        note: Note,
    ) -> Result<String, Error> {
        let count = |n: usize| i64::try_from(n).unwrap_or(i64::MAX);
        match note {
            Note::Searching(_) if self.places.is_empty() => {
                messages.copy(host, "reader.search.wait")
            }
            Note::Searching(percent) => messages.format(
                host,
                "reader.search.searching",
                &[Arg::Int {
                    name: "percent",
                    value: i64::from(percent),
                }],
            ),
            Note::Hits(0) => messages.format(
                host,
                "reader.search.none",
                &[Arg::Text {
                    name: "query",
                    value: &self.query,
                }],
            ),
            Note::Hits(n) => messages.format(host, "reader.search.hits", &[Arg::Count(count(n))]),
            Note::MoreHits(_) => messages.format(
                host,
                "reader.search.more-hits",
                &[Arg::Int {
                    name: "count",
                    value: count(MAX_HITS),
                }],
            ),
        }
    }

    /// Gives back what the screen holds (it closed): the document is the
    /// reading screen's and stays.
    pub fn close(&mut self) {
        self.document = None;
        self.search = None;
    }
}

/// What the display and the font say, for the width of a snippet.
struct Display<'h> {
    font: Option<Font<'h>>,
    size: f32,
    width: f32,
    margin: f32,
}

impl<'h> Display<'h> {
    fn read(host: &'h Host<'h>) -> Option<Self> {
        let display = snapshot::capture_snapshot(host).ok()?;
        let font = Font::acquire_role(host, FontRole::Ui).ok()?;
        let style = Style::read(host, &display, &font).ok()?;
        Some(Self {
            size: style.text,
            width: f32::from(display.display_width()),
            margin: f32::from(style.margin),
            font: Some(font),
        })
    }

    /// The room in a row for a snippet, in pixels: the display's width less the
    /// margins and the room for "100 %".
    fn room(&self) -> f32 {
        let value = self
            .font
            .as_ref()
            .and_then(|font| font.measure_text(self.size, "100 %").ok())
            .unwrap_or(self.size * 3.0);
        let room = self.width - 4.0 * self.margin - value;
        if room > self.size { room } else { ROW_ROOM }
    }
}

#[cfg(test)]
mod tests;
