//! The firmware confirmation's screen (`docs/abi/device.md` 5.0, 5.3; the
//! words and the kernel's side in `native_host::firmware_confirm`): a box in
//! the middle of the screen with the title, the text and the buttons yes,
//! [yes with data,] no. It guards the device, starts with the focus on *No*,
//! drops what arrives within [`GUARD_MS`] of being shown, ends as
//! *dismissed* on Back or after [`TIMEOUT_MS`], and tells the kernel its
//! answer with an event that carries the nonce only the host and this screen
//! know. A text taller than the screen scrolls above the buttons.
use alloc::string::String;
use alloc::vec::Vec;

use super::paint::{self, Look};
use crate::native_host::firmware_confirm::{self as words, ANSWERED};
use crate::native_host::firmware_install::Answer;
use paperpaw_platform::Instant;
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Hint, Id, Params, Point, Rect,
    RefreshMode,
};

/// Input within this long of being shown is dropped (`device.md` 5.0).
const GUARD_MS: u64 = 500;
/// Ten minutes without an answer end it as dismissed (decision 132).
const TIMEOUT_MS: u64 = 600_000;

/// The confirmation; see the [module documentation](self).
pub(crate) struct Confirm {
    look: Look,
    nonce: String,
    title: Vec<String>,
    /// The text's lines, wrapped.
    lines: Vec<String>,
    /// The buttons in their order: yes, [yes with data,] no.
    labels: Vec<String>,
    buttons: Vec<Rect>,
    /// The box on the screen, and where title and text show in it.
    panel: Rect,
    area: Rect,
    title_line: u16,
    text_line: u16,
    /// The first of the title's and the text's lines shown.
    top: usize,
    focus: usize,
    /// When it was shown: the input guard and the timeout count from here.
    shown: Option<Instant>,
    /// It has answered (or ended as dismissed): nothing is told twice.
    done: bool,
}

