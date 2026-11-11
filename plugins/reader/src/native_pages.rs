//! What the reading screen decides without a host: the box a page is laid out
//! in from the reading settings and the display, what an input asks of the page
//! (the next, the one before), the trail of page starts a turn back follows,
//! and how the page before a place is found when the trail is empty (laid out
//! forward from the section's start). The interface-6 screen is the
//! reference until it goes at activation. Everything is allocation-free
//! or fallible.
use alloc::vec::Vec;
use paperpaw_sdk::{
    layout::{BookStyles, Style},
    native::{Error, callbacks::Input},
};

/// The reading settings a page is set with, in the units the settings use.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct Reading {
    /// The text size, in points (`core.font-size`).
    pub size_pt: f32,
    /// The distance between baselines, percent of the size (80 to 250).
    pub line_spacing: u16,
    /// The space between words, percent of the font's (50 to 200).
    pub word_spacing: u16,
    /// The margins at the sides, in millimetres.
    pub margin: u16,
    /// The margins at the top and the bottom, in millimetres.
    pub margin_ends: u16,
    /// Lines as wide as the page.
    pub justify: bool,
    /// The room the status line at the top and the one at the bottom keep,
    /// from the display's edge, in pixels (0: none, `native_status`).
    pub status: (u16, u16),
    /// How paragraphs are set apart: an indent of so many percent of the size
    /// (`core.indent`, `core.indent-size`; 0: none), a space of so many percent
    /// of a line (`core.paragraph-spacing`, `core.paragraph-space`; 0: none).
    pub paragraphs: (u16, u16),
    /// The fewest lines of a paragraph kept at the top and at the bottom of a
    /// page (`core.widows`, `core.orphans`), 1 to 4.
    pub lines_kept: (u8, u8),
    /// Paragraphs broken over all their lines (`core.line-breaking`), not line
    /// by line.
    pub balanced: bool,
    /// Which of the book's own styles are followed (`core.book-…`, the bits of
    /// `paperpaw_layout::BookStyles`).
    pub book: u16,
}

impl Reading {
    /// The settings as the user left them, kept to what they allow; a value
    /// that is not a number of the setting's kind is its default.
    #[must_use]
    pub fn new(
        size_pt: i64,
        line_spacing: i64,
        word_spacing: i64,
        margin: i64,
        margin_ends: i64,
        justify: bool,
    ) -> Self {
        let kept = |v: i64, low: i64, high: i64, default: i64| {
            u16::try_from(if v == 0 && low > 0 {
                default
            } else {
                v.clamp(low, high)
            })
            .unwrap_or(0)
        };
        Self {
            size_pt: f32::from(kept(size_pt, 6, 72, 12)),
            line_spacing: kept(line_spacing, 80, 250, 120),
            word_spacing: kept(word_spacing, 50, 200, 100),
            // Zero is a margin of none, as in Beta 3.
            margin: kept(margin, 0, 30, 4),
            margin_ends: kept(margin_ends, 0, 30, 4),
            justify,
            status: (0, 0),
            // Until the settings say: paragraphs not set apart, line by line,
            // all of the book's styles (`with_paragraphs`, `with_lines`).
            paragraphs: (0, 0),
            lines_kept: (1, 1),
            balanced: false,
            book: u16::MAX,
        }
    }

    /// The same with paragraphs set apart as the settings say: the indent
    /// (`indent` on, `indent_size` percent of the size, 50 to 400) and the
    /// space between them (`spaced` on, `space` percent of a line, 20 to 150).
    #[must_use]
    pub fn with_paragraphs(
        mut self,
        (indent, indent_size): (bool, i64),
        (spaced, space): (bool, i64),
    ) -> Self {
        let kept =
            |v: i64, low: i64, high: i64| u16::try_from(v.clamp(low, high)).unwrap_or(low as u16);
        self.paragraphs = (
            if indent {
                kept(indent_size, 50, 400)
            } else {
                0
            },
            if spaced { kept(space, 20, 150) } else { 0 },
        );
        self
    }

