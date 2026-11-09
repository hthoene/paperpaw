//! The list of books the library shows: its cells (the filter row, a note,
//! the way to earlier and later books, the books), where each goes on a page
//! (a list with covers, or a grid of them), and what a button or a tap on one
//! means. Free of host calls; `native_draw` paints it.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Point, Rect,
    native::{
        Error, Host,
        callbacks::{Input, InputGestureKind},
        commands::{CommandSender, RedrawMode},
    },
};
use paperpaw_ui_kit::{Page, mem::push};

/// What a cell of the list is, and so what choosing it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Row {
    /// Steps the filter.
    Filter,
    /// Says what the library is up to (the books found so far); not chosen.
    Note,
    /// Opens the book at this index of the window.
    Book(usize),
    /// Slides back to the window before this one.
    Previous,
    /// Reads more books: the window grows, or slides on when it is full.
    More,
    /// A labelled value of the book information; chosen, it does nothing.
    Field,
}

/// One cell of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Cell {
    pub row: Row,
    /// A book's title, or what the row says.
    pub title: String,
    /// At the right: how far a book is read, the filter, the books it counts.
    pub value: String,
    /// Below the title: author, series, badges.
    pub note: String,
    /// How far a book is read, 1 to 99: a bar under it.
    pub progress: Option<u8>,
    /// The book read last, shown larger with a word above it.
    pub card: bool,
    /// The badges plugins put on the book, as chips.
    pub chips: Vec<String>,
}

impl Cell {
    /// A cell that is a row of words only.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn plain(row: Row, title: &str, value: &str) -> Result<Self, Error> {
        Ok(Self {
            row,
            title: paperpaw_ui_kit::text::copy(title)?,
            value: paperpaw_ui_kit::text::copy(value)?,
            note: String::new(),
            progress: None,
            card: false,
            chips: Vec::new(),
        })
    }

    /// Whether the focus may rest on it.
    #[must_use]
    pub const fn selectable(&self) -> bool {
        !matches!(self.row, Row::Note)
    }

    const fn is_book(&self) -> bool {
        matches!(self.row, Row::Book(_)) && !self.card
    }
}

/// What the list is laid out with, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metrics {
    /// The area below the header.
    pub area: Rect,
    /// The height of a row of words.
    pub row: u16,
    pub gap: u16,
    pub margin: u16,
    /// Books side by side in a grid (1 for a list).
    pub columns: u16,
}

impl Metrics {
    /// Columns of a grid on a display `width` wide.
    #[must_use]
    pub const fn columns_for(width: u16) -> u16 {
        if width >= 900 {
            4
        } else if width >= 600 {
            3
        } else {
            2
        }
    }

    /// A book's row in a list.
    #[must_use]
    pub const fn book_height(&self) -> u16 {
        self.row + self.row / 2 + self.gap
    }

    /// The last-read card.
    #[must_use]
    pub const fn card_height(&self) -> u16 {
        2 * self.row + self.row / 2 + self.gap
    }

    /// The width of a tile of the grid.
    #[must_use]
    pub fn tile_width(&self) -> u16 {
        let columns = self.columns.max(1);
        let room = self
            .area
            .width
            .saturating_sub(2 * self.margin)
            .saturating_sub(self.gap.saturating_mul(columns - 1));
        room / columns
    }

    /// The cover of a tile: 3 to 4, and not taller than a page lets two be.
    #[must_use]
    pub fn tile_cover(&self) -> u16 {
        let wide = self.tile_width();
        let tall = u32::from(wide) * 4 / 3;
        let most = u32::from(self.area.height) * 3 / 5;
        tall.min(most).max(1).try_into().unwrap_or(1)
    }

    /// A tile: its cover and a line for the title.
    #[must_use]
    pub fn tile_height(&self) -> u16 {
        self.tile_cover()
            .saturating_add(self.row)
            .saturating_add(self.gap)
    }
}

/// A cell and where it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    pub cell: usize,
    pub rect: Rect,
}

