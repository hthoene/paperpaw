//! What the mock screens share: boxes, the look (thick outlines, black and
//! white alone), text, icons, bars, covers and the bands at the top.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Point, Rect,
    native::{
        Error, Host,
        fonts::{Font, TextOrigin},
        image::ImageId,
        library::{Library, Query},
    },
};
use paperpaw_ui_kit::{
    Paint, Style, covers, icons, mem, paint,
    widget::{
        Align, shape,
        text::{self as wtext, Wrapped},
    },
};

use crate::data::Book;

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

    /// Smaller by `d` on every side (larger for a negative `d`).
    pub const fn inset(self, d: i32) -> Self {
        self.inset_xy(d, d)
    }

    pub const fn inset_xy(self, dx: i32, dy: i32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.w - 2 * dx, self.h - 2 * dy)
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
    pub ops: Vec<Paint>,
    /// Pictures drawn after the paint.
    pub pictures: Vec<(ImageId, Rect)>,
    /// The cover of the sample book, where the library has it.
    pub cover: Option<covers::Cover>,
    pub de: bool,
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
}

impl<'a, 'h> Ui<'a, 'h> {
    pub fn new(
        host: &'a Host<'h>,
        font: &'a Font<'h>,
        icons: Option<&'a Font<'h>>,
        style: &Style,
        screen: B,
        de: bool,
    ) -> Self {
        let w = screen.w;
        Self {
            host,
            font,
            icons,
            ops: Vec::new(),
            pictures: Vec::new(),
            cover: None,
            de,
            screen,
            u: style.text,
            small: style.small,
            ink: Color::BLACK,
            paper: Color::WHITE,
            stroke: (w / 120).max(3),
            pad: (w / 16).max(i32::from(style.margin)),
            gap: (w / 24).max(i32::from(style.gap)),
        }
    }

    pub fn tr<'t>(&self, en: &'t str, de: &'t str) -> &'t str {
        if self.de { de } else { en }
    }