    /// The same keeping `widows` and `orphans` lines (1 to 4), breaking
    /// paragraphs `balanced` or line by line, following the book's own
    /// styles `book`.
    #[must_use]
    pub fn with_lines(mut self, (widows, orphans): (i64, i64), balanced: bool, book: u16) -> Self {
        let kept = |v: i64| u8::try_from(v.clamp(1, 4)).unwrap_or(1);
        self.lines_kept = (kept(widows), kept(orphans));
        self.balanced = balanced;
        self.book = book;
        self
    }

    /// How the text is set, as Beta 3 set it: body text of `size` pixels,
    /// paragraphs indented by a part of the size and spaced by a part of the
    /// body's `line` height (the size times the line spacing, at least the
    /// font's own distance), the lines kept together, the line breaking and
    /// the book's own styles.
    #[must_use]
    pub fn style(&self, size: f32, line: f32) -> Style {
        let (indent, space) = self.paragraphs;
        Style::new(
            line * f32::from(space) / 100.0,
            size * f32::from(indent) / 100.0,
            size,
            self.justify,
        )
        .lines_kept(self.lines_kept.0, self.lines_kept.1)
        .balanced(self.balanced)
        .book(BookStyles::from_bits(self.book))
    }

    /// The same with room for status lines `top` and `bottom` pixels from the
    /// display's edges.
    #[must_use]
    pub const fn with_status(mut self, top: u16, bottom: u16) -> Self {
        self.status = (top, bottom);
        self
    }

    /// The factor of a space's width (`word_spacing` as a part of one).
    #[must_use]
    pub fn word_factor(&self) -> f32 {
        f32::from(self.word_spacing) / 100.0
    }

    /// The factor of the line spacing.
    #[must_use]
    pub fn spacing_factor(&self) -> f32 {
        f32::from(self.line_spacing) / 100.0
    }
}

/// Where a page is laid out: the content box on the display, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Box {
    /// Left and top edges.
    pub x: u16,
    /// See `x`.
    pub y: u16,
    /// Width and height.
    pub width: u16,
    /// See `width`.
    pub height: u16,
}

/// The content box of a display of `width` by `height` pixels at `dpi`, with the
/// reading margins; `None` where nothing is left of it (a margin wider than the
/// display): the screen then says so instead of laying out a page.
#[must_use]
pub fn content_box(width: u16, height: u16, dpi: u16, reading: &Reading) -> Option<Box> {
    let tenth_mm = |mm: u16| (u32::from(mm) * 10 * u32::from(dpi) + 127) / 254;
    let (side, end) = (tenth_mm(reading.margin), tenth_mm(reading.margin_ends));
    // The text never runs up to a status line: above or below it, the room the
    // line keeps is the margin where that is more.
    let top = end.max(u32::from(reading.status.0));
    let bottom = end.max(u32::from(reading.status.1));
    let wide = u32::from(width).checked_sub(2 * side)?;
    let high = u32::from(height).checked_sub(top + bottom)?;
    // Less than a letter of room is no page.
    if wide < 16 || high < 16 {
        return None;
    }
    Some(Box {
        x: u16::try_from(side).ok()?,
        y: u16::try_from(top).ok()?,
        width: u16::try_from(wide).ok()?,
        height: u16::try_from(high).ok()?,
    })
}

/// What an input asks of the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Turn {
    /// The page after this one.
    Next,
    /// The page before this one.
    Previous,
    /// Nothing the screen handles.
    Ignored,
}

/// What `input` asks: the page actions and the arrows (right and down on, left
/// and up back), as the keys and edge swipes of the reader's layer and the
/// host's tap zones are mapped to them.
#[must_use]
pub fn turn(input: &Input<'_>) -> Turn {
    match *input {
        Input::Action { action, .. } => match action {
            "core.next-page" | "core.right" | "core.down" => Turn::Next,
            "core.prev-page" | "core.left" | "core.up" => Turn::Previous,
            _ => Turn::Ignored,
        },
        _ => Turn::Ignored,
    }
}