impl Confirm {
    /// The screen the kernel opened with `params` (`firmware_confirm::open_params`).
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let get = |key| params.get(key).unwrap_or("");
        let mut labels = Vec::from([String::from(get(words::YES_WORD))]);
        if let Some(data) = params.get(words::DATA_WORD) {
            labels.push(String::from(data));
        }
        labels.push(String::from(get(words::NO_WORD)));
        let look = Look::of(env.device(), env.settings(), env.fonts());
        let display = env.device().display;
        let margin = look.margin;
        let width = display.width.saturating_sub(2 * margin);
        let inner = width.saturating_sub(2 * margin);
        let (title, lines, title_line, text_line) = match paint::font(env.fonts()) {
            Some(font) => {
                let wrap = |size, text: &str| paint::wrapped(&font, size, text, f32::from(inner));
                let lines = get(words::TEXT)
                    .split('\n')
                    .filter(|_| !get(words::TEXT).is_empty())
                    .flat_map(|paragraph| wrap(look.text, paragraph))
                    .collect();
                (
                    wrap(look.title, get(words::TITLE)),
                    lines,
                    font.line_metrics(look.title).line_height as u16,
                    font.line_metrics(look.text).line_height as u16,
                )
            }
            None => (Vec::new(), Vec::new(), 0, 0),
        };
        // The buttons side by side, one below the other where a word does
        // not fit its share.
        let count = u16::try_from(labels.len()).unwrap_or(u16::MAX);
        let share = inner.saturating_sub(margin * (count - 1)) / count;
        let stacked = paint::font(env.fonts()).is_some_and(|font| {
            labels.iter().any(|label| {
                font.measure(look.small, label) + 2.0 * f32::from(look.gap) > f32::from(share)
            })
        });
        let buttons_h = if stacked {
            look.row * count + margin * (count - 1)
        } else {
            look.row
        };
        let content = title_line * title.len() as u16 + margin + text_line * lines.len() as u16;
        let height =
            (content + buttons_h + 3 * margin).min(display.height.saturating_sub(2 * margin));
        let y = (display.height - height) / 2;
        let panel = Rect::new(margin, y, width, height);
        let row_y = y + height - margin - buttons_h;
        let buttons = (0..count)
            .map(|at| match stacked {
                true => Rect::new(
                    2 * margin,
                    row_y + at * (look.row + margin),
                    inner,
                    look.row,
                ),
                false => Rect::new(2 * margin + at * (share + margin), row_y, share, look.row),
            })
            .collect();
        let area = Rect::new(
            2 * margin,
            y + margin,
            inner,
            row_y.saturating_sub(y + 2 * margin),
        );
        Self {
            look,
            nonce: String::from(get(words::NONCE)),
            title,
            lines,
            focus: labels.len() - 1,
            labels,
            buttons,
            panel,
            area,
            title_line,
            text_line,
            top: 0,
            shown: None,
            done: false,
        }
    }

    /// The box on the screen.
    pub(super) fn region(&self) -> Rect {
        self.panel
    }

    pub(super) fn hint(&self, action: &Id) -> Hint {
        if *action == actions::CONFIRM {
            Hint::Text(self.labels[self.focus].clone())
        } else {
            Hint::Hidden
        }
    }

    /// The height of line `at` of title and text, the room before the text
    /// included.
    fn row_height(&self, at: usize) -> u16 {
        if at < self.title.len() {
            self.title_line
        } else if at == self.title.len() && at > 0 {
            self.text_line + self.look.margin
        } else {
            self.text_line
        }
    }

    /// How many lines show from line `top` on.
    fn fitting_from(&self, top: usize) -> usize {
        let mut used = 0u16;
        let all = self.title.len() + self.lines.len();
        (top..all)
            .take_while(|at| {
                used = used.saturating_add(self.row_height(*at));
                used <= self.area.height
            })
            .count()
    }

    /// The last first line: as far as the text scrolls.
    fn last_top(&self) -> usize {
        let all = self.title.len() + self.lines.len();
        (0..all)
            .find(|top| top + self.fitting_from(*top) >= all)
            .unwrap_or(0)
    }

    /// Scrolls `by` lines a page; whether title and text scroll at all.
    fn scroll(&mut self, forward: bool, cx: &mut Cx<'_, '_>) -> bool {
        let last = self.last_top();
        if last == 0 {
            return false;
        }
        let page = self.fitting_from(self.top).saturating_sub(1).max(1);
        let to = match forward {
            true => (self.top + page).min(last),
            false => self.top.saturating_sub(page),
        };
        if to != self.top {
            self.top = to;
            self.redraw(cx);
        }
        true
    }

    pub(super) fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let look = &self.look;
        canvas.fill(self.panel, Color::WHITE);
        canvas.stroke_round(self.panel, look.border(), look.corner, Color::BLACK);
        let font = paint::font(env.fonts());
        if let Some(font) = &font {
            let mut y = self.area.y;
            for at in self.top..self.top + self.fitting_from(self.top) {
                let height = self.row_height(at);
                let (size, line, line_h) = match self.title.get(at) {
                    Some(line) => (look.title, line, self.title_line),
                    None => (
                        look.text,
                        &self.lines[at - self.title.len()],
                        self.text_line,
                    ),
                };
                // A line at the foot of its row: the room before the text
                // stays above it.
                let row = Rect::new(self.area.x, y + height - line_h, self.area.width, line_h);
                paint::line(canvas, font, size, line, row, Color::BLACK);
                y += height;
            }
        }
        for (at, (label, area)) in self.labels.iter().zip(&self.buttons).enumerate() {
            paint::button(canvas, font.as_ref(), look, *area, label, at == self.focus);
        }
    }

    /// Whether input counts yet (not within [`GUARD_MS`] of being shown).
    fn counts(&self, now: Instant) -> bool {
        self.shown
            .is_some_and(|at| now.0.saturating_sub(at.0) >= GUARD_MS)
    }

    /// What pressing the button `at` answers: the first is a yes, a middle
    /// one a yes with the data, the last a no.
    fn answer_of(&self, at: usize) -> Answer {
        if at == 0 {
            Answer::Yes
        } else if at + 1 == self.labels.len() {
            Answer::No
        } else {
            Answer::YesAndData
        }
    }

    /// Tells the kernel the answer, once, and closes.
    fn finish(&mut self, answer: Answer, cx: &mut Cx<'_, '_>) {
        if core::mem::replace(&mut self.done, true) {
            return;
        }
        let code = match answer {
            Answer::Yes => "yes",
            Answer::No => "no",
            Answer::YesAndData => "yes-data",
            Answer::Dismissed => "dismissed",
        };
        cx.command(Command::Emit(
            ANSWERED,
            Params::new()
                .with(words::NONCE, self.nonce.clone())
                .with(words::ANSWER, code),
        ));
        cx.close();
    }

    pub(super) fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        if self.shown.is_none() {
            let now = cx.env().now();
            self.shown = Some(now);
            cx.wake_at(Instant(now.0.saturating_add(TIMEOUT_MS)));
        }
    }

    pub(super) fn on_timer(&mut self, cx: &mut Cx<'_, '_>) {
        let now = cx.env().now();
        if self
            .shown
            .is_some_and(|at| now.0.saturating_sub(at.0) >= TIMEOUT_MS)
        {
            self.finish(Answer::Dismissed, cx);
        }
    }

    /// Back (and a tap outside) dismiss; nothing else closes it.
    pub(super) fn on_close_request(&mut self, cx: &mut Cx<'_, '_>) -> Handled {
        if self.counts(cx.env().now()) {
            self.finish(Answer::Dismissed, cx);
        }
        Handled::Yes
    }

    pub(super) fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if !self.counts(cx.env().now()) {
            return Handled::Yes;
        }
        if *action == actions::CONFIRM {
            self.finish(self.answer_of(self.focus), cx);
            return Handled::Yes;
        }
        if *action == actions::BACK {
            self.finish(Answer::Dismissed, cx);
            return Handled::Yes;
        }
        // Scrolls or moves the focus; every other action does nothing: only
        // the user's answer counts.
        let forward = [actions::DOWN, actions::NEXT_PAGE].contains(action);
        let back = [actions::UP, actions::PREV_PAGE].contains(action);
        if (forward || back) && self.scroll(forward, cx) {
            return Handled::Yes;
        }
        let last = self.labels.len() - 1;
        let to = if back || [actions::LEFT, actions::PREVIOUS].contains(action) {
            self.focus.saturating_sub(1)
        } else if forward || [actions::RIGHT, actions::NEXT].contains(action) {
            (self.focus + 1).min(last)
        } else {
            self.focus
        };
        if to != self.focus {
            self.focus = to;
            self.redraw(cx);
        }
        Handled::Yes
    }

    /// A swipe over the box scrolls; a tap counts only on its own buttons.
    pub(super) fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        use paperpaw_sdk::Direction;
        if !self.counts(cx.env().now()) {
            return Handled::Yes;
        }
        match gesture.kind {
            GestureKind::Swipe(Direction::Up) if self.panel.contains(gesture.start) => {
                self.scroll(true, cx);
            }
            GestureKind::Swipe(Direction::Down) if self.panel.contains(gesture.start) => {
                self.scroll(false, cx);
            }
            GestureKind::Tap => {
                if let Some(at) = self.hit(gesture.start) {
                    self.finish(self.answer_of(at), cx);
                }
            }
            _ => {}
        }
        Handled::Yes
    }

    fn hit(&self, point: Point) -> Option<usize> {
        self.buttons.iter().position(|b| b.contains(point))
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw_area(self.panel);
        cx.request_refresh(RefreshMode::Fast);
    }
}
