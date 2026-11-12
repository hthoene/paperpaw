//! The review's screen on interface 7: the card (the front; with the back
//! and the example once shown), *Show answer* or the four answers each with
//! when the card would come again, and what the input does (see
//! [`native_review`](crate::native_review) for the decisions).
//!
//! Drawing is laid out first and replayed ([`Paint`]); only the hit regions
//! of the last frame are kept.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind};
use paperpaw_sdk::native::commands::{CommandSender, RedrawMode};
use paperpaw_sdk::native::fonts::{Font, FontRole};
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::snapshot;
use paperpaw_sdk::native::ui::{Instance, Notify, Ui, UiCursor};
use paperpaw_sdk::native::{Error, Host};
use paperpaw_sdk::{Color, Handled, Point, Rect, Storage};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer};
use paperpaw_ui_kit::paint::push;
use paperpaw_ui_kit::{Messages, Page, Paint, Style, geometry, header, paint, text};

use crate::cards::{self, Card};
use crate::fsrs::Grade;
use crate::native_review::{ANSWER_KEYS, Move, Review, Settings, Time};
use crate::native_words::{HostWords, Words};

/// The settings of the review, with the defaults of `declarations.rs`.
const NEW_PER_DAY: &str = "vocabulary.new-per-day";
const RETENTION: &str = "vocabulary.retention";
/// Lines of one text of the card at most.
const LINES: usize = 12;

/// The clock as the host's snapshot tells it: wall time and the offset of the
/// local time. Made again for each input (a review lasts).
pub(crate) struct HostTime<'s> {
    storage: &'s dyn Storage,
    unix: Option<i64>,
    offset: i64,
}

impl<'s> HostTime<'s> {
    /// The time now.
    ///
    /// # Errors
    /// What the snapshot says.
    pub(crate) fn read(host: &Host<'_>, storage: &'s dyn Storage) -> Result<Self, Error> {
        let snapshot = snapshot::capture_snapshot(host)?;
        Ok(Self {
            storage,
            unix: snapshot.unix_seconds(),
            offset: snapshot.utc_offset_minutes().map_or(0, i64::from) * 60,
        })
    }
}

impl Time for HostTime<'_> {
    fn now(&self) -> i64 {
        cards::now_of(self.storage, self.unix)
    }
    fn local(&self, unix: i64) -> i64 {
        unix + self.offset
    }
}

/// The review's settings, as the reader set them.
///
/// # Errors
/// What reading a setting says.
pub(crate) fn settings(host: &Host<'_>) -> Result<Settings, Error> {
    use paperpaw_sdk::native::settings::{SettingValue, read_setting};
    let mut scratch = [0u8; 64];
    let mut int = |key: &str, default: i64| match read_setting(host, key, &mut scratch)? {
        SettingValue::Int(value) => Ok::<i64, Error>(value),
        _ => Ok(default),
    };
    let new_per_day = int(NEW_PER_DAY, 20)?.clamp(0, 1000) as u32;
    let retention = int(RETENTION, 90)?.clamp(70, 99);
    Ok(Settings {
        new_per_day,
        retention,
    })
}

/// One open review.
#[derive(Debug)]
pub(crate) struct ReviewScreen {
    review: Review,
    /// Where the last frame drew its back arrow and buttons.
    header: Page,
    buttons: Vec<Rect>,
}

impl ReviewScreen {
    /// A review of `deck` (else of all), the card `first` shown first.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and what the host says.
    pub(crate) fn new(
        host: &Host<'_>,
        storage: &dyn Storage,
        deck: Option<u32>,
        first: Option<(u32, u32)>,
    ) -> Result<Self, Error> {
        let time = HostTime::read(host, storage)?;
        let mut words = HostWords::new(host)?;
        let review = Review::new(storage, &time, &mut words, settings(host)?, deck, first)?;
        Ok(Self::of(review))
    }

    pub(crate) fn of(review: Review) -> Self {
        Self {
            review,
            header: Page::default(),
            buttons: Vec::new(),
        }
    }

    /// The answers go into the decks, once, as the review ends.
    pub(crate) fn close(&self, storage: &dyn Storage) {
        self.review.close(storage);
    }

