//! Placing parts without computing positions by hand: a column of parts
//! one below the other ([`Stack`]), equal columns side by side
//! ([`columns`]), a grid of cells the focus moves through ([`CellGrid`]), a
//! group on a card or in a frame ([`Ui::card`](super::Ui::card)) and a row of segments,
//! one of them chosen, e.g. tabs over a page ([`Segments`]).
//!
//! ```
//! use paperpaw_sdk::ui::{Stack, columns};
//! use paperpaw_platform::Rect;
//!
//! let mut stack = Stack::new(Rect::new(0, 0, 480, 800), 12);
//! let title = stack.take(40);
//! let buttons = stack.take_bottom(60);
//! let text = stack.rest();
//! let [ok, cancel] = columns(buttons, 2, 12)[..] else { unreachable!() };
//! assert_eq!((title.y, text.y, buttons.y), (0, 52, 740));
//! assert_eq!((ok.width, cancel.x, cancel.right()), (234, 246, 480));
//! ```

use alloc::borrow::Cow;
use alloc::vec::Vec;

use paperpaw_graphics::Canvas;
use paperpaw_platform::{Color, Point, Rect};

use crate::Id;
use crate::builtin::actions;
use crate::env::Env;
use crate::widgets::Frame;

use super::{Ink, UiStyle};

/// `count` columns of `area` side by side, `gap` apart, as wide as each
/// other to a pixel (what dividing leaves goes a pixel each to the last
/// ones), so the last ends where `area` does. None for no columns.
pub fn columns(area: Rect, count: usize, gap: u16) -> Vec<Rect> {
    let Ok(n) = u16::try_from(count) else {
        return Vec::new();
    };
    if n == 0 {
        return Vec::new();
    }
    let room = area.width.saturating_sub(gap.saturating_mul(n - 1));
    let (width, left) = (room / n, room % n);
    let mut x = area.x;
    (0..n)
        .map(|i| {
            let w = width + u16::from(i >= n - left);
            let cell = Rect::new(x, area.y, w, area.height);
            x += w + gap;
            cell
        })
        .collect()
}

/// Buttons with `labels` in `area`, from its top, every word whole: side
/// by side in equal columns where each word fits its column
/// ([`button_width`](super::button_width)), else in rows of fewer, down to
/// one a row (a word too long even for that on more lines, as high as
/// [`button_height_for`](super::button_height_for) says); `gap` apart.
/// Their cells, in the order of `labels`; [`button_cells_height`] says
/// how high they are.
pub fn button_cells(
    env: &Env<'_>,
    style: &UiStyle,
    area: Rect,
    labels: &[&str],
    gap: u16,
) -> Vec<Rect> {
    let per_row = buttons_per_row(env, style, area.width, labels, gap);
    let mut cells = Vec::with_capacity(labels.len());
    let mut y = area.y;
    for row in labels.chunks(per_row) {
        let line = Rect::new(area.x, y, area.width, 0);
        let mut row_cells = columns(line, per_row, gap);
        row_cells.truncate(row.len());
        let height = row_cells
            .iter()
            .zip(row)
            .map(|(cell, label)| super::button_height_for(env, style, cell.width, label))
            .max()
            .unwrap_or(0);
        cells.extend(row_cells.into_iter().map(|c| Rect { height, ..c }));
        y += height + gap;
    }
    cells
}

/// How high [`button_cells`] lays out buttons with `labels` in an area
/// `width` wide.
pub fn button_cells_height(
    env: &Env<'_>,
    style: &UiStyle,
    width: u16,
    labels: &[&str],
    gap: u16,
) -> u16 {
    let area = Rect::new(0, 0, width, 0);
    button_cells(env, style, area, labels, gap)
        .last()
        .map_or(0, |cell| cell.y + cell.height)
}

