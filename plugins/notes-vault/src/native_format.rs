//! The interface-7 `core.format` provider of Markdown notes
//! (`docs/abi/document.md` section 3): one `fmt.call` in, the op's records
//! out, over the vault's [`MarkdownDocument`] and the format plugins' shared
//! provider (`paperpaw_epub_family::provider`).
//!
//! A note is read as root 3 (`path ""`) and its conversion is kept in the
//! document's cache, root 6. What a vault adds (the pictures an embed names
//! anywhere in it, the `.obsidian` folder that marks its top) is read from the
//! card (root 1, `uses.storage`, read only) by the card path the host puts in
//! the call's `text` where the book was opened from the card; without it (a
//! book from elsewhere, or no right to the card) the note is read without a
//! vault: its own folder's pictures are looked for nowhere, embeds are text.
use alloc::boxed::Box;
use alloc::string::String;

use paperpaw_epub_family::provider::FormatProvider;
use paperpaw_sdk::document::Document;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::stage_schemas::core_format_v1::{self as stage, Record};
use paperpaw_sdk::{DirEntry, Storage, StorageError};

use crate::PLUGIN;
use crate::fallible;
use crate::markdown::MarkdownDocument;

/// The op that opens a document: its `text` is the book's card path.
const OPEN: u32 = 2;

/// The provider of the format `notes-vault.markdown`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn markdown_provider() -> Result<FormatProvider, Error> {
    let dir = fallible::plugin_dir(PLUGIN.as_str()).map_err(|_| Error::NoMemory)?;
    Ok(FormatProvider::new(
        "notes-vault.markdown",
        |storage, name, options| {
            // The card path is the name: no name, no vault.
            MarkdownDocument::open_in(storage, name, options.publisher_styles, !name.is_empty())
                .map(|document| Box::new(document) as Box<dyn Document>)
        },
        dir,
        |_| false,
    ))
}

/// The storage of one call: the book and the cache of the document (`base`,
/// as `NativeIo` routes them), and the card, which is where every other
/// path is.
pub struct VaultIo<'a> {
    /// Root 3 for `""`, root 6 for what is below `prefix`.
    base: &'a dyn Storage,
    card: &'a dyn Storage,
    /// The cache folder the parser keeps its conversions in.
    prefix: &'a str,
    /// The book's card path, where the host named it: read as root 3.
    book: &'a str,
}

impl<'a> VaultIo<'a> {
    /// The storage of a call whose book is at `book` on the card (empty:
    /// elsewhere).
    #[must_use]
    pub fn new(
        base: &'a dyn Storage,
        card: &'a dyn Storage,
        prefix: &'a str,
        book: &'a str,
    ) -> Self {
        Self {
            base,
            card,
            prefix,
            book,
        }
    }

    /// Where a path is, and its name there.
    fn route<'p>(&self, path: &'p str) -> (Place, &'p str) {
        let cache = path
            .strip_prefix(self.prefix)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'));
        if cache {
            (Place::Base, path)
        } else if path == self.book {
            // The note itself, which the host gives as the book.
            (Place::Base, "")
        } else {
            (Place::Card, path)
        }
    }
}

/// Which of the two storages a path is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    /// The book (root 3) and the document's cache (root 6).
    Base,
    /// The card, read only.
    Card,
}

impl core::fmt::Debug for VaultIo<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("VaultIo").finish_non_exhaustive()
    }
}

impl Storage for VaultIo<'_> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        match self.route(path) {
            (Place::Base, name) => self.base.read_at(name, offset, buf),
            (Place::Card, name) => self.card.read_at(name, offset, buf),
        }
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        match self.route(path) {
            (Place::Base, name) => self.base.size(name),
            (Place::Card, name) => self.card.size(name),
        }
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        match self.route(path) {
            (Place::Base, name) => self.base.write(name, data),
            // The card is read only.
            (Place::Card, _) => Err(StorageError::Unsupported),
        }
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        match self.route(path) {
            (Place::Base, name) => self.base.append(name, data),
            (Place::Card, _) => Err(StorageError::Unsupported),
        }
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        // Only the card has folders to list.
        match self.route(dir) {
            (Place::Card, name) => self.card.list(name, visit),
            (Place::Base, _) => Err(StorageError::Unsupported),
        }
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        match self.route(path) {
            (Place::Base, name) => self.base.remove(name),
            (Place::Card, _) => Err(StorageError::Unsupported),
        }
    }
}

/// The card path in the text of an op that opens a document, or empty.
fn book_of(input: &[u8]) -> Result<(String, u32), Error> {
    let mut records = stage::input_decode(input).map_err(|_| Error::Invalid)?;
    let Some(Ok(Record::FmtCall(call))) = records.next() else {
        return Err(Error::Invalid);
    };
    let path = if call.op == OPEN {
        fallible::string(call.text).map_err(|_| Error::NoMemory)?
    } else {
        String::new()
    };
    Ok((path, call.op))
}

/// Answers one `core.format` call: `base` is the book and the cache of the
/// callback, `card` the card (or nothing, where the plugin has no right to it).
///
/// # Errors
/// What the provider says: `Invalid` for a payload that is no `fmt.call`,
/// `NotFound` for a document or place that is not there, `NoMemory` for a
/// refused allocation.
pub fn call(
    provider: &mut FormatProvider,
    base: &dyn Storage,
    card: &dyn Storage,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, Error> {
    let (book, _) = book_of(input)?;
    let prefix = fallible::string(provider.cache()).map_err(|_| Error::NoMemory)?;
    let io = VaultIo::new(base, card, &prefix, &book);
    provider.call(&io, input, out)
}