    /// The minute tick: a card answered *Again* may be due. The screen is
    /// drawn again only where it is the one on top (a redraw under another
    /// screen costs a refresh for nothing; it is drawn when it shows again).
    /// `true` where the screen changed.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and what the host says.
    pub(crate) fn tick(
        &mut self,
        host: &Host<'_>,
        storage: &dyn Storage,
        ui: &Ui,
        instance: Instance,
    ) -> Result<bool, Error> {
        let time = HostTime::read(host, storage)?;
        let mut words = HostWords::new(host)?;
        let changed = self.review.tick(storage, &time, &mut words)?;
        if changed && on_top(host, ui, instance)? {
            redraw(host)?;
        }
        Ok(changed)
    }

    /// What the input does: the answers by button or by tap, the focus by the
    /// arrows; *Back* is the host's.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and what the host says.
    pub(crate) fn input(
        &mut self,
        host: &Host<'_>,
        storage: &dyn Storage,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        match *input {
            Input::Action { action, .. } => self.action(host, storage, action),
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => self.tap(host, storage, position),
            _ => Ok(Handled::No),
        }
    }

    fn action(
        &mut self,
        host: &Host<'_>,
        storage: &dyn Storage,
        action: &str,
    ) -> Result<Handled, Error> {
        let step = match action {
            "core.confirm" => {
                if self.review.current.is_none() {
                    return Ok(Handled::No);
                }
                if self.review.revealed {
                    let grade = Grade::ALL[self.review.focus];
                    return self.answer(host, storage, grade).map(|()| Handled::Yes);
                }
                return self.reveal(host, storage).map(|()| Handled::Yes);
            }
            "core.left" | "core.previous" => -1,
            "core.right" | "core.next" => 1,
            _ => return Ok(Handled::No),
        };
        if self.review.current.is_none() {
            return Ok(Handled::No);
        }
        let time = HostTime::read(host, storage)?;
        let mut words = HostWords::new(host)?;
        if self.review.step(&time, &mut words, step)? == Move::Redraw {
            redraw(host)?;
        }
        Ok(Handled::Yes)
    }

    fn tap(&mut self, host: &Host<'_>, storage: &dyn Storage, at: Point) -> Result<Handled, Error> {
        if let Some(action) = self.header.action(at) {
            CommandSender::new(host, &mut [0; 128]).run_action(action)?;
            return Ok(Handled::Yes);
        }
        if self.review.current.is_none() {
            return Ok(Handled::No);
        }
        if !self.review.revealed {
            return self.reveal(host, storage).map(|()| Handled::Yes);
        }
        let Some(at) = self.buttons.iter().position(|rect| rect.contains(at)) else {
            return Ok(Handled::No);
        };
        self.review.focus = at;
        self.answer(host, storage, Grade::ALL[at])
            .map(|()| Handled::Yes)
    }

    fn reveal(&mut self, host: &Host<'_>, storage: &dyn Storage) -> Result<(), Error> {
        let time = HostTime::read(host, storage)?;
        let mut words = HostWords::new(host)?;
        if self.review.reveal(&time, &mut words)? == Move::Redraw {
            redraw(host)?;
        }
        Ok(())
    }

    fn answer(
        &mut self,
        host: &Host<'_>,
        storage: &dyn Storage,
        grade: Grade,
    ) -> Result<(), Error> {
        let time = HostTime::read(host, storage)?;
        let mut words = HostWords::new(host)?;
        let answered = self.review.answer(storage, &time, &mut words, grade)?;
        if !answered.kept {
            let said = words.copy("vocabulary.save-failed")?;
            CommandSender::new(host, &mut [0; 256]).notify(Notify::new(&said)?)?;
        }
        redraw(host)
    }

