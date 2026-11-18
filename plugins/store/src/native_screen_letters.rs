//! *Go to letter* (interface 6: `letters.rs`): the letters A to Z in a grid of
//! five columns; the one chosen closes the screen and goes to the list that
//! opened it ([`KEY`]), which shows its plugins from that letter on. A group of
//! thousands of plugins is then a few steps away with the buttons, and a tap
//! with a finger.
//!
//! The grid is plain arithmetic ([`Grid`], [`step`]), tested without a host;
//! the screen keeps only the grid and the header's tap regions of the last
//! frame and its focus.
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error, Host,
    callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind},
    commands::{CommandSender, RedrawMode},
    fonts::{Font, FontRole},
    snapshot,
};
use paperpaw_sdk::{Handled, Point, Rect};
use paperpaw_ui_kit::style::flip_rect;
use paperpaw_ui_kit::{Messages, Page, Paint, Style, header, paint, text};

use crate::native_screens::{Ctx, Next, Screen};

/// What the screen hands to the list below it: the letter, as text.
pub const KEY: &str = "letter";
/// Letters side by side.
pub const COLUMNS: usize = 5;
/// The letters, from A to Z.
pub const LETTERS: [&str; 26] = [
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S",
    "T", "U", "V", "W", "X", "Y", "Z",
];

/// Letter `index` of A to Z.
#[must_use]
pub fn letter(index: usize) -> Option<&'static str> {
    LETTERS.get(index).copied()
}

/// What choosing letter `index` does: the screen closes and the letter goes
/// to the list below.
///
/// # Errors
/// `NoMemory`.
pub fn choose(index: usize) -> Result<Next, Error> {
    let Some(letter) = letter(index) else {
        return Ok(Next::None);
    };
    Ok(Next::CloseWith {
        key: KEY,
        value: text::copy(letter)?,
    })
}

/// How far an action moves the focus: a letter for left and right (and
/// previous and next), a row for up and down (and the page actions); `None` for
/// the actions that are not the grid's.
#[must_use]
pub fn delta(action: &str) -> Option<isize> {
    let columns = COLUMNS as isize;
    match action {
        "core.left" | "core.previous" => Some(-1),
        "core.right" | "core.next" => Some(1),
        "core.up" | "core.prev-page" => Some(-columns),
        "core.down" | "core.next-page" => Some(columns),
        _ => None,
    }
}

/// Where the focus goes from `focus` by `delta` among the letters: one letter
/// past either end goes round to the other, a row stops at the end (interface
/// 6's `ListView::move_focus`).
#[must_use]
pub fn step(focus: usize, delta: isize) -> usize {
    let Some(last) = LETTERS.len().checked_sub(1) else {
        return 0;
    };
    let target = isize::try_from(focus)
        .unwrap_or(isize::MAX)
        .saturating_add(delta);
    let last = last as isize;
    let to = if delta.abs() == 1 && target < 0 {
        last
    } else if delta.abs() == 1 && target > last {
        0
    } else {
        target.clamp(0, last)
    };
    usize::try_from(to).unwrap_or(0)
}

/// Where the letters are: as high as the screen leaves room for, no higher than
/// wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    area: Rect,
    row: u16,
    rtl: bool,
}

impl Grid {
    /// The grid on a display `width` by `height`, below `top` (the header and
    /// a gap).
    #[must_use]
    pub fn new(width: u16, height: u16, top: u16, margin: u16, gap: u16, rtl: bool) -> Self {
        let rows = LETTERS.len().div_ceil(COLUMNS) as u32;
        let room = u32::from(height.saturating_sub(top.saturating_add(gap)));
        let wide = u32::from(width.saturating_sub(margin.saturating_mul(2)));
        let row = (room / rows).min(wide / COLUMNS as u32).max(1);
        let area = Rect::new(
            margin,
            top,
            u16::try_from(wide).unwrap_or(u16::MAX),
            u16::try_from(row * rows).unwrap_or(u16::MAX),
        );
        Self {
            area,
            row: u16::try_from(row).unwrap_or(u16::MAX),
            rtl,
        }
    }

