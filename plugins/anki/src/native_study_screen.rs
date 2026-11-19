//! The study's screen: the card's front (with the back once shown), *Show
//! answer* or the four answers, and what the input does. Drawing is laid out
//! first and replayed ([`Paint`]); only the hit regions of the last frame are
//! kept.
use alloc::vec::Vec;

use paperpaw_sdk::native::callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind};
use paperpaw_sdk::native::commands::{CommandSender, RedrawMode};
use paperpaw_sdk::native::fonts::{Font, FontRole};
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::snapshot;
use paperpaw_sdk::native::ui::{ButtonHint, Notify};
use paperpaw_sdk::native::{Error, Host};
use paperpaw_sdk::{Color, Handled, Point, Rect};
use paperpaw_ui_kit::files::Volume;
use paperpaw_ui_kit::paint::push;
use paperpaw_ui_kit::{Messages, Page, Paint, Style, geometry, header, paint, text};

use crate::native_study::{ANSWER_KEYS, Study};

/// Lines of one text of the card at most.
const LINES: usize = 12;

/// One open study.
#[derive(Debug)]
pub struct StudyScreen {
    pub study: Study,
    /// Where the last frame drew its back arrow and buttons.
    header: Page,
    buttons: Vec<Rect>,
    /// What the row of button hints was told last (`hint_state`).
    hinted: Option<u8>,
}

impl StudyScreen {
    pub fn of(study: Study) -> Self {
        Self {
            study,
            header: Page::default(),
            buttons: Vec::new(),
            hinted: None,
        }
    }

    /// What the buttons do now: 0 nothing (no card), 1 *Show answer*, else
    /// 2 and the answer the focus is on.
    fn hint_state(&self) -> u8 {
        match (self.study.current.is_some(), self.study.revealed) {
            (false, _) => 0,
            (true, false) => 1,
            (true, true) => 2 + self.study.focus as u8,
        }
    }

