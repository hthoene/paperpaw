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
//! Text in a widget fits its place rather than running out of it: what
//! must stay on one line (a time, a date, a number, a title in a small
//! cell) is made smaller down to a least size and only then shortened
//! with "…" ([`fit_line`], [`draw_line`]); text that may take several
//! lines breaks between words, never inside one, and is made smaller
//! before it is cut ([`Wrapped::fitting`]). Sizes are measured as they
//! are drawn, so fonts with a few sizes only (the packs of small
//! devices) fit as well as fonts drawn at any size ([`largest_size`]).

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::ops::Range;

use paperpaw_graphics::Canvas;
use paperpaw_platform::{Color, Rect};

use crate::env::Env;
use crate::fonts::FontHandle;
use crate::id::Id;
use crate::settings::{Choices, SettingKind, SettingSpec, SettingValue};
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

/// The values of a widget's size setting ([`size_setting`]).
pub const SIZES: &[&str] = &[
    "core.size.small",
    "core.size.medium",
    "core.size.large",
    "core.size.fit",
];

/// [`SIZES`] but "as large as fits".
const SIZES_SET: [&str; 3] = [SIZES[0], SIZES[1], SIZES[2]];

/// The sizes offered: with `fit`, also "as large as fits".
const fn sizes(fit: bool) -> &'static [&'static str] {
    if fit { SIZES } else { &SIZES_SET }
}

/// How large a widget's text is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TextSize {
    Small,
    Medium,
    Large,
    /// As large as its place allows: the widget works it out.
    Fit,
}

impl TextSize {
    /// The size a value of [`SIZES`] names; medium for others.
    pub fn of(value: &str) -> Self {
        match SIZES.iter().position(|s| *s == value) {
            Some(0) => TextSize::Small,
            Some(2) => TextSize::Large,
            Some(3) => TextSize::Fit,
            _ => TextSize::Medium,
        }
    }

    /// Its value in [`SIZES`].
    pub const fn value(self) -> &'static str {
        SIZES[self as usize]
    }

    /// The text size in pixels: the body's, a title's, or larger (also
    /// for [`Fit`](TextSize::Fit), where a widget has nothing better).
    pub fn text(self, style: &UiStyle) -> f32 {
        match self {
            TextSize::Small => style.text,
            TextSize::Medium => style.title,
            TextSize::Large | TextSize::Fit => style.title * 1.6,
        }
    }
}

/// The values of a widget's padding setting ([`padding_spec`]).
pub const PADDINGS: &[&str] = &[
    "core.padding.auto",
    "core.padding.none",
    "core.padding.small",
    "core.padding.normal",
    "core.padding.large",
];

/// How far a widget's content keeps from the edges of its frame (or of
/// its place, without a frame), in steps of the style's gap: none, half
/// a gap, a gap, two gaps. A frame's line comes on top of that, so the
/// content never touches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Padding {
    /// As suits the frame: [`None`](Padding::None) without one, so that
    /// what the widget shows lines up with the page's margins and the
    /// widgets beside it (the grid keeps a gap between them), and
    /// [`Normal`](Padding::Normal) inside one.
    Auto,
    None,
    Small,
    Normal,
    Large,
}

impl Padding {
    /// The padding a value of [`PADDINGS`] names; automatic for others.
    pub fn of(value: &str) -> Self {
        match PADDINGS.iter().position(|p| *p == value) {
            Some(1) => Padding::None,
            Some(2) => Padding::Small,
            Some(3) => Padding::Normal,
            Some(4) => Padding::Large,
            _ => Padding::Auto,
        }
    }

    /// Its value in [`PADDINGS`].
    pub const fn value(self) -> &'static str {
        PADDINGS[self as usize]
    }

    /// The pixels it keeps from the edges of `frame` drawn in `style`.
    pub fn pixels(self, style: &UiStyle, frame: Frame) -> u16 {
        let gap = style.gap;
        let step = match self {
            Padding::Auto if frame == Frame::None => 0,
            Padding::Auto | Padding::Normal => gap,
            Padding::None => 0,
            Padding::Small => gap / 2,
            Padding::Large => gap * 2,
        };
        // Clear of the frame's line (a card's edge as well).
        let line = match frame {
            Frame::None => 0,
            _ => style.line * 2,
        };
        (step + line) as u16
    }
}

/// A widget's place inside its frame, and the colours to draw in there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct WidgetLook {
    /// Where its content goes.
    pub area: Rect,
    /// Text and lines.
    pub ink: Color,
    /// Text that matters less (captions, notes).
    pub faint: Color,
    /// What lies behind it: white, or black on a dark card.
    pub paper: Color,
    pub frame: Frame,
}

