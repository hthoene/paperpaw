//! A text field in a row ([`Ui::text_field`](super::Ui::text_field), edited on the keyboard's
//! screen with [`edit_text`]), rows ticked or not of a list where several
//! may be ([`draw_check_row`]), and text longer than its room, scrolled
//! with the buttons and by touch ([`TextView`]).

use alloc::vec::Vec;
use core::ops::Range;

use paperpaw_graphics::{Canvas, Role};
use paperpaw_platform::{Color, Point, Rect};

use crate::builtin::actions;
use crate::env::Env;
use crate::event::Event;
use crate::gesture::{Direction, Gesture, GestureKind};
use crate::replies::Typed;
use crate::{Command, Id};

use super::{Ink, Response, UiStyle};

/// Where a text field's title and box go in `row`: the title measured
/// first keeps its whole width (`title_width`, the width of its widest
/// line where it is wrapped into `title_lines` lines `line` tall); the
/// box beside it takes what is left, at least a third of the row; where
/// that does not fit, the title takes the top of the row and the box its
/// whole width below it. The box is as tall as a button. Whether it is
/// below is `true` in the third place.
pub fn text_field_layout(
    style: &UiStyle,
    row: Rect,
    title_width: f32,
    title_lines: u16,
    line: u16,
) -> (Rect, Rect, bool) {
    let margin = style.margin as u16;
    let button = super::Segments::height(style);
    // Rounded up, so the title keeps all its room.
    let title_width = title_width as u16 + u16::from(title_width > f32::from(title_width as u16));
    let least = row.width / 3;
    let inner = row.width.saturating_sub(2 * margin);
    if title_lines <= 1 && title_width.saturating_add(margin).saturating_add(least) <= inner {
        let title = Rect::new(row.x + margin, row.y, title_width, row.height);
        let x = title.x + title_width + margin;
        let field = Rect::new(
            x,
            row.y + row.height.saturating_sub(button) / 2,
            (row.x + row.width).saturating_sub(x + margin),
            button,
        );
        return (title, field, false);
    }
    let top = row.y + margin / 2;
    let title = Rect::new(row.x + margin, top, inner, line.saturating_mul(title_lines));
    let field = Rect::new(
        row.x + margin,
        title.y + title.height + style.gap as u16,
        inner,
        button,
    );
    (title, field, true)
}

/// The height of a text field's row `width` wide titled `title`: a row
/// ([`UiStyle::row`]) where the box fits beside the whole title, else the
/// title's lines and the box below them ([`text_field_layout`]).
pub fn text_field_height(env: &Env<'_>, style: &UiStyle, width: u16, title: &str) -> u16 {
    let row = Rect::new(0, 0, width, style.row as u16);
    let Some(font) = style.font(env) else {
        return row.height;
    };
    let (lines, widest, line) = title_lines(&font, style, title, width);
    let (_, field, below) = text_field_layout(style, row, widest, lines, line);
    if below {
        field.y + field.height + style.margin as u16 / 2
    } else {
        row.height
    }
}

/// `title` in the text size: its lines where it is wrapped into a row
/// `width` wide less its margins (one where it fits), the widest, and a
/// line's height.
fn title_lines(
    font: &crate::fonts::FontHandle<'_>,
    style: &UiStyle,
    title: &str,
    width: u16,
) -> (u16, f32, u16) {
    let line = font.line_metrics(style.text).line_height as u16;
    let room = f32::from(width.saturating_sub(2 * style.margin as u16));
    let whole = font.measure(style.text, title);
    // A title that fits beside a third of the row stays on one line.
    if whole <= room - f32::from(width / 3) - style.margin as f32 {
        return (1, whole, line);
    }
    let wrapped = font.wrap(style.text, title, room);
    let widest = wrapped
        .iter()
        .map(|r| font.measure(style.text, &title[r.clone()]))
        .fold(0.0f32, f32::max);
    (wrapped.len().max(1) as u16, widest, line)
}