/// Most page starts kept to turn back along.
pub const TRAIL: usize = 64;

/// Whether a change of the setting `key` changes how a page is laid out (its
/// size, spacing, margins, justification, fonts or hyphenation): the page on
/// show is laid out again and the page before it is found anew.
#[must_use]
pub fn affects_layout(key: &str) -> bool {
    // An empty key: the host says every setting may have changed.
    key.is_empty()
        || matches!(
            key,
            "core.font-size"
                | "core.line-spacing"
                | "core.word-spacing"
                | "core.margin"
                | "core.margin-ends"
                | "core.justify"
                | "core.reading-font"
                | "core.hyphenate"
                | "core.hyphen-min-left"
                | "core.hyphen-min-right"
                | "core.indent"
                | "core.indent-size"
                | "core.paragraph-spacing"
                | "core.paragraph-space"
                | "core.widows"
                | "core.orphans"
                | "core.line-breaking"
                | "core.book-align"
                | "core.book-indent"
                | "core.book-sizes"
                | "core.book-spacing"
                | "core.book-fonts"
                | "core.book-margins"
                | "core.book-line-height"
                | "reader.status.size"
                | "reader.status.edge"
                | "reader.status.top-lines"
                | "reader.status.bottom-lines"
        )
}

/// Whether a change of the setting `key` changes only how the page is drawn (the
/// weight, contrast and smoothness of the text): it is drawn again as laid out.
#[must_use]
pub fn affects_ink(key: &str) -> bool {
    matches!(
        key,
        "core.smooth-text"
            | "core.font-weight"
            | "core.text-contrast"
            | "core.text-sharpness"
            | "core.text-curve"
    )
}

/// The starts of the pages turned on from, newest last.
#[derive(Debug, Default)]
pub struct Trail {
    starts: Vec<(u32, u32)>,
}

impl Trail {
    /// Nothing turned yet.
    #[must_use]
    pub const fn new() -> Self {
        Self { starts: Vec::new() }
    }

    /// Notes the page that starts at `start` as left by a turn on. The oldest
    /// is dropped at the limit.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn push(&mut self, start: (u32, u32)) -> Result<(), Error> {
        if self.starts.len() >= TRAIL {
            self.starts.remove(0);
        }
        self.starts.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.starts.push(start);
        Ok(())
    }

    /// The start of the page before, forgetting it.
    pub fn pop(&mut self) -> Option<(u32, u32)> {
        self.starts.pop()
    }

    /// Forgets the trail (the book was opened at another place).
    pub fn clear(&mut self) {
        self.starts.clear();
    }

    /// Whether a turn back has a start to go to.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.starts.is_empty()
    }
}

/// The start of the page before the one starting at `current`, found by laying
/// pages out from `from` (the start of `current`'s section, or of the section
/// before when `current` is its first page): `next` is the start of the page
/// after the one that starts where it is given. Pages are at most `limit`, so a
/// layout that does not move on cannot hold the screen.
///
/// # Errors
/// What `next` refuses; `NoSpace` for more pages than `limit` or one that does
/// not move on.
pub fn page_before(
    from: (u32, u32),
    current: (u32, u32),
    limit: usize,
    mut next: impl FnMut((u32, u32)) -> Result<(u32, u32), Error>,
) -> Result<Option<(u32, u32)>, Error> {
    if current <= from {
        return Ok(None);
    }
    let mut at = from;
    for _ in 0..limit {
        let after = next(at)?;
        if after <= at {
            return Err(Error::NoSpace);
        }
        if after >= current {
            return Ok(Some(at));
        }
        at = after;
    }
    Err(Error::NoSpace)
}

#[cfg(test)]
mod tests;
