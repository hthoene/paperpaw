//! A dialog over a screen: a framed box with a title, a few lines of text
//! and a row of buttons, the same space all round and between its parts
//! (the style's margin). Where it goes on the screen is the caller's
//! ([`Dialog::place`]); what the buttons do too ([`Dialog::hit`], and the
//! focus moved with [`Dialog::step`]). Nothing in it is ever shortened:
//! where title and text do not fit on the screen even in smaller sizes,
//! they scroll above the buttons ([`Dialog::on_scroll_action`],
//! [`Dialog::on_gesture`]).
//!
//! ```
//! # fn demo(env: &paperpaw_sdk::Env<'_>) {
//! use paperpaw_sdk::ui::{Dialog, UiStyle};
//!
//! let style = UiStyle::new(env);
//! let mut dialog = Dialog::new(env, &style, "Delete the book?", &["It goes for good."], &["Delete", "Keep"]);
//! let room = env.device().display.height - dialog.height();
//! dialog.place(room / 2);
//! # }
//! ```

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_platform::{Color, Point, Rect};

use crate::Id;
use crate::builtin::actions;
use crate::env::Env;
use crate::widgets::Frame;

use super::{Response, UiStyle};

/// A dialog: where its box and buttons are, and what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dialog {
    panel: Rect,
    /// The title's lines: it is never shortened.
    title: Vec<String>,
    lines: Vec<String>,
    labels: Vec<String>,
    buttons: Vec<Rect>,
    /// The height of the title's line and of a line of text.
    title_line: u16,
    text_line: u16,
    /// The height of the screen: the box stays within it.
    room: u16,
    /// The buttons one below the other: their words do not fit side by
    /// side.
    stacked: bool,
    /// Its text in the small size: in the text size it would not fit on
    /// the screen.
    small_text: bool,
    /// The size of its title: the title size, or smaller (the text size,
    /// the small size) where the dialog would not fit on the screen.
    title_size: u8,
    /// A line of a button's word where a word does not fit on one line
    /// of a button as wide as the box (on two lines then), else 0.
    label_line: u16,
    /// Where title and text are shown, above the buttons.
    area: Rect,
    /// The space between title and text, and round the parts.
    gap: u16,
    /// The first of the title's and the text's lines shown: more than
    /// the first where they scroll.
    top: usize,
}

/// The height of a row of a title of `count` lines at `size`: a line, or
/// a heading's line where it has more, so that they read as one.
fn title_row(font: &crate::FontHandle<'_>, size: f32, count: usize) -> u16 {
    if count > 1 {
        font.heading_line_height(size) as u16
    } else {
        font.line_metrics(size).line_height as u16
    }
}