/// Draws a one-line text field in `row`: `title`, whole, then `value` in
/// a framed box (`placeholder`, gray, while it is empty), the value
/// alone shortened to fit. The box is beside the title where it fits
/// with a third of the row, else below it: give the row
/// [`text_field_height`]. With `focused`, the row is focused (draw the
/// focus first, as for other rows). *Select* or a tap opens the keyboard
/// ([`edit_text`]); the value comes back as the reply.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub(crate) fn draw_text_field(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    row: Rect,
    focused: bool,
    title: &str,
    value: &str,
    placeholder: &str,
) {
    let Some(font) = style.font(env) else {
        let (_, field, _) = text_field_layout(style, row, 0.0, 1, 0);
        canvas.describe(Role::Field, value, field).focused(focused);
        return;
    };
    let (lines, widest, line) = title_lines(&font, style, title, row.width);
    let (title_area, field, below) = text_field_layout(style, row, widest, lines, line);
    canvas.describe(Role::Field, value, field).focused(focused);
    let ink = style.focus_ink(focused);
    if below {
        draw_lines(
            canvas, &font, style, style.text, title_area, line, title, ink,
        );
    } else {
        super::draw_label_in(
            canvas,
            &font,
            style,
            super::TextRole::BODY,
            style.text,
            f32::from(title_area.x),
            title_area,
            title,
            ink,
        );
    }
    let margin = style.margin as u16;
    let paper = if focused && style.dark_focus() {
        Color::BLACK
    } else {
        Color::WHITE
    };
    let radius = style.radius(u32::from(field.height));
    canvas.fill_round(field, radius, paper);
    canvas.stroke_round(field, super::button_border(style), radius, ink);
    let inside = Rect::new(
        field.x + margin / 2,
        field.y,
        field.width.saturating_sub(margin),
        field.height,
    );
    let (text, color) = if value.is_empty() {
        let muted = if focused && style.dark_focus() {
            Color::WHITE
        } else {
            style.ink(Ink::Muted)
        };
        (placeholder, muted)
    } else {
        (value, ink)
    };
    let shown = super::fit(&font, style.text, text, f32::from(inside.width));
    super::draw_label_in(
        canvas,
        &font,
        style,
        super::TextRole::BODY,
        style.text,
        f32::from(inside.x),
        inside,
        &shown,
        color,
    );
}

/// `text` wrapped into `area`'s width, a line `line` tall each, from its
/// top.
fn draw_lines(
    canvas: &mut Canvas<'_>,
    font: &crate::fonts::FontHandle<'_>,
    style: &UiStyle,
    size: f32,
    area: Rect,
    line: u16,
    text: &str,
    ink: Color,
) {
    for (i, range) in font
        .wrap(size, text, f32::from(area.width))
        .into_iter()
        .enumerate()
    {
        let at = Rect::new(area.x, area.y + i as u16 * line, area.width, line);
        super::draw_label_in(
            canvas,
            font,
            style,
            super::TextRole::BODY,
            size,
            f32::from(at.x),
            at,
            &text[range],
            ink,
        );
    }
}

/// What opens the keyboard for a text field titled `title` with `value`
/// in it; what is typed comes back as `reply` ([`ask::text`](crate::ask::text)).
pub fn edit_text(title: &str, value: &str, reply: &Event<Typed>) -> Command {
    crate::ask::text(title, reply).initial(value).command()
}

/// Where the box of a ticked row goes: at the row's left, a margin in,
/// as a switch in a plain row is tall, beside the row's first line.
pub fn check_rect(style: &UiStyle, row: Rect) -> Rect {
    let plain = (style.control_row as u16).min(row.height);
    let side = (plain * 7 / 10 * 3 / 4).max(8);
    Rect::new(
        row.x + style.margin as u16,
        row.y + plain.saturating_sub(side) / 2,
        side,
        side,
    )
}

/// The height of a ticked row `width` wide: a row ([`UiStyle::row`]), or
/// taller where `title` (whole, wrapped beside the box) and `note` need
/// more lines.
pub fn check_row_height(
    env: &Env<'_>,
    style: &UiStyle,
    width: u16,
    title: &str,
    note: &str,
) -> u16 {
    let row = Rect::new(0, 0, width, style.row as u16);
    let Some(font) = style.font(env) else {
        return row.height;
    };
    let text = check_text_width(style, row);
    let line = font.line_metrics(style.text).line_height as u16;
    let small = font.line_metrics(style.small).line_height as u16;
    let lines = font.wrap(style.text, title, f32::from(text)).len().max(1) as u16;
    let notes = if note.is_empty() {
        0
    } else {
        font.wrap(style.small, note, f32::from(text)).len().max(1) as u16
    };
    let needed = line * lines + small * notes + style.margin as u16;
    row.height.max(needed)
}

