//! A whole-screen page for the sleep screens that plugins offer (calendar,
//! tasks, statistics, the reading goal, notes, news, smart home): paper, a
//! header with a thick rule, rows of text, thin rules and a footer saying how
//! old the data is. The design is laid out for a screen 480 pixels wide and
//! scaled by the width; ink and paper are the interface's, so only the two
//! need to be told apart (one bit is enough).
//!
//! Everything is queued as [`Paint`] and replayed in the draw callback
//! (`paint::replay`); nothing is fetched while it is drawn.

use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Point, Rect,
    native::{
        Error, Host,
        callbacks::DrawFrame,
        fonts::{Font, FontRole, TextOrigin},
        formatting::Arg,
        snapshot,
    },
};

use crate::messages::Messages;
use crate::paint::{self, Paint};
use crate::style::Style;
use crate::widget::Align;

/// The width the design is made for; sizes are given for it.
const DESIGN_WIDTH: u32 = 480;
/// The margin on both sides, in design pixels.
const MARGIN: u32 = 35;
/// Where the footer's rule is above the bottom edge, and its text.
const FOOT_RULE: u32 = 52;
const FOOT_TEXT: u32 = 42;

/// How a run of text is drawn.
#[derive(Debug, Clone, Copy)]
pub struct Face {
    /// Size in design pixels.
    pub size: u16,
    pub bold: bool,
    /// Secondary text (the style's note colour) instead of ink.
    pub quiet: bool,
}

impl Face {
    /// Regular ink text.
    #[must_use]
    pub const fn new(size: u16) -> Self {
        Self {
            size,
            bold: false,
            quiet: false,
        }
    }

    /// The same, bold.
    #[must_use]
    pub const fn bold(size: u16) -> Self {
        Self {
            size,
            bold: true,
            quiet: false,
        }
    }

    /// The same, in the secondary colour.
    #[must_use]
    pub const fn quiet(size: u16) -> Self {
        Self {
            size,
            bold: false,
            quiet: true,
        }
    }
}

/// The interface's font and look, what a page is made with.
#[derive(Debug)]
pub struct Look<'h> {
    pub font: Font<'h>,
    pub style: Style,
}

impl<'h> Look<'h> {
    /// The interface's font and the style of this display.
    ///
    /// # Errors
    /// The host's refusals and [`Error::NoMemory`].
    pub fn open(host: &'h Host<'h>) -> Result<Self, Error> {
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let style = Style::read(host, &display, &font)?;
        Ok(Self { font, style })
    }

    /// A page the size of `frame`, paper laid.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn page(&self, frame: &DrawFrame<'_>) -> Result<Page<'_, 'h>, Error> {
        Page::new(&self.font, &self.style, frame)
    }
}

/// The page being laid out.
#[derive(Debug)]
pub struct Page<'f, 'h> {
    font: &'f Font<'h>,
    pub out: Vec<Paint>,
    pub area: Rect,
    pub ink: Color,
    pub paper: Color,
    pub note: Color,
}

