//! What the footnote overlay decides without a host (interface 6:
//! `footnote.rs`): whether a followed link opens as a note, how many lines of
//! the note's text fit over the lower half of the page and what the last of
//! them says where it was cut, where the overlay and its two buttons (*Close*
//! and *Go to the note*) lie, which button a key or a tap means and which
//! buttons a move of the focus draws again. The text is wrapped, the labels are
//! measured and the overlay is drawn by the screen, which gives this model the
//! sizes it measured. Every allocation is fallible; the interface-6
//! `footnote.rs` is the reference until it goes at activation.
use alloc::string::String;
use alloc::vec::Vec;
use core::ops::Range;

use paperpaw_sdk::native::Error;
use paperpaw_sdk::{Point, Rect};
use paperpaw_ui_kit::text::copy;

/// The most of the screen a note covers is its height divided by this (half).
pub const SHARE_DIVISOR: u16 = 2;
/// The parameters of the overlay: the note's text, the book and the link.
pub const TEXT: &str = "text";
/// See [`TEXT`].
pub const PATH: &str = "path";
/// See [`TEXT`].
pub const URL: &str = "url";
/// The keys of the buttons' words: *Close*, then *Go to the note*.
pub const BUTTONS: [&str; 2] = ["reader.footnote.close", "reader.footnote.go"];

/// Whether a link's text looks like a note mark: short, or a number in
/// brackets, or a symbol such as `*` or `†`.
#[must_use]
pub fn note_like(label: &str) -> bool {
    let t = label.trim();
    let chars = t.chars().count();
    !t.is_empty()
        && (chars <= 3
            || chars <= 6
                && t.chars()
                    .all(|c| c.is_ascii_digit() || "[]()*†‡§¹²³⁴⁵⁶⁷⁸⁹⁰".contains(c)))
}

/// Whether following a link opens it as a note over the page: the reader shows
/// notes that way (`popup`), the link is to the book and its text looks like a
/// mark. (The note's text must then be found: the screen's part.)
#[must_use]
pub fn opens_as_note(popup: bool, external: bool, label: &str) -> bool {
    popup && !external && note_like(label)
}

/// How many lines of `line` pixels fit in the share of a screen `height` high
/// that a note covers, after a button row (`row`) and three gaps: at least one.
#[must_use]
pub fn lines_room(height: u16, row: u16, gap: u16, line: u16) -> usize {
    let most = height / SHARE_DIVISOR;
    let taken = u32::from(row) + 3 * u32::from(gap);
    let room = u32::from(most).saturating_sub(taken) / u32::from(line.max(1));
    room.max(1) as usize
}

/// The lines the overlay shows: those of `ranges` of `text` (the wrapped
/// lines), at most `room`, each without its trailing space; where the text has
/// more, the last line is what `fit` makes of the rest of the text from where
/// that line begins (shortened to the width).
///
/// # Errors
/// [`Error::NoMemory`], or what `fit` says.
pub fn lines(
    text: &str,
    ranges: &[Range<usize>],
    room: usize,
    mut fit: impl FnMut(&str) -> Result<String, Error>,
) -> Result<Vec<String>, Error> {
    let keep = ranges.len().min(room.max(1));
    let mut out = Vec::new();
    out.try_reserve_exact(keep).map_err(|_| Error::NoMemory)?;
    for range in ranges.iter().take(keep) {
        out.push(copy(
            text.get(range.clone()).unwrap_or_default().trim_end(),
        )?);
    }
    if ranges.len() > keep
        && let Some(start) = ranges.get(keep - 1).map(|r| r.start)
        && let Some(last) = out.last_mut()
    {
        *last = fit(text.get(start..).unwrap_or_default().trim())?;
    }
    Ok(out)
}

/// The sizes the screen measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Metrics {
    /// The display.
    pub width: u16,
    /// See [`Self::width`].
    pub height: u16,
    /// The side margin and the gap between things.
    pub margin: u16,
    /// See [`Self::margin`].
    pub gap: u16,
    /// A row of text, and a line of the note's text.
    pub row: u16,
    /// See [`Self::row`].
    pub line: u16,
    /// A row of buttons that can be touched, and the room to leave below it so
    /// that its buttons keep the side margin from the edge.
    pub button_row: u16,
    /// See [`Self::button_row`].
    pub room: u16,
}

impl Metrics {
    /// The width of a button where two sit side by side.
    #[must_use]
    pub const fn half_width(&self) -> u16 {
        (self.width / 2).saturating_sub(self.margin.saturating_add(self.gap / 2))
    }

