//! A book opened for the first time: the format prepares its document (a
//! conversion the host runs between inputs, `docs/abi/document.md` 2.1) and the
//! screen says so, as Beta 3 did ("Opening the book for the first time"). The
//! document stays open meanwhile; closing it would end the preparation. When the
//! host says its state moved (`event.document_changed`), the screen asks the
//! state and opens the book when it is ready, or says it could not when it
//! failed. The document is given back once: when it failed, or when the screen
//! closes.

use paperpaw_sdk::native::commands::RedrawMode;
use paperpaw_sdk::native::{
    Error, Host,
    document::{DocId, Documents, Preparation},
    library::Library,
};
use paperpaw_ui_kit::mem::buffer;

use super::{Body, Handover, View};

/// A book whose document the format is preparing, and what opens it then.
#[derive(Debug)]
pub(crate) struct Preparing {
    pub(super) id: DocId,
    /// The hand-over's path, root and place.
    pub(super) found: Handover,
    pub(super) whole_pages: bool,
    pub(super) conversion: u64,
}

/// What a state the host gave means for the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Next {
    /// Still preparing: the screen stays as it is.
    Wait,
    /// Ready: the book opens.
    Open,
    /// Failed (or the host ended the document): the screen says the book could
    /// not be opened.
    Fail,
}

/// What the state `asked` of a preparing document means: a document the host
/// no longer knows failed with it.
pub(crate) fn next(asked: Result<Preparation, Error>) -> Result<Next, Error> {
    match asked {
        Ok(Preparation::Ready) => Ok(Next::Open),
        Ok(Preparation::Preparing) | Err(Error::Busy) => Ok(Next::Wait),
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Ok(_) | Err(_) => Ok(Next::Fail),
    }
}

impl View {
    /// The host says a document's state moved, or the screen came to the top
    /// again: a book being prepared opens when it is ready, or says it failed.
    /// Whether the screen is to be drawn again.
    ///
    /// # Errors
    /// `NoMemory`; the document is given back then.
    pub fn prepared(
        &mut self,
        host: &Host<'_>,
        (docs, library): (&Documents, Option<&Library>),
    ) -> Result<bool, Error> {
        let Body::Preparing(book) = &self.body else {
            return Ok(false);
        };
        // The host asks for room for the largest answer of the op (`max_reply` 4,096).
        let asked = match (buffer(64), buffer(4096)) {
            (Ok(mut input), Ok(mut out)) => docs
                .state(host, book.id, &mut input, &mut out)
                .map(|state| (state.preparation(), state.whole_pages(), state.conversion())),
            _ => Err(Error::NoMemory),
        };
        let flags = asked
            .as_ref()
            .ok()
            .map(|&(_, whole, conversion)| (whole, conversion));
        let step = next(asked.map(|(preparation, ..)| preparation));
        // The body that holds the document is taken: whatever happens below, the
        // document is given back at most once.
        let Body::Preparing(mut book) =
            core::mem::replace(&mut self.body, Body::Message("reader.preparing"))
        else {
            return Ok(false);
        };
        match step {
            Ok(Next::Wait) => {
                self.body = Body::Preparing(book);
                Ok(false)
            }
            Ok(Next::Open) => {
                if let Some((whole_pages, conversion)) = flags {
                    book.whole_pages = whole_pages;
                    book.conversion = conversion;
                }
                // `opened` gives the document back on a refusal on the way.
                match Self::opened(host, (docs, library), book) {
                    Ok(opened) => {
                        self.body = opened.body;
                        // Shown already: its pictures are asked for now.
                        if let Body::Open(open) = &mut self.body
                            && open.pictures_pending()
                        {
                            open.redraw_page(host, docs, RedrawMode::Partial).ok();
                        }
                    }
                    Err(Error::NoMemory) => {
                        self.body = Body::Message("reader.no-memory");
                        return Err(Error::NoMemory);
                    }
                    Err(_) => self.body = Body::Message("reader.error"),
                }
                Ok(true)
            }
            Ok(Next::Fail) => {
                Self::forget(host, docs, book.id);
                self.body = Body::Message("reader.error");
                Ok(true)
            }
            Err(error) => {
                Self::forget(host, docs, book.id);
                self.body = Body::Message("reader.no-memory");
                Err(error)
            }
        }
    }

    /// Whether the screen waits for its book to be prepared.
    #[must_use]
    pub const fn preparing(&self) -> bool {
        matches!(self.body, Body::Preparing(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ready_document_opens_a_failed_one_fails_and_one_preparing_waits() {
        assert_eq!(next(Ok(Preparation::Ready)), Ok(Next::Open));
        assert_eq!(next(Ok(Preparation::Preparing)), Ok(Next::Wait));
        assert_eq!(next(Ok(Preparation::Failed)), Ok(Next::Fail));
        // Busy: a call while the host works on it; asked again at the next hint.
        assert_eq!(next(Err(Error::Busy)), Ok(Next::Wait));
        // The host ended the document (cancelled) or refused it: the book failed.
        assert_eq!(next(Err(Error::Cancelled)), Ok(Next::Fail));
        assert_eq!(next(Err(Error::Invalid)), Ok(Next::Fail));
        assert_eq!(next(Err(Error::NoMemory)), Err(Error::NoMemory));
    }
}
