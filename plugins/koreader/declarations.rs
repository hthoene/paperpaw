//! The KOReader exchange's native declarations: the folder of what both sides
//! had at the last exchange per book (the host moves and removes it with the
//! book), and the two events its work starts from: a book opened and a
//! change of annotations (`library_changed`).
use paperpaw_sdk::native::declare::{BookDataKind, Declarations, DeclareError, EventKind};

/// The token of the subscribed events.
pub const EVENTS_TOKEN: u32 = 1;
/// The per-book folder of what both sides had (`books/koreader/<key>/`).
pub const BOOK_DATA: &str = "koreader";

/// The book data and the events: a book opened and a change in the library
/// (the plugin reads its annotations bit). No event for a moved book: the host
/// moves the book data with it.
pub fn navigation<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.book_data(BOOK_DATA, BookDataKind::UserData)?;
    d.subscribe_event(EventKind::BookOpened, EVENTS_TOKEN)?;
    d.subscribe_event(EventKind::LibraryChanged, EVENTS_TOKEN)?;
    d.submit()?;
    Ok(())
}