    /// Draws the card or what there is to say, and the buttons.
    ///
    /// # Errors
    /// Allocation, font, catalog and drawing errors; the hit regions stay
    /// those of the last completed frame.
    pub(crate) fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let style = Style::read(host, &display, &font)?;
        let (width, height) = (display.display_width(), display.display_height());
        let mut messages = Messages::new()?;
        let title = messages.copy(host, "vocabulary.review.title")?;
        let mut header = Page::default();
        header::prepare(
            &mut header,
            host,
            &font,
            &style,
            &mut messages,
            &title,
            width,
            display.has_touch(),
            0,
            1,
        )?;
        let top = style.header.saturating_add(style.gap);
        let review = &self.review;
        // How many cards are left, at the right of the header.
        if review.current.is_some() {
            let left = messages.format(
                host,
                "vocabulary.review.left",
                &[Arg::Count(review.left() as i64)],
            )?;
            let band = Rect::new(0, 0, width, style.header);
            let wide = font.measure_text(style.small, &left)?;
            let x = f32::from(width) - f32::from(style.margin) - wide;
            let ink = if style.dark { style.paper } else { style.note };
            text::label(
                &mut header.paint,
                &font,
                &left,
                style.small,
                x,
                band,
                ink,
                None,
            )?;
        }
        let bar = style.row.saturating_add(style.gap.saturating_mul(2));
        let bar_top = height.saturating_sub(bar);
        let content = Rect::new(0, top, width, bar_top.saturating_sub(top));
        let mut body = Vec::new();
        push(
            &mut body,
            Paint::Fill(
                Rect::new(0, top, width, height.saturating_sub(top)),
                style.paper,
            ),
        )?;
        let mut buttons = Vec::new();
        match &review.current {
            None => text::message(&mut body, &font, &style, content, &review.message)?,
            Some((_, card)) => {
                let strip = Rect::new(0, bar_top, width, bar);
                buttons = lay_out_buttons(&style, strip, review.revealed)?;
                self.card(&mut body, &font, &style, content, card)?;
                let waits = if review.revealed {
                    &review.waits[..]
                } else {
                    &[]
                };
                self.buttons_paint(
                    &mut body,
                    &font,
                    &style,
                    &mut messages,
                    host,
                    &buttons,
                    waits,
                )?;
            }
        }
        paint::replay(frame, &font, header.paint.iter().chain(body.iter()))?;
        font.release()?;
        // Only hit geometry survives the draw callback.
        header.paint = Vec::new();
        self.header = header;
        self.buttons = buttons;
        Ok(())
    }

    /// The front, large and centred; once shown a rule, the back, and the
    /// example small and faint.
    fn card(
        &self,
        paint: &mut Vec<Paint>,
        font: &Font<'_>,
        style: &Style,
        area: Rect,
        card: &Card,
    ) -> Result<(), Error> {
        let wide = f32::from(area.width.saturating_sub(style.margin.saturating_mul(2))).max(1.0);
        let large = style.text * 1.5;
        let bottom = i32::from(area.y) + i32::from(area.height);
        // Where the waits above the buttons are.
        let bottom = if self.review.revealed {
            let line = i32::from(geometry::line_px(font, style.small)?);
            bottom - line - i32::from(style.gap)
        } else {
            bottom
        };
        let mut y = i32::from(area.y) + i32::from(style.gap) * 2;
        centred(
            paint,
            font,
            style,
            &card.front,
            large,
            wide,
            &mut y,
            bottom,
            style.ink,
        )?;
        if !self.review.revealed {
            return Ok(());
        }
        y += i32::from(style.gap);
        let rule = Rect::new(
            style.margin.saturating_mul(2),
            geometry::clamp_u16(i64::from(y)),
            area.width.saturating_sub(style.margin.saturating_mul(4)),
            1,
        );
        push(paint, Paint::Fill(rule, style.note))?;
        y += i32::from(style.gap) * 2;
        centred(
            paint, font, style, &card.back, style.text, wide, &mut y, bottom, style.ink,
        )?;
        if !card.example.is_empty() {
            y += i32::from(style.gap);
            centred(
                paint,
                font,
                style,
                &card.example,
                style.small,
                wide,
                &mut y,
                bottom,
                style.note,
            )?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments, reason = "one bar: geometry, texts, state")]
    fn buttons_paint(
        &self,
        paint: &mut Vec<Paint>,
        font: &Font<'_>,
        style: &Style,
        messages: &mut Messages,
        host: &Host<'_>,
        buttons: &[Rect],
        waits: &[String],
    ) -> Result<(), Error> {
        let thickness = u32::from(style.line).max(1);
        let small = geometry::line_px(font, style.small)?;
        for (i, rect) in buttons.iter().enumerate() {
            let key = if waits.is_empty() {
                "vocabulary.review.show"
            } else {
                ANSWER_KEYS[i]
            };
            let label = messages.copy(host, key)?;
            // The focus is the only button of *Show answer*, or the answer
            // the buttons are on.
            let focused = waits.is_empty() || i == self.review.focus;
            let (fill, ink) = if focused {
                (Some(style.ink), style.paper)
            } else {
                (None, style.ink)
            };
            match fill {
                Some(color) => push(paint, Paint::Focus(*rect, style.corner, None, color))?,
                None => push(
                    paint,
                    Paint::Focus(*rect, style.corner, Some(thickness), ink),
                )?,
            }
            let room = f32::from(rect.width.saturating_sub(style.margin)).max(1.0);
            let fitted = text::fit(font, style.text, &label, room)?;
            let wide = font.measure_text(style.text, &fitted)?;
            let x = f32::from(rect.x) + (f32::from(rect.width) - wide).max(0.0) / 2.0;
            text::label(paint, font, &fitted, style.text, x, *rect, ink, Some(*rect))?;
            if let Some(wait) = waits.get(i) {
                // When it would come again, above its button.
                let fitted = text::fit(font, style.small, wait, f32::from(rect.width))?;
                let wide = font.measure_text(style.small, &fitted)?;
                let x = f32::from(rect.x) + (f32::from(rect.width) - wide).max(0.0) / 2.0;
                let above = Rect::new(
                    rect.x,
                    rect.y.saturating_sub(small.saturating_add(style.gap)),
                    rect.width,
                    small,
                );
                text::label(
                    paint,
                    font,
                    &fitted,
                    style.small,
                    x,
                    above,
                    style.note,
                    None,
                )?;
            }
        }
        Ok(())
    }
}

