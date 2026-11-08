//! Checks of the host's answers before they are passed on, as
//! `doc_service/check.rs` does for a format's: a record of another kind, a
//! value outside its set, rows out of order or a repeated id is `ERR_INVALID`.
use super::super::BookRef;
use crate::native_host::registrations::{self as wire, Record};
use paperpaw_native::host_glue::ERR_INVALID;

const PPM: u32 = 1_000_000;
/// `lib_entry.flags` and `lib_book.flags`: bits 0-7.
/// The flags of a `lib_entry`; a `lib_book` also has bit 8 `missing`.
const ENTRY_FLAGS: u32 = 0xff;
const BOOK_FLAGS: u32 = ENTRY_FLAGS | 1 << 8;
/// `lib_position.flags`: has xpointer, has conversion, from sync.
const POSITION_FLAGS: u32 = 0b111;
/// `lib_annotation.flags`: noted, text cut, note cut, conversion, XPointers.
const ANNOTATION_FLAGS: u32 = 0b1_1111;
/// `lib_anchor.flags`: has a conversion, has XPointers.
const ANCHOR_FLAGS: u32 = 0b11;
/// `lib_entry.progress` of a book never opened.
const NEVER_OPENED: u32 = 255;

fn require(ok: bool) -> Result<(), i32> {
    if ok { Ok(()) } else { Err(ERR_INVALID) }
}

/// 0-100, or 255 for a book never opened.
fn progress_ok(progress: u32) -> bool {
    progress <= 100 || progress == NEVER_OPENED
}

/// The kinds of row a page holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Row {
    Entry,
    Facet,
    Annotation,
    Anchor,
    Field,
}

/// The rows already written to the page, decoded again for the comparison.
struct Earlier<'a>(&'a [u8]);
impl<'a> Iterator for Earlier<'a> {
    type Item = Record<'a>;
    fn next(&mut self) -> Option<Record<'a>> {
        if self.0.is_empty() {
            return None;
        }
        let (record, used) = wire::record_decode_prefix(self.0).ok()?;
        self.0 = &self.0[used..];
        Some(record)
    }
}

/// One row of a page: its kind, its values and its place among `before`, the
/// rows of this page written so far.
pub(super) fn row(kind: Row, record: &Record<'_>, before: &[u8]) -> Result<(), i32> {
    let mut earlier = Earlier(before);
    match (kind, record) {
        (Row::Entry, Record::ServiceLibEntry(row)) => {
            entry(row)?;
            require(!earlier.any(|r| matches!(r, Record::ServiceLibEntry(e) if e.key == row.key)))
        }
        (Row::Facet, Record::ServiceLibFacet(_)) => Ok(()),
        (Row::Annotation, Record::ServiceLibAnnotation(row)) => {
            annotation(row)?;
            // Unique ids, in reading order.
            let start = (row.start_section, row.start_offset);
            require(!earlier.any(|r| match r {
                Record::ServiceLibAnnotation(a) => {
                    a.id == row.id || (a.start_section, a.start_offset) > start
                }
                _ => true,
            }))
        }
        (Row::Anchor, Record::ServiceLibAnchor(row)) => {
            anchor(row)?;
            require(!earlier.any(|r| !matches!(r, Record::ServiceLibAnchor(a) if a.id != row.id)))
        }
        (Row::Field, Record::ServiceLibField(row)) => {
            require(row.id.contains('.') && !row.id.starts_with('.') && !row.id.ends_with('.'))?;
            require(!earlier.any(|r| !matches!(r, Record::ServiceLibField(f) if f.id != row.id)))
        }
        _ => Err(ERR_INVALID),
    }
}

/// `lib_entry`: progress, the flag bits, at most two badges.
pub(super) fn entry(row: &wire::ServiceLibEntry<'_>) -> Result<(), i32> {
    require(
        progress_ok(row.progress)
            && row.flags & !ENTRY_FLAGS == 0
            && row.badges.matches('\n').count() <= 1,
    )
}

