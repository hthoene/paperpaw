//! The skim screen (`reader.skim-screen`, R6b): an overlay over the lower part
//! of the page with where the book is (page label and chapter), a bar across the
//! book with its chapters as ticks and a knob, and four buttons (chapter back,
//! where I was, chapter on, page number). It fills the slot `core.skim`. The
//! model ([`crate::native_skim`]) decides what an input asks; this view reads
//! the hand-over, lays the screen out, draws it and sends the screen's own
//! commands (close, redraw). The *moves* are the reading screen's: an input
//! returns the [`Move`] and the plugin carries it out (`PORTING-7.md`).
//! Beta 3's `skim.rs` is the reference.
use crate::native_skim::{self as model, BUTTONS, Focus, Move, Skim, Step};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Handled, Point, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind},
        commands::{CommandSender, RedrawMode},
        fonts::{Font, FontRole},
        snapshot,
        ui::{HandoverId, Open, Param, Ui},
    },
};
use paperpaw_ui_kit::{
    Chips, Messages, Style,
    mem::{SERVICE_BUFFER, buffer},
    paint::{self, Paint},
    text,
};

/// The screen's name in the host (`declarations::screens`, prefixed).
pub const SCREEN_NAME: &str = "reader.skim-screen";

/// Where the screen was drawn last.
#[derive(Debug)]
struct Layout {
    region: Rect,
    bar: Rect,
    track: Rect,
    chips: Chips,
}

/// One open skim screen.
#[derive(Debug)]
pub struct SkimView {
    skim: Skim,
    layout: Option<Layout>,
}

/// What the screen's hand-over says.
#[derive(Debug, Default)]
struct Told {
    page: String,
    chapter: String,
    permille: String,
    marks: String,
}

