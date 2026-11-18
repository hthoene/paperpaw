//! The offer to go online (offline power mode): a dialog of two buttons, *Go
//! online* first, drawn under the screen's header. Its focus and the buttons'
//! places are kept here; the texts are the shared `core.offline.*`.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{
    Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, Input, InputGestureKind},
        fonts::{Font, FontRole},
        snapshot,
    },
};
use paperpaw_ui_kit::{Dialog, Messages, Page, Paint, Style, dialog::Response, header, paint};

use super::Act;

/// The two buttons of the offer, *Go online* first.
pub(super) const BUTTONS: [&str; 2] = ["core.offline.go-online", "core.offline.not-now"];

/// The offer, and the name the search waits for.
#[derive(Debug)]
pub(super) struct Offer {
    name: String,
    focus: usize,
    /// As the last drawing laid it out; none before the first.
    dialog: Option<Dialog>,
    header: Page,
}

impl Offer {
    pub(super) fn new(name: String) -> Self {
        Self {
            name,
            focus: 0,
            dialog: None,
            header: Page::default(),
        }
    }

    #[cfg(test)]
    pub(super) fn name(&self) -> &str {
        &self.name
    }

    #[cfg(test)]
    pub(super) const fn focus(&self) -> usize {
        self.focus
    }

    /// The button chosen: its act, with the name for *Go online*.
    fn choose(&mut self, button: usize) -> Act {
        if button == 0 {
            Act::Online(core::mem::take(&mut self.name))
        } else {
            Act::NotNow
        }
    }

    fn step(&mut self, action: &str) -> Act {
        self.focus = match action {
            "core.up" | "core.previous" | "core.left" | "core.prev-page" => 0,
            "core.down" | "core.next" | "core.right" | "core.next-page" => 1,
            _ => return Act::Ignored,
        };
        Act::Taken
    }

    /// What a key or a tap means to the offer. A key of its own (back, menu)
    /// is the host's; a tap elsewhere is taken and does nothing.
    pub(super) fn decide(&mut self, input: &Input<'_>) -> Act {
        match *input {
            Input::Action { action, .. } => {
                if action == "core.confirm" {
                    return self.choose(self.focus);
                }
                if let Some(dialog) = self.dialog.as_mut()
                    && dialog.on_action(action) != Response::Ignored
                {
                    return Act::Taken;
                }
                self.step(action)
            }
            Input::Gesture { position, kind, .. } => {
                if self.header.action(position) == Some("core.back") {
                    return Act::Back;
                }
                let Some(dialog) = self.dialog.as_mut() else {
                    return Act::Taken;
                };
                match kind {
                    InputGestureKind::Tap => match dialog.hit(position) {
                        Some(button) => self.choose(button),
                        None => Act::Taken,
                    },
                    InputGestureKind::SwipeUp | InputGestureKind::SwipeDown => {
                        dialog.on_swipe(position, kind == InputGestureKind::SwipeUp);
                        Act::Taken
                    }
                    _ => Act::Taken,
                }
            }
            _ => Act::Ignored,
        }
    }

    /// Draws the header and the dialog in the middle of the screen.
    ///
    /// # Errors
    /// `NoMemory` or what the host says; what was laid out stays.
    pub(super) fn draw(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        title: &str,
    ) -> Result<(), Error> {
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let result = self.paint(host, frame, &display, &font, title);
        font.release()?;
        result
    }

    fn paint(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        display: &snapshot::Snapshot,
        font: &Font<'_>,
        title: &str,
    ) -> Result<(), Error> {
        let style = Style::read(host, display, font)?;
        let (width, height) = (display.display_width(), display.display_height());
        let mut messages = Messages::new()?;
        let mut page = Page::default();
        let heading = messages.copy(host, title)?;
        header::prepare(
            &mut page,
            host,
            font,
            &style,
            &mut messages,
            &heading,
            width,
            display.has_touch(),
            0,
            1,
        )?;
        let top = style.header.saturating_add(style.gap);
        let mut ops: Vec<Paint> = Vec::new();
        paint::push(
            &mut ops,
            Paint::Fill(
                Rect::new(0, top, width, height.saturating_sub(top)),
                style.paper,
            ),
        )?;
        let said = messages.copy(host, "core.offline.text")?;
        let name = messages.copy(host, "core.offline.title")?;
        let mut labels: Vec<String> = Vec::new();
        labels
            .try_reserve_exact(BUTTONS.len())
            .map_err(|_| Error::NoMemory)?;
        for key in BUTTONS {
            labels.push(messages.copy(host, key)?);
        }
        let mut buttons: Vec<&str> = Vec::new();
        buttons
            .try_reserve_exact(labels.len())
            .map_err(|_| Error::NoMemory)?;
        buttons.extend(labels.iter().map(String::as_str));
        let mut dialog = Dialog::new(font, &style, (width, height), &name, &[&said], &buttons)?;
        let at = height.saturating_sub(dialog.height()) / 2;
        dialog.place(at.max(top))?;
        dialog.prepare(&mut ops, font, Some(self.focus))?;
        paint::replay(frame, font, page.paint.iter().chain(ops.iter()))?;
        page.paint = Vec::new();
        self.header = page;
        self.dialog = Some(dialog);
        Ok(())
    }
}