/// Where the cells go, page by page: one under the other in a list; the
/// books side by side in a grid, its other cells across the page.
///
/// # Errors
/// `NoMemory`.
pub fn pages(cells: &[Cell], metrics: &Metrics, grid: bool) -> Result<Vec<Vec<Placed>>, Error> {
    let mut pages: Vec<Vec<Placed>> = Vec::new();
    push(&mut pages, Vec::new())?;
    let area = metrics.area;
    let columns = if grid { metrics.columns.max(1) } else { 1 };
    let mut y = 0u32;
    let mut column = 0u16;
    for (index, cell) in cells.iter().enumerate() {
        let tiled = grid && cell.is_book();
        let high = if tiled {
            metrics.tile_height()
        } else if cell.card {
            metrics.card_height()
        } else if matches!(cell.row, Row::Book(_)) {
            metrics.book_height()
        } else {
            metrics.row
        };
        if !tiled && column > 0 {
            // A grid row that is not full ends where a wider cell begins.
            y += u32::from(metrics.tile_height());
            column = 0;
        }
        let empty = pages.last().is_none_or(Vec::is_empty);
        if column == 0 && y + u32::from(high) > u32::from(area.height) && !empty {
            push(&mut pages, Vec::new())?;
            y = 0;
        }
        let (x, width) = if tiled {
            let wide = metrics.tile_width();
            let at = metrics
                .margin
                .saturating_add(column.saturating_mul(wide.saturating_add(metrics.gap)));
            (at, wide)
        } else {
            (0, area.width)
        };
        let rect = Rect::new(
            area.x.saturating_add(x),
            area.y.saturating_add(u16::try_from(y).unwrap_or(u16::MAX)),
            width,
            high,
        );
        if let Some(page) = pages.last_mut() {
            push(page, Placed { cell: index, rect })?;
        }
        if tiled {
            column += 1;
            if column >= columns {
                y += u32::from(high);
                column = 0;
            }
        } else {
            y += u32::from(high);
        }
    }
    Ok(pages)
}

/// What an input means to the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ListStep {
    /// The focus goes to this cell.
    Focus(usize),
    /// This cell was chosen (confirm, or a tap).
    Chosen(usize),
    /// This cell was held (a long press, or the menu button on it).
    Held(usize),
    /// The header's action (back, a page).
    Header(&'static str),
    /// The title was tapped: the library's menu.
    Menu,
    Ignored,
}

/// The list of books: cells, the focus, and what the last frame drew.
#[derive(Debug, Default)]
pub struct BookList {
    cells: Vec<Cell>,
    focus: usize,
    grid: bool,
    /// What the last frame was laid out with.
    pub(crate) metrics: Option<Metrics>,
    /// The first cell of each page of the last frame.
    pub(crate) starts: Vec<usize>,
    pub(crate) hits: Vec<(Rect, usize)>,
    pub(crate) header: Page,
    /// The header's band, where a tap on the title is the menu.
    pub(crate) band: Option<Rect>,
}

impl BookList {
    /// A list with the focus on its first cell the focus may rest on.
    #[must_use]
    pub fn new(cells: Vec<Cell>, grid: bool) -> Self {
        let mut list = Self {
            cells,
            grid,
            ..Self::default()
        };
        list.focus = list.cells.iter().position(Cell::selectable).unwrap_or(0);
        list
    }

    #[must_use]
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    #[must_use]
    pub const fn focus(&self) -> usize {
        self.focus
    }

    /// Whether the books are tiles of a grid.
    #[must_use]
    pub const fn grid(&self) -> bool {
        self.grid
    }

    /// The focus on `index`, or the last cell when there is none so far.
    pub fn set_focus(&mut self, index: usize) {
        self.focus = index.min(self.cells.len().saturating_sub(1));
    }

    /// The first cell that is a book.
    #[must_use]
    pub fn first_book(&self) -> Option<usize> {
        self.cells
            .iter()
            .position(|cell| matches!(cell.row, Row::Book(_)))
    }

    /// The window indexes of the books on the page the focus is on, as the
    /// last frame laid it out (none before a first frame).
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn visible_books(&self) -> Result<Vec<usize>, Error> {
        let mut books = Vec::new();
        let Some(metrics) = self.metrics else {
            return Ok(books);
        };
        let pages = pages(&self.cells, &metrics, self.grid)?;
        let page = pages
            .iter()
            .rposition(|page| page.first().is_some_and(|first| first.cell <= self.focus))
            .unwrap_or(0);
        for placed in pages.get(page).map_or(&[][..], Vec::as_slice) {
            if let Some(Cell {
                row: Row::Book(at), ..
            }) = self.cells.get(placed.cell)
            {
                push(&mut books, *at)?;
            }
        }
        Ok(books)
    }

    fn after(&self, from: usize) -> Option<usize> {
        (from + 1..self.cells.len()).find(|i| self.cells.get(*i).is_some_and(Cell::selectable))
    }

    fn before(&self, from: usize) -> Option<usize> {
        (0..from)
            .rev()
            .find(|i| self.cells.get(*i).is_some_and(Cell::selectable))
    }