pub(super) fn state(state: &wire::ServiceLibState) -> Result<(), i32> {
    // 0 no books yet, 1 walking, 2 ready; the generation is never 0 (a handle
    // needs one).
    require(
        state.state <= 2
            && state.walk_ppm <= PPM
            && state.generation != 0
            && (state.state != 0 || state.books == 0),
    )
}

/// A cut flag clear means the long form is whole: its length is the text's.
fn cut(flags: u32, bit: u32, text: &str, bytes: u32) -> bool {
    let len = u32::try_from(text.len()).unwrap_or(u32::MAX);
    bytes >= len && (flags & bit != 0 || bytes == len)
}

pub(super) fn book(book: &wire::ServiceLibBook<'_>, named: BookRef<'_>) -> Result<(), i32> {
    require(
        book.generation != 0
            && super::super::args::path_valid(book.path)
            && progress_ok(book.progress)
            && book.flags & !BOOK_FLAGS == 0
            && cut(book.flags, 1 << 5, book.title, book.title_bytes)
            && cut(book.flags, 1 << 6, book.author, book.author_bytes)
            && cut(book.flags, 1 << 7, book.series, book.series_bytes),
    )?;
    // Asked by handle: it is that book.
    match named {
        BookRef::Key { key, .. } => require(book.key == key),
        BookRef::Path(_) => Ok(()),
    }
}

/// `lib_text`: the rules of DOC's `check::text`. At most 1,024 bytes from
/// `offset`, cut only at a UTF-8 boundary (up to three bytes short of the
/// cap); the whole rest when it fits.
pub(super) fn text(
    text: &wire::ServiceLibText<'_>,
    kind: u32,
    index: u32,
    offset: u32,
) -> Result<(), i32> {
    let rest = text.total.checked_sub(offset).ok_or(ERR_INVALID)?;
    let len = u32::try_from(text.text.len()).map_err(|_| ERR_INVALID)?;
    let want = rest.min(1024);
    require(
        text.kind == kind
            && text.index == index
            && len <= want
            && len + 3 >= want
            && (rest > 1024 || len == rest),
    )
}

/// A cover: an image id of the caller (never 0), a picture with a size.
pub(super) fn cover(image: &wire::ServiceDocImage) -> Result<(), i32> {
    require(image.image != 0 && image.width != 0 && image.height != 0 && image.flags & !1 == 0)
}

pub(super) fn position(position: &wire::ServiceLibPosition<'_>) -> Result<(), i32> {
    let flags = position.flags;
    require(
        position.progress_ppm <= PPM
            && flags & !POSITION_FLAGS == 0
            // The XPointer and the conversion are there exactly when flagged.
            && (flags & 1 != 0) == !position.xpointer.is_empty()
            && (flags & 2 != 0 || position.conversion == 0),
    )
}

fn annotation(row: &wire::ServiceLibAnnotation<'_>) -> Result<(), i32> {
    let flags = row.flags;
    require(
        flags & !ANNOTATION_FLAGS == 0
            // A highlight runs forward; a bookmark's end is its own (write rule).
            && (row.kind != 1
                || (row.start_section, row.start_offset) <= (row.end_section, row.end_offset))
            && row.percent <= 100
            && (flags & 0b1000 != 0 || row.conversion == 0)
            && (flags & 1 != 0 || row.note.is_empty()),
    )
}

fn anchor(row: &wire::ServiceLibAnchor<'_>) -> Result<(), i32> {
    let flags = row.flags;
    // Only annotations that have a conversion or XPointers have an anchor.
    require(
        flags != 0
            && flags & !ANCHOR_FLAGS == 0
            && (flags & 1 != 0 || row.conversion == 0)
            && (flags & 2 != 0 || (row.start.is_empty() && row.end.is_empty()))
            && (flags & 2 == 0 || !row.start.is_empty()),
    )
}

/// `lib_key`: 16 and 32 lowercase hex digits.
pub(super) fn key(key: &wire::ServiceLibKey<'_>) -> Result<(), i32> {
    let hex = |text: &str, len: usize| {
        text.len() == len && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    };
    require(hex(key.key, 16) && hex(key.fingerprint, 32))
}
