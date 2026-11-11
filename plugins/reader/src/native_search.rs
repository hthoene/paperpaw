//! What the search screen decides without a host (interface 6: `search.rs`):
//! the text looked for and where it is in a block, the snippet round a hit and
//! how it is cut to the row, the hits kept (at most 100, a short snippet each)
//! and the background scan that reads the book a few blocks at a time while the
//! list fills. The book is a [`Blocks`] the screen backs with `core.document`;
//! drawing and choosing a hit (which goes back and goes to the place, marking
//! the match) are the screen's. Every allocation is fallible; the interface-6
//! `search.rs` is the reference until it goes at activation.
use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::text::concat;

use crate::native_open::Place;

/// Most hits kept; more are counted as "100+".
pub const MAX_HITS: usize = 100;
/// Blocks read per background step.
pub const BLOCKS_PER_STEP: usize = 24;
/// Characters of context before and after a match in its snippet.
pub const BEFORE: usize = 24;
/// See [`BEFORE`].
pub const AFTER: usize = 48;
/// Characters of the text looked for that count: a longer query is cut here, so
/// that a snippet (context and match) and the hits kept stay small.
pub const MAX_NEEDLE: usize = 32;

/// The lower case of `c` (its first character, which suffices to compare).
fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// The text looked for: its characters in lower case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Needle(Vec<char>);

impl Needle {
    /// The needle of `query`, without the space round it, at most
    /// [`MAX_NEEDLE`] characters.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn new(query: &str) -> Result<Self, Error> {
        let query = query.trim();
        let mut chars = Vec::new();
        chars
            .try_reserve_exact(query.chars().count().min(MAX_NEEDLE))
            .map_err(|_| Error::NoMemory)?;
        chars.extend(query.chars().take(MAX_NEEDLE).map(lower));
        Ok(Self(chars))
    }

    /// Whether there is nothing to look for.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The byte ranges of the needle in `text`, ignoring case: whole
    /// characters, not overlapping, each place found. Nothing is allocated: a
    /// block's characters are walked in place.
    pub fn matches<'a>(&'a self, text: &'a str) -> Matches<'a> {
        Matches {
            needle: &self.0,
            text,
            at: 0,
        }
    }
}

/// The places of a [`Needle`] in a text.
#[derive(Debug)]
pub struct Matches<'a> {
    needle: &'a [char],
    text: &'a str,
    at: usize,
}

impl Iterator for Matches<'_> {
    type Item = (usize, usize);

    fn next(&mut self) -> Option<(usize, usize)> {
        if self.needle.is_empty() {
            return None;
        }
        loop {
            let rest = self.text.get(self.at..)?;
            let c = rest.chars().next()?;
            let found = match_chars(rest, self.needle);
            match found {
                Some(length) => {
                    let start = self.at;
                    self.at += length;
                    return Some((start, self.at));
                }
                None => self.at += c.len_utf8(),
            }
        }
    }
}

/// The length in bytes of `needle` at the start of `text`, ignoring case.
fn match_chars(text: &str, needle: &[char]) -> Option<usize> {
    let mut chars = text.chars();
    for &n in needle {
        if lower(chars.next()?) != n {
            return None;
        }
    }
    Some(text.len() - chars.as_str().len())
}

/// The part of `text` a snippet of the match `start..end` shows.
#[must_use]
pub fn snippet_range(text: &str, start: usize, end: usize) -> (usize, usize) {
    let from = text
        .get(..start)
        .and_then(|before| before.char_indices().rev().nth(BEFORE - 1))
        .map_or(0, |(i, _)| i);
    let to = text
        .get(end..)
        .and_then(|after| after.char_indices().nth(AFTER))
        .map_or(text.len(), |(i, _)| end + i);
    (from, to)
}

/// The match `start..end` with some context, on one line: marks (`…`) where
/// there is more, white space as spaces.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn snippet(text: &str, start: usize, end: usize) -> Result<String, Error> {
    let (from, to) = snippet_range(text, start, end);
    let shown = text.get(from..to).unwrap_or_default();
    let mut out = String::new();
    // The context, the match, and two marks, as bytes at most.
    out.try_reserve_exact(shown.len() + 6)
        .map_err(|_| Error::NoMemory)?;
    if from > 0 {
        out.push('…');
    }
    out.extend(
        shown
            .chars()
            .map(|c| if c.is_whitespace() { ' ' } else { c }),
    );
    if to < text.len() {
        out.push('…');
    }
    Ok(out)
}

/// Where the match `start..end` is in its snippet (see [`snippet`]), in
/// characters.
#[must_use]
pub fn snippet_at(text: &str, start: usize, end: usize) -> (usize, usize) {
    let (from, _) = snippet_range(text, start, end);
    let before = usize::from(from > 0)
        + text
            .get(from..start)
            .map_or(0, |context| context.chars().count());
    let length = text
        .get(start..end)
        .map_or(0, |found| found.chars().count());
    (before, before + length)
}

