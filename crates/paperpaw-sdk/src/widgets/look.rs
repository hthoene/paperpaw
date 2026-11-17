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

use crate::env::Env;
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






























/// The height of a line of text `size` pixels large in the UI font.
pub fn line_height(env: &Env<'_>, size: f32) -> u16 {
    UiStyle::new(env)
        .font(env)
        .map_or(size as u16, |f| f.line_metrics(size).line_height as u16)
}

/// How far apart the lines of a heading or title on more lines are
/// ([`FontHandle::heading_line_height`]): closer than those of text
/// ([`line_height`]).
///
/// [`FontHandle::heading_line_height`]: crate::FontHandle::heading_line_height
pub fn heading_line_height(env: &Env<'_>, size: f32) -> u16 {
    UiStyle::new(env)
        .font(env)
        .map_or(size as u16, |f| f.heading_line_height(size) as u16)
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

    /// [`new`](Self::new) or, for a title or heading, its lines a
    /// heading's apart.
    fn made(
        env: &Env<'_>,
        text: &str,
        size: f32,
        (width, height): (u16, u16),
        heading: bool,
    ) -> Self {
        let mut wrapped = Self {
            text: String::from(text),
            size,
            lines: Vec::new(),
            whole: true,
            heading,
        };
        let Some(font) = UiStyle::new(env).font(env) else {
            return wrapped;
        };
        let width = f32::from(width);
        let (ranges, _) = break_words(&font, size, text, width);
        let fits = lines_in(env, size, heading, height);
        let count = ranges.len();
        for (n, range) in ranges.into_iter().enumerate() {
            let own = text[range.clone()].trim_end();
            if n + 1 == fits && n + 1 < count {
                // The last line there is room for: the rest on it, its
                // breaks as spaces.
                let rest = text[range.start..].trim_end().replace('\n', " ");
                wrapped
                    .lines
                    .push(ui::fit(&font, size, &format!("{rest} …"), width));
                wrapped.whole = false;
                break;
            }
            let shown = ui::fit(&font, size, own, width);
            let shortened = shown != own;
            wrapped.lines.push(shown);
            if shortened {
                wrapped.whole = false;
                break;
            }
        }
        wrapped
    }

    /// `text` in lines `width` pixels wide at the largest size from `max`
    /// down to `min` at which it fits `height` pixels whole, no word
    /// wider than a line; at `min` as [`new`](Self::new) where it does
    /// not.
    pub(crate) fn fitting(
        env: &Env<'_>,
        text: &str,
        max: f32,
        min: f32,
        width: u16,
        height: u16,
    ) -> Self {
        Self::fitted(env, text, (max, min), (width, height), false)
    }

    fn fitted(
        env: &Env<'_>,
        text: &str,
        (max, min): (f32, f32),
        (width, height): (u16, u16),
        heading: bool,
    ) -> Self {
        let Some(font) = UiStyle::new(env).font(env) else {
            return Self::made(env, text, max, (width, height), heading);
        };
        let room = f32::from(width);
        let size = largest_size(max, min, |size| {
            let (lines, too_wide) = break_words(&font, size, text, room);
            !too_wide && lines_height(env, size, heading, lines.len()) <= height
        });
        Self::made(
            env,
            text,
            size.unwrap_or(min.min(max)),
            (width, height),
            heading,
        )
    }


    /// Whether its lines show all of the text, nothing shortened or left
    /// out: a widget may then rather show something shorter.
    pub fn whole(&self) -> bool {
        self.whole
    }


    /// The height its lines take.
    pub(crate) fn height(&self, env: &Env<'_>) -> u16 {
        lines_height(env, self.size, self.heading, self.lines.len())
    }

    /// Draws its lines in `area`, each placed as `align` says, together
    /// in the middle from top to bottom; returns the height drawn.
    pub fn draw(
        &self,
        canvas: &mut Canvas<'_>,
        env: &Env<'_>,
        area: Rect,
        align: super::Align,
        color: Color,
    ) -> u16 {
        let Some(font) = UiStyle::new(env).font(env) else {
            return 0;
        };
        let size = self.size;
        let line = f32::from(line_height(env, size)).max(1.0);
        let apart = f32::from(step(env, size, self.heading)).max(1.0);
        let total = self.height(env);
        let top = area.y + area.height.saturating_sub(total) / 2;
        // Told as one text, drawn as lines.
        let at = Rect::new(area.x, top, area.width, total);
        canvas.describe(paperpaw_graphics::Role::Text, &self.text, at);
        let metrics = font.line_metrics(size);
        // Cut at the sides only: an area lower than a line still shows it.
        let tall = Rect::new(
            area.x,
            area.y,
            area.width,
            area.height.max(top - area.y + total),
        );
        let mut canvas = canvas.clipped(ink_room(tall));
        for (n, shown) in self.lines.iter().enumerate() {
            let y = f32::from(top) + apart * n as f32;
            let baseline = y + (line + metrics.ascent - metrics.descent) / 2.0;
            let x = align.x(area, font.measure(size, shown));
            font.draw(&mut canvas, size, x, baseline as i32, shown, color);
        }
        total
    }
}

/// From one line's top to the next one's: a heading's line, or text's.
fn step(env: &Env<'_>, size: f32, heading: bool) -> u16 {
    if heading {
        heading_line_height(env, size)
    } else {
        line_height(env, size)
    }
}

/// The height of `count` lines (none: nothing): a line, and a step for
/// each of the others ([`step`]).
fn lines_height(env: &Env<'_>, size: f32, heading: bool, count: usize) -> u16 {
    match count {
        0 => 0,
        _ => {
            let others = u16::try_from(count - 1).unwrap_or(u16::MAX);
            line_height(env, size).saturating_add(step(env, size, heading).saturating_mul(others))
        }
    }
}

/// How many lines `height` holds (one at least).
fn lines_in(env: &Env<'_>, size: f32, heading: bool, height: u16) -> usize {
    let line = line_height(env, size).max(1);
    let apart = step(env, size, heading).max(1);
    1 + usize::from(height.saturating_sub(line) / apart)
}


