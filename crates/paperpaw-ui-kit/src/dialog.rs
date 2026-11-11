//! A dialog over a screen: a framed box with a title, lines of text and a row
//! of buttons, the same space round and between its parts (the style's margin).
//! Where it goes is the caller's ([`Dialog::place`]); so is what the buttons do
//! ([`Dialog::hit`], and [`Dialog::step`] for the focus). Nothing is shortened:
//! where title and text do not fit the screen even in smaller sizes they
//! scroll above the buttons ([`Dialog::on_action`], [`Dialog::on_swipe`]).
//!
//! Laid out and drawn from owned [`Paint`] operations; allocation is fallible
//! and geometry checked. Differences from the interface-6 dialog: no readable
//! column on wide screens, buttons as tall as a list row, a long button word is
//! shortened with an ellipsis instead of set on two lines, and the room of the
//! button-hint row is not reserved. Where the interface runs right to left
//! ([`Style::rtl`]) side-by-side buttons start at the right, left and right swap
//! in [`Dialog::step`] and its lines start at the right.
use crate::{
    Paint, Style,
    geometry::{clamp_u16, line_px},
    mem::push,
    paint, text,
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::canvas::PartRole;
use paperpaw_sdk::{
    Color, Point, Rect,
    native::{Error, fonts::Font},
};

/// The most lines a title or text wraps to.
const MAX_LINES: usize = 512;

/// What a key did to a dialog's scrolling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response {
    /// Not a scrolling key (or nothing scrolls): the caller's.
    Ignored,
    /// A scrolling key at an end: taken, nothing moved.
    Held,
    /// The text moved: redraw.
    Moved,
}

/// A dialog: where its box and buttons are, and what it says.
#[derive(Debug)]
pub struct Dialog {
    panel: Rect,
    title: Vec<String>,
    lines: Vec<String>,
    labels: Vec<String>,
    buttons: Vec<Rect>,
    title_line: u16,
    text_line: u16,
    button: u16,
    margin: u16,
    line: u16,
    radius: u32,
    room: u16,
    stacked: bool,
    /// The interface runs right to left: buttons and lines start at the right.
    rtl: bool,
    /// Whether to queue what the dialog tells about itself.
    describing: bool,
    small_text: bool,
    title_size: f32,
    text_size: f32,
    small: f32,
    area: Rect,
    top: usize,
    ink: Color,
    paper: Color,
    muted: Color,
}

fn wrap(font: &Font<'_>, size: f32, paragraph: &str, inner: f32) -> Result<Vec<String>, Error> {
    let lines = text::wrap_with(|l| font.measure_text(size, l), paragraph, inner, MAX_LINES)?;
    if lines.is_empty() {
        let mut blank = Vec::new();
        push(&mut blank, String::new())?;
        return Ok(blank);
    }
    Ok(lines)
}

fn wrap_all(
    font: &Font<'_>,
    size: f32,
    paragraphs: &[&str],
    inner: f32,
) -> Result<Vec<String>, Error> {
    let mut all = Vec::new();
    for paragraph in paragraphs {
        for line in wrap(font, size, paragraph, inner)? {
            push(&mut all, line)?;
        }
    }
    Ok(all)
}