/// What the font says of a text, for [`with_hit`].
pub trait Measure {
    /// How wide `c` is.
    fn advance(&mut self, c: char) -> f32;
    /// How wide the mark for more text (`…`) is.
    fn ellipsis(&mut self) -> f32;
}

/// `snippet` shown in `width`: where it does not fit up to the end of the match
/// at characters `at`, it begins later (at a word, after `…`), so that the word
/// found is in the row and not cut off with the context after it.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn with_hit<'s>(
    font: &mut impl Measure,
    snippet: &'s str,
    at: (usize, usize),
    width: f32,
) -> Result<Cow<'s, str>, Error> {
    let byte = |chars: usize| {
        snippet
            .char_indices()
            .nth(chars)
            .map_or(snippet.len(), |(i, _)| i)
    };
    let (start, end) = (byte(at.0), byte(at.1));
    // Room for an ellipsis at either end.
    let room = width - 2.0 * font.ellipsis();
    let mut used = 0.0;
    let mut from = end;
    for (i, c) in snippet.get(..end).unwrap_or(snippet).char_indices().rev() {
        used += font.advance(c);
        if used > room {
            break;
        }
        from = i;
    }
    let before = snippet.get(..from).unwrap_or_default();
    if from == 0 || before.chars().all(|c| c == '…') {
        return Ok(Cow::Borrowed(snippet));
    }
    // A match wider than the row: from its start, cut at the end.
    from = from.min(start);
    // From the start of a word, before the match.
    if let Some(space) = snippet.get(from..start).and_then(|s| s.find(' ')) {
        from += space;
    }
    let rest = snippet.get(from..).unwrap_or_default().trim_start();
    Ok(Cow::Owned(concat(&["…", rest])?))
}

/// One hit, as its row shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Hit {
    /// Where the match begins.
    pub start: Place,
    /// Where it ends.
    pub end: Place,
    /// How far into the book it is.
    pub percent: u8,
    /// The text round it, cut to the row.
    pub snippet: String,
}

/// The book as the scan reads it: blocks of text in sections.
pub trait Blocks {
    /// A block of the book, as the page shows its text.
    type Block;
    /// How many sections the book has.
    fn sections(&self) -> u32;
    /// The block at `at`; `None` where the section has no more. [`Error::NotFound`],
    /// [`Error::Invalid`] and [`Error::Denied`] count as the end of the section (a
    /// block that cannot be read is left out, as in interface 6); any other
    /// error ends the step and is returned by it.
    ///
    /// # Errors
    /// What the host refuses.
    fn block_at(&mut self, at: Place) -> Result<Option<Self::Block>, Error>;
    /// The block's text.
    fn text<'b>(&self, block: &'b Self::Block) -> &'b str;
    /// Where the block ends in its section.
    fn end(&self, block: &Self::Block) -> u32;
    /// The place in the section of the byte `byte` of the block's text.
    fn source_offset(&self, block: &Self::Block, byte: usize) -> u32;
    /// How far into the book `at` is, in percent.
    fn percent(&self, at: Place) -> u8;
}

/// What the header says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Note {
    /// Still looking, `percent` of the book read (in tens).
    Searching(u8),
    /// Done: this many hits.
    Hits(usize),
    /// Done, and there were more than are kept.
    MoreHits(usize),
}

/// A search over a book.
#[derive(Debug)]
pub struct Search {
    needle: Needle,
    /// Where reading continues; `None` when the search is done.
    next: Option<Place>,
    hits: Vec<Hit>,
    /// Hits were left out after [`MAX_HITS`].
    more: bool,
    /// Progress shown in the header, in steps of ten percent.
    shown: u8,
}

impl Search {
    /// A search for `query`, over a book that is open (`opened`: it could be
    /// opened) or not (nothing is looked for then, and the screen says none was
    /// found).
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn new(query: &str, opened: bool) -> Result<Self, Error> {
        let needle = Needle::new(query)?;
        let next = (opened && !needle.is_empty()).then_some(Place {
            section: 0,
            offset: 0,
        });
        Ok(Self {
            needle,
            next,
            hits: Vec::new(),
            more: false,
            shown: 0,
        })
    }

    /// Whether the book is still being read.
    #[must_use]
    pub const fn searching(&self) -> bool {
        self.next.is_some()
    }

    /// The hits so far, in the order of the book.
    #[must_use]
    pub fn hits(&self) -> &[Hit] {
        &self.hits
    }

    /// What the header says.
    #[must_use]
    pub fn note(&self) -> Note {
        if self.next.is_some() {
            Note::Searching(self.shown)
        } else if self.more {
            Note::MoreHits(self.hits.len())
        } else {
            Note::Hits(self.hits.len())
        }
    }

