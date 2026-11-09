//! What the lookup panel shows, without the host: the entries of the providers
//! as one text (the source's name above each entry where there are several),
//! wrapped into lines, a page of lines at a time, and what an input means to it.
use alloc::{string::String, vec::Vec};
use core::ops::Range;
use paperpaw_sdk::{
    Point, Rect,
    native::{
        Error,
        callbacks::{Input, InputGestureKind},
    },
};
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy},
};

/// Entries shown at most (the panel asks one page per provider).
pub const MAX_ENTRIES: usize = 16;
/// Lines kept at most; the rest of a very long text is left off.
pub const MAX_LINES: usize = 600;

/// What one provider found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The provider's name.
    pub source: String,
    /// The definition.
    pub text: String,
}

/// What an input meant to the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Not for the panel.
    Ignored,
    /// The first line shown is now this one.
    Scroll(usize),
    /// The panel should close.
    Close,
    /// A scroll key at the first or last page: taken, nothing moves (it must
    /// not reach the page below).
    Edge,
}

/// The text of a lookup, its lines and the page shown.
#[derive(Debug, Default)]
pub struct Panel {
    text: String,
    lines: Vec<Range<usize>>,
    headings: Vec<Range<usize>>,
    top: usize,
}

/// The entry names that are shown above entries, and the text that holds
/// them: a blank line between entries, `(source)` above the first of each
/// source where entries come from several.
///
/// # Errors
/// [`Error::NoMemory`].
fn compose(entries: &[Entry]) -> Result<(String, Vec<Range<usize>>), Error> {
    let several = entries
        .first()
        .is_some_and(|first| entries.iter().any(|e| e.source != first.source));
    let mut text = String::new();
    let mut headings = Vec::new();
    let mut last: Option<&str> = None;
    for entry in entries.iter().take(MAX_ENTRIES) {
        if entry.text.trim().is_empty() {
            continue;
        }
        let begins = text.len();
        let sep = if text.is_empty() { "" } else { "\n\n" };
        let named = several && last != Some(entry.source.as_str());
        let name = if named {
            concat(&["(", &entry.source, ")\n"])?
        } else {
            String::new()
        };
        text = concat(&[&text, sep, &name, entry.text.trim_end()])?;
        if named {
            let start = begins + sep.len();
            push(&mut headings, start..start + name.len() - 1)?;
            last = Some(&entry.source);
        }
    }
    Ok((text, headings))
}

impl Panel {
    /// The panel for `entries` (or `empty`, a message, where they hold no
    /// text), wrapped to `width` as `measure` measures.
    ///
    /// # Errors
    /// [`Error::NoMemory`] or the measure's error.
    pub fn new(
        entries: &[Entry],
        empty: &str,
        measure: impl FnMut(&str) -> Result<f32, Error>,
        width: f32,
    ) -> Result<Self, Error> {
        let (mut text, mut headings) = compose(entries)?;
        if text.is_empty() {
            text = copy(empty)?;
            headings = Vec::new();
        }
        let lines = wrap(&text, measure, width)?;
        Ok(Self {
            text,
            lines,
            headings,
            top: 0,
        })
    }

    /// The first line shown.
    #[must_use]
    pub const fn top(&self) -> usize {
        self.top
    }

    /// Scrolls so that line `top` is the first shown.
    pub fn set_top(&mut self, top: usize) {
        self.top = top.min(self.lines.len().saturating_sub(1));
    }

    /// The lines of the page of `rows` lines shown, with whether each is the
    /// name of a source (drawn faint).
    pub fn page(&self, rows: usize) -> impl Iterator<Item = (&str, bool)> {
        self.lines.iter().skip(self.top).take(rows).map(|range| {
            let faint = self
                .headings
                .iter()
                .any(|h| h.start <= range.start && range.end <= h.end);
            (self.text.get(range.clone()).unwrap_or(""), faint)
        })
    }

