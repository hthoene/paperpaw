//! The words of a laid-out page with their boxes in the pane and their places in
//! the book, in reading order: what a touch hits, what is chosen, looked up and
//! followed. Measured as the page is drawn (`native_draw::draw_lines`): the same
//! spaces and the same runs, so a box is where its word is. The interface-6
//! `paperpaw_sdk::reading::word_boxes` is the reference; this one measures with
//! the native [`Faces`] and allocates fallibly.
//!
//! A line whose runs are put in order for mixed directions (`LineBidi::REORDER`)
//! is read in logical order, each word where the logical flow puts it (a named
//! gap in `PORTING-7.md`: the interface-6 reader set each where it is drawn).
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::{
    Rect,
    document::Locator,
    layout::{Page, Typesetter},
    native::Error,
    reading::WordBox,
    text::Around,
};

use crate::native_pages;
use crate::native_typeset::{Faces, Measure};

/// Most words kept for one page: more than a page of words can hold at the
/// smallest size; a page with more has the rest left out (not chosen, not hit).
pub const MAX_WORDS: usize = 2_048;

/// The words of `page` with their boxes in the pane: the page's content box at
/// `area`. Lines a filter inserted have no places and give no words.
///
/// # Errors
/// `NoMemory` when a word or the list cannot be allocated.
pub fn word_boxes<M: Measure>(
    faces: &Faces<M>,
    page: &Page,
    area: native_pages::Box,
) -> Result<Vec<WordBox>, Error> {
    let (left, top) = (f32::from(area.x), f32::from(area.y));
    let mut words: Vec<WordBox> = Vec::new();
    // The last word of the line before, where it ended with a hyphen the
    // layout put there: its word goes on with the first of this line.
    let mut broken: Option<usize> = None;
    for line in page.lines.iter().filter(|l| !l.inserted) {
        let first = words.len();
        let mut x = left + line.x;
        // Inside a word that goes on in the next run (a bold start, say).
        let mut joined = false;
        // The next of the line's words in the source.
        let mut ordinal = 0;
        for run in &line.runs {
            let Some(text) = line.text.get(run.start..run.end) else {
                continue;
            };
            let (ascent, height) = faces.line(run.face);
            let box_top = top + line.baseline - ascent;
            let space = faces.advance(run.face, ' ') + line.word_spacing;
            for (n, part) in text.split(' ').enumerate() {
                if n > 0 {
                    x += space;
                    joined = false;
                }
                if part.is_empty() {
                    continue;
                }
                let width = faces.advance_text(run.face, part, &Around::NONE);
                match words.last_mut() {
                    Some(last) if joined => {
                        push_str(&mut last.text, part)?;
                        last.area.width = ((x + width) as u16).saturating_sub(last.area.x);
                    }
                    _ => {
                        if words.len() >= MAX_WORDS {
                            return Ok(words);
                        }
                        let (start, end) = line.words.get(ordinal).copied().unwrap_or_default();
                        ordinal += 1;
                        let mut text = String::new();
                        push_str(&mut text, part)?;
                        words.try_reserve(1).map_err(|_| Error::NoMemory)?;
                        words.push(
                            WordBox::new(
                                Rect::new(x as u16, box_top as u16, width as u16, height as u16),
                                text,
                                Locator::new(line.section, start),
                                Locator::new(line.section, end),
                            )
                            .bidi(line.bidi),
                        );
                        if let Some(head) = broken.take() {
                            join_broken(&mut words, head)?;
                        }
                    }
                }
                x += width;
                joined = true;
            }
        }
        // A hyphen of the layout's ends this line's last word, which goes on
        // in the next line (not in a page of its own: a page ends with it).
        broken = (line.hyphen && words.len() > first).then(|| words.len() - 1);
    }
    Ok(words)
}

