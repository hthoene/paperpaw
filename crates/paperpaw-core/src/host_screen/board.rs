//! The box the host's questions show in (the firmware confirmation, the
//! start notices): in the middle of the screen, a title, a text and buttons,
//! side by side or, where a word does not fit its share, one below the other.
//! A text taller than the screen scrolls above the buttons.
use crate::{Place, Turn};
use alloc::string::String;
use alloc::vec::Vec;

use super::paint::{self, Look};
use paperpaw_sdk::builtin::actions;
use paperpaw_sdk::{Canvas, Color, Gesture, GestureKind, Hint, Id, Point, Rect, RefreshMode};

/// The box; see the [module documentation](self).
pub(super) struct Board {
    look: Look,
    title: Vec<String>,
    /// The text's lines, wrapped.
    lines: Vec<String>,
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
}

impl Board {
    /// The box of `title` and `text` (paragraphs on lines of their own) with
    /// the buttons `labels`, the focus on the button `focus`.
    pub(super) fn new(
        env: &Place<'_>,
        (title, text): (&str, &str),
        labels: Vec<String>,
        focus: usize,
    ) -> Self {
        let look = Look::of(env.device(), env.settings(), env.fonts());
        let display = env.device().display;
        let margin = look.margin;
        let width = display.width.saturating_sub(2 * margin);
        let inner = width.saturating_sub(2 * margin);
        let (title, lines, title_line, text_line) = match paint::font(env.fonts()) {
            Some(font) => {
                let wrap = |size, text: &str| paint::wrapped(&font, size, text, f32::from(inner));
                let lines = text
                    .split('\n')
                    .filter(|_| !text.is_empty())
                    .flat_map(|paragraph| wrap(look.text, paragraph))
                    .collect();
                (
                    wrap(look.title, title),
                    lines,
                    font.line_metrics(look.title).line_height as u16,
                    font.line_metrics(look.text).line_height as u16,
                )
            }
            None => (Vec::new(), Vec::new(), 0, 0),
        };
        let count = u16::try_from(labels.len()).unwrap_or(u16::MAX).max(1);
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
            title,
            lines,
            focus: focus.min(labels.len().saturating_sub(1)),
            labels,
            buttons,
            panel,
            area,
            title_line,
            text_line,
            top: 0,
        }
    }

    /// The box on the screen.
    pub(super) fn region(&self) -> Rect {
        self.panel
    }

    /// The button the focus is on.
    pub(super) fn focus(&self) -> usize {
        self.focus
    }

    /// How many buttons there are.
    pub(super) fn buttons(&self) -> usize {
        self.labels.len()
    }

    /// *Confirm* says the button the focus is on; no other hint shows.
    pub(super) fn hint(&self, action: &Id) -> Hint {
        match self.labels.get(self.focus) {
            Some(label) if *action == actions::CONFIRM => Hint::Text(label.clone()),
            _ => Hint::Hidden,
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

    /// Scrolls a page; whether title and text scroll at all.
    fn scroll(&mut self, forward: bool, cx: &mut Turn<'_, '_>) -> bool {
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

    pub(super) fn draw(&self, canvas: &mut Canvas<'_>, env: &Place<'_>) {
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

    /// Scrolls or moves the focus; whether `action` is one that does (up
    /// and down scroll where the text does, else they move the focus as left
    /// and right do).
    pub(super) fn step(&mut self, action: &Id, cx: &mut Turn<'_, '_>) -> bool {
        let forward = [actions::DOWN, actions::NEXT_PAGE].contains(action);
        let back = [actions::UP, actions::PREV_PAGE].contains(action);
        if (forward || back) && self.scroll(forward, cx) {
            return true;
        }
        let last = self.labels.len().saturating_sub(1);
        let to = if back || [actions::LEFT, actions::PREVIOUS].contains(action) {
            self.focus.saturating_sub(1)
        } else if forward || [actions::RIGHT, actions::NEXT].contains(action) {
            (self.focus + 1).min(last)
        } else {
            return false;
        };
        if to != self.focus {
            self.focus = to;
            self.redraw(cx);
        }
        true
    }

    /// A swipe over the box scrolls; a tap on a button is that button's.
    pub(super) fn on_gesture(&mut self, gesture: Gesture, cx: &mut Turn<'_, '_>) -> Option<usize> {
        use paperpaw_sdk::Direction;
        match gesture.kind {
            GestureKind::Swipe(Direction::Up) if self.panel.contains(gesture.start) => {
                self.scroll(true, cx);
            }
            GestureKind::Swipe(Direction::Down) if self.panel.contains(gesture.start) => {
                self.scroll(false, cx);
            }
            GestureKind::Tap => return self.hit(gesture.start),
            _ => {}
        }
        None
    }

    fn hit(&self, point: Point) -> Option<usize> {
        self.buttons.iter().position(|b| b.contains(point))
    }

    fn redraw(&self, cx: &mut Turn<'_, '_>) {
        cx.request_redraw_area(self.panel);
        cx.request_refresh(RefreshMode::Fast);
    }
}
