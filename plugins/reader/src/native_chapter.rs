//! The chapter in the host's report (view slice R2b-3): the title of the
//! chapter of the page on show and the pages left in it. The model is pure
//! (the starts and the page numbers in, the title and the count out); `find`
//! asks the table through the document ops, page by page, and keeps only the
//! chapter wanted, never the table. Beta 3's `update_chapter`, `chapter_left`
//! are the reference. Every allocation is fallible.
use alloc::string::String;
use paperpaw_sdk::native::{
    Error, Host,
    document::{DocCursor, DocId, Documents},
};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer};
use paperpaw_ui_kit::text::copy;

use crate::native_actions::MAX_ENTRIES;

/// The longest chapter title a report carries, in bytes.
pub const MAX_TITLE: usize = 256;

/// The chapter a place is in: where it starts, where the next one starts, and
/// its title (empty before the first entry of the table).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chapter {
    start: (u32, u32),
    next: Option<(u32, u32)>,
    title: String,
}

/// `text` cut to [`MAX_TITLE`] bytes at a character boundary.
fn cut(text: &str) -> &str {
    let mut end = text.len().min(MAX_TITLE);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// The pages after page `page` (from 1) of `pages` to the end of its chapter,
/// the chapter's next start being on page `next` (from 0; `exact`: it starts
/// with that page, so the chapter ends on the page before) or `None` for the
/// last chapter, which ends with the book. Never more than the pages after
/// this one; `None` for a page outside the book.
#[must_use]
pub fn pages_left(page: u32, pages: u32, next: Option<(u32, bool)>) -> Option<u32> {
    if page == 0 || page > pages {
        return None;
    }
    let last = match next {
        None => pages - 1,
        Some((at, true)) => at.checked_sub(1)?,
        Some((at, false)) => at,
    };
    Some(last.saturating_sub(page - 1).min(pages - page))
}

/// The title the chapter's entry gets: whitespace at the ends dropped, cut.
fn title_of(text: &str) -> &str {
    cut(text.trim())
}

impl Chapter {
    /// The empty chapter from the book's start, before any entry is seen.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            start: (0, 0),
            next: None,
            title: String::new(),
        }
    }

    /// Takes one entry of the table (its exact start and title) into account
    /// for the chapter of `at`: the last start at or before `at` (the later of
    /// equal ones) is the chapter, the first one after it ends it. The table
    /// need not be in order. A title is copied only where it replaces the
    /// kept one.
    ///
    /// # Errors
    /// `NoMemory`; the chapter is then as it was.
    pub fn offer(&mut self, start: (u32, u32), title: &str, at: (u32, u32)) -> Result<(), Error> {
        if start <= at {
            if start >= self.start {
                self.title = copy(title_of(title))?;
                self.start = start;
            }
        } else if self.next.is_none_or(|n| start < n) {
            self.next = Some(start);
        }
        Ok(())
    }

    /// The chapter of `at` among `entries` (start, title).
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn scan<'a>(
        entries: impl Iterator<Item = ((u32, u32), &'a str)>,
        at: (u32, u32),
    ) -> Result<Self, Error> {
        let mut found = Self::empty();
        for (start, title) in entries {
            found.offer(start, title, at)?;
        }
        Ok(found)
    }

    /// Whether the chapter holds `at`.
    #[must_use]
    pub fn covers(&self, at: (u32, u32)) -> bool {
        self.start <= at && self.next.is_none_or(|n| at < n)
    }

    /// The title; empty where there is no chapter.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Where the next chapter starts.
    #[must_use]
    pub const fn next(&self) -> Option<(u32, u32)> {
        self.next
    }
}

/// What the status line's chapter item says at `at` with `chapter` known: its
/// title where it holds `at` and has one; `None` (the book's title is said)
/// before the first chapter, in a book without contents or while it is not
/// known.
#[must_use]
pub fn shown_title(chapter: Option<&Chapter>, at: (u32, u32)) -> Option<&str> {
    chapter
        .filter(|chapter| chapter.covers(at) && !chapter.title().is_empty())
        .map(Chapter::title)
}

/// The chapter of `at` in document `id`, asked of the table page by page (at
/// most [`MAX_ENTRIES`] entries); the starts of entries in the section of `at`
/// are made exact (`toc_target`), as Beta 3 did. Only the chapter is kept.
///
/// # Errors
/// `NoMemory`; a table the host cannot give is no chapter.
pub fn find(
    host: &Host<'_>,
    docs: &Documents,
    id: DocId,
    at: (u32, u32),
) -> Result<Chapter, Error> {
    let (mut input, mut out) = (buffer(256)?, buffer(SERVICE_BUFFER)?);
    let (mut ask, mut reply) = (buffer(256)?, buffer(SERVICE_BUFFER)?);
    let mut found = Chapter::empty();
    let mut cursor = DocCursor::start();
    let mut seen = 0;
    loop {
        let page = match docs.toc(host, id, cursor, &mut input, &mut out) {
            Ok(page) => page,
            // No table: no chapter.
            Err(Error::NotFound) => return Ok(found),
            Err(error) => return Err(error),
        };
        for row in page.rows() {
            seen += 1;
            if seen > MAX_ENTRIES {
                return Ok(found);
            }
            let place = row.position();
            let mut start = (place.section, place.offset);
            if row.approximate()
                && place.section == at.0
                && let Ok(exact) = docs.toc_target(host, id, row.index(), &mut ask, &mut reply)
            {
                start = (exact.section, exact.offset);
            }
            found.offer(start, row.title(), at)?;
        }
        match page.end().next() {
            Some(next) => cursor = next,
            None => return Ok(found),
        }
    }
}

#[cfg(test)]
mod tests;