    /// The snippet of hit `index`, taken out of it (the hit keeps its place
    /// and its progress): a screen that holds the snippet in its row does not
    /// hold it twice. `None` where there is no such hit or it was taken.
    pub fn take_snippet(&mut self, index: usize) -> Option<String> {
        let hit = self.hits.get_mut(index)?;
        (!hit.snippet.is_empty()).then(|| core::mem::take(&mut hit.snippet))
    }

    /// The match of hit `index` to go to: where it begins and ends.
    #[must_use]
    pub fn target(&self, index: usize) -> Option<(Place, Place)> {
        self.hits.get(index).map(|hit| (hit.start, hit.end))
    }

    /// How far the scan has read, in percent; 100 when done.
    pub fn percent<B: Blocks>(&self, book: &B) -> u8 {
        self.next.map_or(100, |at| book.percent(at))
    }

    /// Whether the header's progress changed (in steps of ten percent), or the
    /// search ended, now that `percent` of the book is read; the new progress is
    /// kept.
    pub fn progressed(&mut self, percent: u8) -> bool {
        let tens = percent / 10 * 10;
        if self.next.is_none() || tens != self.shown {
            self.shown = tens;
            return true;
        }
        false
    }

    /// Reads the next [`BLOCKS_PER_STEP`] blocks and keeps their hits, each
    /// snippet cut to `width` by `font`. Returns the number of hits there were
    /// before the step, so that the screen can tell whether it added any to the
    /// page shown. After an error that number is not returned: the screen
    /// compares [`Search::hits`]'s length before and after the call.
    ///
    /// # Errors
    /// Any error of the host other than the three of [`Blocks::block_at`] that
    /// end a section (such as [`Error::NoMemory`] or [`Error::NoSpace`]), or
    /// [`Error::NoMemory`] for the hits kept: the hits so far stay and the scan
    /// stands where it did, so a step repeats the block that failed. A screen that
    /// sees the same error again ends the search and says so, not tries for ever.
    pub fn step<B: Blocks>(
        &mut self,
        book: &mut B,
        font: &mut impl Measure,
        width: f32,
    ) -> Result<usize, Error> {
        let first_new = self.hits.len();
        let Some(mut at) = self.next else {
            return Ok(first_new);
        };
        for _ in 0..BLOCKS_PER_STEP {
            match book.block_at(at) {
                Ok(Some(block)) => {
                    let kept = self.hits.len();
                    if let Err(error) = self.keep(book, &block, at, font, width) {
                        // The block is read again: its hits are not kept twice.
                        self.hits.truncate(kept);
                        return Err(error);
                    }
                    if self.more {
                        self.next = None;
                        return Ok(first_new);
                    }
                    at = past(at, book.end(&block));
                }
                // The end of a section, or one that cannot be read.
                Ok(None) | Err(Error::NotFound | Error::Invalid | Error::Denied) => {
                    if at.section.saturating_add(1) >= book.sections() {
                        self.next = None;
                        return Ok(first_new);
                    }
                    at = Place {
                        section: at.section + 1,
                        offset: 0,
                    };
                }
                Err(error) => {
                    // Where the scan stood stays where it was.
                    return Err(error);
                }
            }
            self.next = Some(at);
        }
        Ok(first_new)
    }

    /// Keeps the hits of `block`, which was read at `at`.
    fn keep<B: Blocks>(
        &mut self,
        book: &B,
        block: &B::Block,
        at: Place,
        font: &mut impl Measure,
        width: f32,
    ) -> Result<(), Error> {
        let text = book.text(block);
        for (start, end) in self.needle.matches(text) {
            if self.hits.len() == MAX_HITS {
                self.more = true;
                break;
            }
            let start_at = Place {
                section: at.section,
                offset: book.source_offset(block, start),
            };
            let end_at = Place {
                section: at.section,
                offset: book.source_offset(block, end),
            };
            let shown = snippet(text, start, end)?;
            let cut = with_hit(font, &shown, snippet_at(text, start, end), width)?;
            let row = match cut {
                Cow::Borrowed(_) => shown,
                Cow::Owned(cut) => cut,
            };
            self.hits.try_reserve(1).map_err(|_| Error::NoMemory)?;
            self.hits.push(Hit {
                start: start_at,
                end: end_at,
                percent: book.percent(start_at),
                snippet: row,
            });
        }
        Ok(())
    }
}

/// Where reading goes on after a block that ended at `end` while the scan stood
/// at `at`: past the block, and at least one further.
const fn past(at: Place, end: u32) -> Place {
    Place {
        section: at.section,
        offset: if end > at.offset {
            end
        } else {
            at.offset.saturating_add(1)
        },
    }
}

#[cfg(test)]
mod tests;