/// The width a ticked row's text has beside its box.
fn check_text_width(style: &UiStyle, row: Rect) -> u16 {
    let check = check_rect(style, row);
    let x = check.x + check.width + style.margin as u16;
    (row.x + row.width).saturating_sub(x + style.margin as u16)
}

/// Draws a box, ticked (`on`) or not, in `rect`; `inverted` on a focused
/// (black) row. A look of a plugin's own may paint it
/// ([`Painters::check`](super::look::Painters::check)).
pub fn draw_check(canvas: &mut Canvas<'_>, style: &UiStyle, rect: Rect, on: bool, inverted: bool) {
    canvas.describe(Role::Check, "", rect).on(Some(on));
    let part = super::look::CheckPart { rect, on, inverted };
    if style.look.check(canvas, style, part) {
        return;
    }
    let (ink, paper) = if inverted {
        (Color::WHITE, Color::BLACK)
    } else {
        (Color::BLACK, Color::WHITE)
    };
    let border = super::button_border(style);
    let radius = (style.corner / 2).min(u32::from(rect.height) / 4);
    if !on {
        canvas.fill_round(rect, radius, paper);
        canvas.stroke_round(rect, border, radius, ink);
        return;
    }
    canvas.fill_round(rect, radius, ink);
    // The tick: down to the lower third, then up to the right.
    let side = i32::from(rect.width);
    let (x, y) = (i32::from(rect.x), i32::from(rect.y));
    let width = (border * 2).max(2);
    let low = Point::new(x + side * 2 / 5, y + side * 7 / 10);
    canvas.draw_line(Point::new(x + side / 4, y + side / 2), low, width, paper);
    canvas.draw_line(
        low,
        Point::new(x + side * 3 / 4, y + side * 3 / 10),
        width,
        paper,
    );
}

/// Draws a row of a list where several may be ticked: its box on the
/// left ([`draw_check`]), `title` beside it, `note` small below where
/// given. Draw the focus first where it is focused, as for other rows.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub fn draw_check_row(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    row: Rect,
    focused: bool,
    title: &str,
    note: &str,
    on: bool,
) {
    let check = check_rect(style, row);
    draw_check(canvas, style, check, on, focused && style.dark_focus());
    let Some(font) = style.font(env) else {
        return;
    };
    let ink = style.focus_ink(focused);
    let x = check.x + check.width + style.margin as u16;
    let width = check_text_width(style, row);
    let line = font.line_metrics(style.text).line_height as u16;
    let small = font.line_metrics(style.small).line_height as u16;
    let titles = font.wrap(style.text, title, f32::from(width));
    let notes = if note.is_empty() {
        Vec::new()
    } else {
        font.wrap(style.small, note, f32::from(width))
    };
    // The text whole, in the middle of the row's height.
    let height = line * titles.len().max(1) as u16 + small * notes.len() as u16;
    let mut y = row.y + row.height.saturating_sub(height) / 2;
    for range in titles {
        let at = Rect::new(x, y, width, line);
        super::draw_label_in(
            canvas,
            &font,
            style,
            super::TextRole::BODY,
            style.text,
            f32::from(x),
            at,
            &title[range],
            ink,
        );
        y += line;
    }
    let muted = if focused && style.dark_focus() {
        Color::WHITE
    } else {
        style.ink(Ink::Muted)
    };
    for range in notes {
        let at = Rect::new(x, y, width, small);
        super::draw_label_in(
            canvas,
            &font,
            style,
            super::TextRole::SMALL,
            style.small,
            f32::from(x),
            at,
            &note[range],
            muted,
        );
        y += small;
    }
}

/// Text longer than its room, a page of lines at a time: the up and down
/// buttons scroll a few lines, the page buttons a page; by touch, a swipe
/// up or down scrolls a page, a tap in the lower or upper half too. The
/// caller keeps the text; the view keeps where its lines begin and end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextView {
    area: Rect,
    /// Each line: where it lies in the text.
    lines: Vec<Range<u32>>,
    line: u16,
    /// The first line shown.
    top: usize,
}