/// How many of the buttons with `labels` go side by side in a row `width`
/// wide, `gap` apart, each word whole: all where they fit, else as many
/// as rows of even length take (half of them, a third, ...), down to one.
fn buttons_per_row(env: &Env<'_>, style: &UiStyle, width: u16, labels: &[&str], gap: u16) -> usize {
    let count = labels.len().max(1);
    let Some(font) = style.font(env) else {
        return count;
    };
    let fits = |n: usize| {
        let room = width.saturating_sub(gap.saturating_mul(n as u16 - 1)) / n as u16;
        labels.iter().all(|label| {
            let size = if label.chars().count() <= 1 {
                style.text
            } else {
                style.small
            };
            super::button_width(style, font.measure(size, label)) <= f32::from(room)
        })
    };
    (1..=count)
        .map(|rows| count.div_ceil(rows))
        .find(|&n| fits(n))
        .unwrap_or(1)
}

/// A column of parts, `gap` apart: taken from the top down and from the
/// bottom up, the rest between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stack {
    area: Rect,
    gap: u16,
    /// Where the next part from the top goes.
    top: u16,
    /// Where the last part from the bottom begins.
    bottom: u16,
    /// Whether parts were taken from either end (a gap comes before the
    /// next).
    taken_top: bool,
    taken_bottom: bool,
}

impl Stack {
    /// The parts of `area`, `gap` apart.
    pub fn new(area: Rect, gap: u16) -> Self {
        Self {
            area,
            gap,
            top: area.y,
            bottom: area.y + area.height,
            taken_top: false,
            taken_bottom: false,
        }
    }

    /// The next part from the top, `height` tall (at most what is left),
    /// as wide as the area.
    pub fn take(&mut self, height: u16) -> Rect {
        if self.taken_top {
            self.top = (self.top + self.gap).min(self.bottom);
        }
        let height = height.min(self.bottom - self.top);
        let part = Rect::new(self.area.x, self.top, self.area.width, height);
        self.top += height;
        self.taken_top = true;
        part
    }

    /// The next part from the bottom, `height` tall (at most what is
    /// left), above those taken there.
    pub fn take_bottom(&mut self, height: u16) -> Rect {
        if self.taken_bottom {
            self.bottom = self.bottom.saturating_sub(self.gap).max(self.top);
        }
        let height = height.min(self.bottom - self.top);
        self.bottom -= height;
        self.taken_bottom = true;
        Rect::new(self.area.x, self.bottom, self.area.width, height)
    }

    /// What is left between the parts taken, a gap from each.
    pub fn rest(&self) -> Rect {
        let top = if self.taken_top {
            (self.top + self.gap).min(self.bottom)
        } else {
            self.top
        };
        let bottom = if self.taken_bottom {
            self.bottom.saturating_sub(self.gap).max(top)
        } else {
            self.bottom
        };
        Rect::new(self.area.x, top, self.area.width, bottom - top)
    }
}

/// A grid of cells, `columns` across, rows `cell_height` tall, `gap`
/// apart: where each cell of a page goes, which one a tap hits, where the
/// focus goes with the buttons (left and right along the cells, up and
/// down a row).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellGrid {
    area: Rect,
    columns: u16,
    cell_height: u16,
    gap: u16,
}

impl CellGrid {
    /// The grid in `area`; at least one column, cells at least a pixel
    /// tall.
    pub fn new(area: Rect, columns: u16, cell_height: u16, gap: u16) -> Self {
        Self {
            area,
            columns: columns.max(1),
            cell_height: cell_height.max(1),
            gap,
        }
    }

    /// The rows that fit (at least one).
    pub fn rows(&self) -> usize {
        usize::from((self.area.height + self.gap) / (self.cell_height + self.gap)).max(1)
    }

    /// The cells a page holds.
    pub fn per_page(&self) -> usize {
        self.rows() * usize::from(self.columns)
    }

    /// Where cell `index` (of all, on any page) goes on its page.
    pub fn cell(&self, index: usize) -> Rect {
        let on_page = index % self.per_page();
        let columns = usize::from(self.columns);
        let column = columns_of(self.area, self.columns, self.gap)[on_page % columns];
        let row = (on_page / columns) as u16;
        Rect::new(
            column.x,
            self.area.y + row * (self.cell_height + self.gap),
            column.width,
            self.cell_height,
        )
    }