impl SkimView {
    /// The screen the hand-over describes; one that cannot be read shows an
    /// empty bar.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn open(host: &Host<'_>, ui: &Ui, handover: Option<HandoverId>) -> Result<Self, Error> {
        let keys = snapshot::capture_snapshot(host).is_ok_and(|s| !s.has_touch());
        let mut skim = Skim::new(keys);
        if let Some(handover) = handover {
            match read(host, ui, handover) {
                Ok(told) => {
                    skim.update(&told.page, &told.chapter, &told.permille)?;
                    skim.set_marks(model::parse_marks(&told.marks)?);
                }
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => {}
            }
        }
        Ok(Self { skim, layout: None })
    }

    /// Takes over where the book is now and draws the screen again.
    ///
    /// # Errors
    /// `NoMemory` and the host's refusals.
    pub fn update(
        &mut self,
        host: &Host<'_>,
        (label, chapter, permille): (&str, &str, &str),
    ) -> Result<(), Error> {
        self.skim.update(label, chapter, permille)?;
        redraw(host)
    }

    /// What an input did. A step that closes the screen closes it here; the
    /// move it asks for (if any) is returned for the reading screen.
    ///
    /// # Errors
    /// The host's refusals.
    pub fn input(
        &mut self,
        host: &Host<'_>,
        input: &Input<'_>,
    ) -> Result<(Handled, Option<Move>), Error> {
        let step = match *input {
            Input::Action { action, .. } => self.skim.action(action),
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => self.tap(position),
            _ => Step::Ignored,
        };
        match step {
            Step::Ignored => Ok((Handled::No, None)),
            Step::Redraw => redraw(host).map(|()| (Handled::Yes, None)),
            Step::Do { go, close } => {
                if close {
                    let mut scratch = [0; 64];
                    CommandSender::new(host, &mut scratch).close_screen()?;
                } else {
                    redraw(host)?;
                }
                Ok((Handled::Yes, go))
            }
        }
    }

    fn tap(&mut self, at: Point) -> Step {
        let Some(layout) = &self.layout else {
            return Step::Ignored;
        };
        if let Some(index) = layout.chips.hit(at) {
            return self.skim.tap_button(index);
        }
        if layout.bar.contains(at) {
            let from = i32::from(layout.track.x);
            return Skim::tap_bar(at.x - from, layout.track.width);
        }
        if layout.region.contains(at) {
            // On the screen, not on a control: nothing to do.
            return Step::Redraw;
        }
        Skim::tap_outside()
    }

    /// Where the screen was drawn last (the page shows above it).
    #[must_use]
    pub fn region(&self) -> Option<Rect> {
        self.layout.as_ref().map(|l| l.region)
    }

    /// Draws the screen over the lower part of the display.
    ///
    /// # Errors
    /// Allocation, font and drawing errors.
    pub fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let drawn = self.render(host, frame, &font);
        let released = font.release();
        drawn.and(released)
    }

    fn render(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        font: &Font<'_>,
    ) -> Result<(), Error> {
        let display = snapshot::capture_snapshot(host)?;
        let style = Style::read(host, &display, font)?;
        let (width, tall) = (display.display_width(), display.display_height());
        let mut messages = Messages::new()?;
        let mut words: Vec<String> = Vec::new();
        for key in BUTTONS {
            paperpaw_ui_kit::mem::push(&mut words, messages.copy(host, key)?)?;
        }
        let labels = paperpaw_ui_kit::mem::strs(&words)?;
        let finger = Chips::finger(&display);
        let inner = width.saturating_sub(2 * style.margin);
        let chips_height = Chips::height_for(font, &style, finger, inner, &labels)?;
        let bar_height = Chips::height(&style, finger);
        let info = paperpaw_ui_kit::geometry::line_px(font, style.text)?.max(1);
        let height = (3 * style.gap)
            .saturating_add(info)
            .saturating_add(bar_height)
            .saturating_add(chips_height)
            .saturating_add(style.margin)
            .min(tall);
        let region = Rect::new(0, tall - height, width, height);
        let info_area = Rect::new(style.margin, region.y + style.gap, inner, info);
        let bar = Rect::new(
            style.margin,
            info_area.y + info + style.gap,
            inner,
            bar_height,
        );
        let strip = Rect::new(
            style.margin,
            bar.y + bar_height + style.gap,
            inner,
            chips_height,
        );
        let chips = Chips::new(font, &style, finger, strip, &labels)?;
        let track = track_of((style.line, style.gap), bar);
        let mut body: Vec<Paint> = Vec::new();
        paint::push(&mut body, Paint::Fill(region, style.paper))?;
        paint::push(
            &mut body,
            Paint::Fill(
                Rect::new(region.x, region.y, region.width, style.line.max(2)),
                style.ink,
            ),
        )?;
        self.info(&mut body, font, &style, info_area)?;
        self.bar(&mut body, &style, (bar, track))?;
        chips.prepare(&mut body, font, usize::MAX, self.skim.button())?;
        paint::replay(frame, font, body.iter())?;
        self.layout = Some(Layout {
            region,
            bar,
            track,
            chips,
        });
        Ok(())
    }

    /// Where one is: the page, then the chapter.
    fn info(
        &self,
        body: &mut Vec<Paint>,
        font: &Font<'_>,
        style: &Style,
        area: Rect,
    ) -> Result<(), Error> {
        let x = f32::from(style.margin);
        text::label(
            body,
            font,
            self.skim.label(),
            style.text,
            x,
            area,
            style.ink,
            None,
        )?;
        if self.skim.chapter().is_empty() {
            return Ok(());
        }
        let end =
            x + font.measure_text(style.text, self.skim.label())? + 2.0 * f32::from(style.gap);
        text::label(
            body,
            font,
            self.skim.chapter(),
            style.small,
            end,
            area,
            style.muted,
            None,
        )
    }

    /// The bar: read up to here, the chapters as ticks, a knob where one is, a
    /// frame round it while it has the focus.
    fn bar(
        &self,
        body: &mut Vec<Paint>,
        style: &Style,
        (bar, track): (Rect, Rect),
    ) -> Result<(), Error> {
        let (focus, keys) = self.skim.focus();
        let on_bar = keys && focus == Focus::Bar;
        if on_bar {
            let side = style.margin / 2;
            let around = Rect::new(
                bar.x.saturating_sub(side),
                bar.y,
                bar.width + 2 * side,
                bar.height,
            );
            paint::push(
                body,
                Paint::Focus(
                    around,
                    style.corner,
                    Some(u32::from(style.line.max(1))),
                    style.ink,
                ),
            )?;
        }
        paint::push(body, Paint::Fill(track, Color::gray(170)))?;
        let done = model::done_width(track.width, self.skim.permille());
        paint::push(
            body,
            Paint::Fill(Rect::new(track.x, track.y, done, track.height), style.ink),
        )?;
        let tick = track.height.saturating_mul(3);
        for &mark in self
            .skim
            .marks()
            .iter()
            .filter(|&&m| m > 0 && m < model::WHOLE)
        {
            let at = track.x.saturating_add(model::done_width(track.width, mark));
            let top = (track.y + track.height / 2).saturating_sub(tick / 2);
            paint::push(
                body,
                Paint::Fill(Rect::new(at, top, style.line.max(1), tick), style.ink),
            )?;
        }
        let knob = knob_of(bar);
        let left = track.x.saturating_add(done).saturating_sub(knob / 2);
        paint::push(
            body,
            Paint::Fill(
                Rect::new(left, bar.y + (bar.height - knob) / 2, knob, knob),
                style.ink,
            ),
        )
    }
}

