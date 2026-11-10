//! A footnote over the page, as a native overlay screen filling `core.footnote`:
//! the note's text over the lower part of the screen with *Close* and *Go to the
//! note*. Opened when a link that looks like a note mark is followed and the
//! setting `core.footnotes` says so; the hand-over has `text`, `path` (the book)
//! and `url` (the link). The interface-6 screen (`footnote.rs`) is the reference;
//! *Go to the note* is told to the reading screen through the plugin
//! ([`Pressed::Go`]) where Beta 3 sent an event.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind},
        commands::{CommandSender, RedrawMode},
        fonts::{Font, FontRole},
        snapshot,
        ui::{HandoverId, Ui},
    },
};
use paperpaw_ui_kit::{
    Chips, Messages, Style,
    mem::{SERVICE_BUFFER, buffer},
    paint::{self, Paint},
    text,
};

/// Longest note kept, in bytes: more is never shown on a screen.
pub const MAX_TEXT: usize = 2_048;
/// The most of the display's height a note covers, in twentieths.
const SHARE: u16 = 10;

/// The keys of the buttons' words: close, then follow the link.
const BUTTONS: [&str; 2] = ["reader.footnote.close", "reader.footnote.go"];

/// What a press of the note's buttons asks of the plugin.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Pressed {
    /// Nothing for the plugin (the note closed, or nothing happened).
    None,
    /// Go to the note: the book and the link, to follow in its reading screen.
    Go {
        /// The book's path.
        path: String,
        /// The link's target.
        href: String,
    },
}

/// Where the note and its buttons were drawn last.
#[derive(Debug)]
struct Layout {
    region: Rect,
    chips: Chips,
}

/// One open note.
#[derive(Debug, Default)]
pub struct Footnote {
    text: String,
    path: String,
    href: String,
    /// The button the focus is on (none on a touch screen).
    focus: Option<usize>,
    layout: Option<Layout>,
}

impl Footnote {
    /// The note `handover` carries; a hand-over that cannot be read is a note
    /// for nothing, closed by any key.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn open(host: &Host<'_>, ui: &Ui, handover: Option<HandoverId>) -> Result<Self, Error> {
        let mut note = Self::default();
        if let Some(handover) = handover
            && let Err(Error::NoMemory) = note.read(host, ui, handover)
        {
            return Err(Error::NoMemory);
        }
        Ok(note)
    }

    fn read(&mut self, host: &Host<'_>, ui: &Ui, handover: HandoverId) -> Result<(), Error> {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        for param in ui.params(host, handover, &mut input, &mut out)? {
            let param = param?;
            let value = param.value();
            match param.key() {
                "text" => self.text = text::copy(clip(value))?,
                "path" => self.path = text::copy(value)?,
                "url" => self.href = text::copy(value)?,
                _ => {}
            }
        }
        Ok(())
    }

    /// What an input did: moves the focus, presses a button, or closes the note.
    ///
    /// # Errors
    /// The host's refusals.
    pub fn input(
        &mut self,
        host: &Host<'_>,
        input: &Input<'_>,
    ) -> Result<(Handled, Pressed), Error> {
        let Some(layout) = &self.layout else {
            return Ok((Handled::No, Pressed::None));
        };
        // Which button is pressed (`Some(None)`: the note is closed with none).
        let pressed = match *input {
            Input::Action { action, .. } => match action {
                "core.back" | "core.menu" => Some(None),
                "core.confirm" => Some(Some(self.focus.unwrap_or(0))),
                _ => {
                    let Some(to) = layout.chips.step(self.focus.unwrap_or(0), action) else {
                        return Ok((Handled::No, Pressed::None));
                    };
                    self.focus = Some(to);
                    let mut scratch = [0; 64];
                    CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Fast)?;
                    return Ok((Handled::Yes, Pressed::None));
                }
            },
            // A tap on a button presses it, anywhere else is done reading the note.
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => Some(layout.chips.hit(position)),
            _ => return Ok((Handled::No, Pressed::None)),
        };
        self.press(host, pressed.flatten())
    }

    /// Closes the note; a press of *Go to the note* (`1`) says where to.
    fn press(
        &mut self,
        host: &Host<'_>,
        button: Option<usize>,
    ) -> Result<(Handled, Pressed), Error> {
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).close_screen()?;
        let pressed = if button == Some(1) {
            Pressed::Go {
                path: core::mem::take(&mut self.path),
                href: core::mem::take(&mut self.href),
            }
        } else {
            Pressed::None
        };
        Ok((Handled::Yes, pressed))
    }

    /// Draws the note over the lower part of the screen.
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
        if self.focus.is_none() && !display.has_touch() {
            self.focus = Some(0);
        }
        let mut messages = Messages::new()?;
        let close = messages.copy(host, BUTTONS[0])?;
        let go = messages.copy(host, BUTTONS[1])?;
        let finger = Chips::finger(&display);
        let chips_height = Chips::height(&style, finger);
        let line = paperpaw_ui_kit::geometry::line_px(font, style.text)?.max(1);
        let most = tall / 20 * SHARE;
        let room = most
            .saturating_sub(chips_height)
            .saturating_sub(3 * style.gap)
            .saturating_sub(style.margin);
        let wide = f32::from(width.saturating_sub(2 * style.margin)).max(1.0);
        let lines = text::wrap_with(
            |t| font.measure_text(style.text, t),
            &self.text,
            wide,
            usize::from(room / line).max(1),
        )?;
        let rows = u16::try_from(lines.len()).unwrap_or(u16::MAX);
        let height = (3 * style.gap)
            .saturating_add(line.saturating_mul(rows))
            .saturating_add(chips_height)
            .saturating_add(style.margin)
            .min(tall);
        let region = Rect::new(0, tall - height, width, height);
        let strip = Rect::new(
            style.margin,
            region.y + height - chips_height - style.margin,
            width.saturating_sub(2 * style.margin),
            chips_height,
        );
        let chips = Chips::new(font, &style, finger, strip, &[close.as_str(), go.as_str()])?;
        let mut body: Vec<Paint> = Vec::new();
        paint::push(&mut body, Paint::Fill(region, style.paper))?;
        paint::push(
            &mut body,
            Paint::Fill(
                Rect::new(region.x, region.y, region.width, style.line.max(2)),
                style.ink,
            ),
        )?;
        let mut y = region.y.saturating_add(style.gap);
        for row in &lines {
            let area = Rect::new(region.x, y, region.width, line);
            text::label(
                &mut body,
                font,
                row,
                style.text,
                f32::from(style.margin),
                area,
                style.ink,
                None,
            )?;
            y = y.saturating_add(line);
        }
        chips.prepare(&mut body, font, usize::MAX, self.focus)?;
        paint::replay(frame, font, body.iter())?;
        self.layout = Some(Layout { region, chips });
        Ok(())
    }

    /// Where the note was drawn last.
    #[must_use]
    pub fn region(&self) -> Option<Rect> {
        self.layout.as_ref().map(|l| l.region)
    }
}

/// `text` cut at a character boundary within [`MAX_TEXT`] bytes.
fn clip(text: &str) -> &str {
    let mut end = text.len().min(MAX_TEXT);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests;
