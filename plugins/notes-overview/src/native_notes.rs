//! The rows of a book's notes, a page of [`MOST_NOTES`] at a time. The notes
//! come from the library in reading order, a few to a reply; a screen page
//! starts at a [`Start`] (the cursor of a reply and how many of its rows came
//! before) and the next page's start is kept, so *Next notes* reads on from
//! it and *Previous notes* from the start before. Only one page of rows is
//! held, so a book of any length costs the same memory.
use crate::model::{self, Kind};
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error, Host,
    library::{AnnotationCursor, AnnotationKind, BookRef, Library},
};
use paperpaw_ui_kit::{Item, mem::push};

/// Notes of one book shown on a page. With the texts cut at
/// [`model::TEXT_MAX`] and [`model::NOTE_MAX`] a row costs at most 350 bytes
/// (`ROW_WORST` in the tests), so a page holds 100 x that, about 44 KiB.
pub const MOST_NOTES: usize = 100;
/// A pass over a book's notes for one page ends after this many replies (a
/// reply holds a few notes, so a page of 100 has a dozen; even a reply of one
/// note, 125 for a page, is within it), so a host that never ends cannot hold
/// the screen.
pub const MOST_REPLIES: usize = 256;
/// Pages of one book that are reachable: 25,600 notes. Further notes are in
/// the export only.
pub const MOST_SCREEN_PAGES: usize = 256;

/// The most a row of a note takes: the item (three strings and flags), the
/// quoted text (two curly quotes, 6 bytes), the detail (the note or the kind,
/// then `· 100 %`, at most 16 bytes more) and 8 bytes of allocator header for
/// each of the three strings.
#[cfg(test)]
pub const ROW_WORST: usize =
    core::mem::size_of::<Item>() + model::TEXT_MAX + 6 + model::NOTE_MAX + 16 + 3 * 8;

/// Where a page of notes starts: the cursor of a library reply and how many
/// of that reply's rows come before the page.
#[derive(Debug, Clone, Copy, Default)]
pub struct Start<C> {
    cursor: C,
    skip: usize,
}

/// What the rows of a notes screen are: a *Previous notes* row if the page
/// is not the first, the notes, a *Next notes* row if more follow.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Layout {
    /// 1 where the *Previous notes* row is the first, else 0.
    pub before: usize,
    /// How many notes follow.
    pub notes: usize,
    /// Whether the *Next notes* row is the last.
    pub more: bool,
}

/// What choosing a row of a notes screen does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Show the page before.
    Previous,
    /// Show the page after.
    Next,
    /// A note.
    Note,
}

impl Layout {
    /// What choosing row `index` does.
    #[must_use]
    pub const fn choice(self, index: usize) -> Choice {
        if self.before == 1 && index == 0 {
            Choice::Previous
        } else if self.more && index == self.before + self.notes {
            Choice::Next
        } else {
            Choice::Note
        }
    }

    /// Where the note on row `index` starts, out of the `places` of the page:
    /// `None` for a row that is not a note.
    #[must_use]
    pub fn place(self, places: &[Place], index: usize) -> Option<Place> {
        if self.choice(index) != Choice::Note {
            return None;
        }
        places.get(index.checked_sub(self.before)?).copied()
    }
}

/// One note as the library tells it.
#[derive(Debug, Clone, Copy)]
pub struct Row<'a> {
    /// Highlight, bookmark or a kind a newer reader keeps.
    pub kind: Kind,
    /// The text, or the bookmark's label (the library's preview).
    pub text: &'a str,
    /// The reader's own note on a highlight.
    pub note: &'a str,
    /// How far in the book, 0 to 100.
    pub percent: u8,
    /// Where the note starts: the section and the offset in it (the reader
    /// opens the book there).
    pub place: Place,
}

/// Where a note starts in its book: section and offset, as the library keeps
/// them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Place {
    pub section: u32,
    pub offset: u32,
}

/// Whether the reading of a reply goes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Go,
    Stop,
}

/// What a page asks of the library.
pub trait Replies {
    /// A position in the book's notes.
    type Cursor: Copy + Default;

