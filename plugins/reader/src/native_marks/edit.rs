//! Making and removing highlights and their notes in the library (`library.md`
//! 2.2, ops 15 and 17-21), as Beta 3's `annotations::add/remove/set_note` did
//! for the book's annotation file: *Highlight* on text that has a highlight
//! removes the highlights there, else adds one; *Note* makes the highlight if
//! there is none, and the note typed is set on it (an empty one takes it away).
//!
//! The library changes a set in a staged transaction at the annotation
//! generation that was read; where another change came between the read and the
//! commit (`State`) the work is done again once, as a sync would. Nothing is
//! half done: a refused edit aborts the transaction.
use super::Span;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error, Host,
    library::{
        AnnotationCursor, AnnotationEdit, AnnotationKind, BookRef, Library, MAX_EDITS_PER_CALL,
        Transaction,
    },
};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer};

/// The most highlights removed in one go; a text with more (only a
/// hand-written file has) loses the rest with the next *Remove highlight*.
pub const MAX_REMOVED: usize = 24;

/// Longest text a highlight keeps (the library cuts at its own limit).
pub const TEXT_MAX: usize = 512;

/// What a new highlight is.
#[derive(Clone, Copy, Debug)]
pub struct New<'a> {
    /// The chosen words, from the start of the first to the end of the last.
    pub span: Span,
    /// The progress at its start, in whole percent (0 to 100).
    pub percent: u32,
    /// The conversion the places belong to.
    pub conversion: u64,
    /// The time, in seconds since 1970 (0 where unknown).
    pub now: u64,
    /// The chosen text, on one line (see [`snippet`]).
    pub text: &'a str,
}

/// A highlight of the library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: u32,
    pub span: Span,
    pub percent: u32,
    pub created: u64,
    pub text: String,
    pub note: String,
}

/// The highlights that overlap a stretch, and the generation they were read at.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Overlap {
    pub generation: u32,
    /// The ids of every one (at most [`MAX_REMOVED`]).
    pub ids: Vec<u32>,
    /// The first one, whole: where the note goes.
    pub first: Option<Row>,
}

/// What *Highlight* did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toggled {
    Added,
    Removed,
}

/// `text` on one line with its white space as spaces, in at most [`TEXT_MAX`] bytes.
///
/// # Errors
/// `NoMemory`.
pub fn snippet(text: &str) -> Result<String, Error> {
    crate::native_bookmark::snippet(text, TEXT_MAX)
}

/// Whether two stretches share a place (a stretch of no length shares none).
#[must_use]
pub fn overlaps(a: &Span, b: &Span) -> bool {
    a.start < a.end && b.start < b.end && a.start < b.end && b.start < a.end
}

/// The highlights of the book at `book` that overlap `span`, in the conversion
/// `mine` (a row of another one is left out, one with none is taken).
///
/// # Errors
/// `State` for a book the library does not know, `NoMemory`, the host's refusals.
pub fn overlapping(
    host: &Host<'_>,
    library: &Library,
    book: BookRef<'_>,
    (span, mine): (Span, u64),
) -> Result<Overlap, Error> {
    let (mut input, mut out) = (buffer(256)?, buffer(SERVICE_BUFFER)?);
    let mut found = Overlap::default();
    let mut cursor = AnnotationCursor::start();
    let mut seen = 0usize;
    loop {
        let page = library.annotations(host, book, cursor, &mut input, &mut out)?;
        found.generation = page.end().generation();
        for row in page.rows() {
            seen += 1;
            let row = row?;
            let there = Span {
                start: (row.start_section(), row.start_offset()),
                end: (row.end_section(), row.end_offset()),
                noted: row.noted(),
            };
            if row.kind() != AnnotationKind::Highlight
                || !row.conversion().is_none_or(|c| c == mine)
                || !overlaps(&there, &span)
                || found.ids.len() >= MAX_REMOVED
            {
                continue;
            }
            found.ids.try_reserve(1).map_err(|_| Error::NoMemory)?;
            found.ids.push(row.id());
            if found.first.is_none() {
                found.first = Some(Row {
                    id: row.id(),
                    span: there,
                    percent: row.percent(),
                    created: row.created(),
                    text: crate::native_bookmark::snippet(row.text(), TEXT_MAX)?,
                    note: crate::native_bookmark::snippet(row.note(), TEXT_MAX)?,
                });
            }
        }
        match page.end().next() {
            Some(next) if seen < super::MAX_ROWS => cursor = next,
            _ => return Ok(found),
        }
    }
}

