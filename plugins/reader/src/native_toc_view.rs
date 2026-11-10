//! The table-of-contents screen (`reader.toc`): the entries of the book's own
//! table (`core.document` ops 7 and 8) in a list, the focus on the chapter the
//! reader is in, and a choice that tells the reading screen where to go. The
//! headings found by scanning blocks (for a book without a table of its own,
//! `native_toc`'s model) are not shown yet (`PORTING-7.md`).
use crate::{
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
        }
    }

    /// The table of the book the hand-over names, read.
    ///
    /// # Errors
    /// `NoMemory`; the book's own refusals are a screen that says so.
    pub fn open(
        host: &Host<'_>,
        ui: &Ui,
        docs: &Documents,
        (handover, shared): (Option<HandoverId>, Option<DocId>),
    ) -> Result<Self, Error> {
        let found = match handover {
            Some(handover) => read_handover(host, ui, handover).unwrap_or_default(),
            None => Handover::default(),
        };
        if found.path.is_empty() {
            return Ok(Self::none("reader.no-book"));
        }
        let mut view = Self::none("reader.error");
        let id = if let Some(id) = shared {
            id
        } else {
            let (mut input, mut out) = (buffer(256)?, buffer(SERVICE_BUFFER)?);
            let book = OpenBook::new(&found.path).root(found.root());
            let (id, preparation) = match docs.open_book(host, &book, &mut input, &mut out) {
                Ok(state) => (state.id(), state.preparation()),
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
            id
        };
        view.document = Some(id);
        let at = place_of(&found);
        match view.read(host, docs, id, at) {
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
    /// chapter `at` is in.
    fn read(
        &mut self,
        host: &Host<'_>,
        docs: &Documents,
        id: DocId,
        at: Place,
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
        if self.entries.is_empty() {
            self.message = Some("reader.no-toc");
            return Ok(());
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