    fn page_of(&self, cell: usize) -> usize {
        self.starts
            .iter()
            .rposition(|start| *start <= cell)
            .unwrap_or(0)
    }

    /// A step by a row in a grid: the book a row below or above, else the
    /// next cell that way.
    fn vertical(&self, down: bool) -> Option<usize> {
        let columns = usize::from(self.metrics.map_or(1, |m| m.columns.max(1)));
        if self.grid
            && self.cells.get(self.focus).is_some_and(Cell::is_book)
            && let Some(target) = if down {
                self.focus.checked_add(columns)
            } else {
                self.focus.checked_sub(columns)
            }
            && self.cells.get(target).is_some_and(Cell::is_book)
        {
            return Some(target);
        }
        if down {
            self.after(self.focus)
        } else {
            self.before(self.focus)
        }
    }

    /// What `input` means, without doing anything about it.
    #[must_use]
    pub fn decide(&self, input: &Input<'_>) -> ListStep {
        match *input {
            Input::Action { action, .. } => {
                let page = self.page_of(self.focus);
                let first_of = |page: usize| {
                    let start = self.starts.get(page).copied()?;
                    (start..self.cells.len())
                        .find(|i| self.cells.get(*i).is_some_and(Cell::selectable))
                };
                let target = match action {
                    "core.confirm" if self.cells.get(self.focus).is_some_and(Cell::selectable) => {
                        return ListStep::Chosen(self.focus);
                    }
                    "core.menu" => {
                        return match self.cells.get(self.focus) {
                            Some(Cell {
                                row: Row::Book(_), ..
                            }) => ListStep::Held(self.focus),
                            _ => ListStep::Menu,
                        };
                    }
                    "core.down" => self.vertical(true),
                    "core.up" => self.vertical(false),
                    "core.next" | "core.right" => self.after(self.focus),
                    "core.previous" | "core.left" => self.before(self.focus),
                    "core.next-page" => first_of(page + 1).or_else(|| self.after(self.focus)),
                    "core.prev-page" if page > 0 => first_of(page - 1),
                    "core.prev-page" => self.before(self.focus),
                    _ => return ListStep::Ignored,
                };
                target.map_or(ListStep::Focus(self.focus), ListStep::Focus)
            }
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => self.tap(position),
            Input::Gesture {
                position,
                kind: InputGestureKind::LongPress,
                ..
            } => match self.tap(position) {
                ListStep::Chosen(cell) => ListStep::Held(cell),
                _ => ListStep::Ignored,
            },
            _ => ListStep::Ignored,
        }
    }

    fn tap(&self, position: Point) -> ListStep {
        if let Some(action) = self.header.action(position) {
            return ListStep::Header(action);
        }
        if self.band.is_some_and(|band| band.contains(position)) {
            return ListStep::Menu;
        }
        match self.hits.iter().find(|(rect, _)| rect.contains(position)) {
            Some((_, cell)) if self.cells.get(*cell).is_some_and(Cell::selectable) => {
                ListStep::Chosen(*cell)
            }
            // A note is taken and does nothing.
            Some(_) => ListStep::Focus(self.focus),
            None => ListStep::Ignored,
        }
    }