/// The word at `head`, which ends a line with a hyphen the layout put there,
/// and the word after it, its other part: each is the whole word (without that
/// hyphen) with its whole place in the book, so that a touch on either part
/// chooses the word, and what is kept of it is the word, by its place.
fn join_broken(words: &mut [WordBox], head: usize) -> Result<(), Error> {
    let tail = head + 1;
    let Some((front, back)) = words.get(head).zip(words.get(tail)) else {
        return Ok(());
    };
    let part = front.text.strip_suffix('-').unwrap_or(&front.text);
    let mut whole = String::new();
    whole
        .try_reserve(part.len() + back.text.len())
        .map_err(|_| Error::NoMemory)?;
    whole.push_str(part);
    whole.push_str(&back.text);
    let (start, end) = (front.start, back.end);
    let mut again = String::new();
    push_str(&mut again, &whole)?;
    for (at, text) in [(head, whole), (tail, again)] {
        if let Some(word) = words.get_mut(at) {
            word.text = text;
            word.start = start;
            word.end = end;
        }
    }
    Ok(())
}

/// Whether the page's last word is one the layout broke at the page's end: its
/// other part is the first word of the next page, which this page does not hold.
#[must_use]
pub fn ends_open(words: &[WordBox], page: &Page) -> bool {
    let Some(line) = page.lines.iter().rev().find(|l| !l.inserted) else {
        return false;
    };
    let last = words.last();
    line.hyphen
        && page.end != page.start
        && last
            .zip(line.words.last())
            .is_some_and(|(word, &(_, end))| {
                word.end == Locator::new(line.section, end) && word.text.ends_with('-')
            })
}

/// The first word of the first line of `page`, whole, with where it ends: the
/// other part of a word the page before broke at its end.
///
/// # Errors
/// `NoMemory` when the word cannot be allocated.
pub fn first_word(page: &Page) -> Result<Option<(String, Locator)>, Error> {
    let Some(line) = page.lines.iter().find(|l| !l.inserted) else {
        return Ok(None);
    };
    let Some(&(_, end)) = line.words.first() else {
        return Ok(None);
    };
    let mut word = String::new();
    // Past the word: a space after it.
    let mut done = false;
    for run in &line.runs {
        let Some(text) = line.text.get(run.start..run.end) else {
            continue;
        };
        for (n, part) in text.split(' ').enumerate() {
            if n > 0 {
                done |= !word.is_empty();
            }
            if part.is_empty() || done {
                continue;
            }
            push_str(&mut word, part)?;
        }
    }
    Ok((!word.is_empty()).then(|| (word, Locator::new(line.section, end))))
}

/// The page's last word, broken at its end, made whole with `rest` (the first
/// word of the next page), which ends at `end`.
///
/// # Errors
/// `NoMemory` when the word cannot be allocated.
pub fn join_open_end(words: &mut [WordBox], rest: &str, end: Locator) -> Result<(), Error> {
    let Some(word) = words.last_mut() else {
        return Ok(());
    };
    let part = word.text.strip_suffix('-').unwrap_or(&word.text);
    let mut whole = String::new();
    whole
        .try_reserve(part.len() + rest.len())
        .map_err(|_| Error::NoMemory)?;
    whole.push_str(part);
    whole.push_str(rest);
    word.text = whole;
    word.end = end;
    Ok(())
}

fn push_str(to: &mut String, text: &str) -> Result<(), Error> {
    to.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
    to.push_str(text);
    Ok(())
}

/// How far beside a word a touch still hits it, in pixels (the gap between words).
pub const SLACK: i32 = 4;

/// The word whose box a touch at `(x, y)` hits, with a little slack at the
/// sides (as the interface-6 reader: none above or below, the lines do not
/// touch), else none.
#[must_use]
pub fn word_at(words: &[WordBox], x: i32, y: i32) -> Option<usize> {
    words.iter().position(|w| {
        let a = w.area;
        x >= i32::from(a.x) - SLACK
            && x < i32::from(a.x) + i32::from(a.width) + SLACK
            && y >= i32::from(a.y)
            && y < i32::from(a.y) + i32::from(a.height)
    })
}

#[cfg(test)]
mod tests;