    /// Where letter `index` is.
    #[must_use]
    pub fn cell(&self, index: usize) -> Option<Rect> {
        if index >= LETTERS.len() {
            return None;
        }
        let width = self.area.width / COLUMNS as u16;
        let (line, column) = (index / COLUMNS, index % COLUMNS);
        let rect = Rect::new(
            self.area
                .x
                .saturating_add(width.saturating_mul(column as u16)),
            self.area
                .y
                .saturating_add(self.row.saturating_mul(line as u16)),
            width,
            self.row,
        );
        Some(flip_rect(self.rtl, self.area, rect))
    }

    /// The letter at `point`.
    #[must_use]
    pub fn hit(&self, point: Point) -> Option<usize> {
        (0..LETTERS.len()).find(|&i| self.cell(i).is_some_and(|cell| cell.contains(point)))
    }
}

/// What the last frame drew: where to tap.
#[derive(Debug)]
struct Layout {
    header: Page,
    grid: Grid,
}

/// The screen.
#[derive(Debug, Default)]
pub struct View {
    focus: usize,
    layout: Option<Layout>,
}

fn redraw(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
}

impl View {
    /// What an action does.
    fn action(&mut self, host: &Host<'_>, action: &str) -> Result<(Handled, Next), Error> {
        if action == "core.confirm" {
            return Ok((Handled::Yes, choose(self.focus)?));
        }
        let Some(delta) = delta(action) else {
            return Ok((Handled::No, Next::None));
        };
        let to = step(self.focus, delta);
        if to != self.focus {
            self.focus = to;
            redraw(host)?;
        }
        Ok((Handled::Yes, Next::None))
    }

    fn tap(&self, host: &Host<'_>, position: Point) -> Result<(Handled, Next), Error> {
        let Some(layout) = &self.layout else {
            return Ok((Handled::No, Next::None));
        };
        if let Some(action) = layout.header.action(position) {
            let mut scratch = [0; 128];
            CommandSender::new(host, &mut scratch).run_action(action)?;
            return Ok((Handled::Yes, Next::None));
        }
        match layout.grid.hit(position) {
            Some(index) => Ok((Handled::Yes, choose(index)?)),
            None => Ok((Handled::No, Next::None)),
        }
    }

    fn render(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        font: &Font<'_>,
    ) -> Result<(), Error> {
        let display = snapshot::capture_snapshot(host)?;
        let style = Style::read(host, &display, font)?;
        let (width, height) = (display.display_width(), display.display_height());
        let top = style.header.saturating_add(style.gap);
        let mut messages = Messages::new()?;
        let title = messages.copy(host, "store.letter")?;
        let mut page = Page::default();
        header::prepare(
            &mut page,
            host,
            font,
            &style,
            &mut messages,
            &title,
            width,
            display.has_touch(),
            0,
            1,
        )?;
        let grid = Grid::new(width, height, top, style.margin, style.gap, style.rtl);
        let mut body: Vec<Paint> = Vec::new();
        paint::push(
            &mut body,
            Paint::Fill(
                Rect::new(0, top, width, height.saturating_sub(top)),
                style.paper,
            ),
        )?;
        let size = style.title * 1.2;
        for (index, letter) in LETTERS.iter().enumerate() {
            let Some(cell) = grid.cell(index) else {
                continue;
            };
            let focused = index == self.focus;
            if focused {
                paint::push(&mut body, Paint::Fill(cell, style.ink))?;
            }
            let ink = if focused { style.paper } else { style.ink };
            let wide = font.measure_text(size, letter)?;
            let x = f32::from(cell.x) + (f32::from(cell.width) - wide).max(0.0) / 2.0;
            text::label(&mut body, font, letter, size, x, cell, ink, Some(cell))?;
        }
        paint::replay(frame, font, page.paint.iter().chain(body.iter()))?;
        page.paint = Vec::new();
        self.layout = Some(Layout { header: page, grid });
        Ok(())
    }
}

impl Screen for View {
    fn open(_: &Ctx<'_>, _: &[(&str, &str)]) -> Result<Self, Error> {
        Ok(Self::default())
    }

