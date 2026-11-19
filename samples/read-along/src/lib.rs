//! Read-along, a PaperPaw sample: marks the sentence being read aloud on the
//! page, for plugin interface 7. What it does without a device: which words
//! of a line are read ([`span`]), and what it draws around them ([`draw`]).
//! `main.rs` is the plugin around it.
//!
//! A layer is drawn over the text, so a mark never fills a box (it would
//! hide the words): a frame, or a thick line under them.
#![cfg_attr(not(test), no_std)]

use paperpaw_sdk::native::{
    Error,
    page_layers::{LayerAnswer, Tone},
};

/// The pixels a frame stands off the words, sideways and above and below.
pub const PAD: i32 = 2;
/// The pixels a frame or an underline is thick.
pub const THICKNESS: u32 = 2;

/// How the sentence read aloud is marked (the setting `read-along.style`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// A frame around the words of each line.
    Frame,
    /// A thick line under them.
    Underline,
    /// Not marked: the plugin does not follow reading aloud.
    Off,
}

impl Style {
    /// The style a setting value (a choice's translation key) names; a
    /// value that is not one is the frame.
    #[must_use]
    pub fn of(choice: &str) -> Self {
        match choice {
            "read-along.style.underline" => Self::Underline,
            "read-along.style.off" => Self::Off,
            _ => Self::Frame,
        }
    }

    /// What the plugin tells the reader to know when its drawing changes.
    #[must_use]
    pub const fn signature(self) -> u64 {
        1 << 40 | self as u64
    }
}

/// The part of a line to mark: its words' box, in pixels of the text area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark {
    pub left: i32,
    pub right: i32,
    pub top: i32,
    pub bottom: i32,
}

/// The stretch of a line that is read aloud: from the left of the first word
/// `held` says is read to the right of the last. `words` are the line's
/// words' left edge and width in order; a word of no width has no known box
/// and is left out.
pub fn span(
    words: impl Iterator<Item = (u16, u16)>,
    held: impl Fn(u32) -> bool,
) -> Option<(i32, i32)> {
    let mut found: Option<(i32, i32)> = None;
    for (number, (x, width)) in (0u32..).zip(words) {
        if width == 0 || !held(number) {
            continue;
        }
        let (left, right) = (i32::from(x), i32::from(x) + i32::from(width));
        found = Some(found.map_or((left, right), |(l, r)| (l.min(left), r.max(right))));
    }
    found
}

/// Draws `mark` as `style` says.
///
/// # Errors
/// The reply is full (`NoSpace`, `Invalid`): the rest is left out.
pub fn draw(answer: &mut LayerAnswer<'_>, style: Style, mark: Mark) -> Result<(), Error> {
    match style {
        Style::Off => Ok(()),
        Style::Frame => {
            let (x, y) = (mark.left - PAD, mark.top - 1);
            let size = (
                (mark.right - mark.left + 2 * PAD) as u32,
                (mark.bottom - mark.top + 2) as u32,
            );
            answer.rect((x, y), size, (Tone::NONE, Tone::BLACK), THICKNESS)
        }
        Style::Underline => {
            let y = mark.bottom + 1;
            answer.segment((mark.left, y), (mark.right, y), Tone::BLACK, THICKNESS + 1)
        }
    }
}

#[cfg(test)]
mod tests;