impl Dialog {
    /// A dialog as wide as the screen less its margins, as tall as `title`,
    /// `text` (each a paragraph, wrapped) and `buttons` need, at the top until
    /// [placed](Self::place). Never taller than the screen less its margins: the
    /// text and then the title get smaller, and what still does not fit scrolls.
    ///
    /// # Errors
    /// [`Error::NoMemory`], or a font reply that is refused as unbounded.
    pub fn new(
        font: &Font<'_>,
        style: &Style,
        screen: (u16, u16),
        title: &str,
        text: &[&str],
        buttons: &[&str],
    ) -> Result<Self, Error> {
        let margin = style.margin;
        let width = screen.0.saturating_sub(margin.saturating_mul(2));
        let inner = f32::from(width.saturating_sub(margin.saturating_mul(2))).max(1.0);
        let count = u16::try_from(buttons.len()).unwrap_or(u16::MAX);
        let share = inner as u16;
        let share =
            share.saturating_sub(margin.saturating_mul(count.saturating_sub(1))) / count.max(1);
        let gap = f32::from(style.gap);
        let mut stacked = false;
        if count > 1 {
            for label in buttons {
                let size = if label.chars().count() == 1 {
                    style.text
                } else {
                    style.small
                };
                let wide = font.measure_text(size, label)?;
                let need = if wide >= 10.0 * gap {
                    wide + 2.0 * gap
                } else {
                    wide * 1.2
                };
                stacked |= need > f32::from(share);
            }
        }
        let title_lines = if title.is_empty() {
            Vec::new()
        } else {
            wrap(font, style.title, title, inner)?
        };
        let mut dialog = Self {
            panel: Rect::new(margin, 0, width, 0),
            title_line: title_row(font, style.title, title_lines.len())?,
            text_line: line_px(font, style.text)?,
            title: title_lines,
            lines: wrap_all(font, style.text, text, inner)?,
            labels: copy_all(buttons)?,
            buttons: Vec::new(),
            button: style.row,
            margin,
            line: style.line,
            radius: style.corner,
            room: screen.1,
            stacked,
            rtl: style.rtl,
            describing: style.describing,
            small_text: false,
            title_size: style.title,
            text_size: style.text,
            small: style.small,
            area: Rect::new(0, 0, 0, 0),
            top: 0,
            ink: style.ink,
            paper: style.paper,
            muted: style.muted,
        };
        if dialog.fitting_lines() < dialog.lines.len() {
            dialog.small_text = true;
            dialog.text_size = style.small;
            dialog.text_line = line_px(font, style.small)?;
            dialog.lines = wrap_all(font, style.small, text, inner)?;
        }
        for size in [style.text.min(style.title), style.small.min(style.title)] {
            if dialog.fitting_lines() >= dialog.lines.len() || title.is_empty() {
                break;
            }
            dialog.title_size = size;
            dialog.title = wrap(font, size, title, inner)?;
            dialog.title_line = title_row(font, size, dialog.title.len())?;
        }
        dialog.lay_out(0)?;
        Ok(dialog)
    }

    fn button_block(&self) -> u16 {
        let count = u16::try_from(self.labels.len()).unwrap_or(u16::MAX);
        match (count, self.stacked) {
            (0, _) => 0,
            (_, true) => self
                .button
                .saturating_mul(count)
                .saturating_add(self.margin.saturating_mul(count - 1)),
            _ => self.button,
        }
    }

    fn title_height(&self) -> u16 {
        self.title_line
            .saturating_mul(u16::try_from(self.title.len()).unwrap_or(u16::MAX))
    }

    /// How many lines of text fit the screen beside title and buttons.
    fn fitting_lines(&self) -> usize {
        let (title, buttons) = (self.title_height(), self.button_block());
        let fixed = [title, buttons, 1].iter().filter(|p| **p > 0).count() as u16;
        let room = self
            .room
            .saturating_sub(self.margin.saturating_mul(2))
            .saturating_sub(self.margin.saturating_mul(fixed + 1))
            .saturating_sub(title)
            .saturating_sub(buttons);
        usize::from(room / self.text_line.max(1))
    }