    fn draw(&mut self, ctx: &Ctx<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let font = Font::acquire_role(ctx.host, FontRole::Ui)?;
        let drawn = self.render(ctx.host, frame, &font);
        let released = font.release();
        drawn.and(released)
    }

    fn input(&mut self, ctx: &Ctx<'_>, input: &Input<'_>) -> Result<(Handled, Next), Error> {
        match *input {
            Input::Action { action, .. } => self.action(ctx.host, action),
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => self.tap(ctx.host, position),
            _ => Ok((Handled::No, Next::None)),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test assertions")]
    use super::*;

    #[test]
    fn the_letters_run_from_a_to_z() {
        assert_eq!(LETTERS.len(), 26);
        assert_eq!(letter(0), Some("A"));
        assert_eq!(letter(25), Some("Z"));
        assert_eq!(letter(26), None);
        for (index, letter) in LETTERS.iter().enumerate() {
            assert_eq!(letter.as_bytes(), [b'A' + index as u8]);
        }
    }

    #[test]
    fn a_letter_chosen_goes_to_the_list_below() {
        assert_eq!(
            choose(2).unwrap(),
            Next::CloseWith {
                key: "letter",
                value: alloc::string::String::from("C")
            }
        );
        assert_eq!(choose(26).unwrap(), Next::None);
    }

    #[test]
    fn the_actions_move_by_a_letter_or_a_row() {
        for (action, by) in [
            ("core.left", -1),
            ("core.previous", -1),
            ("core.right", 1),
            ("core.next", 1),
            ("core.up", -5),
            ("core.prev-page", -5),
            ("core.down", 5),
            ("core.next-page", 5),
        ] {
            assert_eq!(delta(action), Some(by), "{action}");
        }
        assert_eq!(delta("core.confirm"), None);
        assert_eq!(delta("core.back"), None);
    }

    #[test]
    fn the_focus_moves_as_interface_6_moved_it() {
        for focus in 0..26 {
            for by in [-5, -1, 1, 5] {
                assert_eq!(
                    step(focus, by),
                    paperpaw_sdk::ui::step_focus(focus, by, 26),
                    "{focus} by {by}"
                );
            }
        }
        // A letter past an end goes round, a row stops there.
        assert_eq!((step(0, -1), step(25, 1)), (25, 0));
        assert_eq!((step(2, -5), step(24, 5)), (0, 25));
    }

    #[test]
    fn the_grid_is_five_columns_as_high_as_the_screen_allows() {
        // The X4: 480 by 800, a header of 60 and a gap of 8.
        let grid = Grid::new(480, 800, 68, 16, 8, false);
        let (a, b, f, z) = (
            grid.cell(0).unwrap(),
            grid.cell(1).unwrap(),
            grid.cell(5).unwrap(),
            grid.cell(25).unwrap(),
        );
        assert_eq!(a.x, 16);
        assert_eq!(a.width, (480 - 32) / 5);
        assert_eq!(b.x, a.x + a.width);
        assert_eq!((f.x, f.y), (a.x, a.y + a.height));
        // No higher than wide, and every letter is on the screen.
        assert!(a.height <= a.width);
        assert!(z.y + z.height <= 800);
        assert_eq!(grid.cell(26), None);
        // A tap meets the letter drawn there, and nothing outside.
        for i in 0..26 {
            let cell = grid.cell(i).unwrap();
            let middle = Point::new(
                i32::from(cell.x) + i32::from(cell.width) / 2,
                i32::from(cell.y) + i32::from(cell.height) / 2,
            );
            assert_eq!(grid.hit(middle), Some(i));
        }
        assert_eq!(grid.hit(Point::new(2, 2)), None);
        // A grid where the interface runs right to left is the mirror image.
        let mirrored = Grid::new(480, 800, 68, 16, 8, true);
        assert_eq!(mirrored.cell(0).unwrap().x, 16 + 448 - a.width);
        assert_eq!(mirrored.cell(0).unwrap().y, a.y);
    }

    #[test]
    fn a_tiny_screen_still_has_cells() {
        let grid = Grid::new(10, 10, 20, 8, 2, false);
        assert!(grid.cell(25).is_some());
        assert!(grid.cell(0).unwrap().height >= 1);
    }
}