    /// Tells the row of button hints what *Select* and the arrows do, where
    /// that changed.
    pub fn sync_hint(&mut self, host: &Host<'_>) {
        let state = self.hint_state();
        if self.hinted == Some(state) {
            return;
        }
        self.hinted = Some(state);
        let (confirm, arrows) = match state {
            0 => (None, false),
            1 => (Some("anki.show"), false),
            n => (ANSWER_KEYS.get(usize::from(n - 2)).copied(), true),
        };
        let send = |hint: Result<ButtonHint<'_>, Error>| {
            if let Ok(hint) = hint {
                match CommandSender::new(host, &mut [0; 256]).set_hint(hint) {
                    Ok(()) | Err(_) => {}
                }
            }
        };
        match confirm {
            Some(key) => {
                if let Ok(words) = Messages::new().and_then(|mut m| m.copy(host, key)) {
                    send(ButtonHint::words("core.confirm", &words));
                }
            }
            None => send(ButtonHint::hidden("core.confirm")),
        }
        for action in ["core.left", "core.right"] {
            send(if arrows {
                ButtonHint::usual(action)
            } else {
                ButtonHint::hidden(action)
            });
        }
    }

    /// What the input does: the answers by button or by tap, the focus by the
    /// arrows; *Back* is the host's.
    ///
    /// # Errors
    /// `NoMemory` and what the host says.
    pub fn input(
        &mut self,
        host: &Host<'_>,
        volume: &dyn Volume,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        let handled = match *input {
            Input::Action { action, .. } => self.action(host, volume, action),
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => self.tap(host, volume, position),
            _ => Ok(Handled::No),
        };
        self.sync_hint(host);
        handled
    }

    fn action(
        &mut self,
        host: &Host<'_>,
        volume: &dyn Volume,
        action: &str,
    ) -> Result<Handled, Error> {
        let by = match action {
            "core.confirm" => {
                if self.study.current.is_none() {
                    return Ok(Handled::No);
                }
                if self.study.revealed {
                    let ease = self.study.focus as u8 + 1;
                    return self.answer(host, volume, ease).map(|()| Handled::Yes);
                }
                self.study.reveal();
                redraw(host)?;
                return Ok(Handled::Yes);
            }
            "core.left" | "core.previous" => -1,
            "core.right" | "core.next" => 1,
            _ => return Ok(Handled::No),
        };
        if self.study.current.is_none() {
            return Ok(Handled::No);
        }
        if self.study.step(by) {
            redraw(host)?;
        }
        Ok(Handled::Yes)
    }

    fn tap(&mut self, host: &Host<'_>, volume: &dyn Volume, at: Point) -> Result<Handled, Error> {
        if let Some(action) = self.header.action(at) {
            CommandSender::new(host, &mut [0; 128]).run_action(action)?;
            return Ok(Handled::Yes);
        }
        if self.study.current.is_none() {
            return Ok(Handled::No);
        }
        if !self.study.revealed {
            self.study.reveal();
            redraw(host)?;
            return Ok(Handled::Yes);
        }
        let Some(at) = self.buttons.iter().position(|rect| rect.contains(at)) else {
            return Ok(Handled::No);
        };
        self.study.focus = at;
        self.answer(host, volume, at as u8 + 1)
            .map(|()| Handled::Yes)
    }

    fn answer(&mut self, host: &Host<'_>, volume: &dyn Volume, ease: u8) -> Result<(), Error> {
        match self.study.answer(volume, ease) {
            Ok(()) => {}
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(error) => {
                // The queue is full or the card cannot be written: the answer
                // is not kept, and the reader is told.
                let key = if error == Error::NoSpace {
                    "anki.queue-full"
                } else {
                    "anki.not-saved"
                };
                let said = Messages::new()?.copy(host, key)?;
                CommandSender::new(host, &mut [0; 256]).notify(Notify::new(&said)?)?;
            }
        }
        redraw(host)
    }

    /// Draws the card or what there is to say, and the buttons.
    ///
    /// # Errors
    /// Allocation, font, catalog and drawing errors; the hit regions stay
    /// those of the last completed frame.
    pub fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let style = Style::read(host, &display, &font)?;
        let (width, height) = (display.display_width(), display.display_height());
        let mut messages = Messages::new()?;
        let title = messages.copy(host, "anki.study")?;
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
        let study = &self.study;
        // How many cards are left, at the right of the header.
        if study.current.is_some() {
            let left = messages.format(host, "anki.left", &[Arg::Count(study.left() as i64)])?;
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
        match &study.current {
            None => {
                let message = messages.copy(host, study.message())?;
                text::message(&mut body, &font, &style, content, &message)?;
            }
            Some(card) => {
                let strip = Rect::new(0, bar_top, width, bar);
                buttons = lay_out_buttons(&style, strip, study.revealed)?;
                self.card(&mut body, &font, &style, content, &card.front, &card.back)?;
                self.buttons_paint(&mut body, &font, &style, &mut messages, host, &buttons)?;
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

    /// The front, large and centred; once shown a rule and the back.
    fn card(
        &self,
        paint: &mut Vec<Paint>,
        font: &Font<'_>,
        style: &Style,
        area: Rect,
        front: &str,
        back: &str,
    ) -> Result<(), Error> {
        let wide = f32::from(area.width.saturating_sub(style.margin.saturating_mul(2))).max(1.0);
        let large = style.text * 1.5;
        let bottom = i32::from(area.y) + i32::from(area.height);
        let mut y = i32::from(area.y) + i32::from(style.gap) * 2;
        centred(
            paint, font, style, front, large, wide, &mut y, bottom, style.ink,
        )?;
        if !self.study.revealed {
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
            paint, font, style, back, style.text, wide, &mut y, bottom, style.ink,
        )
    }

    fn buttons_paint(
        &self,
        paint: &mut Vec<Paint>,
        font: &Font<'_>,
        style: &Style,
        messages: &mut Messages,
        host: &Host<'_>,
        buttons: &[Rect],
    ) -> Result<(), Error> {
        let thickness = u32::from(style.line).max(1);
        for (i, rect) in buttons.iter().enumerate() {
            let key = if self.study.revealed {
                ANSWER_KEYS[i]
            } else {
                "anki.show"
            };
            let label = messages.copy(host, key)?;
            // The focus is the only button of *Show answer*, or the answer
            // the buttons are on.
            let focused = !self.study.revealed || i == self.study.focus;
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
    text::centred(
        paint,
        font,
        text,
        (size, ink),
        (style.margin, wide),
        (y, bottom),
        LINES,
    )
}

fn redraw(host: &Host<'_>) -> Result<(), Error> {
    CommandSender::new(host, &mut [0; 64]).queue_redraw(RedrawMode::Partial)
}