    /// The cell of page `page` at `point`, of `count` cells; none between
    /// cells or past the last.
    pub fn hit(&self, point: Point, page: usize, count: usize) -> Option<usize> {
        let first = page * self.per_page();
        (first..count.min(first + self.per_page())).find(|i| self.cell(*i).contains(point))
    }

    /// Where the focus goes from `focus` of `count` cells with `action`:
    /// left and right (and previous and next) one cell, up and down one
    /// row, staying in the grid. `None` for other actions.
    pub fn step(&self, focus: usize, action: &Id, count: usize) -> Option<usize> {
        let last = count.checked_sub(1)?;
        let row = usize::from(self.columns);
        let to = if [actions::LEFT, actions::PREVIOUS].contains(action) {
            focus.saturating_sub(1)
        } else if [actions::RIGHT, actions::NEXT].contains(action) {
            focus + 1
        } else if *action == actions::UP {
            focus.checked_sub(row).unwrap_or(focus)
        } else if *action == actions::DOWN {
            if focus + row <= last {
                focus + row
            } else {
                focus
            }
        } else {
            return None;
        };
        Some(to.min(last))
    }

    /// The page cell `index` is on.
    pub fn page_of(&self, index: usize) -> usize {
        index / self.per_page()
    }
}

/// [`columns`] for a grid: the same, never empty.
fn columns_of(area: Rect, count: u16, gap: u16) -> Vec<Rect> {
    columns(area, usize::from(count.max(1)), gap)
}

/// Draws a card or frame round `area` (`frame`, as widgets have them:
/// a line, a light card, a dark card, nothing), as the look in use
/// paints frames ([`look::Painters::frame`](super::look::Painters)), and
/// returns where its parts go: the style's margin in from its edges
/// (nothing in for [`Frame::None`]). On a dark card, draw in white.
pub(crate) fn draw_card(
    canvas: &mut Canvas<'_>,
    style: &UiStyle,
    area: Rect,
    frame: Frame,
) -> Rect {
    paint_frame(canvas, style, area, frame);
    card_inside(style, area, frame)
}

/// Where the parts of a card round `area` go ([`Ui::card`](super::Ui::card)).
pub fn card_inside(style: &UiStyle, area: Rect, frame: Frame) -> Rect {
    if frame == Frame::None {
        return area;
    }
    let margin = style.margin as u16;
    Rect::new(
        area.x + margin,
        area.y + margin,
        area.width.saturating_sub(2 * margin),
        area.height.saturating_sub(2 * margin),
    )
}

/// Paints `frame` round `area`: the look's painter, else the usual.
pub(crate) fn paint_frame(canvas: &mut Canvas<'_>, style: &UiStyle, area: Rect, frame: Frame) {
    let part = super::look::FramePart::new(area, frame);
    if frame != Frame::None && style.look.frame(canvas, style, part) {
        return;
    }
    match frame {
        Frame::None => {}
        Frame::Line => super::stroke_round(
            canvas,
            style,
            area,
            super::button_border(style),
            Color::BLACK,
        ),
        Frame::Card if style.grays <= 2 => {
            // A light gray would be a heavy pattern on black and white:
            // the sparsest there is, in a thin line as cards in lists.
            let card = style.ink(Ink::CARD);
            let sparser = |c: u8| c + (255 - c) / 2;
            let pattern = Color::rgb(sparser(card.r), sparser(card.g), sparser(card.b));
            canvas.fill_round_shaded(area, style.corner, pattern);
            let line = (style.line / 2).max(1);
            canvas.stroke_round(area, line, style.corner, Color::BLACK);
        }
        Frame::Card => canvas.fill_round_shaded(area, style.corner, style.ink(Ink::CARD)),
        Frame::Dark => super::fill_round(canvas, style, area, Color::BLACK),
    }
}

