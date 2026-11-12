//! The bookmark of the page on show (`reader.toggle-bookmark`): a page that has
//! one loses it, any other gets one, kept as an annotation of the book in the
//! library (`docs/abi/library.md` 2.2). As in Beta 3 a bookmark is on the page
//! when it starts there, and all bookmarks of a page go together; a new one
//! keeps the page's text (the start of it), the progress in percent and the
//! time.
//!
//! The library changes an annotation set in a staged transaction at the
//! annotation generation that was read: where another change came between the
//! reading and the commit (the host answers `State`), the page is looked at
//! again once, as a sync would.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::layout::PageLine;
use paperpaw_sdk::native::{
    Error, Host,
    commands::CommandSender,
    library::{AnnotationCursor, AnnotationEdit, AnnotationKind, BookRef, Library, Transaction},
    ui::Notify,
};
use paperpaw_ui_kit::{
    Messages,
    mem::{SERVICE_BUFFER, buffer},
};

/// The most annotations looked through for one book; a book with more is
/// answered as it is read so far (the page's bookmark may then be missed).
pub const MAX_ROWS: usize = 8192;

/// The most bookmarks of one page that are removed in one go. A page with more
/// (only a hand-written file has) loses the rest with the next toggle.
pub const MAX_ON_PAGE: usize = 24;

/// The part of a book a page shows: from its start up to (not including) the
/// start of the next page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub start: (u32, u32),
    pub end: (u32, u32),
}

impl Range {
    /// Whether something that starts at `at` is on the page.
    #[must_use]
    pub fn holds(&self, at: (u32, u32)) -> bool {
        at >= self.start && at < self.end
    }

    /// Whether the page shows nothing (it is past the end).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

/// What the page on show says about itself.
#[derive(Clone, Copy, Debug)]
pub struct Page<'a> {
    pub range: Range,
    /// The text of the page, as a bookmark keeps the start of it.
    pub text: &'a str,
    /// The progress at the page's start, 0 to 100.
    pub percent: u32,
    /// The conversion the places belong to.
    pub conversion: u64,
    /// The time, in seconds since 1970 (0 where unknown).
    pub now: u64,
}

/// What a toggle did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Done {
    Added,
    Removed,
}

/// The bookmarks that are on `range`, and the annotation generation read.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub generation: u32,
    pub ids: Vec<u32>,
}

/// The progress `ppm` (millionths) as the whole percent a mark keeps.
#[must_use]
pub const fn percent_of(ppm: u32) -> u32 {
    let percent = (ppm as u64 + 5_000) / 10_000;
    if percent > 100 { 100 } else { percent as u32 }
}

/// Whether a row of the annotations is a bookmark of this page: a bookmark that
/// starts there, in the conversion the places are in (a row of another
/// conversion has places that mean something else; one with none is taken).
#[must_use]
pub fn is_here(
    range: &Range,
    kind: AnnotationKind,
    at: (u32, u32),
    conversion: Option<u64>,
    mine: u64,
) -> bool {
    kind == AnnotationKind::Bookmark && conversion.is_none_or(|c| c == mine) && range.holds(at)
}

/// What the text of a page needs of one of its lines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineView<'a> {
    pub text: &'a str,
    pub baseline: f32,
    pub section: u32,
    /// Where the first and the last word of the line are in the source.
    pub first: Option<(u32, u32)>,
    pub last: Option<(u32, u32)>,
    /// A filter put the line there: it is not the book's.
    pub inserted: bool,
}

impl<'a> LineView<'a> {
    /// What of `line` the text of a page needs.
    #[must_use]
    pub fn of(line: &'a PageLine) -> Self {
        Self {
            text: &line.text,
            baseline: line.baseline,
            section: line.section,
            first: line.words.first().copied(),
            last: line.words.last().copied(),
            inserted: line.inserted,
        }
    }
}

/// The text of the lines of a page as the book has it: a word broken at the end
/// of a line is whole again, without the hyphen the layout put there (a hyphen
/// of the book's own stays); what a filter inserted is left out.
///
/// # Errors
/// `NoMemory`.
pub fn page_text(lines: &[PageLine]) -> Result<String, Error> {
    text_of(lines.len(), |i| lines.get(i).map(LineView::of))
}

/// [`page_text`] over `count` lines that `line` gives by their number.
///
/// # Errors
/// `NoMemory`.
pub fn text_of<'a>(
    count: usize,
    line: impl Fn(usize) -> Option<LineView<'a>>,
) -> Result<String, Error> {
    let mut out = String::new();
    let mut joined = false;
    for i in 0..count {
        let Some(this) = line(i) else {
            break;
        };
        if this.inserted {
            continue;
        }
        if i > 0 && !joined && !out.is_empty() {
            push(&mut out, " ")?;
        }
        let next = line(i + 1);
        // A drop capital: the next line starts no lower.
        if next.is_some_and(|n| n.baseline <= this.baseline) {
            // The word it starts goes on beside it.
            push(&mut out, this.text)?;
            joined = true;
            continue;
        }
        let following = next
            .filter(|n| n.section == this.section)
            .and_then(|n| n.first);
        joined = this.text.ends_with('-')
            && matches!((this.last, following), (Some((_, end)), Some((start, _))) if start == end);
        let mut text = this.text;
        if let Some((start, end)) = this.last.filter(|_| joined) {
            let word = text.rsplit(' ').next().unwrap_or(text);
            if (end.saturating_sub(start) as usize) < word.len() {
                text = text.strip_suffix('-').unwrap_or(text);
            }
        }
        push(&mut out, text)?;
    }
    Ok(out)
}

