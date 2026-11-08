//! Ops 15 `annotations` and 16 `anchors`, and the whole texts of op 4 (kinds
//! 5 and 6): the rows of a book's annotation file
//! (`paperpaw_sdk::annotations::try_rows`), a page of them at a time.
//!
//! A page scans the file once: rows before the cursor are skipped, rows are
//! offered until the page is full, and the scan goes on to count them all
//! (`total`), as nothing is held. The generation on the wire is the file's
//! own number ([`annotations::generation_of`]), never 0.
use super::{fail, valid_prefix, zeroed};
use crate::abi7::context::RowsEnd;
use crate::native_host::lib_service::{One, Rows};
use crate::native_host::registrations::{self as wire, Record};
use core::ops::ControlFlow;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE};
use paperpaw_platform::Storage;
use paperpaw_sdk::annotations::{self, Kind, Row, Window, Windows};

/// Bytes of a text and of a note a `lib_annotation` carries.
const TEXT: u32 = 512;
const NOTE: u32 = 512;
/// Bytes of an XPointer a `lib_anchor` carries.
const XPOINTER: u32 = 1024;
/// Bytes of a part of a long text or note (`lib_text`).
const PART: u32 = 1024;

/// `lib_annotation.flags`.
const NOTED: u32 = 1;
const TEXT_CUT: u32 = 1 << 1;
const NOTE_CUT: u32 = 1 << 2;
const HAS_CONVERSION: u32 = 1 << 3;
const HAS_XPOINTERS: u32 = 1 << 4;

/// Whether the row's XPointers fit the wire: it has a start and neither is
/// over 1,024 bytes. One over (the file was changed by something other than a
/// writer of ours, which refuses it) is as if the row had none: the page it is
/// on still answers. Op 15 knows the lengths only; op 16 also needs the text
/// to be whole UTF-8 to carry it.
fn xpointers_fit(row: &Row<'_>) -> bool {
    row.start_xpointer.whole > 0 && row.start_xpointer.whole.max(row.end_xpointer.whole) <= XPOINTER
}

/// The kind on the wire: 0 bookmark, 1 highlight, 2 + the letter of another.
fn kind(kind: Kind) -> u32 {
    match kind {
        Kind::Bookmark => 0,
        Kind::Highlight => 1,
        Kind::Other(code) => 2 + u32::from(code),
        _ => 2,
    }
}

/// The generation the file has on the wire, and the request checked against it.
fn generation(storage: &dyn Storage, path: &str, asked: u32) -> Result<u32, i32> {
    let head = annotations::try_head(storage, path).map_err(fail)?;
    if asked != 0 && asked != head.generation {
        return Err(ERR_STATE);
    }
    Ok(head.generation)
}

/// A page of rows `make` builds from the rows of the file: those from `cursor`
/// on, while `visit` takes them.
fn page(
    storage: &dyn Storage,
    path: &str,
    cursor: u32,
    asked: u32,
    windows: &Windows,
    visit: Rows<'_>,
    make: &dyn for<'r> Fn(&'r Row<'_>) -> Option<Result<Record<'r>, i32>>,
) -> Result<RowsEnd, i32> {
    let generation = generation(storage, path, asked)?;
    let mut scratch = zeroed(windows.scratch().ok_or(ERR_NO_SPACE)?)?;
    // Rows that count (a row without an anchor is no anchor), where the page
    // is, and why it stopped.
    let (mut counted, mut next, mut full) = (0_u32, cursor, false);
    let mut failure = None;
    annotations::try_rows(storage, path, windows, &mut scratch, &mut |_, row| {
        let Some(made) = make(row) else {
            return ControlFlow::Continue(());
        };
        let index = counted;
        counted = counted.saturating_add(1);
        if index < cursor || full || failure.is_some() {
            return ControlFlow::Continue(());
        }
        match made {
            Err(error) => failure = Some(error),
            Ok(record) => {
                if visit(record).is_break() {
                    full = true;
                } else {
                    next = index.saturating_add(1);
                }
            }
        }
        ControlFlow::Continue(())
    })
    .map_err(fail)?;
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(RowsEnd {
        total: counted,
        generation,
        next: next.min(counted),
    })
}

/// `part` of a text as a `lib_annotation` shows it: the text and whether it
/// was cut.
fn cut(part: annotations::Part<'_>) -> (&str, bool) {
    (part.text, part.whole as usize > part.text.len())
}

