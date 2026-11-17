//! A look widgets share, so that widgets of different plugins sit well
//! together on a page and the reader shapes them all alike: a frame
//! around a widget ([`Frame`]: none, a line, a light card, a dark card
//! with light text), how far its content keeps from the frame
//! ([`Padding`]), how large its text is ([`TextSize`]), a small caption
//! above it ([`draw_caption`]), and switches for what it shows
//! ([`part_setting`]). Each is a setting in the widget's settings group
//! ([`settings_group`](super::settings_group)), named after the widget:
//! `<widget>.frame`, `<widget>.padding`, `<widget>.size`,
//! `<widget>.<part>`.
//!
//! A widget draws its frame first and then its content into the area the
//! frame leaves, in the frame's colours:
//!
//! ```ignore
//! fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
//!     let look = widgets::draw_frame(canvas, env, ID, area);
//!     let size = widgets::size(env, ID).text(&UiStyle::new(env));
//!     // ... text in look.ink, notes in look.faint, inside look.area
//! }
//! ```
//!
//! **The shared look is a helper, never a must.** A widget may draw all
//! of itself: its own frame, plate, shape and colours, without
//! [`draw_frame`] or [`widget_look`], and then no setting for widgets
//! (the reader's own or those of all widgets: frame, paper, ink,
//! corners, frame line, shadow, spacing) changes what it draws; a grid
//! lays only the screen's [`Plate`](super::Plate) under it. A widget
//! using the helpers may still take what it likes: make its own
//! [`WidgetLook`] ([`WidgetLook::new`]), change the fields of the one it
//! is given, or leave out the settings it does not offer (each reads as
//! automatic). A theme or look plugin paints every frame itself with a
//! frame painter ([`Painters::frame`](crate::ui::look::Painters)); it is
//! given the widget's corners and line in the style, and a paper the
//! reader chose for a widget is painted instead of it. Since interface 6,
//! revision 1 of the `widgets` family, [`widget_look`], [`draw_frame`], [`frame_height`] and
//! [`draw_caption`] run the firmware's code (shared functions, ADR 0026),
//! so the look stays alike across plugins and a later change reaches
//! plugins built from then on without a rebuild.
//!
//! Text in a widget fits its place rather than running out of it: what
//! must stay on one line (a time, a date, a number, a title in a small
//! cell) is made smaller down to a least size and only then shortened
//! with "…" ([`fit_line`], [`draw_line`]); text that may take several
//! lines breaks between words, never inside one, and is made smaller
//! before it is cut ([`Wrapped::fitting`]). Sizes are measured as they
//! are drawn, so fonts with a few sizes only (the packs of small
//! devices) fit as well as fonts drawn at any size ([`largest_size`]).

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::ops::Range;

use paperpaw_graphics::Canvas;
use paperpaw_platform::{Color, Rect};

use crate::fonts::FontHandle;
use crate::ui::{self, UiStyle};

/// The values of a widget's frame setting ([`frame_setting`]).
pub const FRAMES: &[&str] = &[
    "core.frame.none",
    "core.frame.line",
    "core.frame.card",
    "core.frame.dark",
];

/// What a widget is drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Frame {
    /// Nothing: the widget on the page itself.
    None,
    /// A line round it, with the style's corners.
    Line,
    /// A light card behind it (dithered where the panel has few grays).
    Card,
    /// A dark card, the widget's text light on it.
    Dark,
}

impl Frame {
    /// The frame a value of [`FRAMES`] names; none for others.
    pub fn of(value: &str) -> Self {
        match FRAMES.iter().position(|f| *f == value) {
            Some(1) => Frame::Line,
            Some(2) => Frame::Card,
            Some(3) => Frame::Dark,
            _ => Frame::None,
        }
    }

    /// Its value in [`FRAMES`].
    pub const fn value(self) -> &'static str {
        FRAMES[self as usize]
    }
}

































/// How far letters may reach past the box their advance gives them, left
/// and right (a slant, a slash's tail); text in a widget's area is cut
/// there, so that a letter that reaches further (the fraction slash,
/// U+2044, overhangs its neighbours by design) stays inside.
const OVERHANG: u16 = 2;

/// Where text placed in `area` may draw: `area`, [`OVERHANG`] wider on
/// each side.
fn ink_room(area: Rect) -> Rect {
    let left = area.x.saturating_sub(OVERHANG);
    Rect::new(
        left,
        area.y,
        area.width + (area.x - left) + OVERHANG,
        area.height,
    )
}


/// The largest size from `max` down to `min` at which `fits` holds, or
/// `None` where it does not even at `min`. Sizes go down in small steps
/// and are measured as they are drawn: a font with a few sizes only (a
/// pack) draws the nearest of its own, so a step may change nothing,
/// and the size returned is one whose drawing fits, not one that only
/// asked to.
pub fn largest_size(max: f32, min: f32, mut fits: impl FnMut(f32) -> bool) -> Option<f32> {
    let min = min.min(max).max(1.0);
    let mut size = max.max(min);
    while size > min {
        if fits(size) {
            return Some(size);
        }
        size = (size * 0.92).max(min);
    }
    fits(min).then_some(min)
}





/// The lines `text` breaks into at `size` in `width` pixels: between
/// words and at line breaks, never inside a word; and whether a word is
/// wider than a line alone.
fn break_words(
    font: &FontHandle<'_>,
    size: f32,
    text: &str,
    width: f32,
) -> (Vec<Range<usize>>, bool) {
    let mut lines = Vec::new();
    let mut too_wide = false;
    let mut start = 0;
    for paragraph in text.split('\n') {
        let base = start;
        start += paragraph.len() + 1;
        let mut line: Option<Range<usize>> = None;
        let mut at = 0;
        for word in paragraph.split(' ') {
            let range = base + at..base + at + word.len();
            at += word.len() + 1;
            if word.is_empty() {
                continue;
            }
            if font.measure(size, word) > width {
                too_wide = true;
            }
            line = match line {
                Some(current) if font.measure(size, &text[current.start..range.end]) <= width => {
                    Some(current.start..range.end)
                }
                Some(current) => {
                    lines.push(current);
                    Some(range)
                }
                None => Some(range),
            };
        }
        lines.push(line.unwrap_or(base..base));
    }
    (lines, too_wide)
}

/// A text broken into the lines [`draw_wrapped`] draws, once: made
/// where what it says or where it goes changes (e.g. in
/// [`Widget::place`](super::Widget::place)), drawn as often as needed
/// without measuring the text again.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Wrapped {
    text: String,
    size: f32,
    /// The lines, the last one shortened with "…" where there is more.
    lines: Vec<String>,
    /// Whether the lines hold all of the text.
    whole: bool,
    /// Its lines a heading's apart ([`heading_line_height`]), not text's.
    heading: bool,
}

impl Wrapped {





    /// Whether its lines show all of the text, nothing shortened or left
    /// out: a widget may then rather show something shorter.
    pub fn whole(&self) -> bool {
        self.whole
    }



}





