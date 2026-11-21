//! What the reading screen takes from its hand-over: the book (`path`, `root`)
//! and where it opens (`section` and `offset`, `page`, or the heading of a note,
//! `reader.anchor`; `native_open`).
use alloc::string::String;
use core::fmt::Write as _;
use paperpaw_sdk::native::{
    Error, Host,
    document::{DocId, Documents, Root},
    ui::{HandoverId, Ui},
};
use paperpaw_ui_kit::{
    mem::{SERVICE_BUFFER, buffer},
    text,
};

use super::MAX_PATH;
use crate::native_open;

/// The hand-over's values, as far as the screen reads them.
#[derive(Debug, Default)]
pub(crate) struct Handover {
    pub(crate) path: String,
    pub(crate) root: Option<String>,
    pub(crate) section: Option<String>,
    pub(crate) offset: Option<String>,
    pub(super) page: Option<String>,
    /// The anchor of the heading to open at (`reader.anchor`: a notes vault's
    /// link to a heading of another note, `Note.md#part`).
    pub(super) anchor: Option<String>,
}

impl Handover {
    /// The root the book is opened on (the library's unless the hand-over says).
    ///
    /// A plugin that hands a file of its own over names its folder: `0` (data)
    /// or `2` (install), with the `path` (`document.md` 1, root 5). The reader
    /// reads it as the handed file (`doc_open` root 5), never from its own
    /// data folder. `5` is the reader's own code for a book it opened that
    /// way (`root_code`, for its table-of-contents screen).
    pub(crate) fn root(&self) -> Root {
        match self.root.as_deref() {
            Some("0" | "2" | "5") => Root::Handed,
            _ => Root::Library,
        }
    }

    /// The hand-over of a book this screen showed (its path and root code):
    /// opened again, at its saved place.
    pub(crate) fn of_book(path: String, root: &str) -> Result<Self, Error> {
        Ok(Self {
            path,
            root: Some(text::copy(root)?),
            ..Self::default()
        })
    }

    pub(super) fn get(&self, key: &str) -> Option<&str> {
        match key {
            native_open::SECTION => self.section.as_deref(),
            native_open::OFFSET => self.offset.as_deref(),
            native_open::PAGE => self.page.as_deref(),
            _ => None,
        }
    }

    /// The heading the hand-over's anchor names, as the place it opens at:
    /// where document `id` has that anchor (`#` and the anchor, as a link in
    /// it leads there), as Beta 3 opened a note at the heading its link named.
    /// A place the hand-over names itself wins; an anchor the document does
    /// not have opens the book where it would open without it.
    ///
    /// # Errors
    /// `NoMemory` and the host's refusals other than "no such place".
    pub(super) fn place_anchor(
        &mut self,
        host: &Host<'_>,
        docs: &Documents,
        id: DocId,
    ) -> Result<(), Error> {
        let Some(anchor) = self.anchor.take() else {
            return Ok(());
        };
        if self.section.is_some() || self.offset.is_some() {
            return Ok(());
        }
        let mut href = String::new();
        href.try_reserve_exact(anchor.len() + 1)
            .map_err(|_| Error::NoMemory)?;
        href.push('#');
        href.push_str(&anchor);
        let (mut input, mut out) = (buffer(2048)?, buffer(4096)?);
        let at = match docs.link_target(host, id, &href, &mut input, &mut out) {
            Ok(at) => at,
            Err(Error::NotFound | Error::Busy | Error::Invalid) => return Ok(()),
            Err(error) => return Err(error),
        };
        self.section = Some(decimal(at.section)?);
        self.offset = Some(decimal(at.offset)?);
        Ok(())
    }
}

/// `n` in decimal, as a hand-over writes a place.
fn decimal(n: u32) -> Result<String, Error> {
    let mut text = String::new();
    // `u32::MAX` has ten digits: the write needs no more room.
    text.try_reserve_exact(10).map_err(|_| Error::NoMemory)?;
    write!(text, "{n}").map_err(|_| Error::NoMemory)?;
    Ok(text)
}

pub(crate) fn read_handover(
    host: &Host<'_>,
    ui: &Ui,
    handover: HandoverId,
) -> Result<Handover, Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let mut found = Handover::default();
    for param in ui.params(host, handover, &mut input, &mut out)? {
        let param = param?;
        let value = param.value();
        match param.key() {
            "path" if value.len() <= MAX_PATH => found.path = text::copy(value)?,
            "root" => found.root = Some(text::copy(value)?),
            native_open::SECTION => found.section = Some(text::copy(value)?),
            native_open::OFFSET => found.offset = Some(text::copy(value)?),
            native_open::PAGE => found.page = Some(text::copy(value)?),
            crate::ANCHOR => found.anchor = Some(text::copy(value)?),
            _ => {}
        }
    }
    Ok(found)
}