impl TextView {
    /// `text` (paragraphs apart by line breaks) wrapped into `area` in the
    /// style's text size.
    pub fn new(env: &Env<'_>, style: &UiStyle, area: Rect, text: &str) -> Self {
        let Some(font) = style.font(env) else {
            return Self::of_lines(area, 1, Vec::new());
        };
        let width = f32::from(area.width.saturating_sub(style.gap as u16));
        let mut lines = Vec::new();
        let mut start = 0;
        for paragraph in text.split('\n') {
            let wrapped = font.wrap(style.text, paragraph, width);
            if wrapped.is_empty() {
                lines.push(start as u32..start as u32);
            }
            for range in wrapped {
                lines.push((start + range.start) as u32..(start + range.end) as u32);
            }
            start += paragraph.len() + 1;
        }
        lines.shrink_to_fit();
        let line = font.line_metrics(style.text).line_height as u16;
        Self::of_lines(area, line, lines)
    }

    pub(crate) fn of_lines(area: Rect, line: u16, lines: Vec<Range<u32>>) -> Self {
        Self {
            area,
            lines,
            line: line.max(1),
            top: 0,
        }
    }

    /// The lines that fit at once (at least one).
    pub fn rows(&self) -> usize {
        usize::from(self.area.height / self.line).max(1)
    }

    /// The first line shown.
    pub fn top(&self) -> usize {
        self.top
    }

    /// The first line the last page begins with.
    fn last_top(&self) -> usize {
        self.lines.len().saturating_sub(self.rows())
    }

    /// Scrolls `by` lines (back where negative), within the text; whether
    /// it moved.
    pub fn scroll(&mut self, by: isize) -> bool {
        let to = self.top.saturating_add_signed(by).min(self.last_top());
        let moved = to != self.top;
        self.top = to;
        moved
    }

    /// Scrolls as `action` says: up and down a third of a page, the page
    /// buttons (and previous and next) a page: [`Response::Changed`]
    /// where it moved. Other actions are not its.
    pub fn on_action(&mut self, action: &Id) -> Response {
        let page = self.rows() as isize;
        let few = (page / 3).max(1);
        let by = if *action == actions::DOWN {
            few
        } else if *action == actions::UP {
            -few
        } else if [actions::NEXT_PAGE, actions::NEXT].contains(action) {
            page
        } else if [actions::PREV_PAGE, actions::PREVIOUS].contains(action) {
            -page
        } else {
            return Response::Ignored;
        };
        Response::changed_if(self.scroll(by))
    }

    /// Scrolls as `gesture` says: a swipe up a page on, down a page back;
    /// a tap in the lower half a page on, in the upper half back.
    /// [`Response::Changed`] where it moved.
    pub fn on_gesture(&mut self, gesture: &Gesture) -> Response {
        if !self.area.contains(gesture.start) {
            return Response::Ignored;
        }
        let page = self.rows() as isize;
        let middle = i32::from(self.area.y) + i32::from(self.area.height) / 2;
        let by = match gesture.kind {
            GestureKind::Swipe(Direction::Up) => page,
            GestureKind::Swipe(Direction::Down) => -page,
            GestureKind::Tap if gesture.start.y >= middle => page,
            GestureKind::Tap => -page,
            _ => return Response::Ignored,
        };
        Response::changed_if(self.scroll(by))
    }