/// A row of segments side by side, one of them chosen: a few options to
/// pick from at a glance, or tabs over a page. Each segment is a button
/// ([`Ui::button`](super::Ui::button)), the chosen one on; with the
/// buttons, left and right move the focus, *Select* chooses. They keep
/// their labels, translation keys (or text that is none) translated where
/// they are measured and drawn, as [`Chips`](super::Chips) do: drawing and
/// hitting allocate nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segments {
    cells: Vec<Rect>,
    labels: Vec<Cow<'static, str>>,
}

impl Segments {
    /// Segments with `labels` in `area`, of equal width, the style's gap
    /// apart.
    pub fn new<L: Into<Cow<'static, str>>>(
        style: &UiStyle,
        area: Rect,
        labels: impl IntoIterator<Item = L>,
    ) -> Self {
        let labels = keep(labels);
        Self {
            cells: columns(area, labels.len(), style.gap as u16),
            labels,
        }
    }

    /// Segments with `labels` in `area`, from its top, every word whole:
    /// side by side where each fits its share, else in more rows
    /// ([`button_cells`]); as high as [`Segments::height_for`] says.
    pub fn fitted<L: Into<Cow<'static, str>>>(
        env: &Env<'_>,
        style: &UiStyle,
        area: Rect,
        labels: impl IntoIterator<Item = L>,
    ) -> Self {
        let labels = keep(labels);
        let shown: Vec<&str> = labels.iter().map(|l| env.i18n().tr(l)).collect();
        Self {
            cells: button_cells(env, style, area, &shown, style.gap as u16),
            labels,
        }
    }

    /// The height [`Segments::fitted`] takes for `labels` in an area
    /// `width` wide: [`Segments::height`] where they fit side by side.
    pub fn height_for(env: &Env<'_>, style: &UiStyle, width: u16, labels: &[&str]) -> u16 {
        let shown: Vec<&str> = labels.iter().map(|l| env.i18n().tr(l)).collect();
        button_cells_height(env, style, width, &shown, style.gap as u16)
    }

    /// The height a row of segments takes: a button's in a row that can
    /// be touched.
    pub fn height(style: &UiStyle) -> u16 {
        let row = style.touch_row(style.control_row) as u16;
        super::button_height(style, Rect::new(0, 0, 0, row))
    }

    /// Where segment `index` is.
    pub fn cell(&self, index: usize) -> Option<Rect> {
        self.cells.get(index).copied()
    }

    /// The segment at `point`.
    pub fn hit(&self, point: Point) -> Option<usize> {
        self.cells.iter().position(|c| c.contains(point))
    }

    /// Where the focus goes from `focus` with `action`: left and right
    /// (and previous and next), staying in the row. `None` for other
    /// actions.
    pub fn step(&self, focus: usize, action: &Id) -> Option<usize> {
        let last = self.cells.len().checked_sub(1)?;
        if [actions::LEFT, actions::PREVIOUS].contains(action) {
            Some(focus.saturating_sub(1))
        } else if [actions::RIGHT, actions::NEXT].contains(action) {
            Some((focus + 1).min(last))
        } else {
            None
        }
    }

    /// The label of segment `index`, as given (a translation key).
    pub fn label(&self, index: usize) -> Option<&str> {
        self.labels.get(index).map(|label| &**label)
    }

    /// Draws the segments, `chosen` on and `focus` (where the focus is on
    /// them) focused.
    pub fn draw(&self, ui: &mut super::Ui<'_>, chosen: usize, focus: Option<usize>) {
        let (env, style) = (ui.env(), ui.style());
        let canvas = ui.canvas();
        for (i, (cell, label)) in self.cells.iter().zip(&self.labels).enumerate() {
            super::draw_button(
                canvas,
                env,
                style,
                *cell,
                env.i18n().tr(label),
                i == chosen,
                focus == Some(i),
            );
        }
    }
}

