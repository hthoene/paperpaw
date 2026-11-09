//! What the sync asks of the book and the library: the book's KOReader
//! fingerprint (`core.library` op 25 `book_key`), and a place's XPointer and the
//! place of an XPointer (`core.document` ops `xpointer` and `locate`,
//! `document.md` section 2). A service the device does not offer, a book the
//! library does not know and a book without XPointers all answer "unavailable"
//! (`Ok(None)`): the sync then makes no fetch and sends nothing, or sends the
//! progress alone, as interface 6 did.
use alloc::string::String;

use paperpaw_sdk::native::{
    Error, Host,
    document::{DocId, DocPosition, Documents, OpenBook, Preparation},
    library::{BookRef, Library},
};
use paperpaw_ui_kit::{mem::buffer, text::copy};

/// The input and the reply a service call needs; 4,096 bytes always suffice.
const BUFFER: usize = 4096;

/// The two services, opened where the device has them.
#[derive(Debug, Default)]
pub struct Backing {
    library: Option<Library>,
    documents: Option<Documents>,
}

impl Backing {
    /// The services the device offers (`None` for each it does not).
    ///
    /// # Errors
    /// `NoMemory`, and what a service says other than that it is not offered.
    pub fn open(host: &Host<'_>) -> Result<Self, Error> {
        Ok(Self {
            library: optional(Library::open(host))?,
            documents: optional(Documents::open(host))?,
        })
    }

    /// Whether a position can be sent at all: the library names the book. Nothing
    /// is declared for a push while it cannot (it would wake the radio for
    /// nothing).
    #[must_use]
    pub const fn can_send(&self) -> bool {
        self.library.is_some()
    }

    /// The KOReader fingerprint of the book at `path`: the 32 hex digits of its
    /// partial MD5 (`library.md` section 6); `None` for a book the library does
    /// not know or no library.
    ///
    /// # Errors
    /// `NoMemory`; `Busy` where the host asks to try again.
    pub fn digest(&self, host: &Host<'_>, path: &str) -> Result<Option<String>, Error> {
        let Some(library) = &self.library else {
            return Ok(None);
        };
        let Ok(book) = BookRef::path(path) else {
            return Ok(None);
        };
        let (mut input, mut out) = (buffer(BUFFER)?, buffer(BUFFER)?);
        match library.book_key(host, book, &mut input, &mut out) {
            Ok(key) if key.fingerprint().is_empty() => Ok(None),
            Ok(key) => copy(key.fingerprint()).map(Some),
            Err(error) => unavailable(error),
        }
    }

    /// The XPointer of the place `at` (section, offset) of the book at `path`;
    /// `None` where the book has none (a format without XPointers, a book still
    /// being prepared, no document service).
    ///
    /// # Errors
    /// `NoMemory`; `Busy` where the host asks to try again.
    pub fn xpointer(
        &self,
        host: &Host<'_>,
        path: &str,
        at: (u32, u32),
    ) -> Result<Option<String>, Error> {
        self.with_document(host, path, |documents, id, input, out| {
            match documents.xpointer(host, id, DocPosition::new(at.0, at.1), input, out) {
                Ok("") => Ok(None),
                Ok(text) => copy(text).map(Some),
                Err(error) => unavailable(error),
            }
        })
    }

    /// The place (section, offset) of the book at `path` that `xpointer` names;
    /// `None` where it is empty, not found or cannot be told. (Interface 6 then
    /// went by the progress; that needs the size of each section, which the
    /// document service does not give yet: `PORTING-7.md`.)
    ///
    /// # Errors
    /// `NoMemory`; `Busy` where the host asks to try again.
    pub fn locate(
        &self,
        host: &Host<'_>,
        path: &str,
        xpointer: &str,
    ) -> Result<Option<(u32, u32)>, Error> {
        if xpointer.is_empty() {
            return Ok(None);
        }
        self.with_document(host, path, |documents, id, input, out| {
            match documents.locate(host, id, xpointer, input, out) {
                Ok(at) => Ok(Some((at.section, at.offset))),
                Err(error) => unavailable(error),
            }
        })
    }

    /// Opens the book, runs `ask` on it when it is ready and has XPointers, and
    /// closes it again (also when `ask` fails).
    fn with_document<T>(
        &self,
        host: &Host<'_>,
        path: &str,
        ask: impl FnOnce(&Documents, DocId, &mut [u8], &mut [u8]) -> Result<Option<T>, Error>,
    ) -> Result<Option<T>, Error> {
        let Some(documents) = &self.documents else {
            return Ok(None);
        };
        let (mut input, mut out) = (buffer(BUFFER)?, buffer(BUFFER)?);
        let (id, usable) =
            match documents.open_book(host, &OpenBook::new(path), &mut input, &mut out) {
                Ok(state) => {
                    let ready = state.preparation() == Preparation::Ready && state.has_xpointers();
                    // A book still converting: asked again at the next chance.
                    if state.preparation() == Preparation::Preparing {
                        let id = state.id();
                        close(documents, host, id, &mut input, &mut out);
                        return Err(Error::Busy);
                    }
                    (state.id(), ready)
                }
                Err(error) => return unavailable(error),
            };
        let found = if usable {
            ask(documents, id, &mut input, &mut out)
        } else {
            Ok(None)
        };
        close(documents, host, id, &mut input, &mut out);
        found
    }
}

/// Closes a document; one that cannot be closed is the host's to forget.
fn close(documents: &Documents, host: &Host<'_>, id: DocId, input: &mut [u8], out: &mut [u8]) {
    match documents.close(host, id, input, out) {
        Ok(()) | Err(_) => {}
    }
}

/// `Ok(None)` for a service the device does not offer or the plugin may not use.
fn optional<T>(opened: Result<T, Error>) -> Result<Option<T>, Error> {
    match opened {
        Ok(service) => Ok(Some(service)),
        Err(Error::Unsupported | Error::NotFound | Error::Denied) => Ok(None),
        Err(error) => Err(error),
    }
}

/// What a refused call means: no memory and busy are said as they are, the rest
/// (not found, denied, unsupported, invalid) is "unavailable".
fn unavailable<T>(error: Error) -> Result<Option<T>, Error> {
    match error {
        Error::NoMemory | Error::Busy => Err(error),
        _ => Ok(None),
    }
}
