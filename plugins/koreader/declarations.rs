//! The KOReader exchange's native declarations: the folder of what both sides
//! had at the last exchange per book (moved and removed with the book), and
//! the book events its work starts from.
use paperpaw_sdk::native::declare::{BookDataKind, Declarations, DeclareError, EventKind};

/// The token of the subscribed events.
pub const EVENTS_TOKEN: u32 = 1;
/// The per-book folder of what both sides had (`books/koreader/<key>/`).
pub const BOOK_DATA: &str = "koreader";

/// The book data and the events: a book opened, its annotations changed
/// (`library_changed`), and a book moved.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.book_data(BOOK_DATA, BookDataKind::UserData)?;
    d.subscribe_event(EventKind::BookOpened, EVENTS_TOKEN)?;
    d.subscribe_event(EventKind::LibraryChanged, EVENTS_TOKEN)?;
    d.submit()?;
    Ok(())
}