impl<'f, 'h> Page<'f, 'h> {
    /// A page the size of the frame, paper laid.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn new(font: &'f Font<'h>, style: &Style, frame: &DrawFrame<'_>) -> Result<Self, Error> {
        let clamp = crate::geometry::clamp_u16;
        let area = Rect::new(
            clamp(i64::from(frame.rect.x)),
            clamp(i64::from(frame.rect.y)),
            clamp(i64::from(frame.rect.width)),
            clamp(i64::from(frame.rect.height)),
        );
        let mut page = Self {
            font,
            out: Vec::new(),
            area,
            ink: style.ink,
            paper: style.paper,
            note: style.note,
        };
        paint::push(&mut page.out, Paint::Fill(area, style.paper))?;
        Ok(page)
    }

    /// `design` pixels of the 480-wide design, in this screen's pixels.
    #[must_use]
    pub fn u(&self, design: u32) -> u16 {
        let wide = u32::from(self.area.width).max(1);
        crate::geometry::clamp_u16(i64::from(design.saturating_mul(wide) / DESIGN_WIDTH))
    }

    /// The left edge of the content, from the screen's.
    #[must_use]
    pub fn left(&self) -> u16 {
        self.area.x.saturating_add(self.u(MARGIN))
    }

    /// The width of the content.
    #[must_use]
    pub fn width(&self) -> u16 {
        self.area
            .width
            .saturating_sub(self.u(MARGIN).saturating_mul(2))
    }

    /// A rectangle `y` design pixels from the top, `h` high, `x` from the
    /// content's left edge, `w` wide (`0`: to its right edge).
    #[must_use]
    pub fn rect(&self, x: u32, y: u32, w: u32, h: u32) -> Rect {
        let left = self.left().saturating_add(self.u(x));
        let width = if w == 0 {
            self.width().saturating_sub(self.u(x))
        } else {
            self.u(w)
        };
        Rect::new(
            left,
            self.area.y.saturating_add(self.u(y)),
            width,
            self.u(h),
        )
    }

    /// How tall `size` design pixels of text are.
    ///
    /// # Errors
    /// Font refusals.
    pub fn line(&self, size: u16) -> Result<u16, Error> {
        crate::geometry::line_px(self.font, f32::from(self.u(u32::from(size))))
    }

    /// How wide `text` is in `face`.
    ///
    /// # Errors
    /// Font refusals.
    pub fn measure(&self, face: Face, text: &str) -> Result<f32, Error> {
        let size = f32::from(self.u(u32::from(face.size)));
        let width = self.font.measure_text(size, text)?;
        Ok(if face.bold {
            width + crate::paint::bold_extra(size) as f32
        } else {
            width
        })
    }

    /// `text` in `row`, placed by `align`, shortened with an ellipsis where it
    /// is wider, vertically centred by its line.
    ///
    /// # Errors
    /// Font refusals and [`Error::NoMemory`].
    pub fn put(&mut self, row: Rect, face: Face, align: Align, text: &str) -> Result<(), Error> {
        self.put_in(row, face, align, text, None)
    }

    /// As [`Page::put`], in `color` (a paper-coloured text on an ink bar).
    ///
    /// # Errors
    /// Font refusals and [`Error::NoMemory`].
    pub fn put_in(
        &mut self,
        row: Rect,
        face: Face,
        align: Align,
        text: &str,
        color: Option<Color>,
    ) -> Result<(), Error> {
        if text.is_empty() {
            return Ok(());
        }
        let size = f32::from(self.u(u32::from(face.size)));
        let shown = crate::text::fit(self.font, size, text, f32::from(row.width))?;
        let width = self.font.measure_text(size, &shown)?;
        let x = align.x(row, width);
        let (ascent, descent, _) = crate::geometry::line_metrics(self.font, size)?;
        let baseline = i32::from(row.y)
            .saturating_add(((f32::from(row.height) + ascent - descent) / 2.0) as i32);
        let color = color.unwrap_or(if face.quiet { self.note } else { self.ink });
        let origin = TextOrigin::new(x, baseline);
        let op = if face.bold {
            Paint::Bold {
                text: shown,
                size,
                origin,
                color,
            }
        } else {
            Paint::Text {
                text: shown,
                size,
                origin,
                color,
                clip: None,
            }
        };
        paint::push(&mut self.out, op)
    }

    /// `text` wrapped to `area`'s width and at most `lines` lines (cut with an
    /// ellipsis past them), from the top of `area`; returns the height it
    /// took.
    ///
    /// # Errors
    /// Font refusals and [`Error::NoMemory`].
    pub fn wrap(
        &mut self,
        area: Rect,
        face: Face,
        align: Align,
        (text, lines): (&str, u16),
    ) -> Result<u16, Error> {
        let size = f32::from(self.u(u32::from(face.size)));
        let room = crate::geometry::line_px(self.font, size)?.saturating_mul(lines.max(1));
        let wrapped =
            crate::widget::text::Wrapped::new(self.font, text, size, (area.width, room), false)?;
        let color = if face.quiet { self.note } else { self.ink };
        let height = wrapped.height(self.font)?;
        let area = Rect::new(area.x, area.y, area.width, height);
        wrapped.draw(&mut self.out, self.font, area, align, color)
    }

    /// A rectangle of ink.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn fill(&mut self, rect: Rect) -> Result<(), Error> {
        paint::push(&mut self.out, Paint::Fill(rect, self.ink))
    }

    /// A rectangle of paper (to hollow out an ink one).
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn clear(&mut self, rect: Rect) -> Result<(), Error> {
        paint::push(&mut self.out, Paint::Fill(rect, self.paper))
    }

    /// A rule across the content, `y` design pixels from the top, `thick`
    /// design pixels thick (at least one pixel).
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn rule(&mut self, y: u32, thick: u32) -> Result<(), Error> {
        let mut rect = self.rect(0, y, 0, thick);
        rect.height = rect.height.max(1);
        self.fill(rect)
    }

    /// An outline of `rect`, `thick` pixels thick, corners rounded by `radius`.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn frame(&mut self, rect: Rect, radius: u32, thick: u32) -> Result<(), Error> {
        paint::push(
            &mut self.out,
            Paint::Focus(rect, radius, Some(thick.max(1)), self.ink),
        )
    }

    /// A straight line, `thick` pixels thick.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn stroke(&mut self, from: Point, to: Point, thick: u32) -> Result<(), Error> {
        paint::push(
            &mut self.out,
            Paint::Line {
                from,
                to,
                width: thick.max(1),
                color: self.ink,
            },
        )
    }

    /// The header: `title` large and bold on the left, `side` smaller on the
    /// right, a thick rule under both.
    ///
    /// # Errors
    /// Font refusals and [`Error::NoMemory`].
    pub fn header(&mut self, title: &str, side: &str) -> Result<(), Error> {
        let side_w = if side.is_empty() {
            0
        } else {
            self.measure(Face::quiet(20), side)? as u16 + self.u(16)
        };
        let mut row = self.rect(0, 30, 0, 52);
        row.width = row.width.saturating_sub(side_w);
        self.put(row, Face::bold(34), Align::Left, title)?;
        let side_row = self.rect(0, 30, 0, 52);
        self.put(side_row, Face::quiet(20), Align::Right, side)?;
        self.rule(92, 3)
    }

    /// The footer: a thin rule above the bottom edge and `text` under it, in
    /// the secondary colour (what it is from and how old it is).
    ///
    /// # Errors
    /// Font refusals and [`Error::NoMemory`].
    pub fn footer(&mut self, text: &str) -> Result<(), Error> {
        let top = u32::from(self.area.height) * DESIGN_WIDTH / u32::from(self.area.width).max(1);
        let rule = top.saturating_sub(FOOT_RULE);
        self.rule(rule, 1)?;
        let row = self.rect(0, top.saturating_sub(FOOT_TEXT), 0, 28);
        self.put(row, Face::quiet(16), Align::Left, text)
    }

    /// Queues `ops` too.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn push(&mut self, op: Paint) -> Result<(), Error> {
        paint::push(&mut self.out, op)
    }

    /// Draws the page into `frame`.
    ///
    /// # Errors
    /// Canvas, font and drawing refusals.
    pub fn show(self, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        paint::replay(frame, self.font, self.out.iter())
    }
}