    /// Does what [`Self::decide`] says about the focus (with a redraw) or the
    /// header's action; a chosen or held cell is only reported.
    ///
    /// # Errors
    /// Command queueing errors; the focus is unchanged then.
    pub fn input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<ListStep, Error> {
        let step = self.decide(input);
        match step {
            ListStep::Focus(to) => {
                if to != self.focus {
                    let mut scratch = [0; 64];
                    CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)?;
                    self.focus = to;
                }
            }
            ListStep::Chosen(cell) | ListStep::Held(cell) => self.focus = cell,
            ListStep::Header(action) => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).run_action(action)?;
            }
            ListStep::Menu | ListStep::Ignored => {}
        }
        Ok(step)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;
    use alloc::vec;

    fn metrics() -> Metrics {
        Metrics {
            area: Rect::new(0, 50, 480, 400),
            row: 40,
            gap: 8,
            margin: 12,
            columns: 2,
        }
    }

    fn cell(row: Row) -> Cell {
        Cell::plain(row, "t", "").unwrap()
    }

    fn books(n: usize) -> Vec<Cell> {
        (0..n).map(|at| cell(Row::Book(at))).collect()
    }

    #[test]
    fn a_list_stacks_its_cells_and_starts_a_page_where_the_next_does_not_fit() {
        let mut cells = vec![cell(Row::Filter)];
        cells.extend(books(8));
        let m = metrics();
        let pages = pages(&cells, &m, false).unwrap();
        // 40 + 68 per book: the filter and five books fill 380 of 400.
        assert_eq!(pages[0].len(), 6);
        assert_eq!(pages[0][1].rect.y, 50 + 40);
        assert_eq!(pages[1][0].cell, 6);
        assert_eq!(pages[1][0].rect.y, 50);
        assert!(pages.iter().flatten().all(|p| p.rect.width == 480));
    }

    #[test]
    fn a_grid_puts_books_side_by_side_and_full_rows_across() {
        let mut cells = vec![cell(Row::Filter)];
        cells.extend(books(3));
        cells.push(cell(Row::More));
        let m = metrics();
        let pages = pages(&cells, &m, true).unwrap();
        let all: Vec<_> = pages.iter().flatten().collect();
        // The first two books share a row; the third starts the next.
        assert_eq!(all[1].rect.y, all[2].rect.y);
        assert!(all[2].rect.x > all[1].rect.x);
        assert!(all[3].rect.y > all[1].rect.y || pages.len() > 1);
        assert_eq!(all[1].rect.width, m.tile_width());
        // Every cell is placed once.
        assert_eq!(all.len(), cells.len());
    }

    #[test]
    fn a_page_holds_at_least_one_cell_however_tall_it_is() {
        let m = Metrics {
            area: Rect::new(0, 0, 100, 10),
            ..metrics()
        };
        let pages = pages(&books(3), &m, false).unwrap();
        assert_eq!(pages.len(), 3);
        assert!(pages.iter().all(|page| page.len() == 1));
    }

    #[test]
    fn the_focus_skips_a_note_and_the_grid_steps_a_row_down() {
        let mut cells = vec![cell(Row::Filter), cell(Row::Note)];
        cells.extend(books(5));
        let mut list = BookList::new(cells, true);
        list.metrics = Some(metrics());
        assert_eq!(list.focus(), 0);
        let down = Input::Action {
            now_ms: 0,
            action: "core.down",
        };
        assert_eq!(list.decide(&down), ListStep::Focus(2));
        list.set_focus(2);
        // Two columns: down from the first book is the third.
        assert_eq!(list.decide(&down), ListStep::Focus(4));
        let up = Input::Action {
            now_ms: 0,
            action: "core.up",
        };
        list.set_focus(4);
        assert_eq!(list.decide(&up), ListStep::Focus(2));
        list.set_focus(2);
        assert_eq!(list.decide(&up), ListStep::Focus(0));
        let confirm = Input::Action {
            now_ms: 0,
            action: "core.confirm",
        };
        assert_eq!(list.decide(&confirm), ListStep::Chosen(2));
        let menu = Input::Action {
            now_ms: 0,
            action: "core.menu",
        };
        assert_eq!(list.decide(&menu), ListStep::Held(2));
        list.set_focus(0);
        assert_eq!(list.decide(&menu), ListStep::Menu);
        let other = Input::Action {
            now_ms: 0,
            action: "core.nothing",
        };
        assert_eq!(list.decide(&other), ListStep::Ignored);
    }

    #[test]
    fn a_tap_chooses_a_hold_holds_and_the_title_opens_the_menu() {
        let mut list = BookList::new(books(2), false);
        list.hits = vec![
            (Rect::new(0, 50, 480, 68), 0),
            (Rect::new(0, 118, 480, 68), 1),
        ];
        list.band = Some(Rect::new(0, 0, 480, 50));
        let at = |x, y, kind| Input::Gesture {
            now_ms: 0,
            position: Point::new(x, y),
            kind,
        };
        assert_eq!(
            list.decide(&at(10, 120, InputGestureKind::Tap)),
            ListStep::Chosen(1)
        );
        assert_eq!(
            list.decide(&at(10, 120, InputGestureKind::LongPress)),
            ListStep::Held(1)
        );
        assert_eq!(
            list.decide(&at(200, 20, InputGestureKind::Tap)),
            ListStep::Menu
        );
        assert_eq!(
            list.decide(&at(10, 400, InputGestureKind::Tap)),
            ListStep::Ignored
        );
    }

    #[test]
    fn the_books_of_the_page_the_focus_is_on_are_the_visible_ones() {
        let mut list = BookList::new(books(8), false);
        assert!(list.visible_books().unwrap().is_empty());
        list.metrics = Some(metrics());
        // Five books of 68 fit 400; the sixth starts the next page.
        assert_eq!(list.visible_books().unwrap(), [0, 1, 2, 3, 4]);
        list.set_focus(6);
        assert_eq!(list.visible_books().unwrap(), [5, 6, 7]);
    }
}