/// The buttons of the bar: one before the back is shown, four after.
fn lay_out_buttons(style: &Style, strip: Rect, revealed: bool) -> Result<Vec<Rect>, Error> {
    let n: u16 = if revealed { 4 } else { 1 };
    let (margin, gap) = (style.margin, style.gap);
    let width = strip.width.saturating_sub(2 * margin + gap * (n - 1)) / n;
    let mut buttons = Vec::new();
    buttons
        .try_reserve_exact(usize::from(n))
        .map_err(|_| Error::NoMemory)?;
    for i in 0..n {
        buttons.push(Rect::new(
            margin + i * (width + gap),
            strip.y.saturating_add(gap),
            width,
            strip.height.saturating_sub(2 * gap),
        ));
    }
    Ok(buttons)
}

/// `text` wrapped and centred in lines from `*y` on, until `bottom`.
#[allow(
    clippy::too_many_arguments,
    reason = "one text: where, how large, how wide"
)]
fn centred(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &Style,
    text: &str,
    size: f32,
    wide: f32,
    y: &mut i32,
    bottom: i32,
    ink: Color,
) -> Result<(), Error> {
    let lines = text::wrap_with(|line| font.measure_text(size, line), text, wide, LINES)?;
    let high = geometry::line_px(font, size)?;
    for line in &lines {
        if *y + i32::from(high) > bottom {
            break;
        }
        let measured = font.measure_text(size, line)?;
        let x = f32::from(style.margin) + (wide - measured).max(0.0) / 2.0;
        let row = Rect::new(
            style.margin,
            geometry::clamp_u16(i64::from(*y)),
            geometry::clamp_u16(wide as i64),
            high,
        );
        text::label(paint, font, line, size, x, row, ink, None)?;
        *y += i32::from(high);
    }
    Ok(())
}

fn redraw(host: &Host<'_>) -> Result<(), Error> {
    CommandSender::new(host, &mut [0; 64]).queue_redraw(RedrawMode::Partial)
}

/// The bytes of a stack request (a cursor and no more).
const STACK_REQUEST: usize = 256;

/// Whether `instance` is the screen on top of the stack (the stack is read
/// top first; a screen the host leaves out as guarded is not ours).
fn on_top(host: &Host<'_>, ui: &Ui, instance: Instance) -> Result<bool, Error> {
    // The request is a cursor and no more; the reply is a page of rows.
    let (mut input, mut out) = (buffer(STACK_REQUEST)?, buffer(SERVICE_BUFFER)?);
    let page = ui.stack(host, UiCursor::start(), &mut input, &mut out)?;
    match page.into_iter().next() {
        Some(row) => Ok(row?.instance() == instance),
        None => Ok(false),
    }
}