/// Days, times and catalog texts for a page: the words a sleep screen needs,
/// from the catalog (`core.weekday.*`, `core.month.*`, `core.date`) and the
/// reader's clock settings.
#[derive(Debug)]
pub struct Words<'h> {
    host: &'h Host<'h>,
    say: Messages,
    twelve: bool,
}

impl<'h> Words<'h> {
    /// The words of this host.
    ///
    /// # Errors
    /// The host's refusal.
    pub fn new(host: &'h Host<'h>) -> Result<Self, Error> {
        let twelve = crate::widget::settings::text_of(host, "core.clock-format", |value| {
            value == paperpaw_sdk::time::CLOCK_12H
        })?;
        Ok(Self {
            host,
            say: Messages::new()?,
            twelve,
        })
    }

    /// The catalog's text for `key`.
    ///
    /// # Errors
    /// Host refusals and [`Error::NoMemory`].
    pub fn copy(&mut self, key: &str) -> Result<String, Error> {
        self.say.copy(self.host, key)
    }

    /// The catalog's text for `key` with `args` put in.
    ///
    /// # Errors
    /// Host refusals and [`Error::NoMemory`].
    pub fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error> {
        self.say.format(self.host, key, args)
    }

    /// `value` in the reader's digits.
    ///
    /// # Errors
    /// Host refusals and [`Error::NoMemory`].
    pub fn number(&mut self, value: u32) -> Result<String, Error> {
        let mut digits = [0u8; 20];
        let text = crate::text::decimal(value as usize, &mut digits);
        self.say.digits(self.host, text)
    }