    /// Draws the lines shown of `text` (the text it was made of), and on
    /// the right a thin bar where they are in the whole, where there is
    /// more than a page.
    pub fn draw(&self, ui: &mut super::Ui<'_>, text: &str) {
        let (env, style) = (ui.env(), ui.style());
        let canvas = ui.canvas();
        let Some(font) = style.font(env) else {
            return;
        };
        // Only what fits in its area, none where it has no room.
        let fit = usize::from(self.area.height / self.line);
        let shown = self.lines.iter().skip(self.top).take(fit);
        for (i, range) in shown.enumerate() {
            let y = self.area.y + i as u16 * self.line;
            let at = Rect::new(self.area.x, y, self.area.width, self.line);
            let part = text
                .get(range.start as usize..range.end as usize)
                .unwrap_or_default();
            super::draw_label_in(
                canvas,
                &font,
                style,
                super::TextRole::BODY,
                style.text,
                f32::from(at.x),
                at,
                part,
                Color::BLACK,
            );
        }
        if self.lines.len() > self.rows() {
            let bar = (style.line.max(1) * 2) as u16;
            let x = (self.area.x + self.area.width).saturating_sub(bar);
            let whole = u32::from(self.area.height);
            let all = self.lines.len() as u32;
            let length = (whole * self.rows() as u32 / all).max(u32::from(bar) * 4) as u16;
            let y = self.area.y + (whole * self.top as u32 / all) as u16;
            canvas.fill(
                Rect::new(x, y, bar, length.min(self.area.height)),
                style.ink(Ink::Muted),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::STYLE;
    use paperpaw_platform::PointerTool;

    fn view(lines: usize) -> TextView {
        let ranges = (0..lines as u32).map(|i| i..i).collect();
        TextView::of_lines(Rect::new(0, 100, 300, 200), 20, ranges)
    }

    fn gesture(kind: GestureKind, y: i32) -> Gesture {
        Gesture::new(
            kind,
            Point::new(50, y),
            Point::new(50, y),
            PointerTool::Finger,
        )
    }

    #[test]
    fn a_text_view_scrolls_by_buttons_within_the_text() {
        let mut v = view(25);
        assert_eq!(v.rows(), 10);
        assert!(v.on_action(&actions::DOWN).changed());
        assert_eq!(v.top(), 3);
        assert!(v.on_action(&actions::NEXT_PAGE).changed());
        assert_eq!(v.top(), 13);
        // Not past the last page.
        assert!(v.on_action(&actions::NEXT_PAGE).changed());
        assert_eq!(v.top(), 15);
        assert_eq!(v.on_action(&actions::NEXT_PAGE), Response::Same);
        assert!(v.on_action(&actions::PREV_PAGE).changed());
        assert_eq!(v.top(), 5);
        assert!(!v.on_action(&actions::CONFIRM).taken());
        // Text that fits does not scroll.
        assert_eq!(view(5).on_action(&actions::DOWN), Response::Same);
    }

    #[test]
    fn a_text_view_scrolls_by_touch() {
        let mut v = view(25);
        assert!(
            v.on_gesture(&gesture(GestureKind::Swipe(Direction::Up), 150))
                .changed()
        );
        assert_eq!(v.top(), 10);
        assert!(v.on_gesture(&gesture(GestureKind::Tap, 120)).changed());
        assert_eq!(v.top(), 0);
        assert!(v.on_gesture(&gesture(GestureKind::Tap, 280)).changed());
        assert_eq!(v.top(), 10);
        // Outside its area: not its.
        assert!(!v.on_gesture(&gesture(GestureKind::Tap, 50)).taken());
    }

    #[test]
    fn a_text_field_keeps_its_title_whole_and_its_box_beside_or_below() {
        let margin = STYLE.margin as u16;
        let row = Rect::new(0, 40, 480, 60);
        // A short title: the box beside it takes the rest of the row.
        let (title, field, below) = text_field_layout(&STYLE, row, 100.0, 1, 20);
        assert!(!below);
        assert_eq!((title.x, title.width), (margin, 100));
        assert_eq!(field.x, margin + 100 + margin);
        assert_eq!(field.right(), u32::from(480 - margin));
        assert!(field.y >= row.y && field.bottom() <= row.bottom());
        // A long one (no room for a third beside it): the box below, both
        // as wide as the row less its margins.
        let (title, field, below) = text_field_layout(&STYLE, row, 400.0, 1, 20);
        assert!(below);
        assert_eq!(
            (title.width, field.width),
            (480 - 2 * margin, 480 - 2 * margin)
        );
        assert!(field.y >= title.bottom() as u16);
        // A wrapped one: all its lines above the box.
        let (title, field, _) = text_field_layout(&STYLE, row, 300.0, 3, 20);
        assert_eq!(title.height, 60);
        assert!(field.y >= title.bottom() as u16);
        let check = check_rect(&STYLE, row);
        assert_eq!(check.x, margin);
        assert_eq!(check.width, check.height);
        assert!(check.bottom() <= row.bottom());
    }

    #[test]
    fn editing_a_field_asks_for_text_with_its_value() {
        const NAMED: Event<Typed> = Event::new("demo.named");
        let command = edit_text("Name", "Ada", &NAMED);
        assert!(
            matches!(command, Command::OpenSlot(..) | Command::OpenScreen(..)),
            "{command:?}"
        );
    }
}