impl WidgetLook {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(area: Rect, ink: Color, faint: Color, paper: Color, frame: Frame) -> Self {
        Self {
            area,
            ink,
            faint,
            paper,
            frame,
        }
    }

    /// The same look in another area (e.g. a caption's).
    pub fn within(self, area: Rect) -> Self {
        Self { area, ..self }
    }
}

/// The id of `widget`'s setting `key`: `<widget>.<key>`.
pub fn setting_id(widget: &Id, key: &str) -> Id {
    Id::parse(&format!("{widget}.{key}")).unwrap_or_else(|_| widget.clone())
}

/// The setting of what `widget` is drawn in, `<widget>.frame`, in the
/// widget's settings group; [`frame`] reads it.
pub fn frame_setting(widget: &Id, default: Frame) -> SettingSpec {
    choice_setting(
        widget,
        "frame",
        "core.setting.frame",
        FRAMES,
        default.value(),
    )
    .description("core.setting.frame.help")
}

/// [`frame_setting`] kept in the program (a `static` list), for a
/// `static` list of the widget's settings: `id` must be
/// `<widget>.frame`.
pub const fn frame_spec(id: &'static str, widget: Id, default: Frame) -> SettingSpec {
    SettingSpec::choice(id, "core.setting.frame", FRAMES, default.value(), widget)
        .description("core.setting.frame.help")
}

/// The setting of how far `widget`'s content keeps from its frame,
/// `<widget>.padding`, in the widget's settings group; [`padding`] reads
/// it. Automatic ([`Padding::Auto`]) suits nearly every widget.
pub fn padding_setting(widget: &Id, default: Padding) -> SettingSpec {
    choice_setting(
        widget,
        "padding",
        "core.setting.padding",
        PADDINGS,
        default.value(),
    )
    .description("core.setting.padding.help")
}

/// [`padding_setting`] kept in the program (a `static` list): `id`
/// must be `<widget>.padding`.
pub const fn padding_spec(id: &'static str, widget: Id, default: Padding) -> SettingSpec {
    SettingSpec::choice(
        id,
        "core.setting.padding",
        PADDINGS,
        default.value(),
        widget,
    )
    .description("core.setting.padding.help")
}

/// How far `widget`'s content keeps from its frame, by its
/// [`padding_setting`]; automatic without one.
pub fn padding(env: &Env<'_>, widget: &str) -> Padding {
    Padding::of(env.settings().text(&format!("{widget}.padding")))
}

/// [`size_setting`] kept in the program (a `static` list): `id`
/// must be `<widget>.size`.
pub const fn size_spec(id: &'static str, widget: Id, default: TextSize, fit: bool) -> SettingSpec {
    SettingSpec::choice(id, "core.setting.size", sizes(fit), default.value(), widget)
        .description("core.setting.size.help")
}

/// [`part_setting`] kept in the program (a `static` list): `id`
/// must be `<widget>.<part>`.
pub const fn part_spec(
    id: &'static str,
    widget: Id,
    title: &'static str,
    default: bool,
) -> SettingSpec {
    SettingSpec::switch(id, title, default, widget).description("core.setting.part.help")
}

/// What `widget` is drawn in, by its [`frame_setting`]; none without one.
pub fn frame(env: &Env<'_>, widget: &str) -> Frame {
    let own = format!("{widget}.frame");
    // Every widget's, where the reader chose one for all and not one for
    // this widget.
    let all = env
        .settings
        .text(crate::builtin::settings::WIDGET_FRAME.static_str());
    if all != OWN_FRAME && FRAMES.contains(&all) && !env.settings().is_chosen(&own) {
        return Frame::of(all);
    }
    Frame::of(env.settings().text(&own))
}

/// The value of [`WIDGET_FRAME`](crate::builtin::settings::WIDGET_FRAME)
/// for each widget its own frame.
pub const OWN_FRAME: &str = "core.frame.own";

/// The values of [`WIDGET_FRAME`](crate::builtin::settings::WIDGET_FRAME):
/// each widget its own, or one of [`FRAMES`] for all.
pub const ALL_FRAMES: &[&str] = &[
    OWN_FRAME,
    "core.frame.none",
    "core.frame.line",
    "core.frame.card",
    "core.frame.dark",
];

/// The setting of how large `widget`'s text is, `<widget>.size`, in the
/// widget's settings group; with `fit`, also "as large as fits". [`size`]
/// reads it.
pub fn size_setting(widget: &Id, default: TextSize, fit: bool) -> SettingSpec {
    choice_setting(
        widget,
        "size",
        "core.setting.size",
        sizes(fit),
        default.value(),
    )
    .description("core.setting.size.help")
}