    /// The device's time now: the day (days since 1970) and the second of the
    /// day on the reader's clock; `None` where the device does not know it.
    ///
    /// # Errors
    /// The host's refusal.
    pub fn now(&self) -> Result<Option<(i32, u32)>, Error> {
        let Some(unix) = snapshot::capture_snapshot(self.host)?.unix_seconds() else {
            return Ok(None);
        };
        self.local(unix).map(Some)
    }

    /// A Unix time as a local day and second of the day.
    ///
    /// # Errors
    /// The host's refusal.
    pub fn local(&self, unix: i64) -> Result<(i32, u32), Error> {
        let zone = crate::widget::cx::zone_offset(self.host, unix)?;
        let local = unix.saturating_add(i64::from(zone));
        Ok((
            i32::try_from(local.div_euclid(86_400)).unwrap_or(0),
            u32::try_from(local.rem_euclid(86_400)).unwrap_or(0),
        ))
    }

    fn named(&mut self, prefix: &str, n: u8) -> Result<String, Error> {
        let mut digits = [0u8; 20];
        let key =
            crate::text::concat(&[prefix, crate::text::decimal(usize::from(n), &mut digits)])?;
        self.say.copy(self.host, &key)
    }

    /// The name of a weekday (0 is Sunday), long or short.
    ///
    /// # Errors
    /// Host refusals and [`Error::NoMemory`].
    pub fn weekday(&mut self, day: i32, short: bool) -> Result<String, Error> {
        let n = paperpaw_sdk::time::weekday_of(i64::from(day));
        self.named(
            if short {
                "core.weekday.short."
            } else {
                "core.weekday."
            },
            n % 7,
        )
    }

    /// The name of a month (1 to 12), long or short.
    ///
    /// # Errors
    /// Host refusals and [`Error::NoMemory`].
    pub fn month(&mut self, month: u8, short: bool) -> Result<String, Error> {
        self.named(
            if short {
                "core.month.short."
            } else {
                "core.month."
            },
            month.clamp(1, 12),
        )
    }

    /// A day as the reader writes dates: "Friday, 20 November" (`long`), or
    /// "Fri 20 Nov".
    ///
    /// # Errors
    /// Host refusals and [`Error::NoMemory`].
    pub fn date(&mut self, day: i32, long: bool) -> Result<String, Error> {
        let (_, month, date) = paperpaw_sdk::time::civil_from_days(i64::from(day));
        let weekday = self.weekday(day, !long)?;
        let month = self.month(month, !long)?;
        self.say.format(
            self.host,
            if long { "core.date" } else { "core.date.short" },
            &[
                Arg::Text {
                    name: "weekday",
                    value: &weekday,
                },
                Arg::Int {
                    name: "day",
                    value: i64::from(date),
                },
                Arg::Text {
                    name: "month",
                    value: &month,
                },
            ],
        )
    }

    /// A time of day (seconds into it) on the reader's clock.
    ///
    /// # Errors
    /// Host refusals and [`Error::NoMemory`].
    pub fn clock(&mut self, secs: u32) -> Result<String, Error> {
        // Both below 60: no cut.
        let (hour, minute) = (((secs / 3600) % 24) as u8, ((secs / 60) % 60) as u8);
        crate::widget::cx::clock_text(&mut self.say, self.host, self.twelve, (hour, minute))
    }

    /// The footer's text: `key` (a message with `{when}` and what `args`
    /// name) with the date and time of `unix`, or, where it is 0 (never),
    /// `never` (a message with `args` only).
    ///
    /// # Errors
    /// Host refusals and [`Error::NoMemory`].
    pub fn stand(
        &mut self,
        (key, never): (&str, &str),
        unix: i64,
        args: &[Arg<'_>],
    ) -> Result<String, Error> {
        if unix <= 0 {
            return self.say.format(self.host, never, args);
        }
        let (day, secs) = self.local(unix)?;
        let date = self.date(day, false)?;
        let time = self.clock(secs)?;
        let when = crate::text::concat(&[&date, ", ", &time])?;
        let mut all: Vec<Arg<'_>> = Vec::new();
        all.try_reserve(args.len() + 1)
            .map_err(|_| Error::NoMemory)?;
        all.extend(args.iter().copied());
        all.push(Arg::Text {
            name: "when",
            value: &when,
        });
        self.say.format(self.host, key, &all)
    }
}