/// `labels` as the parts keep them: keys without a copy.
fn keep<L: Into<Cow<'static, str>>>(labels: impl IntoIterator<Item = L>) -> Vec<Cow<'static, str>> {
    labels.into_iter().map(Into::into).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::STYLE;

    #[test]
    fn columns_share_the_width_and_end_with_the_area() {
        let cells = columns(Rect::new(10, 5, 103, 20), 3, 4);
        assert_eq!(cells.len(), 3);
        assert_eq!((cells[0].x, cells[0].width), (10, 31));
        assert_eq!((cells[1].x, cells[1].width), (45, 32));
        assert_eq!(
            (cells[2].x, cells[2].width, cells[2].right()),
            (81, 32, 113)
        );
        assert!(columns(Rect::new(0, 0, 10, 10), 0, 2).is_empty());
    }

    #[test]
    fn a_stack_takes_from_both_ends_a_gap_apart() {
        let mut stack = Stack::new(Rect::new(0, 100, 50, 200), 10);
        assert_eq!(stack.rest(), Rect::new(0, 100, 50, 200));
        assert_eq!(stack.take(30), Rect::new(0, 100, 50, 30));
        assert_eq!(stack.take(20), Rect::new(0, 140, 50, 20));
        assert_eq!(stack.take_bottom(40), Rect::new(0, 260, 50, 40));
        assert_eq!(stack.rest(), Rect::new(0, 170, 50, 80));
        // More than is left: what is left.
        assert_eq!(stack.take(500).height, 90);
        assert_eq!(stack.rest().height, 0);
    }

    #[test]
    fn a_grid_places_hits_and_steps() {
        let grid = CellGrid::new(Rect::new(0, 0, 310, 220), 3, 100, 5);
        assert_eq!((grid.rows(), grid.per_page()), (2, 6));
        assert_eq!(grid.cell(4), Rect::new(105, 105, 100, 100));
        // Page 1 begins anew at the top.
        assert_eq!(grid.cell(6), Rect::new(0, 0, 100, 100));
        assert_eq!(grid.hit(Point::new(150, 150), 0, 10), Some(4));
        assert_eq!(grid.hit(Point::new(150, 150), 1, 10), None);
        assert_eq!(grid.hit(Point::new(102, 50), 0, 10), None);
        assert_eq!(grid.step(1, &actions::DOWN, 10), Some(4));
        assert_eq!(grid.step(8, &actions::DOWN, 10), Some(8));
        assert_eq!(grid.step(4, &actions::UP, 10), Some(1));
        assert_eq!(grid.step(9, &actions::RIGHT, 10), Some(9));
        assert_eq!(grid.step(0, &actions::LEFT, 10), Some(0));
        assert_eq!(grid.step(0, &actions::CONFIRM, 10), None);
        assert_eq!(grid.page_of(7), 1);
    }

    #[test]
    fn segments_hit_and_step_within_their_row() {
        let segments = Segments::new(&STYLE, Rect::new(0, 0, 300, 40), ["a", "b", "c"]);
        let first = segments.cell(0).unwrap();
        assert_eq!(
            segments.hit(Point::new(i32::from(first.x) + 1, 10)),
            Some(0)
        );
        assert_eq!(segments.hit(Point::new(299, 10)), Some(2));
        assert_eq!(segments.step(2, &actions::RIGHT), Some(2));
        assert_eq!(segments.step(1, &actions::LEFT), Some(0));
        assert_eq!(segments.step(1, &actions::UP), None);
    }

    #[test]
    fn a_card_keeps_its_parts_a_margin_in() {
        let area = Rect::new(10, 10, 200, 100);
        let margin = STYLE.margin as u16;
        assert_eq!(card_inside(&STYLE, area, Frame::None), area);
        assert_eq!(
            card_inside(&STYLE, area, Frame::Line),
            Rect::new(10 + margin, 10 + margin, 200 - 2 * margin, 100 - 2 * margin)
        );
    }
}