    pub fn push(&mut self, op: Paint) -> Result<(), Error> {
        paint::push(&mut self.ops, op)
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
            origin: TextOrigin::new(x, baseline),
            color,
        })
    }

    /// Text on several lines, as large as fits between `max` and `min`,
    /// centred vertically in `b`.
    pub fn wrapped(
        &mut self,
        b: B,
        align: Align,
        (max, min): (f32, f32),
        text: &str,
        color: Color,
    ) -> Result<(), Error> {
        let wrapped = Wrapped::fitting(self.font, text, (max, min), (px(b.w), px(b.h)), true)?;
        wrapped
            .draw(&mut self.ops, self.font, b.rect(), align, color)
            .map(|_| ())
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

    /// The radius of cards and tiles.
    pub fn radius(&self) -> i32 {
        (self.screen.w / 22).max(8)
    }

    /// The focus of the buttons: a second outline round the part.
    pub fn ring(&mut self, b: B, radius: i32) -> Result<(), Error> {
        let out = self.stroke + 3;
        self.frame(b.inset(-out), radius + out, self.stroke, self.ink)
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

    /// A progress bar: a thick pill, filled from the left.
    pub fn bar(&mut self, b: B, percent: i32, ink: Color) -> Result<(), Error> {
        let line = (b.h / 5).clamp(2, self.stroke);
        self.frame(b, b.h / 2, line, ink)?;
        let inner = b.inset(line + 2);
        let filled = inner.w * percent.clamp(0, 100) / 100;
        if percent > 0 && inner.h > 0 {
            let part = inner.left(filled.max(inner.h));
            self.plate(part, inner.h / 2, ink)?;
        }
        Ok(())
    }

    /// A book's cover in `b`: the library's picture of the sample book where
    /// it has one, else a drawn one (a frame, the title, the author).
    pub fn cover(&mut self, b: B, book: &Book, ink: Color, paper: Color) -> Result<(), Error> {
        if let (true, Some(cover)) = (book.real, self.cover) {
            let at = covers::fitted((cover.width, cover.height), b.rect(), (500, 500));
            mem::push(&mut self.pictures, (cover.image, at))?;
            let around = B::new(
                i32::from(at.x) - 2,
                i32::from(at.y) - 2,
                i32::from(at.width) + 4,
                i32::from(at.height) + 4,
            );
            return self.frame(around, 0, 2, ink);
        }
        let line = (b.w / 40).clamp(2, 4);
        self.plate(b, 4, paper)?;
        self.frame(b, 4, line, ink)?;
        let band = b.inset(line).top(b.h / 9);
        self.fill(band, ink)?;
        if b.w < 110 {
            // Too small for words: two lines stand for them.
            let long = B::new(b.x + b.w / 5, b.y + b.h * 42 / 100, b.w * 3 / 5, line);
            self.fill(long, ink)?;
            let short = B::new(b.x + b.w / 3, b.y + b.h * 54 / 100, b.w / 3, line);
            return self.fill(short, ink);
        }
        let inner = b.inset_xy(b.w / 10, 0);
        let title = inner.part(18, 70);
        let max = b.h as f32 * 0.13;
        self.wrapped(title, Align::Center, (max, max * 0.4), book.title, ink)?;
        let rule = B::new(b.x + b.w * 3 / 8, b.y + b.h * 74 / 100, b.w / 4, line);
        self.fill(rule, ink)?;
        let size = (b.h as f32 * 0.075).max(8.0);
        self.fit(inner.part(78, 92), Align::Center, size, book.author, ink)
    }

    /// The band at the top of a home screen: the time and the battery.
    /// Returns what is left below it.
    pub fn status(&mut self) -> Result<B, Error> {
        let size = self.small.min(self.screen.w as f32 / 16.0);
        let h = (size * 2.2) as i32;
        let band = self.screen.top(h).inset_xy(self.pad, 0);
        self.bold(band, Align::Left, size, "12:40", self.ink)?;
        let battery = self.tr("Battery 82 %", "Akku 82 %");
        self.text(band, Align::Right, size, battery, self.ink)?;
        Ok(self.screen.below(h, 0))
    }

    /// The band at the top of another screen: its title, a note (a page
    /// count) and a thick line. Returns what is left below it.
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
        self.bold(
            band.left(band.w - note_w),
            Align::Left,
            size,
            title,
            self.ink,
        )?;
        if !note.is_empty() {
            self.text(band, Align::Right, note_size, note, self.ink)?;
        }
        let line = B::new(
            self.screen.x + self.pad,
            self.screen.y + h - self.stroke,
            self.screen.w - 2 * self.pad,
            self.stroke,
        );
        self.fill(line, self.ink)?;
        Ok(self.screen.below(h, 0))
    }

    /// A disc with an icon: filled where `on`, an outline where not.
    pub fn toggle(
        &mut self,
        centre: Point,
        radius: i32,
        icon: &str,
        on: bool,
    ) -> Result<(), Error> {
        let r = radius.max(1) as u32;
        let around = B::new(centre.x - radius, centre.y - radius, 2 * radius, 2 * radius);
        if on {
            shape::fill_circle(&mut self.ops, centre, r, self.ink)?;
            self.icon(icon, radius as f32 * 1.3, around, self.paper)
        } else {
            shape::stroke_circle(&mut self.ops, centre, r, self.stroke as u32, self.ink)?;
            self.icon(icon, radius as f32 * 1.3, around, self.ink)
        }
    }
}

/// How far a book is read, in words.
pub fn state(ui: &Ui<'_, '_>, book: &Book) -> Result<String, Error> {
    match book.percent {
        0 => paperpaw_ui_kit::text::copy(ui.tr("New", "Neu")),
        100 => paperpaw_ui_kit::text::copy(ui.tr("Finished", "Gelesen")),
        percent => paperpaw_ui_kit::text::format(format_args!("{percent} %")),
    }
}

/// The cover of the sample library's book (*Alice*), where the library has
/// one ready; none on every refusal.
pub fn sample_cover(host: &Host<'_>) -> Option<covers::Cover> {
    let library = Library::open(host).ok()?;
    let found = (|| {
        let mut input = mem::buffer(mem::SERVICE_BUFFER).ok()?;
        let mut out = mem::buffer(mem::SERVICE_BUFFER).ok()?;
        let query = Query::new();
        let page = library.query(host, &query, &mut input, &mut out).ok()?;
        let generation = page.end().generation();
        let key = page
            .rows()
            .filter_map(Result::ok)
            .find(|row| row.title().contains("Alice"))
            .map(|row| row.key())?;
        match covers::cover(host, &library, (key, generation)).ok()? {
            covers::Covered::Ready(cover) => Some(cover),
            _ => None,
        }
    })();
    let _ = library.release(host);
    found
}