/// What a transaction changes: the edits staged, in the order given.
fn commit(
    host: &Host<'_>,
    library: &Library,
    book: BookRef<'_>,
    generation: u32,
    edits: &[AnnotationEdit<'_>],
) -> Result<(), Error> {
    let (mut input, mut out) = (buffer(4096)?, buffer(4096)?);
    let mut transaction =
        library.edit_begin(host, book, generation.max(1), &mut input[..1024], &mut out)?;
    let staged = stage(
        host,
        library,
        &mut transaction,
        edits,
        (&mut input, &mut out),
    );
    if let Err(error) = staged {
        // The host ended the transaction on an error of the commit itself.
        library
            .edit_abort(host, &mut transaction, &mut input[..256], &mut out[..64])
            .ok();
        return Err(error);
    }
    Ok(())
}

fn stage(
    host: &Host<'_>,
    library: &Library,
    transaction: &mut Transaction,
    edits: &[AnnotationEdit<'_>],
    (input, out): (&mut [u8], &mut [u8]),
) -> Result<(), Error> {
    for chunk in edits.chunks(MAX_EDITS_PER_CALL) {
        library.edit_add(host, transaction, chunk, input, out)?;
    }
    library
        .edit_commit(
            host,
            transaction,
            input.get_mut(..256).ok_or(Error::NoSpace)?,
            out,
        )
        .map(|_| ())
}

/// The new highlight as an edit.
fn adding<'a>(new: &New<'a>, note: &'a str) -> AnnotationEdit<'a> {
    AnnotationEdit::add(AnnotationKind::Highlight, new.span.start, new.span.end)
        .percent(new.percent.min(100))
        .conversion(new.conversion)
        .times(new.now, new.now)
        .text(new.text)
        .note(note)
}

/// *Highlight*: the highlights that overlap `new` are removed; where there are
/// none the text is highlighted.
///
/// # Errors
/// What the library refuses; `NoMemory`.
pub fn toggle(
    host: &Host<'_>,
    library: &Library,
    path: &str,
    new: &New<'_>,
) -> Result<Toggled, Error> {
    let book = BookRef::path(path)?;
    // Another change between the read and the commit: look again once.
    match toggle_once(host, library, book, new) {
        Err(Error::State) => toggle_once(host, library, book, new),
        done => done,
    }
}

fn toggle_once(
    host: &Host<'_>,
    library: &Library,
    book: BookRef<'_>,
    new: &New<'_>,
) -> Result<Toggled, Error> {
    let found = overlapping(host, library, book, (new.span, new.conversion))?;
    if found.ids.is_empty() {
        commit(host, library, book, found.generation, &[adding(new, "")])?;
        return Ok(Toggled::Added);
    }
    let mut edits: Vec<AnnotationEdit<'_>> = Vec::new();
    edits
        .try_reserve_exact(found.ids.len())
        .map_err(|_| Error::NoMemory)?;
    for id in &found.ids {
        edits.push(AnnotationEdit::remove(*id)?);
    }
    commit(host, library, book, found.generation, &edits)?;
    Ok(Toggled::Removed)
}

/// What *Note* starts with: the stretch the note belongs to (the highlight
/// there, made if there was none) and the note it has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub span: Span,
    pub note: String,
}

/// *Note*: the highlight that overlaps `new` (the first), made where there is
/// none; the note it has is given back to be edited.
///
/// # Errors
/// What the library refuses; `NoMemory`.
pub fn begin_note(
    host: &Host<'_>,
    library: &Library,
    path: &str,
    new: &New<'_>,
) -> Result<Target, Error> {
    let book = BookRef::path(path)?;
    match begin_once(host, library, book, new) {
        Err(Error::State) => begin_once(host, library, book, new),
        done => done,
    }
}

fn begin_once(
    host: &Host<'_>,
    library: &Library,
    book: BookRef<'_>,
    new: &New<'_>,
) -> Result<Target, Error> {
    let found = overlapping(host, library, book, (new.span, new.conversion))?;
    if let Some(row) = found.first {
        return Ok(Target {
            span: row.span,
            note: row.note,
        });
    }
    commit(host, library, book, found.generation, &[adding(new, "")])?;
    Ok(Target {
        span: new.span,
        note: String::new(),
    })
}

/// Sets the note of the highlight that starts where `target` does (an empty
/// note takes it away). A highlight that is gone leaves nothing to do.
///
/// # Errors
/// What the library refuses; `NoMemory`.
pub fn set_note(
    host: &Host<'_>,
    library: &Library,
    path: &str,
    (target, conversion, now): (Span, u64, u64),
    note: &str,
) -> Result<(), Error> {
    let book = BookRef::path(path)?;
    match note_once(host, library, book, (target, conversion, now), note) {
        Err(Error::State) => note_once(host, library, book, (target, conversion, now), note),
        done => done,
    }
}

fn note_once(
    host: &Host<'_>,
    library: &Library,
    book: BookRef<'_>,
    (target, conversion, now): (Span, u64, u64),
    note: &str,
) -> Result<(), Error> {
    let found = overlapping(host, library, book, (target, conversion))?;
    let Some(row) = found.first else {
        return Ok(());
    };
    let edit = AnnotationEdit::replace(
        row.id,
        AnnotationKind::Highlight,
        row.span.start,
        row.span.end,
    )?
    .percent(row.percent.min(100))
    .conversion(conversion)
    .times(row.created, now.max(row.created))
    .text(&row.text)
    .note(note);
    commit(host, library, book, found.generation, &[edit])
}
