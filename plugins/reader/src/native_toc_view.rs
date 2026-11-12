//! The table-of-contents screen (`reader.toc`): the entries of the book's own
//! table (`core.document` ops 7 and 8) in a list, the focus on the chapter the
//! reader is in, and a choice that tells the reading screen where to go. A
//! book whose own table has at most one entry shows the headings found in its
//! blocks instead (`native_headings`): kept from an earlier opening, or found
//! a step at a time (`core.work`, [`TocView::step`]) while the screen says how
//! far it is (`native_finding`).
use crate::{
    native_finding::Finding,
    native_headings::{self, Scan, Step as ScanStep},
    native_toc::{self, Entry, MAX_HEADINGS, MAX_LEVEL, Place},
    native_view::{Handover, read_handover},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host,
        callbacks::{DrawFrame, Input},
        document::{DocCursor, DocId, Documents, OpenBook, Preparation},
        storage::DataStorage,
        ui::{HandoverId, Ui},
    },
};
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages,
    mem::{SERVICE_BUFFER, buffer},
};

/// One open table of contents.
#[derive(Debug)]
pub struct TocView {
    /// The book's document: the reading screen's, or one opened here.
    document: Option<DocId>,
    /// The document was opened by this screen (and is closed with it): the X4
    /// holds one open document per plugin, so the reading screen's is used.
    owned: bool,
    /// Entries as the host listed them: where each leads and whether that is exact.
    entries: Vec<(Entry, bool)>,
    list: ItemList,
    /// Why there is nothing to list (the key of the text).
    message: Option<&'static str>,
    /// The headings scan under way, and how far the screen says it is.
    scan: Option<(Scan, Finding)>,
    /// Where the reader is: the focus once the headings are found.
    at: Place,
}

/// What a step of the headings scan did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stepped {
    /// The screen is to be drawn again.
    pub redraw: bool,
    /// More of the book remains: the work is to be declared again.
    pub again: bool,
}

/// What a choice on the screen meant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing the screen handles.
    Ignored,
    /// Handled; the screen stays.
    Handled,
    /// Go to this place and close the screen.
    Go(Place),
}

/// The title of an entry, indented by its level.
fn row_title(entry: &Entry) -> Result<String, Error> {
    let mut title = String::new();
    let indent = usize::from(entry.level.min(MAX_LEVEL)) * 2;
    title
        .try_reserve(indent + entry.title.len())
        .map_err(|_| Error::NoMemory)?;
    title.extend(core::iter::repeat_n(' ', indent));
    title.push_str(&entry.title);
    Ok(title)
}

impl TocView {
    fn none(message: &'static str) -> Self {
        Self {
            document: None,
            owned: false,
            entries: Vec::new(),
            list: ItemList::default(),
            message: Some(message),
            scan: None,
            at: Place::default(),
        }
    }

    /// The table of the book the hand-over names, read. `shared` is the reading
    /// screen's document and the conversion its places belong to.
    ///
    /// # Errors
    /// `NoMemory`; the book's own refusals are a screen that says so.
    pub fn open(
        host: &Host<'_>,
        ui: &Ui,
        docs: &Documents,
        (handover, shared): (Option<HandoverId>, Option<(DocId, u64)>),
    ) -> Result<Self, Error> {
        let found = match handover {
            Some(handover) => read_handover(host, ui, handover).unwrap_or_default(),
            None => Handover::default(),
        };
        if found.path.is_empty() {
            return Ok(Self::none("reader.no-book"));
        }
        let mut view = Self::none("reader.error");
        let (id, conversion) = if let Some(shared) = shared {
            shared
        } else {
            let (mut input, mut out) = (buffer(256)?, buffer(SERVICE_BUFFER)?);
            let book = OpenBook::new(&found.path).root(found.root());
            let (id, preparation, conversion) =
                match docs.open_book(host, &book, &mut input, &mut out) {
                    Ok(state) => (state.id(), state.preparation(), state.conversion()),
                    Err(Error::NoMemory) => return Err(Error::NoMemory),
                    Err(Error::Busy) => return Ok(Self::none("reader.not-ready")),
                    Err(_) => return Ok(Self::none("reader.error")),
                };
            view.owned = true;
            if !matches!(preparation, Preparation::Ready) {
                view.document = Some(id);
                view.message = Some("reader.preparing");
                return Ok(view);
            }
            (id, conversion)
        };
        view.document = Some(id);
        view.at = place_of(&found);
        match view.read(host, docs, id, (&found.path, conversion)) {
            Ok(()) => {}
            Err(Error::NoMemory) => {
                view.close(host, docs);
                return Err(Error::NoMemory);
            }
            Err(_) => view.message = Some("reader.error"),
        }
        Ok(view)
    }