/// How large `widget`'s text is, by its [`size_setting`]; medium without
/// one.
pub fn size(env: &Env<'_>, widget: &str) -> TextSize {
    TextSize::of(env.settings().text(&format!("{widget}.size")))
}

/// A switch for a part of what `widget` shows, `<widget>.<part>`, in the
/// widget's settings group; [`shows`] reads it.
pub fn part_setting(
    widget: &Id,
    part: &str,
    title: impl Into<Cow<'static, str>>,
    default: bool,
) -> SettingSpec {
    let mut spec = SettingSpec::new(
        setting_id(widget, part),
        "",
        SettingKind::Bool,
        SettingValue::Bool(default),
        widget.clone(),
    )
    .description("core.setting.part.help");
    spec.title = title.into();
    spec
}

/// Whether `widget` shows `part` ([`part_setting`]).
pub fn shows(env: &Env<'_>, widget: &str, part: &str) -> bool {
    env.settings().bool(&format!("{widget}.{part}"))
}

/// A choice of `values` for `widget`, `<widget>.<key>`, in the widget's
/// settings group.
pub fn choice_setting(
    widget: &Id,
    key: &str,
    title: impl Into<Cow<'static, str>>,
    values: &'static [&'static str],
    default: &'static str,
) -> SettingSpec {
    let mut spec = SettingSpec::new(
        setting_id(widget, key),
        "",
        SettingKind::Choice(Choices::Fixed(values)),
        SettingValue::Text(Cow::Borrowed(default)),
        widget.clone(),
    );
    spec.title = title.into();
    spec
}

/// `widget`'s place inside its frame in `area` and the colours there,
/// without drawing (e.g. for its targets). Its content keeps from the
/// frame as its [`padding`] says, but never more than a quarter of the
/// place on each side, so small places keep room for it.
pub fn widget_look(env: &Env<'_>, widget: &str, area: Rect) -> WidgetLook {
    let style = UiStyle::new(env);
    let frame = frame(env, widget);
    let pad = padding(env, widget).pixels(&style, frame);
    let pad_x = pad.min(area.width / 4);
    let pad_y = pad.min(area.height / 4);
    let inner = Rect::new(
        area.x + pad_x,
        area.y + pad_y,
        area.width.saturating_sub(2 * pad_x),
        area.height.saturating_sub(2 * pad_y),
    );
    let (ink, faint, paper) = match frame {
        Frame::Dark => (
            Color::WHITE,
            if style.strong {
                Color::WHITE
            } else {
                style.ink(ui::Ink::OnDarkMuted)
            },
            Color::BLACK,
        ),
        _ => (Color::BLACK, style.ink(ui::Ink::Muted), Color::WHITE),
    };
    WidgetLook {
        area: inner,
        ink,
        faint,
        paper,
        frame,
    }
}

/// Draws `widget`'s frame in `area` (cleared before) and returns where
/// its content goes and in which colours.
pub fn draw_frame(canvas: &mut Canvas<'_>, env: &Env<'_>, widget: &str, area: Rect) -> WidgetLook {
    let style = UiStyle::new(env);
    let look = widget_look(env, widget, area);
    // As cards are: a look of a plugin's own paints it, else the usual.
    crate::ui::layout::paint_frame(canvas, &style, area, look.frame);
    look
}