/// The side of the knob that says where one is.
fn knob_of(bar: Rect) -> u16 {
    (bar.height / 2).max(8)
}

/// The bar's track, where its line is drawn: inside the bar by half a knob and a
/// little more, so that the knob stays inside it at either end.
fn track_of((line, gap): (u16, u16), bar: Rect) -> Rect {
    let h = line.saturating_mul(3).max(3).min(bar.height);
    let inset = (knob_of(bar) / 2 + gap / 2).min(bar.width / 4);
    Rect::new(
        bar.x + inset,
        bar.y + (bar.height - h) / 2,
        bar.width - 2 * inset,
        h,
    )
}

fn redraw(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Fast)
}

fn read(host: &Host<'_>, ui: &Ui, handover: HandoverId) -> Result<Told, Error> {
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let mut told = Told::default();
    for param in ui.params(host, handover, &mut input, &mut out)? {
        let param = param?;
        let value = param.value();
        match param.key() {
            model::params::PAGE => told.page = text::copy(value)?,
            model::params::CHAPTER => told.chapter = text::copy(value)?,
            model::params::PERMILLE => told.permille = text::copy(value)?,
            model::params::MARKS => told.marks = text::copy(value)?,
            _ => {}
        }
    }
    Ok(told)
}

/// Opens the skim screen over the book on show. `page` is the page's label,
/// `chapter` its chapter's title, `permille` the thousandths read and `marks`
/// the chapters' places on the bar (comma separated thousandths); empty where
/// not known.
///
/// # Errors
/// `NoMemory` and the host's refusals.
pub fn show(
    host: &Host<'_>,
    ui: &Ui,
    (page, chapter, permille, marks): (&str, &str, &str, &str),
) -> Result<(), Error> {
    let list = [
        Param::new(model::params::PAGE, page)?,
        Param::new(model::params::CHAPTER, chapter)?,
        Param::new(model::params::PERMILLE, permille)?,
        Param::new(model::params::MARKS, marks)?,
    ];
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let handover = ui.handover(host, &list, &mut input, &mut out)?;
    let mut scratch = [0; 256];
    CommandSender::new(host, &mut scratch).open(Open::screen(SCREEN_NAME)?.handover(handover))?;
    Ok(())
}

#[cfg(test)]
mod tests;
