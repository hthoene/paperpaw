//! What the screens share: boxes, the look (thick outlines, the style's ink
//! and paper alone), text, icons, the round switches, the bars, the bands
//! at the top, and the parts the focus visits.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Point, Rect,
    native::{Error, Host, canvas::PartRole, fonts::Font},
};
use paperpaw_ui_kit::{
    Messages, Paint, Style, focus, icons, paint, parts,
    widget::{
        Align, shape,
        text::{self as wtext},
    },
};

use crate::model::{Target, Zone};

/// A box in pixels, signed so that sums need no care.
#[derive(Debug, Clone, Copy)]
pub struct B {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

fn px(value: i32) -> u16 {
    u16::try_from(value.max(0)).unwrap_or(u16::MAX)
}

impl B {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub fn rect(self) -> Rect {
        Rect::new(px(self.x), px(self.y), px(self.w), px(self.h))
    }

    /// Smaller by `dx` and `dy` on every side (larger for a negative one).
    pub const fn inset_xy(self, dx: i32, dy: i32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.w - 2 * dx, self.h - 2 * dy)
    }

    pub const fn inset(self, d: i32) -> Self {
        self.inset_xy(d, d)
    }

    /// Row `index` of `count` equal rows, `gap` apart.
    pub const fn row(self, index: i32, count: i32, gap: i32) -> Self {
        let h = (self.h - gap * (count - 1)) / count;
        Self::new(self.x, self.y + index * (h + gap), self.w, h)
    }

    /// Column `index` of `count` equal columns, `gap` apart.
    pub const fn col(self, index: i32, count: i32, gap: i32) -> Self {
        let w = (self.w - gap * (count - 1)) / count;
        Self::new(self.x + index * (w + gap), self.y, w, self.h)
    }

    pub const fn top(self, h: i32) -> Self {
        Self::new(self.x, self.y, self.w, h)
    }

    /// What is left below the top `h` and a `gap`.
    pub const fn below(self, h: i32, gap: i32) -> Self {
        Self::new(self.x, self.y + h + gap, self.w, self.h - h - gap)
    }

    pub const fn bottom(self, h: i32) -> Self {
        Self::new(self.x, self.y + self.h - h, self.w, h)
    }

    /// What is left above the bottom `h` and a `gap`.
    pub const fn above(self, h: i32, gap: i32) -> Self {
        Self::new(self.x, self.y, self.w, self.h - h - gap)
    }

    pub const fn left(self, w: i32) -> Self {
        Self::new(self.x, self.y, w, self.h)
    }

    /// What is left to the right of the left `w` and a `gap`.
    pub const fn right_of(self, w: i32, gap: i32) -> Self {
        Self::new(self.x + w + gap, self.y, self.w - w - gap, self.h)
    }

    /// The part from `from` to `to` hundredths of the height.
    pub const fn part(self, from: i32, to: i32) -> Self {
        Self::new(
            self.x,
            self.y + self.h * from / 100,
            self.w,
            self.h * (to - from) / 100,
        )
    }

    /// A box `w` by `h` in the middle of this one.
    pub const fn centred(self, w: i32, h: i32) -> Self {
        Self::new(self.x + (self.w - w) / 2, self.y + (self.h - h) / 2, w, h)
    }

    pub const fn middle(self) -> Point {
        Point {
            x: self.x + self.w / 2,
            y: self.y + self.h / 2,
        }
    }
}

/// What a screen is drawn with.
pub struct Ui<'a, 'h> {
    pub host: &'a Host<'h>,
    pub font: &'a Font<'h>,
    pub icons: Option<&'a Font<'h>>,
    pub style: &'a Style,
    pub messages: Messages,
    pub ops: Vec<Paint>,
    /// What the focus visits, in the order drawn.
    pub zones: Vec<Zone>,
    /// The part of the display that is the screen's.
    pub screen: B,
    /// The interface's text size in pixels, and its small one.
    pub u: f32,
    pub small: f32,
    pub ink: Color,
    pub paper: Color,
    /// How thick outlines are.
    pub stroke: i32,
    pub pad: i32,
    pub gap: i32,
    /// The part the focus is on, to draw it as chosen.
    pub focus: usize,
}