    /// Reads the entries (at most [`MAX_HEADINGS`]) and puts the focus on the
    /// chapter the reader is in. Where the book's own table has at most one
    /// entry, the headings kept for the book (`path`, its `conversion`) are
    /// shown, or found first.
    fn read(
        &mut self,
        host: &Host<'_>,
        docs: &Documents,
        id: DocId,
        (path, conversion): (&str, u64),
    ) -> Result<(), Error> {
        let (mut input, mut out) = (buffer(256)?, buffer(SERVICE_BUFFER)?);
        let mut cursor = DocCursor::start();
        'pages: loop {
            let page = docs.toc(host, id, cursor, &mut input, &mut out)?;
            for row in page.rows() {
                if self.entries.len() >= MAX_HEADINGS {
                    break 'pages;
                }
                let Some(title) = native_toc::title_of(row.title())? else {
                    continue;
                };
                let position = row.position();
                let entry = Entry::new(
                    title,
                    u8::try_from(row.level()).unwrap_or(u8::MAX),
                    Place::new(position.section, position.offset),
                );
                self.entries.try_reserve(1).map_err(|_| Error::NoMemory)?;
                self.entries.push((entry, row.approximate()));
            }
            match page.end().next() {
                Some(next) => cursor = next,
                None => break,
            }
        }
        drop((input, out));
        if native_toc::wants_scan(self.entries.len()) {
            let kept = native_headings::load(&DataStorage::new(host), path, conversion)?;
            match kept {
                // Found before, for this conversion of the book.
                Some(kept) => {
                    if let Some(headings) = native_toc::prefer(Some(kept), self.entries.len()) {
                        return self.show_headings(headings);
                    }
                }
                None => return self.start_scan(host, docs, id, (path, conversion)),
            }
        }
        self.show_table(host, docs, id)
    }

    /// The headings scan starts: the screen says it is finding the chapters.
    fn start_scan(
        &mut self,
        host: &Host<'_>,
        docs: &Documents,
        id: DocId,
        (path, conversion): (&str, u64),
    ) -> Result<(), Error> {
        let (mut input, mut out) = (buffer(256)?, buffer(SERVICE_BUFFER)?);
        let sections = docs.metadata(host, id, &mut input, &mut out)?.sections();
        self.scan = Some((Scan::new(path, conversion, sections)?, Finding::new()));
        self.message = None;
        Ok(())
    }

    /// Whether the headings are being found (the work is to be declared).
    #[must_use]
    pub const fn scanning(&self) -> bool {
        self.scan.is_some()
    }

    /// Reads the next blocks of the book for headings; once the scan ends the
    /// screen lists them (or the book's own table, where they are no more).
    ///
    /// # Errors
    /// `NoMemory`; then [`TocView::give_up`].
    pub fn step(&mut self, host: &Host<'_>, docs: &Documents) -> Result<Stepped, Error> {
        let (Some((scan, finding)), Some(id)) = (self.scan.as_mut(), self.document) else {
            return Ok(Stepped::default());
        };
        let (mut input, mut out) = (buffer(256)?, buffer(SERVICE_BUFFER)?);
        let storage = DataStorage::new(host);
        let step = scan.step(&storage, &mut |at| {
            native_headings::read_seen((host, docs, id), (&mut input[..], &mut out[..]), at)
        });
        drop((input, out));
        if step == ScanStep::More {
            let redraw = finding.update(scan.percent());
            return Ok(Stepped {
                redraw,
                again: true,
            });
        }
        let found = self.scan.take().map(|(scan, _)| scan.into_entries());
        match native_toc::prefer(found.filter(|_| step == ScanStep::Done), self.entries.len()) {
            Some(headings) => self.show_headings(headings)?,
            None => self.show_table(host, docs, id)?,
        }
        Ok(Stepped {
            redraw: true,
            again: false,
        })
    }

    /// The scan failed with `error`: the screen says so.
    pub fn give_up(&mut self, error: Error) {
        self.scan = None;
        self.entries = Vec::new();
        self.list = ItemList::default();
        self.message = Some(if error == Error::NoMemory {
            "reader.no-memory"
        } else {
            "reader.error"
        });
    }

    /// The headings found listed, the focus on the one the reader is past.
    fn show_headings(&mut self, headings: Vec<Entry>) -> Result<(), Error> {
        let focus = native_toc::current(&headings, self.at);
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(headings.len())
            .map_err(|_| Error::NoMemory)?;
        // A heading's place is exact: no target to ask for.
        entries.extend(headings.into_iter().map(|entry| (entry, false)));
        self.entries = entries;
        self.fill()?;
        self.list.set_focus(focus);
        Ok(())
    }

    /// The rows of the entries; `false` where there are none (the screen says
    /// so).
    fn fill(&mut self) -> Result<bool, Error> {
        if self.entries.is_empty() {
            self.list = ItemList::default();
            self.message = Some("reader.no-toc");
            return Ok(false);
        }
        self.message = None;
        let mut items = Vec::new();
        items
            .try_reserve(self.entries.len())
            .map_err(|_| Error::NoMemory)?;
        for (entry, _) in &self.entries {
            items.push(Item::row(&row_title(entry)?, "")?);
        }
        self.list = ItemList::new(items);
        Ok(true)
    }

    /// The book's own table listed, the focus on the chapter the reader is in.
    fn show_table(&mut self, host: &Host<'_>, docs: &Documents, id: DocId) -> Result<(), Error> {
        if !self.fill()? {
            return Ok(());
        }
        let at = self.at;
        let focus = native_toc::current_of_table(
            self.entries.iter().map(|(e, _)| e.at.section),
            at,
            |index| {
                let (entry, approximate) = self.entries.get(index)?;
                if !approximate {
                    return Some(entry.at);
                }
                let (mut input, mut out) = (buffer(256).ok()?, buffer(SERVICE_BUFFER).ok()?);
                let exact = docs
                    .toc_target(host, id, u32::try_from(index).ok()?, &mut input, &mut out)
                    .ok()?;
                Some(Place::new(exact.section, exact.offset))
            },
        );
        self.list.set_focus(focus);
        Ok(())
    }

    /// What an input did.
    ///
    /// # Errors
    /// The host's refusals.
    pub fn input(
        &mut self,
        host: &Host<'_>,
        docs: &Documents,
        input: &Input<'_>,
    ) -> Result<(Handled, Outcome), Error> {
        let step = self.list.input(host, input)?;
        let outcome = match step {
            ItemStep::Chosen(index) => self
                .target(host, docs, index)
                .map_or(Outcome::Handled, Outcome::Go),
            ItemStep::Ignored => Outcome::Ignored,
            _ => Outcome::Handled,
        };
        Ok((ItemList::handled(step), outcome))
    }

    /// Where entry `index` leads: the exact place where the host's was approximate.
    fn target(&self, host: &Host<'_>, docs: &Documents, index: usize) -> Option<Place> {
        let (entry, approximate) = self.entries.get(index)?;
        let (Some(id), true) = (self.document, *approximate) else {
            return Some(entry.at);
        };
        let (mut input, mut out) = (buffer(256).ok()?, buffer(SERVICE_BUFFER).ok()?);
        match docs.toc_target(host, id, u32::try_from(index).ok()?, &mut input, &mut out) {
            Ok(exact) => Some(Place::new(exact.section, exact.offset)),
            Err(_) => Some(entry.at),
        }
    }

    /// Draws the list, or what says why there is none.
    ///
    /// # Errors
    /// Allocation, font and drawing errors.
    pub fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        if let Some((_, finding)) = &self.scan {
            return finding.draw(host, frame);
        }
        let mut messages = Messages::new()?;
        let title = messages.copy(host, "reader.toc")?;
        match self.message {
            Some(key) => {
                let text = messages.copy(host, key)?;
                self.list.draw_with(
                    host,
                    frame,
                    &title,
                    true,
                    Footer::Empty {
                        title: &text,
                        text: "",
                    },
                )
            }
            None => self.list.draw_with(host, frame, &title, true, Footer::None),
        }
    }

    /// Gives the document back (the screen closed).
    pub fn close(&mut self, host: &Host<'_>, docs: &Documents) {
        let id = self.document.take();
        if let Some(id) = id.filter(|_| self.owned)
            && let (Ok(mut input), Ok(mut out)) = (buffer(64), buffer(64))
        {
            docs.close(host, id, &mut input, &mut out).ok();
        }
    }
}

/// The place the hand-over says the reader is at (the start where it says none).
fn place_of(found: &Handover) -> Place {
    native_toc::place_of(found.section.as_deref(), found.offset.as_deref()).unwrap_or_default()
}

#[cfg(test)]
mod tests;