pub(super) fn annotations(
    storage: &dyn Storage,
    path: &str,
    cursor: u32,
    asked: u32,
    visit: Rows<'_>,
) -> Result<RowsEnd, i32> {
    let windows = Windows::new()
        .text(Window::new(0, TEXT))
        .note(Window::new(0, NOTE))
        .key(Window::new(0, annotations::KEY_MAX as u32));
    page(storage, path, cursor, asked, &windows, visit, &|row| {
        let (text, text_cut) = cut(row.text);
        let (note, note_cut) = cut(row.note);
        let mut flags = 0;
        for (set, bit) in [
            (row.note.whole > 0, NOTED),
            (text_cut, TEXT_CUT),
            (note_cut, NOTE_CUT),
            (row.conversion.is_some(), HAS_CONVERSION),
            (xpointers_fit(row), HAS_XPOINTERS),
        ] {
            if set {
                flags |= bit;
            }
        }
        Some(Ok(Record::ServiceLibAnnotation(
            wire::ServiceLibAnnotation {
                id: row.id,
                kind: kind(row.kind),
                start_section: row.start.section,
                start_offset: row.start.offset,
                end_section: row.end.section,
                end_offset: row.end.offset,
                flags,
                percent: u32::from(row.percent),
                conversion: row.conversion.unwrap_or(0),
                created: u64::try_from(row.created).unwrap_or(0),
                updated: u64::try_from(row.updated).unwrap_or(0),
                client_key: row.key.text,
                text,
                note,
            },
        )))
    })
}

pub(super) fn anchors(
    storage: &dyn Storage,
    path: &str,
    cursor: u32,
    asked: u32,
    visit: Rows<'_>,
) -> Result<RowsEnd, i32> {
    let windows = Windows::new().xpointers(Window::new(0, XPOINTER), Window::new(0, XPOINTER));
    page(storage, path, cursor, asked, &windows, visit, &|row| {
        let conversion = row.conversion.is_some();
        let whole = |p: annotations::Part<'_>| p.whole as usize == p.text.len();
        let xpointers = xpointers_fit(row) && whole(row.start_xpointer) && whole(row.end_xpointer);
        if !conversion && !xpointers {
            // Only annotations that have an anchor are rows (XPointers that
            // cannot be carried are no anchor).
            return None;
        }
        let (start, end) = if xpointers {
            (row.start_xpointer.text, row.end_xpointer.text)
        } else {
            ("", "")
        };
        Some(Ok(Record::ServiceLibAnchor(wire::ServiceLibAnchor {
            id: row.id,
            flags: u32::from(conversion) | (u32::from(xpointers) << 1),
            conversion: row.conversion.unwrap_or(0),
            start,
            end,
        })))
    })
}

/// Op 4, kinds 5 (the text) and 6 (the note) of the annotation `id`: a part
/// of at most 1,024 bytes from `offset`.
pub(super) fn text(
    storage: &dyn Storage,
    path: &str,
    at: &wire::ServiceLibTextAt,
    visit: One<'_>,
) -> Result<(), i32> {
    let window = Window::new(at.offset, PART);
    let windows = if at.kind == 5 {
        Windows::new().text(window)
    } else {
        Windows::new().note(window)
    };
    let mut scratch = zeroed(windows.scratch().ok_or(ERR_NO_SPACE)?)?;
    let mut found = None;
    annotations::try_rows(storage, path, &windows, &mut scratch, &mut |_, row| {
        if row.id != at.index {
            return ControlFlow::Continue(());
        }
        let part = if at.kind == 5 { row.text } else { row.note };
        found = Some((part.whole, part.text.len()));
        ControlFlow::Break(())
    })
    .map_err(fail)?;
    let (total, _) = found.ok_or(ERR_NOT_FOUND)?;
    if at.offset > total {
        return Err(ERR_INVALID);
    }
    // The bytes are in the scratch's window (the one that was asked for).
    let bytes = &scratch[..(total - at.offset).min(PART) as usize];
    // The text starts at a character: an offset inside one is the caller's.
    if bytes.first().is_some_and(|b| b & 0xc0 == 0x80) {
        return Err(ERR_INVALID);
    }
    let shown = valid_prefix(bytes);
    if shown.is_empty() && total > at.offset {
        return Err(ERR_INVALID);
    }
    visit(Record::ServiceLibText(wire::ServiceLibText {
        kind: at.kind,
        index: at.index,
        total,
        text: shown,
    }));
    Ok(())
}