impl Dialog {
    /// A dialog as wide as the screen less its margins, as tall as
    /// `title`, `text` (each a paragraph, wrapped) and `buttons` need, at
    /// the top of the screen until [placed](Self::place). Nothing is
    /// shortened: the title wraps, the buttons go one below the other
    /// where their words do not fit side by side (on two lines where a
    /// word is longer than the box is wide). Never taller than the screen
    /// less its margins: text and then the title get smaller, and where
    /// they still do not fit they scroll above the buttons.
    pub fn new(
        env: &Env<'_>,
        style: &UiStyle,
        title: &str,
        text: &[&str],
        buttons: &[&str],
    ) -> Self {
        let display = env.device().display;
        let margin = style.margin as u16;
        let width = display.width.saturating_sub(2 * margin);
        let inner = f32::from(width.saturating_sub(2 * margin));
        let wrap = |font: &crate::fonts::FontHandle<'_>, size: f32, paragraph: &str| {
            let wrapped = font.wrap(size, paragraph, inner);
            if wrapped.is_empty() {
                return alloc::vec![String::new()];
            }
            wrapped
                .into_iter()
                .map(|range| paragraph[range].to_string())
                .collect::<Vec<_>>()
        };
        let (title_lines, stacked) = match style.font(env) {
            Some(font) if !title.is_empty() => (wrap(&font, style.title, title), false),
            _ => (alloc::vec![title.to_string()], false),
        };
        // The buttons side by side where each word fits its share.
        let stacked = stacked
            || style.font(env).is_some_and(|font| {
                let count = buttons.len() as u16;
                let share =
                    (inner as u16).saturating_sub(margin * count.saturating_sub(1)) / count.max(1);
                count > 1
                    && buttons.iter().any(|label| {
                        let size = if label.chars().count() == 1 {
                            style.text
                        } else {
                            style.small
                        };
                        super::button_width(style, font.measure(size, label)) > f32::from(share)
                    })
            });
        let lines_of = |font: &crate::fonts::FontHandle<'_>, size: f32| -> Vec<String> {
            text.iter()
                .flat_map(|paragraph| wrap(font, size, paragraph))
                .collect()
        };
        // A word too long for a button as wide as the box: buttons of two
        // lines, one below the other.
        let full = inner - 2.0 * super::label_pad(style, inner);
        let label_line = style.font(env).map_or(0, |font| {
            let long = buttons
                .iter()
                .any(|label| label.chars().count() > 1 && font.measure(style.small, label) > full);
            if long {
                font.line_metrics(style.small).line_height as u16
            } else {
                0
            }
        });
        let stacked = stacked || label_line > 0;
        let (title_line, text_line, lines) = match style.font(env) {
            Some(font) => (
                title_row(&font, style.title, title_lines.len()),
                font.line_metrics(style.text).line_height as u16,
                lines_of(&font, style.text),
            ),
            None => (0, 0, Vec::new()),
        };
        let mut dialog = Self {
            panel: Rect::new(margin, 0, width, 0),
            title: if title.is_empty() {
                Vec::new()
            } else {
                title_lines
            },
            lines,
            labels: buttons.iter().map(|b| b.to_string()).collect(),
            buttons: Vec::new(),
            title_line,
            text_line,
            // The screen above the row of button hints.
            room: display
                .height
                .saturating_sub(super::hints::hints_height(env)),
            stacked,
            small_text: false,
            title_size: 0,
            label_line,
            area: Rect::new(0, 0, 0, 0),
            gap: margin,
            top: 0,
        };
        // Too tall for the screen: the text smaller, then the title,
        // before any of the text is cut.
        if let Some(font) = style.font(env) {
            if dialog.fitting_lines(style) < dialog.lines.len() {
                dialog.small_text = true;
                dialog.text_line = font.line_metrics(style.small).line_height as u16;
                dialog.lines = lines_of(&font, style.small);
            }
            for smaller in [1u8, 2] {
                if dialog.fitting_lines(style) >= dialog.lines.len() || title.is_empty() {
                    break;
                }
                let size = Self::title_size_of(style, smaller);
                dialog.title_size = smaller;
                dialog.title = wrap(&font, size, title);
                dialog.title_line = title_row(&font, size, dialog.title.len());
            }
        }
        dialog.lines.shrink_to_fit();
        dialog.lay_out(style, 0);
        dialog
    }

    /// The title's size: the title size (0), the text size (1), the small
    /// size (2).
    fn title_size_of(style: &UiStyle, smaller: u8) -> f32 {
        match smaller {
            0 => style.title,
            1 => style.text.min(style.title),
            _ => style.small.min(style.title),
        }
    }

    /// A button's height: two lines of a word where one is that long.
    fn button_height(&self, style: &UiStyle) -> u16 {
        super::Segments::height(style) + self.label_line
    }

    /// How many lines of text fit on the screen beside title and buttons.
    fn fitting_lines(&self, style: &UiStyle) -> usize {
        let margin = style.margin as u16;
        let button = self.button_height(style);
        let count = u16::try_from(self.labels.len()).unwrap_or(u16::MAX);
        let button_h = match (count, self.stacked) {
            (0, _) => 0,
            (_, true) => button
                .saturating_mul(count)
                .saturating_add(margin.saturating_mul(count - 1)),
            _ => button,
        };
        let title_h = self.title_height();
        let fixed = [title_h, button_h, 1].iter().filter(|p| **p > 0).count() as u16;
        let room = self
            .room
            .saturating_sub(2 * margin)
            .saturating_sub(margin.saturating_mul(fixed + 1))
            .saturating_sub(title_h)
            .saturating_sub(button_h);
        usize::from(room / self.text_line.max(1))
    }

    /// Its height: the margin above, between and below its parts.
    pub fn height(&self) -> u16 {
        self.panel.height
    }

    /// Its box.
    pub fn panel(&self) -> Rect {
        self.panel
    }

    /// Puts its top at `y`, or higher where it would not end on the
    /// screen.
    pub fn place(&mut self, y: u16) {
        let y = y.min(self.room.saturating_sub(self.panel.height));
        let dy = i32::from(y) - i32::from(self.panel.y);
        self.panel.y = y;
        self.area.y = (i32::from(self.area.y) + dy).max(0) as u16;
        for button in &mut self.buttons {
            button.y = (i32::from(button.y) + dy).max(0) as u16;
        }
    }

    /// Whether its title and text scroll: they do not fit on the screen.
    pub fn scrolls(&self) -> bool {
        self.last_top() > 0
    }

    /// Scrolls title and text `by` lines (back where negative); whether
    /// they moved.
    pub fn scroll(&mut self, by: isize) -> bool {
        let to = self.top.saturating_add_signed(by).min(self.last_top());
        let moved = to != self.top;
        self.top = to;
        moved
    }

    /// Where title and text scroll: the page buttons and up and down
    /// scroll them a page less a line ([`Response::Changed`] where they
    /// moved). Other actions, and all where nothing scrolls, are the
    /// caller's: the buttons' choice stays with it.
    pub fn on_action(&mut self, action: &Id) -> Response {
        if !self.scrolls() {
            return Response::Ignored;
        }
        let page = self.fitting_from(self.top).saturating_sub(1).max(1) as isize;
        let by = if [actions::NEXT_PAGE, actions::DOWN].contains(action) {
            page
        } else if [actions::PREV_PAGE, actions::UP].contains(action) {
            -page
        } else {
            return Response::Ignored;
        };
        Response::changed_if(self.scroll(by))
    }

    /// A swipe up or down over its box scrolls title and text a page
    /// ([`Response::Changed`] where they moved).
    pub fn on_gesture(&mut self, gesture: &crate::Gesture) -> Response {
        use crate::gesture::{Direction, GestureKind};
        if !self.scrolls() || !self.panel.contains(gesture.start) {
            return Response::Ignored;
        }
        let page = self.fitting_from(self.top).saturating_sub(1).max(1) as isize;
        let by = match gesture.kind {
            GestureKind::Swipe(Direction::Up) => page,
            GestureKind::Swipe(Direction::Down) => -page,
            _ => return Response::Ignored,
        };
        Response::changed_if(self.scroll(by))
    }

    /// The height of line `i` of title and text, the space before the
    /// text's first line included.
    fn row_height(&self, i: usize) -> u16 {
        if i < self.title.len() {
            self.title_line
        } else if i == self.title.len() && i > 0 {
            self.text_line + self.gap
        } else {
            self.text_line
        }
    }

    /// How many lines from line `top` fit in its area (at least one,
    /// where there are any).
    fn fitting_from(&self, top: usize) -> usize {
        let rows = self.title.len() + self.lines.len();
        let mut used = 0u16;
        let mut count = 0;
        for i in top..rows {
            // The first line shown has no space before it.
            let height = if i == top && i >= self.title.len() {
                self.text_line
            } else {
                self.row_height(i)
            };
            if used.saturating_add(height) > self.area.height && count > 0 {
                break;
            }
            used = used.saturating_add(height);
            count += 1;
        }
        // At least one where there is one: a line taller than the area.
        count.max(1).min(rows.saturating_sub(top))
    }

    /// The first line where the last of title and text is shown at the
    /// bottom of the area: 0 where all fit.
    fn last_top(&self) -> usize {
        let rows = self.title.len() + self.lines.len();
        (0..rows)
            .find(|top| top + self.fitting_from(*top) >= rows)
            .unwrap_or(0)
    }

    /// Where button `index` is.
    pub fn button(&self, index: usize) -> Option<Rect> {
        self.buttons.get(index).copied()
    }

    /// The button at `point`.
    pub fn hit(&self, point: Point) -> Option<usize> {
        self.buttons.iter().position(|b| b.contains(point))
    }

    /// Where the focus goes from button `focus` with `action`: left and
    /// right, up and down (and previous and next) along the buttons, side
    /// by side or one below the other. `None` for other actions.
    pub fn step(&self, focus: usize, action: &Id) -> Option<usize> {
        let last = self.buttons.len().checked_sub(1)?;
        if [actions::LEFT, actions::UP, actions::PREVIOUS].contains(action) {
            Some(focus.saturating_sub(1))
        } else if [actions::RIGHT, actions::DOWN, actions::NEXT].contains(action) {
            Some((focus + 1).min(last))
        } else {
            None
        }
    }

    fn lay_out(&mut self, style: &UiStyle, y: u16) {
        let margin = style.margin as u16;
        self.gap = margin;
        let button = self.button_height(style);
        let count = u16::try_from(self.labels.len()).unwrap_or(u16::MAX);
        let button_h = match (count, self.stacked) {
            (0, _) => 0,
            (_, true) => button
                .saturating_mul(count)
                .saturating_add(margin.saturating_mul(count - 1)),
            _ => button,
        };
        let (title_h, text_h) = (self.title_height(), self.text_height());
        let between = if title_h > 0 && text_h > 0 { margin } else { 0 };
        let content = title_h.saturating_add(between).saturating_add(text_h);
        let parts = u16::from(content > 0) + u16::from(button_h > 0);
        let needed = content
            .saturating_add(button_h)
            .saturating_add(margin.saturating_mul(parts + 1));
        // Never taller than the screen less its margins: what does not
        // fit scrolls.
        let height = needed.min(self.room.saturating_sub(2 * margin));
        self.panel = Rect::new(self.panel.x, y, self.panel.width, height);
        let inside = super::card_inside(style, self.panel, Frame::Line);
        let mut stack = super::Stack::new(inside, margin);
        let row = stack.take_bottom(button_h);
        self.area = stack.rest();
        self.buttons = if self.stacked {
            let mut stack = super::Stack::new(row, margin);
            self.labels.iter().map(|_| stack.take(button)).collect()
        } else {
            super::columns(row, self.labels.len(), margin)
        };
        self.top = self.top.min(self.last_top());
    }

    /// The height of its title's lines.
    fn title_height(&self) -> u16 {
        let lines = u16::try_from(self.title.len()).unwrap_or(u16::MAX);
        self.title_line.saturating_mul(lines)
    }

    /// The height of its text's lines.
    fn text_height(&self) -> u16 {
        let lines = u16::try_from(self.lines.len()).unwrap_or(u16::MAX);
        self.text_line.saturating_mul(lines)
    }

    /// Draws it, white with a frame (as the look draws frames), the
    /// button `focus` focused.
    pub fn draw(&self, ui: &mut super::Ui<'_>, focus: Option<usize>) {
        let (env, style) = (ui.env(), ui.style());
        let canvas = ui.canvas();
        canvas.fill(self.panel, Color::WHITE);
        let inside = super::draw_card(canvas, style, self.panel, Frame::Line);
        let _ = inside;
        if let Some(font) = style.font(env) {
            let title_size = Self::title_size_of(style, self.title_size);
            let text_size = if self.small_text {
                style.small
            } else {
                style.text
            };
            let mut y = self.area.y;
            let shown = self.fitting_from(self.top);
            for i in self.top..self.top + shown {
                let first = i == self.top;
                if i < self.title.len() {
                    let line = &self.title[i];
                    let row = Rect::new(self.area.x, y, self.area.width, self.title_line);
                    let x = f32::from(row.x)
                        + (f32::from(row.width) - font.measure(title_size, line)).max(0.0) / 2.0;
                    super::draw_label_in(
                        canvas,
                        &font,
                        style,
                        super::TextRole::TITLE,
                        title_size,
                        x,
                        row,
                        line,
                        Color::BLACK,
                    );
                    y += self.title_line;
                } else {
                    if i == self.title.len() && i > 0 && !first {
                        y += self.gap;
                    }
                    let line = &self.lines[i - self.title.len()];
                    let row = Rect::new(self.area.x, y, self.area.width, self.text_line);
                    super::draw_label_in(
                        canvas,
                        &font,
                        style,
                        super::TextRole::BODY,
                        text_size,
                        f32::from(row.x),
                        row,
                        line,
                        Color::BLACK,
                    );
                    y += self.text_line;
                }
            }
            if self.scrolls() {
                // Where the lines shown are in the whole, at the right.
                let rows = (self.title.len() + self.lines.len()) as u32;
                let bar = (style.line.max(1) * 2) as u16;
                let whole = u32::from(self.area.height);
                let length = (whole * shown as u32 / rows.max(1)).max(u32::from(bar) * 4) as u16;
                let at = self.area.y + (whole * self.top as u32 / rows.max(1)) as u16;
                let x = (self.panel.x + self.panel.width).saturating_sub(bar + style.gap as u16);
                canvas.fill(
                    Rect::new(x, at, bar, length.min(self.area.height)),
                    style.ink(super::Ink::MUTED),
                );
            }
        }
        // The buttons are there (and told) without a font too.
        // A word too long for one line on two: the buttons are as high.
        for (i, (rect, label)) in self.buttons.iter().zip(&self.labels).enumerate() {
            super::draw_button(canvas, env, style, *rect, label, false, focus == Some(i));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::STYLE;

    fn dialog(lines: usize, buttons: usize) -> Dialog {
        let mut dialog = Dialog {
            panel: Rect::new(20, 0, 440, 0),
            title: alloc::vec![String::from("Title")],
            lines: (0..lines).map(|_| String::from("line")).collect(),
            labels: (0..buttons).map(|_| String::from("button")).collect(),
            buttons: Vec::new(),
            title_line: 30,
            text_line: 20,
            room: 800,
            stacked: false,
            small_text: false,
            title_size: 0,
            label_line: 0,
            area: Rect::new(0, 0, 0, 0),
            gap: 10,
            top: 0,
        };
        dialog.lay_out(&STYLE, 0);
        dialog
    }

    #[test]
    fn buttons_whose_words_do_not_fit_side_by_side_go_one_below_the_other() {
        let margin = STYLE.margin as u16;
        let button = crate::ui::Segments::height(&STYLE);
        let mut d = dialog(1, 2);
        d.stacked = true;
        d.lay_out(&STYLE, 0);
        let (first, second) = (d.button(0).unwrap(), d.button(1).unwrap());
        assert_eq!(
            (first.width, second.width),
            (440 - 2 * margin, 440 - 2 * margin)
        );
        assert_eq!(second.y, first.y + button + margin);
        assert_eq!(second.bottom(), u32::from(d.height() - margin));
        assert_eq!(d.step(0, &actions::DOWN), Some(1));
        assert_eq!(d.step(1, &actions::UP), Some(0));
    }

    #[test]
    fn a_dialog_keeps_one_space_all_round() {
        let margin = STYLE.margin as u16;
        let button = crate::ui::Segments::height(&STYLE);
        let d = dialog(2, 2);
        assert_eq!(d.height(), 4 * margin + 30 + 40 + button);
        let (first, second) = (d.button(0).unwrap(), d.button(1).unwrap());
        // The buttons a margin from the box's sides and bottom, and apart.
        assert_eq!(first.x, 20 + margin);
        assert_eq!(second.right(), u32::from(20 + 440 - margin));
        assert_eq!(second.x, first.right() as u16 + margin);
        assert_eq!(first.bottom(), u32::from(d.height() - margin));
    }

    #[test]
    fn a_placed_dialog_moves_its_buttons_and_hits_them() {
        let mut d = dialog(1, 2);
        d.place(300);
        let second = d.button(1).unwrap();
        assert_eq!(d.panel().y, 300);
        assert_eq!(
            d.hit(Point::new(i32::from(second.x) + 1, i32::from(second.y) + 1)),
            Some(1)
        );
        assert_eq!(d.hit(Point::new(0, 0)), None);
        assert_eq!(d.step(0, &actions::RIGHT), Some(1));
        assert_eq!(d.step(1, &actions::RIGHT), Some(1));
        assert_eq!(d.step(1, &actions::CONFIRM), None);
    }

    #[test]
    fn a_dialog_too_tall_for_the_x4_scrolls_its_text_above_its_buttons() {
        let margin = STYLE.margin as u16;
        let mut d = dialog(500, 2);
        assert!(d.height() <= 800 - 2 * margin, "{}", d.height());
        // Nothing cut: every line kept, they scroll.
        assert_eq!(d.lines.len(), 500);
        assert!(d.scrolls());
        // Placed low: it moves up so all of it shows; the buttons inside.
        d.place(700);
        assert!(d.panel().bottom() <= 800);
        for button in [d.button(0).unwrap(), d.button(1).unwrap()] {
            assert!(button.bottom() <= u32::from(d.panel().bottom() as u16 - margin));
            assert!(button.y >= d.area.bottom() as u16);
        }
        assert!(d.on_action(&actions::NEXT_PAGE).changed());
        assert!(d.top > 0);
        assert!(d.scroll(10_000));
        assert_eq!(d.top, d.last_top());
        assert!(!d.on_action(&actions::CONFIRM).taken());
        // Lines beyond what any number holds: no overflow.
        assert!(dialog(70_000, 1).height() <= 800);
        // One that fits does not scroll.
        assert!(!dialog(2, 2).scrolls());
    }

    #[test]
    fn a_dialog_without_title_or_text_draws_none() {
        let mut d = dialog(0, 1);
        d.title.clear();
        d.lay_out(&STYLE, 0);
        assert_eq!(d.fitting_from(0), 0);
        assert!(!d.scrolls());
    }

    #[test]
    fn without_text_or_buttons_no_space_is_kept_for_them() {
        let margin = STYLE.margin as u16;
        assert_eq!(dialog(0, 0).height(), 2 * margin + 30);
    }
}