impl<'a, 'h> Ui<'a, 'h> {
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn new(
        host: &'a Host<'h>,
        (font, icons): (&'a Font<'h>, Option<&'a Font<'h>>),
        style: &'a Style,
        screen: B,
        focus: usize,
    ) -> Result<Self, Error> {
        let w = screen.w;
        Ok(Self {
            host,
            font,
            icons,
            style,
            messages: Messages::new()?,
            ops: Vec::new(),
            zones: Vec::new(),
            screen,
            u: style.text,
            small: style.small,
            ink: style.ink,
            paper: style.paper,
            stroke: (w / 120).max(i32::from(style.line)).max(3),
            pad: (w / 16).max(i32::from(style.margin)),
            gap: (w / 24).max(i32::from(style.gap)),
            focus,
        })
    }

    /// The catalog's text for `key`.
    ///
    /// # Errors
    /// The host's refusal and [`Error::NoMemory`].
    pub fn t(&mut self, key: &str) -> Result<String, Error> {
        self.messages.copy(self.host, key)
    }

    pub fn push(&mut self, op: Paint) -> Result<(), Error> {
        paint::push(&mut self.ops, op)
    }

    /// Whether the part about to be noted is the one the focus is on.
    pub fn focused(&self) -> bool {
        self.zones.len() == self.focus
    }