    fn lay_out(&mut self, y: u16) -> Result<(), Error> {
        let margin = self.margin;
        let buttons = self.button_block();
        let title = self.title_height();
        let text = self
            .text_line
            .saturating_mul(u16::try_from(self.lines.len()).unwrap_or(u16::MAX));
        let between = if title > 0 && text > 0 { margin } else { 0 };
        let content = title.saturating_add(between).saturating_add(text);
        let parts = u16::from(content > 0) + u16::from(buttons > 0);
        let needed = content
            .saturating_add(buttons)
            .saturating_add(margin.saturating_mul(parts + 1));
        let height = needed.min(self.room.saturating_sub(margin.saturating_mul(2)));
        self.panel = Rect::new(self.panel.x, y, self.panel.width, height);
        // The card's inside: a line of frame and a margin all round is the
        // margin already counted; the frame is drawn on the box's edge.
        let inside = self.panel.inset(margin, margin);
        let bottom = inside.y.saturating_add(inside.height);
        let row_top = bottom.saturating_sub(buttons);
        let above = row_top.saturating_sub(inside.y);
        let above = if buttons > 0 && content > 0 {
            above.saturating_sub(margin)
        } else {
            above
        };
        self.area = Rect::new(inside.x, inside.y, inside.width, above);
        self.buttons.clear();
        let count = self.labels.len();
        self.buttons
            .try_reserve_exact(count)
            .map_err(|_| Error::NoMemory)?;
        for at in 0..count {
            let rect = if self.stacked {
                let at = at as u16;
                Rect::new(
                    inside.x,
                    row_top.saturating_add(at.saturating_mul(self.button.saturating_add(margin))),
                    inside.width,
                    self.button,
                )
            } else {
                let n = count as u16;
                let each = inside.width.saturating_sub(margin.saturating_mul(n - 1)) / n.max(1);
                Rect::new(
                    inside
                        .x
                        .saturating_add((at as u16).saturating_mul(each.saturating_add(margin))),
                    row_top,
                    each,
                    self.button,
                )
            };
            self.buttons
                .push(crate::style::flip_rect(self.rtl, inside, rect));
        }
        self.top = self.top.min(self.last_top());
        Ok(())
    }

    /// Its height.
    #[must_use]
    pub fn height(&self) -> u16 {
        self.panel.height
    }

    /// Its box.
    #[must_use]
    pub fn panel(&self) -> Rect {
        self.panel
    }

    /// Puts its top at `y`, or higher where it would not end on the screen.
    ///
    /// # Errors
    /// [`Error::NoMemory`]; it stays where it was.
    pub fn place(&mut self, y: u16) -> Result<(), Error> {
        let y = y.min(self.room.saturating_sub(self.panel.height));
        self.lay_out(y)
    }

    /// Whether title and text scroll: they do not fit the screen.
    #[must_use]
    pub fn scrolls(&self) -> bool {
        self.last_top() > 0
    }

    /// Scrolls `by` lines (back where negative); whether they moved.
    pub fn scroll(&mut self, by: isize) -> bool {
        let to = self.top.saturating_add_signed(by).min(self.last_top());
        let moved = to != self.top;
        self.top = to;
        moved
    }

    /// The page buttons and up and down scroll a page less a line where the
    /// text scrolls; anything else is the caller's.
    pub fn on_action(&mut self, action: &str) -> Response {
        if !self.scrolls() {
            return Response::Ignored;
        }
        let page = self.fitting_from(self.top).saturating_sub(1).max(1) as isize;
        let by = match action {
            "core.next-page" | "core.down" => page,
            "core.prev-page" | "core.up" => -page,
            _ => return Response::Ignored,
        };
        if self.scroll(by) {
            Response::Moved
        } else {
            Response::Held
        }
    }

    /// A swipe up (`up`) or down over the box scrolls a page.
    pub fn on_swipe(&mut self, at: Point, up: bool) -> Response {
        if !self.scrolls() || !self.panel.contains(at) {
            return Response::Ignored;
        }
        let page = self.fitting_from(self.top).saturating_sub(1).max(1) as isize;
        if self.scroll(if up { page } else { -page }) {
            Response::Moved
        } else {
            Response::Held
        }
    }

    fn row_height(&self, i: usize) -> u16 {
        if i < self.title.len() {
            self.title_line
        } else if i == self.title.len() && i > 0 {
            self.text_line.saturating_add(self.margin)
        } else {
            self.text_line
        }
    }