    /// One reply of notes from `cursor`: `each` is called with the index of a
    /// row in the reply and the row, for every row, until it says `Stop`.
    /// Returns where the next reply starts, or `None` at the last.
    ///
    /// # Errors
    /// `State` for a stale cursor, `NotFound` for a book that is gone; the
    /// error of `each`.
    fn reply(
        &mut self,
        cursor: Self::Cursor,
        each: &mut dyn FnMut(usize, &Row<'_>) -> Result<Flow, Error>,
    ) -> Result<Option<Self::Cursor>, Error>;
}

/// What a row on the screen shows.
#[derive(Debug, Clone, Copy)]
pub enum Show<'a, 'b> {
    /// The row that goes to the page before.
    Previous,
    /// The row that goes to the page after.
    More,
    /// A note.
    Note(&'a Row<'b>),
}

/// The rows of one page.
#[derive(Debug)]
pub struct Page<C> {
    pub items: Vec<Item>,
    /// Where each note of the page starts, in the order of the notes (not
    /// the *Previous notes* and *Next notes* rows).
    pub places: Vec<Place>,
    pub layout: Layout,
    /// Where the page after this one starts, if there are more notes.
    pub next: Option<Start<C>>,
}

/// The rows of the page that starts at `pages[last]` (`pages` is the start of
/// every page shown so far, the last being this one), each made by `make`.
/// Kinds a newer reader keeps are not shown.
///
/// # Errors
/// `State` for a cursor the library no longer knows (the book's notes
/// changed): the caller starts again from the first page. `NotFound` for a
/// book that is gone; `NoMemory`; the host's other refusals.
pub fn page<R: Replies>(
    source: &mut R,
    pages: &[Start<R::Cursor>],
    make: &mut dyn FnMut(Show<'_, '_>) -> Result<Item, Error>,
) -> Result<Page<R::Cursor>, Error> {
    let start = pages.last().copied().unwrap_or_default();
    let mut items = Vec::new();
    let mut places = Vec::new();
    let back = pages.len() > 1;
    if back {
        push(&mut items, make(Show::Previous)?)?;
    }
    let (mut cursor, mut skip) = (start.cursor, start.skip);
    let mut next = None;
    let mut ended = false;
    for _ in 0..MOST_REPLIES {
        let mut full_at = None;
        let after = source.reply(cursor, &mut |at, row| {
            if at < skip || row.kind == Kind::Other {
                return Ok(Flow::Go);
            }
            if items.len() - usize::from(back) >= MOST_NOTES {
                full_at = Some(at);
                return Ok(Flow::Stop);
            }
            push(&mut places, row.place)?;
            push(&mut items, make(Show::Note(row))?)?;
            Ok(Flow::Go)
        })?;
        if let Some(at) = full_at {
            next = Some(Start { cursor, skip: at });
            ended = true;
            break;
        }
        skip = 0;
        match after {
            Some(after) => cursor = after,
            None => {
                ended = true;
                break;
            }
        }
    }
    if !ended {
        // Replies ran out before the page did: the notes go on at `cursor`.
        next = Some(Start { cursor, skip: 0 });
    }
    // Past the last page that can be reached the notes are in the export only.
    if pages.len() >= MOST_SCREEN_PAGES {
        next = None;
    }
    let notes = items.len() - usize::from(back);
    if next.is_some() {
        push(&mut items, make(Show::More)?)?;
    }
    Ok(Page {
        places,
        layout: Layout {
            before: usize::from(back),
            notes,
            more: next.is_some(),
        },
        items,
        next,
    })
}

/// [`Replies`] over the library service.
#[derive(Debug)]
pub struct HostReplies<'a, 'h> {
    pub library: &'a Library,
    pub host: &'a Host<'h>,
    pub book: BookRef<'a>,
    pub input: &'a mut [u8],
    pub out: &'a mut [u8],
}

impl Replies for HostReplies<'_, '_> {
    type Cursor = AnnotationCursor;

    fn reply(
        &mut self,
        cursor: AnnotationCursor,
        each: &mut dyn FnMut(usize, &Row<'_>) -> Result<Flow, Error>,
    ) -> Result<Option<AnnotationCursor>, Error> {
        let reply = self
            .library
            .annotations(self.host, self.book, cursor, self.input, self.out)?;
        for (at, row) in reply.rows().enumerate() {
            let row = row?;
            let shown = Row {
                kind: match row.kind() {
                    AnnotationKind::Highlight => Kind::Highlight,
                    AnnotationKind::Bookmark => Kind::Bookmark,
                    _ => Kind::Other,
                },
                text: model::cut(row.text(), model::TEXT_MAX),
                note: model::cut(row.note(), model::NOTE_MAX),
                percent: u8::try_from(row.percent().min(100)).unwrap_or(100),
                place: Place {
                    section: row.start_section(),
                    offset: row.start_offset(),
                },
            };
            if each(at, &shown)? == Flow::Stop {
                break;
            }
        }
        Ok(reply.end().next())
    }
}