    /// Notes a part the focus visits, and tells the host what it is: a button
    /// named `label`, a switch (`on`) or a slider.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn zone(&mut self, target: Target, b: B, role: PartRole, label: &str) -> Result<(), Error> {
        let focused = self.focused();
        let describing = self.style.describing;
        paint::describe_named(
            &mut self.ops,
            describing,
            role,
            b.rect(),
            label,
            focused,
            false,
        )?;
        paperpaw_ui_kit::mem::push(
            &mut self.zones,
            Zone {
                target,
                rect: b.rect(),
            },
        )
    }

    /// The largest size up to `max` at which every one of `texts` is `width`
    /// wide or less (never under 45 % of `max`).
    pub fn common(&self, texts: &[&str], max: f32, width: i32) -> Result<f32, Error> {
        let room = width.max(1) as f32;
        let min = max * 0.45;
        let size = wtext::largest_size(max, min, |size| {
            for text in texts {
                if self.font.measure_text(size, text)? > room {
                    return Ok(false);
                }
            }
            Ok(true)
        })?;
        Ok(size.unwrap_or(min))
    }

    /// One line at `size`, shortened where it does not fit.
    pub fn text(
        &mut self,
        b: B,
        align: Align,
        size: f32,
        text: &str,
        color: Color,
    ) -> Result<(), Error> {
        wtext::line(
            &mut self.ops,
            self.font,
            b.rect(),
            align,
            (size, size),
            text,
            color,
        )
        .map(|_| ())
    }

    /// One line as large as fits, from `size` down to 45 % of it.
    pub fn fit(
        &mut self,
        b: B,
        align: Align,
        size: f32,
        text: &str,
        color: Color,
    ) -> Result<(), Error> {
        wtext::line(
            &mut self.ops,
            self.font,
            b.rect(),
            align,
            (size, size * 0.45),
            text,
            color,
        )
        .map(|_| ())
    }

    /// One line, bolder, as large as fits from `size` down.
    pub fn bold(
        &mut self,
        b: B,
        align: Align,
        size: f32,
        text: &str,
        color: Color,
    ) -> Result<(), Error> {
        let extra = paint::bold_extra(size) as f32 * text.chars().count() as f32;
        let room = (b.w as f32 - extra).max(1.0);
        let (size, shown) = wtext::fit_line(self.font, size, size * 0.45, text, room)?;
        let width = self.font.measure_text(size, &shown)? + extra;
        let x = align.x(b.rect(), width);
        let baseline = wtext::ink_baseline(self.font, size, &shown, b.rect())?;
        self.push(Paint::Bold {
            text: shown,
            size,
            origin: paperpaw_sdk::native::fonts::TextOrigin::new(x, baseline),
            color,
        })
    }

    /// A rounded outline, `thickness` wide, growing inwards.
    pub fn frame(&mut self, b: B, radius: i32, thickness: i32, color: Color) -> Result<(), Error> {
        self.push(Paint::Focus(
            b.rect(),
            radius.max(0) as u32,
            Some(thickness.max(1) as u32),
            color,
        ))
    }

    /// A rounded plate.
    pub fn plate(&mut self, b: B, radius: i32, color: Color) -> Result<(), Error> {
        self.push(Paint::Focus(b.rect(), radius.max(0) as u32, None, color))
    }

    pub fn fill(&mut self, b: B, color: Color) -> Result<(), Error> {
        shape::fill(&mut self.ops, b.rect(), color)
    }

    /// The radius of cards.
    pub fn radius(&self) -> i32 {
        (self.screen.w / 22).max(8) * 3 / 2
    }

    /// The icon `name` in the middle of `b`.
    pub fn icon(&mut self, name: &str, size: f32, b: B, color: Color) -> Result<(), Error> {
        let Some(font) = self.icons else {
            return Ok(());
        };
        let Some(glyph) = icons::glyph(self.host, name)? else {
            return Ok(());
        };
        icons::draw_centred(&mut self.ops, font, &glyph, size, b.rect(), color)
    }

    /// The focus on `b`, as the theme draws it; returns the ink of what is
    /// drawn on it.
    pub fn focus_on(&mut self, b: B) -> Result<Color, Error> {
        focus::frame(&mut self.ops, self.style, b.rect())
    }

    /// The focus of the round parts and the bars: a second outline round
    /// them (the card's focus is the theme's, [`Ui::focus_on`]).
    pub fn ring(&mut self, b: B, radius: i32) -> Result<(), Error> {
        let out = self.stroke + 3;
        let (stroke, ink) = (self.stroke, self.ink);
        self.frame(b.inset(-out), radius + out, stroke, ink)
    }

    /// A card: a thick round outline; the focus on it where `focused`.
    /// Returns its inside and the ink to draw in.
    pub fn card(&mut self, b: B, focused: bool) -> Result<(B, Color), Error> {
        let radius = self.radius();
        let stroke = self.stroke;
        let ink = self.ink;
        self.frame(b, radius, stroke, ink)?;
        let tone = if focused { self.focus_on(b)? } else { ink };
        Ok((b.inset_xy(self.pad * 3 / 4, stroke + 8), tone))
    }

    /// A progress bar: the kit's, `permille` thousandths full.
    pub fn bar(&mut self, b: B, permille: u16, ink: Color) -> Result<(), Error> {
        parts::progress_bar(&mut self.ops, self.style, b.rect(), permille, ink)
    }

    /// The height of a bar of the kit's, for room.
    pub fn bar_height(&self) -> i32 {
        i32::from(parts::bar_height(self.style))
    }

    /// A tag in a capsule: the kit's pill.
    pub fn pill(&mut self, b: B, text: &str, ink: Color, filled: bool) -> Result<(), Error> {
        parts::pill(
            &mut self.ops,
            self.font,
            self.style,
            b.rect(),
            text,
            (ink, filled),
        )
    }

    /// The width a pill with `text` needs.
    pub fn pill_width(&self, text: &str) -> Result<i32, Error> {
        parts::pill_width(self.font, self.style, text).map(i32::from)
    }

    /// A round switch: a disc of `radius` at `centre` with an icon, filled where
    /// `on`, an outline where not; the focus a frame round it.
    pub fn disc(&mut self, centre: Point, radius: i32, icon: &str, on: bool) -> Result<B, Error> {
        let r = radius.max(1) as u32;
        let around = B::new(centre.x - radius, centre.y - radius, 2 * radius, 2 * radius);
        if on {
            shape::fill_circle(&mut self.ops, centre, r, self.ink)?;
            self.icon(icon, radius as f32 * 1.3, around, self.paper)?;
        } else {
            shape::stroke_circle(&mut self.ops, centre, r, self.stroke as u32, self.ink)?;
            self.icon(icon, radius as f32 * 1.3, around, self.ink)?;
        }
        Ok(around)
    }

    /// The band at the top of a screen: its title, a note (a page count) and a
    /// thick line. Returns what is left below it.
    pub fn header(&mut self, title: &str, note: &str) -> Result<B, Error> {
        let size = (self.u * 1.6).min(self.screen.w as f32 / 9.0);
        let h = (size * 2.0) as i32;
        let band = self.screen.top(h).inset_xy(self.pad, 0);
        let note_size = self.small.min(size * 0.7);
        let note_w = if note.is_empty() {
            0
        } else {
            self.font.measure_text(note_size, note)? as i32 + self.gap
        };
        let ink = self.ink;
        self.bold(band.left(band.w - note_w), Align::Left, size, title, ink)?;
        if !note.is_empty() {
            self.text(band, Align::Right, note_size, note, ink)?;
        }
        let line = B::new(
            self.screen.x + self.pad,
            self.screen.y + h - self.stroke,
            self.screen.w - 2 * self.pad,
            self.stroke,
        );
        self.fill(line, ink)?;
        Ok(self.screen.below(h, 0))
    }
}

/// `text` in the language's digits and plain ASCII where a count is meant: a
/// number and a percent sign.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn percent_text(percent: u8) -> Result<String, Error> {
    paperpaw_ui_kit::text::format(format_args!("{percent} %"))
}