    fn fitting_from(&self, top: usize) -> usize {
        let rows = self.title.len() + self.lines.len();
        let (mut used, mut count) = (0u16, 0);
        for i in top..rows {
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
        count.max(1).min(rows.saturating_sub(top))
    }

    fn last_top(&self) -> usize {
        let rows = self.title.len() + self.lines.len();
        (0..rows)
            .find(|top| top + self.fitting_from(*top) >= rows)
            .unwrap_or(0)
    }

    /// Where button `index` is.
    #[must_use]
    pub fn button(&self, index: usize) -> Option<Rect> {
        self.buttons.get(index).copied()
    }

    /// The button at `point`.
    #[must_use]
    pub fn hit(&self, point: Point) -> Option<usize> {
        self.buttons.iter().position(|b| b.contains(point))
    }

    /// Where the focus goes from button `focus` with `action`: left/right and
    /// up/down (and previous/next) along the buttons; `None` for other actions.
    #[must_use]
    pub fn step(&self, focus: usize, action: &str) -> Option<usize> {
        let last = self.buttons.len().checked_sub(1)?;
        let (back, on) = (focus.saturating_sub(1), focus.saturating_add(1).min(last));
        // Side by side the first button is at the right where the interface
        // runs right to left (see `lay_out`): left is then on towards the
        // last. One below the other, left and right keep their old way.
        let mirrored = self.rtl && !self.stacked;
        match action {
            "core.up" | "core.previous" => Some(back),
            "core.down" | "core.next" => Some(on),
            "core.left" => Some(if mirrored { on } else { back }),
            "core.right" => Some(if mirrored { back } else { on }),
            _ => None,
        }
    }

    /// Sets whether the dialog tells what it is (`canvas.describe`): from
    /// [`paint::asking`] in the draw callback; off unless set.
    pub fn set_describing(&mut self, describing: bool) {
        self.describing = describing;
    }

    /// Prepares its drawing, button `focus` focused.
    ///
    /// # Errors
    /// [`Error::NoMemory`] or a refused font reply.
    pub fn prepare(
        &self,
        paint: &mut Vec<Paint>,
        font: &Font<'_>,
        focus: Option<usize>,
    ) -> Result<(), Error> {
        paint::describe(
            paint,
            self.describing,
            PartRole::Dialog,
            self.panel,
            false,
            false,
        )?;
        paint::push(paint, Paint::Fill(self.panel, self.paper))?;
        let thickness = u32::from(self.line).max(1);
        paint::push(
            paint,
            Paint::Focus(self.panel, self.radius, Some(thickness), self.ink),
        )?;
        let shown = self.fitting_from(self.top);
        let mut y = self.area.y;
        for i in self.top..self.top.saturating_add(shown) {
            if i < self.title.len() {
                let line = &self.title[i];
                let row = Rect::new(self.area.x, y, self.area.width, self.title_line);
                let wide = font.measure_text(self.title_size, line)?;
                let x = f32::from(row.x) + (f32::from(row.width) - wide).max(0.0) / 2.0;
                text::label(paint, font, line, self.title_size, x, row, self.ink, None)?;
                y = y.saturating_add(self.title_line);
            } else {
                if i == self.title.len() && i > 0 && i != self.top {
                    y = y.saturating_add(self.margin);
                }
                let line = &self.lines[i - self.title.len()];
                let row = Rect::new(self.area.x, y, self.area.width, self.text_line);
                // From the start side of the text area: the right one where
                // the interface runs right to left.
                text::label_in(
                    paint,
                    font,
                    self.rtl,
                    self.area,
                    line,
                    self.text_size,
                    f32::from(row.x),
                    row,
                    self.ink,
                    None,
                )?;
                y = y.saturating_add(self.text_line);
            }
        }
        if self.scrolls() {
            let rows = (self.title.len() + self.lines.len()) as u64;
            let bar = self.line.max(1).saturating_mul(2);
            let whole = u64::from(self.area.height);
            let length = (whole * shown as u64 / rows.max(1)).max(u64::from(bar) * 4);
            let at = u64::from(self.area.y) + whole * self.top as u64 / rows.max(1);
            let x = (self.panel.x.saturating_add(self.panel.width))
                .saturating_sub(bar.saturating_add(self.margin));
            let place = Rect::new(
                x,
                clamp_u16(at as i64),
                bar,
                clamp_u16(length.min(whole) as i64),
            );
            let place = crate::style::flip_rect(self.rtl, self.panel, place);
            paint::push(paint, Paint::Fill(place, self.muted))?;
        }
        for (i, (rect, label)) in self.buttons.iter().zip(&self.labels).enumerate() {
            let focused = focus == Some(i);
            paint::describe(
                paint,
                self.describing,
                PartRole::Button,
                *rect,
                focused,
                false,
            )?;
            let (fill, ink) = if focused {
                (Some(self.ink), self.paper)
            } else {
                (None, self.ink)
            };
            match fill {
                Some(color) => {
                    paint::push(paint, Paint::Focus(*rect, self.radius, None, color))?;
                }
                None => {
                    paint::push(
                        paint,
                        Paint::Focus(*rect, self.radius, Some(thickness), ink),
                    )?;
                }
            }
            let room = f32::from(rect.width.saturating_sub(self.margin.saturating_mul(2))).max(1.0);
            let fitted = text::fit(font, self.small, label, room)?;
            let wide = font.measure_text(self.small, &fitted)?;
            let x = f32::from(rect.x) + (f32::from(rect.width) - wide).max(0.0) / 2.0;
            text::label(paint, font, &fitted, self.small, x, *rect, ink, Some(*rect))?;
        }
        Ok(())
    }
}

fn copy_all(items: &[&str]) -> Result<Vec<String>, Error> {
    let mut found = Vec::new();
    found
        .try_reserve_exact(items.len())
        .map_err(|_| Error::NoMemory)?;
    for item in items {
        found.push(text::copy(item)?);
    }
    Ok(found)
}

fn title_row(font: &Font<'_>, size: f32, count: usize) -> Result<u16, Error> {
    // A heading's line where there are several lines, so that they read as one.
    let line = line_px(font, size)?;
    Ok(if count > 1 {
        line.saturating_add(line / 10)
    } else {
        line
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog(lines: usize, buttons: usize, stacked: bool) -> Dialog {
        let mut dialog = Dialog {
            panel: Rect::new(20, 0, 440, 0),
            title: ["Title"].map(String::from).into(),
            lines: (0..lines).map(|_| String::from("line")).collect(),
            labels: (0..buttons).map(|_| String::from("button")).collect(),
            buttons: Vec::new(),
            title_line: 30,
            text_line: 20,
            button: 50,
            margin: 20,
            line: 2,
            radius: 4,
            room: 800,
            stacked,
            rtl: false,
            describing: false,
            small_text: false,
            title_size: 24.0,
            text_size: 16.0,
            small: 12.0,
            area: Rect::new(0, 0, 0, 0),
            top: 0,
            ink: Color::BLACK,
            paper: Color::WHITE,
            muted: Color::gray(90),
        };
        dialog.lay_out(0).unwrap();
        dialog
    }

    fn mirrored(lines: usize, buttons: usize, stacked: bool) -> Dialog {
        let mut dialog = dialog(lines, buttons, stacked);
        dialog.rtl = true;
        dialog.lay_out(0).unwrap();
        dialog
    }

    #[test]
    fn side_by_side_the_first_button_is_at_the_right_where_the_interface_runs_so() {
        let (ltr, rtl) = (dialog(2, 3, false), mirrored(2, 3, false));
        for i in 0..3 {
            let (a, b) = (ltr.button(i).unwrap(), rtl.button(i).unwrap());
            assert_eq!((a.y, a.width, a.height), (b.y, b.width, b.height));
        }
        assert!(rtl.button(0).unwrap().x > rtl.button(2).unwrap().x);
        assert!(ltr.button(0).unwrap().x < ltr.button(2).unwrap().x);
        // Taps and the keys follow where they are drawn.
        let first = rtl.button(0).unwrap();
        assert_eq!(
            rtl.hit(Point::new(i32::from(first.x) + 1, i32::from(first.y) + 1)),
            Some(0)
        );
        assert_eq!(rtl.step(0, "core.left"), Some(1));
        assert_eq!(rtl.step(1, "core.right"), Some(0));
        assert_eq!(rtl.step(0, "core.next"), Some(1));
        assert_eq!(ltr.step(0, "core.left"), Some(0));
        assert_eq!(ltr.step(0, "core.right"), Some(1));
    }

    #[test]
    fn stacked_buttons_keep_their_order_and_their_keys() {
        let (ltr, rtl) = (dialog(2, 3, true), mirrored(2, 3, true));
        for i in 0..3 {
            assert_eq!(ltr.button(i), rtl.button(i));
        }
        assert_eq!(rtl.step(0, "core.right"), Some(1));
        assert_eq!(rtl.step(1, "core.left"), Some(0));
    }

    #[test]
    fn a_dialog_keeps_one_space_all_round() {
        let d = dialog(2, 2, false);
        assert_eq!(d.height(), 4 * 20 + 30 + 40 + 50);
        let (first, second) = (d.button(0).unwrap(), d.button(1).unwrap());
        assert_eq!(first.x, 20 + 20);
        assert_eq!(second.right(), 20 + 440 - 20);
        assert_eq!(second.x, first.right() as u16 + 20);
        assert_eq!(first.bottom(), u32::from(d.height() - 20));
    }

    #[test]
    fn stacked_buttons_go_one_below_the_other() {
        let d = dialog(1, 2, true);
        let (first, second) = (d.button(0).unwrap(), d.button(1).unwrap());
        assert_eq!((first.width, second.width), (400, 400));
        assert_eq!(second.y, first.y + 50 + 20);
        assert_eq!(d.step(0, "core.down"), Some(1));
        assert_eq!(d.step(1, "core.up"), Some(0));
    }

    #[test]
    fn a_placed_dialog_moves_its_buttons_and_hits_them() {
        let mut d = dialog(1, 2, false);
        d.place(300).unwrap();
        let second = d.button(1).unwrap();
        assert_eq!(d.panel().y, 300);
        assert_eq!(
            d.hit(Point::new(i32::from(second.x) + 1, i32::from(second.y) + 1)),
            Some(1)
        );
        assert_eq!(d.hit(Point::new(0, 0)), None);
        assert_eq!(d.step(0, "core.right"), Some(1));
        assert_eq!(d.step(1, "core.right"), Some(1));
        assert_eq!(d.step(1, "core.confirm"), None);
    }

    #[test]
    fn text_too_tall_for_the_screen_scrolls_above_the_buttons() {
        let mut d = dialog(500, 2, false);
        assert!(d.height() <= 800 - 40);
        assert_eq!(d.lines.len(), 500, "nothing is cut");
        assert!(d.scrolls());
        d.place(700).unwrap();
        assert!(d.panel().bottom() <= 800);
        for button in [d.button(0).unwrap(), d.button(1).unwrap()] {
            assert!(button.y >= d.area.bottom() as u16);
        }
        assert_eq!(d.on_action("core.next-page"), Response::Moved);
        assert!(d.top > 0);
        assert!(d.scroll(10_000));
        assert_eq!(d.top, d.last_top());
        assert_eq!(d.on_action("core.next-page"), Response::Held);
        assert_eq!(d.on_action("core.confirm"), Response::Ignored);
        assert!(!dialog(2, 2, false).scrolls());
    }

    #[test]
    fn absurd_sizes_do_not_overflow() {
        assert!(dialog(70_000, 1, false).height() <= 800);
        let many = dialog(2, 200, true);
        assert_eq!(many.buttons.len(), 200);
        let mut d = dialog(0, 0, false);
        d.title.clear();
        d.lay_out(u16::MAX).unwrap();
        assert!(!d.scrolls());
    }

    #[test]
    fn layout_survives_every_refused_allocation() {
        let mut d = dialog(3, 3, false);
        crate::fail_alloc::sweep(|| d.place(100).map(|()| d.buttons.len()));
    }
}