    /// The pages of `rows` lines, and the one shown (both from 1).
    #[must_use]
    pub fn pages(&self, rows: usize) -> (usize, usize) {
        let rows = rows.max(1);
        (self.top / rows + 1, self.lines.len().div_ceil(rows).max(1))
    }

    /// What `input` means; `region` is where the panel is drawn and `rows` the
    /// lines it holds. Touch: the right part scrolls on, the left part goes
    /// back (up, or closes on the first page), outside closes.
    #[must_use]
    pub fn decide(&self, rows: usize, region: Rect, input: &Input<'_>) -> Step {
        let rows = rows.max(1);
        let forward = || {
            let next = self.top + rows;
            if next < self.lines.len() {
                Step::Scroll(next)
            } else {
                Step::Edge
            }
        };
        let back = || {
            if self.top == 0 {
                Step::Edge
            } else {
                Step::Scroll(self.top.saturating_sub(rows))
            }
        };
        match *input {
            Input::Action { action, .. } => match action {
                "core.next-page" | "core.down" | "core.next" | "core.right" => forward(),
                "core.prev-page" | "core.up" | "core.previous" | "core.left" => back(),
                "core.confirm" | "core.back" => Step::Close,
                _ => Step::Ignored,
            },
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => self.tap(region, position, forward, back),
            _ => Step::Ignored,
        }
    }

    fn tap(
        &self,
        region: Rect,
        position: Point,
        forward: impl Fn() -> Step,
        back: impl Fn() -> Step,
    ) -> Step {
        if !region.contains(position) {
            return Step::Close;
        }
        let middle = i32::from(region.x) + i32::from(region.width) / 2;
        let step = if position.x >= middle {
            forward()
        } else {
            back()
        };
        // On the first page the left part has nothing to go back to: it closes.
        if step == Step::Edge && position.x < middle {
            Step::Close
        } else {
            step
        }
    }
}

/// `text` broken into lines of at most `width` by words (inside a word where
/// one alone is too wide), as byte ranges; a line break of the text is a line
/// break, a blank line stays a blank line.
///
/// # Errors
/// [`Error::NoMemory`] or the measure's error.
fn wrap(
    text: &str,
    mut measure: impl FnMut(&str) -> Result<f32, Error>,
    width: f32,
) -> Result<Vec<Range<usize>>, Error> {
    let mut lines = Vec::new();
    let mut start = 0;
    for paragraph in text.split('\n') {
        let before = lines.len();
        let mut from = start;
        let mut end = start;
        for (at, word) in words(paragraph) {
            let at = start + at;
            let stop = at + word.len();
            if end > from && measure(&text[from..stop])? > width {
                push(&mut lines, from..end)?;
                from = at;
            }
            // A word wider than the line is cut where it no longer fits.
            let mut rest = at;
            while measure(&text[from..stop])? > width && stop > rest {
                let mut cut = rest;
                for (i, c) in text[rest..stop].char_indices() {
                    if measure(&text[from..rest + i + c.len_utf8()])? > width && rest + i > from {
                        break;
                    }
                    cut = rest + i + c.len_utf8();
                }
                if cut <= from {
                    cut = (from + text[from..].chars().next().map_or(1, char::len_utf8)).min(stop);
                }
                push(&mut lines, from..cut)?;
                from = cut;
                rest = cut;
                if lines.len() >= MAX_LINES {
                    return Ok(lines);
                }
            }
            end = stop;
        }
        if end > from {
            push(&mut lines, from..end)?;
        }
        if lines.len() == before {
            push(&mut lines, start..start)?;
        }
        if lines.len() >= MAX_LINES {
            lines.truncate(MAX_LINES);
            return Ok(lines);
        }
        start += paragraph.len() + 1;
    }
    Ok(lines)
}

/// The words of `paragraph` with where each begins.
fn words(paragraph: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut at = 0;
    paragraph.split(' ').filter_map(move |word| {
        let begin = at;
        at += word.len() + 1;
        (!word.is_empty()).then_some((begin, word))
    })
}

#[cfg(test)]
mod tests;