    /// The width of a button where they sit one below the other.
    #[must_use]
    pub const fn inner_width(&self) -> u16 {
        self.width.saturating_sub(self.margin.saturating_mul(2))
    }
}

/// Where the overlay and its buttons lie.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Layout {
    /// The overlay: the lower part of the display.
    pub region: Rect,
    /// The slots of *Close* and *Go to the note*.
    pub buttons: [Rect; 2],
}

/// Whether the buttons fit side by side: each label needs a slot no higher than
/// a row of buttons in the half width.
#[must_use]
pub fn side_by_side(slot_heights: [u16; 2], metrics: &Metrics) -> bool {
    slot_heights.iter().all(|&h| h <= metrics.button_row)
}

fn clamp(value: u32) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

/// The overlay for `lines` lines of text. `slots` are the heights of the
/// buttons' slots one below the other (used where they are not side by side).
#[must_use]
pub fn layout(metrics: &Metrics, lines: usize, side_by_side: bool, slots: [u16; 2]) -> Layout {
    let (gap, row) = (u32::from(metrics.gap), u32::from(metrics.button_row));
    let buttons_height = if side_by_side {
        row
    } else {
        u32::from(slots[0]) + gap + u32::from(slots[1])
    };
    let text = u32::from(metrics.line).saturating_mul(u32::try_from(lines).unwrap_or(u32::MAX));
    let height = clamp(
        (gap * 3 - gap / 2)
            .saturating_add(text)
            .saturating_add(buttons_height)
            .saturating_add(u32::from(metrics.room)),
    );
    let region = Rect::new(
        0,
        metrics.height.saturating_sub(height),
        metrics.width,
        height,
    );
    let below = u32::from(height)
        .saturating_sub(buttons_height)
        .saturating_sub(u32::from(metrics.room));
    let buttons_y = clamp(u32::from(region.y) + below);
    let (margin, gap) = (metrics.margin, metrics.gap);
    let buttons = if side_by_side {
        let half = metrics.width / 2;
        let wide = metrics.half_width();
        [
            Rect::new(margin, buttons_y, wide, metrics.button_row),
            Rect::new(
                half.saturating_add(gap / 2),
                buttons_y,
                wide,
                metrics.button_row,
            ),
        ]
    } else {
        let inner = metrics.inner_width();
        [
            Rect::new(margin, buttons_y, inner, slots[0]),
            Rect::new(
                margin,
                buttons_y.saturating_add(slots[0]).saturating_add(gap),
                inner,
                slots[1],
            ),
        ]
    };
    Layout { region, buttons }
}

/// What the overlay's buttons do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Press {
    /// Done reading: back to the page.
    Close,
    /// Back to the page, then follow the link to the note's place.
    Follow,
}

/// The button `index` pressed.
#[must_use]
pub const fn press(index: usize) -> Press {
    if index == 1 {
        Press::Follow
    } else {
        Press::Close
    }
}

/// A key the overlay reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Key {
    /// Left, up, previous, previous page.
    Previous,
    /// Right, down, next, next page.
    Next,
    /// Confirm.
    Confirm,
    /// Back or menu.
    Back,
}

/// What a key does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Decision {
    /// The focus goes to this button.
    Focus(usize),
    /// A button is pressed.
    Press(Press),
    /// Back to the page.
    Close,
}

/// What `key` does where the focus is on `focus` (none after a touch): the
/// previous keys go to *Close*, the next to *Go to the note*, confirm presses
/// the focused button (*Close* where none is), back closes.
#[must_use]
pub const fn on_key(focus: Option<usize>, key: Key) -> Decision {
    match key {
        Key::Previous => Decision::Focus(0),
        Key::Next => Decision::Focus(1),
        Key::Confirm => Decision::Press(press(match focus {
            Some(index) => index,
            None => 0,
        })),
        Key::Back => Decision::Close,
    }
}

/// What a tap at `at` does: a button where it is on one, else done reading the
/// note.
#[must_use]
pub fn on_tap(buttons: &[Rect; 2], at: Point) -> Press {
    buttons
        .iter()
        .position(|b| b.contains(at))
        .map_or(Press::Close, press)
}

/// The two buttons to draw again after the focus moved from `before` to
/// `after`: where it was (or, where it was nowhere, where it is) and where it
/// is; `None` where either is none, which draws the whole overlay again.
#[must_use]
pub fn redraw(before: Option<usize>, after: Option<usize>) -> Option<(usize, usize)> {
    Some((before.or(after)?, after?))
}

#[cfg(test)]
mod tests;