/// The height `widget`'s frame takes above and below its content, as its
/// settings make it ([`widget_look`]): what a widget adds to the height of what
/// it shows.
pub fn frame_height(env: &Env<'_>, widget: &str) -> u16 {
    let probe = Rect::new(0, 0, 1000, 1000);
    1000 - widget_look(env, widget, probe).area.height
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

/// The height a caption takes ([`draw_caption`]), the gap below it
/// included.
pub fn caption_height(env: &Env<'_>) -> u16 {
    let style = UiStyle::new(env);
    line_height(env, style.small) + style.gap as u16 / 2
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

/// Draws `text` small at the top of `look`'s area, placed as `widget`'s
/// alignment says ([`align`](super::align)), e.g. "Today" over the
/// numbers; returns the area left below it.
pub fn draw_caption(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    widget: &str,
    look: &WidgetLook,
    text: &str,
) -> Rect {
    let style = UiStyle::new(env);
    let area = look.area;
    let height = caption_height(env).min(area.height);
    if let Some(font) = style.font(env) {
        let text = ui::fit(&font, style.small, text, f32::from(area.width));
        let x = super::align(env, widget).x(area, font.measure(style.small, &text));
        let row = Rect::new(area.x, area.y, area.width, line_height(env, style.small));
        let mut canvas = canvas.clipped(ink_room(area));
        ui::draw_label(&mut canvas, &font, style.small, x, row, &text, look.faint);
    }
    Rect::new(area.x, area.y + height, area.width, area.height - height)
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

/// `text` on one line `width` pixels wide: the size from `max` down to
/// `min` at which it fits whole, or at `min` shortened with "…". For
/// what must not wrap: a time, a date, a number, a title in a small
/// cell.
pub fn fit_line(
    font: &FontHandle<'_>,
    max: f32,
    min: f32,
    text: &str,
    width: f32,
) -> (f32, String) {
    match largest_size(max, min, |size| font.measure(size, text) <= width) {
        Some(size) => (size, String::from(text)),
        None => {
            let min = min.min(max);
            (min, ui::fit(font, min, text, width))
        }
    }
}

/// The baseline that puts the ink of `text` at `size` in the middle of
/// `band` from top to bottom, rather than its line (which leaves more
/// room above the letters than below them): digits and words then sit
/// in the middle of their place as signs do.
pub fn ink_baseline(font: &FontHandle<'_>, size: f32, text: &str, band: Rect) -> i32 {
    let (top, bottom) = font.ink(size, text).unwrap_or_else(|| {
        let metrics = font.line_metrics(size);
        (-(metrics.ascent as i32), -(metrics.descent as i32))
    });
    let room = i32::from(band.height) - (bottom - top);
    i32::from(band.y) + room.div_euclid(2) - top
}

/// Draws `text` on one line in `area` ([`fit_line`]: from `max` down to
/// `min`, then shortened), placed as `align` says and centred by its ink
/// from top to bottom; returns the size drawn.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub fn draw_line(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    area: Rect,
    align: super::Align,
    max: f32,
    min: f32,
    text: &str,
    color: Color,
) -> f32 {
    let Some(font) = UiStyle::new(env).font(env) else {
        return min;
    };
    let (size, shown) = fit_line(&font, max, min, text, f32::from(area.width));
    let width = font.measure(size, &shown);
    let x = align.x(area, width);
    let at = Rect::new(x.max(0.0) as u16, area.y, width as u16, area.height);
    canvas.describe(paperpaw_graphics::Role::Text, text, at);
    let baseline = ink_baseline(&font, size, &shown, area);
    font.draw(canvas, size, x, baseline, &shown, color);
    size
}

/// Draws `text` `size` pixels large in `area`, broken into lines between
/// words where it is wider, each line placed as `align` says, together
/// in the middle from top to bottom. Where it does not fit, it is made
/// smaller, down to the style's small size, and only then are words
/// shortened with "…" and lines left out (the last one shown ending in
/// "…"). Returns the height drawn. A widget drawing the same text again
/// and again (in every band of a frame) breaks it once ([`Wrapped`]) and
/// draws that.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub fn draw_wrapped(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    area: Rect,
    align: super::Align,
    size: f32,
    text: &str,
    color: Color,
) -> u16 {
    let small = UiStyle::new(env).small;
    Wrapped::fitting(env, text, size, small, area.width, area.height)
        .draw(canvas, env, area, align, color)
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
    /// `text` `size` pixels large in lines `width` pixels wide, broken
    /// between words, as many as fit `height` pixels (at least one). Where
    /// there is more, or a word is wider than a line, the text ends there
    /// in "…": never a shortened word with more text after it.
    pub fn new(env: &Env<'_>, text: &str, size: f32, width: u16, height: u16) -> Self {
        Self::made(env, text, size, (width, height), false)
    }

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
    pub fn fitting(env: &Env<'_>, text: &str, max: f32, min: f32, width: u16, height: u16) -> Self {
        Self::fitted(env, text, (max, min), (width, height), false)
    }

    /// A title or heading as [`fitting`](Self::fitting) makes text, its
    /// lines a heading's apart ([`heading_line_height`]): closer, so that
    /// they read as one.
    pub fn heading(env: &Env<'_>, text: &str, max: f32, min: f32, width: u16, height: u16) -> Self {
        Self::fitted(env, text, (max, min), (width, height), true)
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

    /// The size its lines are drawn at.
    pub fn size(&self) -> f32 {
        self.size
    }

    /// Whether its lines show all of the text, nothing shortened or left
    /// out: a widget may then rather show something shorter.
    pub fn whole(&self) -> bool {
        self.whole
    }

    /// Its lines as drawn.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// The height its lines take.
    pub fn height(&self, env: &Env<'_>) -> u16 {
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

/// Draws `text` in the body size in the middle of `look`'s area, e.g.
/// what a widget would show is not there yet ("No words saved yet"):
/// smaller, down to the small size, where it does not fit.
pub fn draw_message(canvas: &mut Canvas<'_>, env: &Env<'_>, look: &WidgetLook, text: &str) {
    let style = UiStyle::new(env);
    let (area, center) = (look.area, super::Align::Center);
    draw_wrapped(canvas, env, area, center, style.text, text, look.ink);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_and_sizes_round_trip() {
        for frame in [Frame::None, Frame::Line, Frame::Card, Frame::Dark] {
            assert_eq!(Frame::of(frame.value()), frame);
        }
        for size in [
            TextSize::Small,
            TextSize::Medium,
            TextSize::Large,
            TextSize::Fit,
        ] {
            assert_eq!(TextSize::of(size.value()), size);
        }
        for padding in [
            Padding::Auto,
            Padding::None,
            Padding::Small,
            Padding::Normal,
            Padding::Large,
        ] {
            assert_eq!(Padding::of(padding.value()), padding);
        }
        assert_eq!(Padding::of("core.padding.huge"), Padding::Auto);
        assert_eq!(Frame::of("core.frame.cloud"), Frame::None);
        assert_eq!(TextSize::of(""), TextSize::Medium);
    }

    #[test]
    fn settings_belong_to_the_widget() {
        let widget = Id::new_static("pack.clock");
        let frame = frame_setting(&widget, Frame::Card);
        assert_eq!(frame.id.as_str(), "pack.clock.frame");
        assert_eq!(frame.group, widget);
        let size = size_setting(&widget, TextSize::Fit, false);
        assert_eq!(size.id.as_str(), "pack.clock.size");
        // Without "fit", it is not offered.
        assert!(matches!(&size.kind, SettingKind::Choice(v) if v.len() == 3));
        let part = part_setting(&widget, "date", "pack.date", true);
        assert_eq!(part.id.as_str(), "pack.clock.date");
        assert_eq!(part.default, SettingValue::Bool(true));
    }

    #[test]
    fn padding_keeps_clear_of_the_frame() {
        let style = UiStyle {
            text: 20.0,
            small: 15.0,
            title: 23.0,
            head: 23.0,
            row: 20,
            control_row: 20,
            header: 24,
            margin: 12,
            gap: 16,
            line: 2,
            dark_header: false,
            corner: 4,
            strong: false,
            pill: false,
            focus: ui::Focus::Fill,
            separators: ui::Separators::None,
            plain_header: false,
            cards: false,
            grays: 2,
            finger: 0,
            look: crate::ui::look::Look::none(),
            palette: crate::ui::Palette::DEFAULT,
            direction: 0,
        };
        // Without a frame, automatic is none: lined up with the margins;
        // inside one, normal, as frames always had it.
        assert_eq!(Padding::Auto.pixels(&style, Frame::None), 0);
        assert_eq!(Padding::Small.pixels(&style, Frame::None), 8);
        assert_eq!(Padding::Auto.pixels(&style, Frame::Line), 20);
        assert_eq!(Padding::Normal.pixels(&style, Frame::Card), 20);
        // None leaves the frame's line alone, nothing more.
        assert_eq!(Padding::None.pixels(&style, Frame::None), 0);
        assert_eq!(Padding::None.pixels(&style, Frame::Dark), 4);
        assert_eq!(Padding::Large.pixels(&style, Frame::None), 32);
        let widget = Id::new_static("pack.clock");
        let padding = padding_setting(&widget, Padding::Auto);
        assert_eq!(padding.id.as_str(), "pack.clock.padding");
        assert_eq!(padding.group, widget);
    }

    #[test]
    fn largest_size_is_one_that_fits() {
        // A font of three sizes: what is asked for is drawn at the
        // nearest below, so a step down may change nothing.
        let drawn = |size: f32| {
            [36.0, 28.0, 20.0]
                .into_iter()
                .find(|s| *s <= size)
                .unwrap_or(20.0)
        };
        let width = |size: f32| drawn(size) * 4.0;
        assert_eq!(
            largest_size(48.0, 20.0, |s| width(s) <= 150.0).map(drawn),
            Some(36.0)
        );
        assert_eq!(
            largest_size(48.0, 20.0, |s| width(s) <= 120.0).map(drawn),
            Some(28.0)
        );
        assert_eq!(largest_size(48.0, 20.0, |s| width(s) <= 79.0), None);
        // Never larger than asked, never below the least.
        assert_eq!(largest_size(30.0, 20.0, |_| true), Some(30.0));
        assert_eq!(largest_size(10.0, 20.0, |_| true), Some(10.0));
    }
}