fn push(out: &mut String, text: &str) -> Result<(), Error> {
    out.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
    out.push_str(text);
    Ok(())
}

/// `text` on one line with its white space as spaces, in at most `max` bytes (cut
/// at a character; Beta 3 ended a cut text in `…`, which a native edit does not:
/// the library cuts at its own limit and says it did).
///
/// # Errors
/// `NoMemory`.
pub fn snippet(text: &str, max: usize) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve(text.len().min(max))
        .map_err(|_| Error::NoMemory)?;
    for c in text.chars() {
        let c = if c.is_whitespace() { ' ' } else { c };
        if out.len() + c.len_utf8() > max {
            break;
        }
        out.push(c);
    }
    Ok(out)
}

/// The bookmarks of the book at `book` that are on `range`.
///
/// # Errors
/// `State` for a book the library does not know, `NoMemory`, the host's refusals.
pub fn find(
    host: &Host<'_>,
    library: &Library,
    book: BookRef<'_>,
    (range, conversion): (&Range, u64),
) -> Result<Found, Error> {
    let (mut input, mut out) = (buffer(1024)?, buffer(SERVICE_BUFFER)?);
    let mut found = Found::default();
    let mut cursor = AnnotationCursor::start();
    let mut seen = 0usize;
    loop {
        let page = library.annotations(host, book, cursor, &mut input, &mut out)?;
        found.generation = page.end().generation();
        for row in page.rows() {
            seen += 1;
            let row = row?;
            let at = (row.start_section(), row.start_offset());
            if is_here(range, row.kind(), at, row.conversion(), conversion)
                && found.ids.len() < MAX_ON_PAGE
            {
                found.ids.try_reserve(1).map_err(|_| Error::NoMemory)?;
                found.ids.push(row.id());
            }
        }
        match page.end().next() {
            Some(next) if seen < MAX_ROWS => cursor = next,
            _ => return Ok(found),
        }
    }
}

/// Toggles the bookmark of `page` of the book at `path`.
///
/// # Errors
/// What the library refuses; `NoMemory`. Nothing is half done: a refused edit
/// aborts the transaction.
pub fn toggle(
    host: &Host<'_>,
    library: &Library,
    path: &str,
    page: &Page<'_>,
) -> Result<Done, Error> {
    if page.range.is_empty() {
        return Err(Error::NotFound);
    }
    let book = BookRef::path(path)?;
    let text = snippet(page.text, 512)?;
    // Another change between the read and the commit: look again once.
    match attempt(host, library, book, page, &text) {
        Err(Error::State) => attempt(host, library, book, page, &text),
        done => done,
    }
}

fn attempt(
    host: &Host<'_>,
    library: &Library,
    book: BookRef<'_>,
    page: &Page<'_>,
    text: &str,
) -> Result<Done, Error> {
    let found = find(host, library, book, (&page.range, page.conversion))?;
    let (mut input, mut out) = (buffer(4096)?, buffer(4096)?);
    let mut transaction = library.edit_begin(
        host,
        book,
        found.generation.max(1),
        &mut input[..1024],
        &mut out,
    )?;
    let staged = stage(
        host,
        library,
        &mut transaction,
        (&found, page, text),
        (&mut input, &mut out),
    );
    if let Err(error) = staged {
        // The host ended the transaction on an error of the commit itself.
        library
            .edit_abort(host, &mut transaction, &mut input[..256], &mut out[..64])
            .ok();
        return Err(error);
    }
    Ok(if found.ids.is_empty() {
        Done::Added
    } else {
        Done::Removed
    })
}

fn stage(
    host: &Host<'_>,
    library: &Library,
    transaction: &mut Transaction,
    (found, page, text): (&Found, &Page<'_>, &str),
    (input, out): (&mut [u8], &mut [u8]),
) -> Result<(), Error> {
    if found.ids.is_empty() {
        let edit =
            AnnotationEdit::add(AnnotationKind::Bookmark, page.range.start, page.range.start)
                .percent(page.percent.min(100))
                .conversion(page.conversion)
                .times(page.now, page.now)
                .text(text);
        library.edit_add(host, transaction, &[edit], input, out)?;
    } else {
        // One edit a call: a page has one bookmark but a hand-written file.
        for id in &found.ids {
            let edit = AnnotationEdit::remove(*id)?;
            library.edit_add(host, transaction, &[edit], input, out)?;
        }
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

/// Says that a bookmark, highlight or note could not be kept, by the library's
/// refusal `error` (`native_place::marks_refused`); a message that cannot be
/// made or shown is dropped.
pub fn say_not_saved(host: &Host<'_>, error: Error) {
    let key = crate::native_place::marks_refused(error);
    let Ok(text) = Messages::new().and_then(|mut m| m.copy(host, key)) else {
        return;
    };
    let Ok(note) = Notify::new(&text) else {
        return;
    };
    let mut scratch = [0; 512];
    CommandSender::new(host, &mut scratch).notify(note).ok();
}

#[cfg(test)]
mod tests;
