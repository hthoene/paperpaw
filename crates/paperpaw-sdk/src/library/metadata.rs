//! What the library's walk reads of a new book: its metadata alone
//! (title, author, series, tags), not its cover, which takes a picture's
//! decoding (seconds for a large one on the X4) and its writing. The
//! details say so (`metadata = on`, `indexed = off`): the walk takes them
//! as they are next time, and a view that shows the book reads its cover
//! when it first does ([`index`](super::index()), one book at a time), as
//! for any book not indexed yet.

use paperpaw_platform::Storage;

use super::{Book, DETAILS, book_for_update, load_details, today};
use crate::Env;
use crate::config;

/// The key the details of a book carry where its metadata was read from
/// it, its cover not.
const METADATA: &str = "metadata";

/// [`index`](super::index()) without the cover: reads title, author,
/// series and tags from the book and keeps them, marked as read.
pub(crate) fn index_details(env: &Env<'_>, path: &str) -> Book {
    let storage = env.storage();
    // Details that could not be read are not written over.
    let (mut book, known) = match book_for_update(storage, path) {
        Ok(book) => (book, true),
        Err(_) => (Book::default(), false),
    };
    book.size = storage.size(path).unwrap_or_default();
    if book.added.is_none() {
        book.added = today(env);
    }
    if let Ok(document) = env.formats().open(storage, path) {
        // Copied fallibly; with no memory the book is not written down (it
        // is read from the book again at the next walk).
        if super::fallible::try_merge_metadata(&mut book, document.metadata()).is_err() {
            // Not indexed (nor marked as read), and of no known size: the
            // catalog line made of it names the file, and a line of a book
            // of another size is made again at the next walk (a walk that
            // finds the size it names takes the line as it is).
            book.indexed = false;
            book.size = 0;
            return book;
        }
    }
    // Also where it cannot be opened: not tried again at every walk.
    if known {
        write_read(storage, path, &book);
    }
    book
}

/// What the library knows of the book at `path` (of size `size`) where
/// it was read from the book: indexed, or its metadata read
/// ([`index_details`]). A book replaced by one of the same size keeps the
/// details of the one before (as an indexed one always did).
pub(crate) fn read_book(storage: &dyn Storage, path: &str, size: u64) -> Option<Book> {
    let source = super::fallible::surviving(load_details(storage, path))??;
    let book = super::fallible::surviving(
        super::fallible::try_parse_details(&source).map_err(|_| crate::data::Error::OutOfMemory),
    )?;
    (book.size == size && (book.indexed || metadata_read(&source))).then_some(book)
}

/// Whether details `source` (of this kind) say the metadata was read.
fn metadata_read(source: &str) -> bool {
    let (mut read, mut version) = (false, 1);
    for entry in config::parse(source).filter_map(Result::ok) {
        match (entry.section, entry.key) {
            ("book", METADATA) => read = entry.value.as_bool().unwrap_or_default(),
            (_, "version") => version = entry.value.as_u32().unwrap_or(1),
            _ => {}
        }
    }
    read && version == DETAILS
}

/// Keeps `book`, marked as read from the book ([`METADATA`]); not where
/// the details there could not be read.
fn write_read(storage: &dyn Storage, path: &str, book: &Book) {
    let Ok(before) = load_details(storage, path) else {
        return;
    };
    let before = before.unwrap_or_default();
    // Kept as what the library does not write itself.
    let before = match metadata_read(&before) {
        true => before,
        false => match super::fallible::join(&[&before, "\n[book]\n", METADATA, " = on\n"]) {
            Ok(marked) => marked,
            Err(_) => return,
        },
    };
    // Too large, or no memory for it: not kept, as the next walk reads
    // the book again.
    let (Ok(ini), Ok(file)) = (
        super::details::try_details_text(book, &before),
        super::fallible::try_book_file(path),
    ) else {
        return;
    };
    #[expect(
        clippy::let_underscore_must_use,
        reason = "details not kept are read from the book again at the next walk"
    )]
    let _ = storage.write(&file, ini.as_bytes());
}
