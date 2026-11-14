//! The tools drawer (`home.drawer`): every tool at once, as icons in a grid
//! (as a phone lists its apps), opened by *All tools*. A tap or *Confirm* goes
//! back to the home screen and runs the tool; the buttons move between the
//! tools, the page buttons by a screenful; a swipe down goes back.
//!
//! The arithmetic (columns, rows, the cell of a place, the focus' moves) is
//! `Grid`, without the host and tested without it. `Drawer` reads the device,
//! draws and takes input; what a draw laid out is kept as hit geometry only
//! (cells and the header's regions) for the input that follows.
//!
//! Where the interface runs right to left the grid is mirrored: the first tool
//! at the right, *left* going on to the next one. Not here (`PORTING-7.md`):
//! drawing/input through a host (H1/H4).
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Point, Rect,
    keys::InputMode,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind},
        canvas::PartRole,
        commands::CommandSender,
        fonts::{Font, FontRole},
        snapshot,
    },
    widgets::grid as sdk_grid,
};
use paperpaw_ui_kit::{
    Messages, Page, Paint, Style, header,
    mem::{SERVICE_BUFFER, buffer, push},
    paint,
};

use crate::native_dashboard::{redraw, ring};
use crate::native_focus::Press;
use crate::native_icons::{self as icons, Fonts, Tool};
use crate::native_source::HostSource;

/// The screen this is declared as.
pub const SCREEN: &str = "home.drawer";
/// Columns of the drawer on a screen higher than wide; more on one that is
/// wider (`grid::columns_for`).
const COLUMNS: u8 = 4;

/// Where the tools go: columns, cells, pages. No host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grid {
    area: Rect,
    columns: usize,
    cell_height: u16,
    per_page: usize,
    /// Where the interface runs right to left: the grid is mirrored, the
    /// first tool at the right.
    rtl: bool,
}

impl Grid {
    /// The grid of an area: the columns the area's shape gives, cells about as
    /// high as wide (a line higher, of `extra` pixels, where a name takes two),
    /// at least `row` high, with `air` above and below for a focus ring.
    #[must_use]
    pub fn new(area: Rect, row: u16, extra: u16, air: u16) -> Self {
        let columns = usize::from(sdk_grid::columns_for(area, COLUMNS)).max(1);
        let wide = area.width / u16::try_from(columns).unwrap_or(1);
        let high = (u32::from(wide) * 4 / 5) as u16;
        let cell_height = high
            .max(row)
            .saturating_add(extra)
            .saturating_add(air.saturating_mul(2))
            .max(1);
        let rows = usize::from(area.height / cell_height).max(1);
        Self {
            area,
            columns,
            cell_height,
            per_page: columns * rows,
            rtl: false,
        }
    }

    /// The same grid mirrored where `rtl`: its cells, the places a tap finds
    /// and the sides the buttons mean.
    #[must_use]
    pub const fn mirrored(mut self, rtl: bool) -> Self {
        self.rtl = rtl;
        self
    }

    /// Tools side by side.
    #[must_use]
    pub const fn columns(&self) -> usize {
        self.columns
    }

    /// Tools on a page.
    #[must_use]
    pub const fn per_page(&self) -> usize {
        self.per_page
    }

    /// The pages `len` tools take (at least one).
    #[must_use]
    pub fn pages(&self, len: usize) -> usize {
        len.div_ceil(self.per_page).max(1)
    }

    /// The cell of the place `slot` on a page.
    #[must_use]
    pub fn cell(&self, slot: usize) -> Rect {
        let columns = u16::try_from(self.columns).unwrap_or(1);
        let wide = self.area.width / columns;
        let (column, row) = (slot % self.columns, slot / self.columns);
        let cell = Rect::new(
            self.area
                .x
                .saturating_add(wide.saturating_mul(u16::try_from(column).unwrap_or(0))),
            self.area.y.saturating_add(
                self.cell_height
                    .saturating_mul(u16::try_from(row).unwrap_or(u16::MAX)),
            ),
            wide,
            self.cell_height,
        );
        paperpaw_ui_kit::style::flip_rect(self.rtl, self.area, cell)
    }

    /// The place of the tool `index` and the page it is on.
    #[must_use]
    pub fn place(&self, index: usize) -> (usize, usize) {
        (index / self.per_page, index % self.per_page)
    }

    /// The tool of `len` at `point` on the page `page`, if any.
    #[must_use]
    pub fn hit(&self, point: Point, page: usize, len: usize) -> Option<usize> {
        let first = page.checked_mul(self.per_page)?;
        (0..self.per_page)
            .take(len.saturating_sub(first))
            .find(|slot| self.cell(*slot).contains(point))
            .map(|slot| first + slot)
    }

    /// Where a press takes the focus from `at` among `len` tools: sideways by
    /// one, up and down by a row. The ends stay.
    #[must_use]
    pub fn moved(&self, at: usize, press: Press, len: usize) -> usize {
        let to = match press {
            // Left and right are where the tools are: from the right, the
            // next one is to the left.
            Press::Left if self.rtl => at.saturating_add(1),
            Press::Right if self.rtl => at.saturating_sub(1),
            Press::Left | Press::Previous => at.saturating_sub(1),
            Press::Right | Press::Next => at.saturating_add(1),
            Press::Up => at.saturating_sub(self.columns),
            Press::Down => at.saturating_add(self.columns),
            Press::Confirm => at,
        };
        to.min(len.saturating_sub(1))
    }

    /// Where a page button takes the focus: a page on, or back.
    #[must_use]
    pub fn paged(&self, at: usize, forward: bool, len: usize) -> usize {
        let to = if forward {
            at.saturating_add(self.per_page)
        } else {
            at.saturating_sub(self.per_page)
        };
        to.min(len.saturating_sub(1))
    }
}

/// What the last draw laid out, for the input that follows: hit geometry only.
#[derive(Debug)]
struct Laid {
    grid: Grid,
    header: Page,
}

/// The tools drawer.
#[derive(Debug, Default)]
pub struct Drawer {
    /// The actions of the tools, in the menu's order.
    tools: Vec<String>,
    /// The tool the focus stands on, and whether it shows.
    focus: usize,
    shown: bool,
    laid: Option<Laid>,
}

impl Drawer {
    /// A drawer of the tools the menu lists. The focus is drawn at once only
    /// where the reader used buttons last (`last_input`); with `None` (not
    /// known: the screen is opened before it is shown) it is not, so the first
    /// button press only shows where it is, and the show that follows
    /// (`shown`) decides anew.
    ///
    /// # Errors
    /// [`Error::NoMemory`] and the host's refusals.
    pub fn new(host: &Host<'_>, last_input: Option<InputMode>) -> Result<Self, Error> {
        let display = snapshot::capture_snapshot(host)?;
        let src = HostSource::read(host, display.display_dpi())?;
        let mut tools = Vec::new();
        tools
            .try_reserve_exact(src.tool_rows().len())
            .map_err(|_| Error::NoMemory)?;
        for (action, _) in src.tool_rows() {
            push(&mut tools, paperpaw_ui_kit::text::copy(action)?)?;
        }
        Ok(Self {
            tools,
            focus: 0,
            shown: last_input == Some(InputMode::Buttons),
            laid: None,
        })
    }

    /// Shown again after the reader used `last_input` last.
    pub fn shown(&mut self, last_input: Option<InputMode>) {
        self.shown = last_input == Some(InputMode::Buttons);
    }

    /// Draws the header and the page of tools the focus is on.
    ///
    /// # Errors
    /// Allocation, font, canvas and host errors; the hit geometry of the last
    /// completed draw stays where one fails.
    pub fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let drawn = self.paint(host, frame, &display, &font);
        let released = font.release();
        let laid = drawn?;
        released?;
        self.laid = Some(laid);
        Ok(())
    }

    /// Every tool's name in lines, and the most lines any name takes: every
    /// tool keeps room for them, so that icons and names line up.
    fn wrap_names(
        &self,
        host: &Host<'_>,
        font: &Font<'_>,
        style: &Style,
        src: &HostSource,
        area: Rect,
        air: u16,
    ) -> Result<(Vec<icons::Name>, usize), Error> {
        let probe = Grid::new(area, style.row, 0, air);
        let probe_cell = probe.cell(0);
        let mut wrapped = Vec::new();
        wrapped
            .try_reserve_exact(self.tools.len())
            .map_err(|_| Error::NoMemory)?;
        let mut most = 0;
        let hyphens = icons::Hyphens::of(host);
        let mut breaks = |word: &str, out: &mut _| hyphens.breaks(host, word, out);
        for action in &self.tools {
            let name = src.tool_name(action);
            let name = icons::name_lines(font, style, probe_cell, name, &mut breaks)?;
            most = most.max(name.lines.len());
            wrapped.push(name);
        }
        Ok((wrapped, most))
    }

    fn paint(
        &self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        display: &snapshot::Snapshot,
        font: &Font<'_>,
    ) -> Result<Laid, Error> {
        let style = Style::read(host, display, font)?;
        let src = HostSource::read(host, display.display_dpi())?;
        let (width, height) = (display.display_width(), display.display_height());
        let top = style.header.saturating_add(style.gap);
        let area = Rect::new(
            style.margin,
            top,
            width.saturating_sub(2 * style.margin),
            height.saturating_sub(top),
        );
        let air = style.gap / 2 + style.line * 3;
        let (wrapped, most) = self.wrap_names(host, font, &style, &src, area, air)?;
        let step = paperpaw_ui_kit::geometry::line_px(font, style.small)?;
        let extra = if most > 1 { step } else { 0 };
        let grid = Grid::new(area, style.row, extra, air).mirrored(style.rtl);
        let len = self.tools.len();
        let page = grid.place(self.focus.min(len.saturating_sub(1))).0;
        let mut header_page = Page::default();
        let mut messages = Messages::new()?;
        let title = messages.copy(host, SCREEN)?;
        header::prepare(
            &mut header_page,
            host,
            font,
            &style,
            &mut messages,
            &title,
            width,
            display.has_touch(),
            page,
            grid.pages(len),
        )?;
        let icon_font = paperpaw_ui_kit::icons::font(host)?;
        let circle = paperpaw_ui_kit::icons::glyph(host, "circle")?;
        let fonts = Fonts {
            ui: font,
            icons: icon_font.as_ref(),
            circle: circle.as_ref(),
        };
        let mut below = Vec::new();
        paint::push(&mut below, Paint::Fill(area, style.paper))?;
        let first = page * grid.per_page();
        let describing = paint::asking(frame);
        for slot in 0..grid.per_page().min(len.saturating_sub(first)) {
            let index = first + slot;
            let (Some(action), Some(name)) = (self.tools.get(index), wrapped.get(index)) else {
                break;
            };
            let face = icons::face(host, &src, action)?;
            let tool = Tool {
                lines: &name.lines,
                size: name.size,
                face,
                row_lines: most,
            };
            let cell = grid.cell(slot).inset(0, air);
            // Told as a button by its whole name, not by the first line of it.
            paint::describe_named(
                &mut below,
                describing,
                PartRole::Button,
                cell,
                src.tool_name(action),
                self.shown && index == self.focus,
                false,
            )?;
            icons::draw_tool(&mut below, &fonts, &style, cell, &tool)?;
        }
        let mut above = Vec::new();
        if self.shown && len > 0 {
            let at = self.focus.min(len - 1);
            if grid.place(at).0 == page {
                ring(
                    &mut above,
                    grid.cell(grid.place(at).1).inset(0, air),
                    &style,
                )?;
            }
        }
        paint::replay_icons(
            frame,
            font,
            fonts.icons,
            header_page.paint.iter().chain(below.iter()),
        )?;
        paint::replay(frame, font, above.iter())?;
        // Only hit geometry survives the draw callback.
        header_page.paint = Vec::new();
        Ok(Laid {
            grid,
            header: header_page,
        })
    }

    /// Takes an input: a button moves the focus or runs the tool it stands on,
    /// a tap runs the tool under it or the header's arrow, a swipe down goes
    /// back.
    ///
    /// # Errors
    /// Allocation and host errors; nothing is staged then.
    pub fn input(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        match *input {
            Input::Action { action, .. } => self.action(host, action),
            Input::Gesture { position, kind, .. } => match kind {
                InputGestureKind::Tap => self.tapped(host, position),
                InputGestureKind::SwipeDown => {
                    back(host)?;
                    Ok(Handled::Yes)
                }
                _ => Ok(Handled::No),
            },
            _ => Ok(Handled::No),
        }
    }

    fn action(&mut self, host: &Host<'_>, action: &str) -> Result<Handled, Error> {
        let page = match action {
            "core.next-page" => Some(true),
            "core.prev-page" => Some(false),
            _ => None,
        };
        let press = Press::of(action);
        if page.is_none() && press.is_none() {
            return Ok(Handled::No);
        }
        let Some(laid) = self.laid.as_ref() else {
            return Ok(Handled::No);
        };
        let len = self.tools.len();
        if len == 0 {
            return Ok(Handled::Yes);
        }
        let at = self.focus.min(len - 1);
        let to = match (page, press) {
            (Some(forward), _) => laid.grid.paged(at, forward, len),
            // The first press only shows where the focus is.
            (None, _) if !self.shown => at,
            (None, Some(Press::Confirm)) => {
                self.run(host, at)?;
                return Ok(Handled::Yes);
            }
            (None, Some(press)) => laid.grid.moved(at, press, len),
            (None, None) => return Ok(Handled::No),
        };
        if to != self.focus || !self.shown {
            self.focus = to;
            self.shown = true;
            redraw(host)?;
        }
        Ok(Handled::Yes)
    }

    fn tapped(&mut self, host: &Host<'_>, position: Point) -> Result<Handled, Error> {
        let Some(laid) = self.laid.as_ref() else {
            return Ok(Handled::No);
        };
        match laid.header.action(position) {
            Some("core.back") => {
                back(host)?;
                return Ok(Handled::Yes);
            }
            Some(action) => return self.action(host, action),
            None => {}
        }
        let page = laid.grid.place(self.focus).0;
        match laid.grid.hit(position, page, self.tools.len()) {
            Some(index) => {
                self.run(host, index)?;
                Ok(Handled::Yes)
            }
            None => Ok(Handled::No),
        }
    }

    /// Back to the home screen, then the tool: both staged together, so a
    /// refusal stages neither.
    fn run(&self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let Some(action) = self.tools.get(index) else {
            return Ok(());
        };
        let mut scratch = buffer(SERVICE_BUFFER)?;
        let mut sender = CommandSender::new(host, &mut scratch);
        sender.back()?;
        sender.run_action(action)
    }
}

fn back(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).back()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    /// 4 columns of 100, cells 80 high with no extra or air: 5 rows in 400.
    fn grid() -> Grid {
        Grid::new(Rect::new(10, 50, 400, 400), 20, 0, 0)
    }

    #[test]
    fn the_cells_lie_in_reading_order_in_the_area() {
        let grid = grid();
        assert_eq!((grid.columns(), grid.per_page()), (4, 20));
        assert_eq!(grid.cell(0), Rect::new(10, 50, 100, 80));
        assert_eq!(grid.cell(3), Rect::new(310, 50, 100, 80));
        assert_eq!(grid.cell(4), Rect::new(10, 130, 100, 80));
        assert_eq!(grid.pages(0), 1);
        assert_eq!(grid.pages(20), 1);
        assert_eq!(grid.pages(21), 2);
        assert_eq!(grid.place(23), (1, 3));
    }

    #[test]
    fn a_wide_area_has_more_columns_and_a_low_one_still_a_row() {
        let wide = Grid::new(Rect::new(0, 0, 800, 400), 20, 0, 0);
        assert_eq!(wide.columns(), 6);
        let low = Grid::new(Rect::new(0, 0, 400, 10), 20, 0, 0);
        assert_eq!(low.per_page(), low.columns());
        // Room kept for a second line of a name and for a focus ring.
        let roomy = Grid::new(Rect::new(0, 0, 400, 400), 20, 10, 5);
        assert_eq!(roomy.cell(4).y, 100);
    }

    #[test]
    fn a_mirrored_grid_starts_at_the_right_and_left_goes_on_to_the_next_tool() {
        let grid = grid().mirrored(true);
        assert_eq!(grid.cell(0), Rect::new(310, 50, 100, 80));
        assert_eq!(grid.cell(3), Rect::new(10, 50, 100, 80));
        assert_eq!(grid.cell(4), Rect::new(310, 130, 100, 80));
        // A tap finds the tool where it is drawn.
        assert_eq!(grid.hit(Point::new(315, 55), 0, 30), Some(0));
        assert_eq!(grid.hit(Point::new(15, 55), 0, 30), Some(3));
        // Left and right are where the tools are; next and previous are not.
        assert_eq!(grid.moved(0, Press::Left, 30), 1);
        assert_eq!(grid.moved(1, Press::Right, 30), 0);
        assert_eq!(grid.moved(0, Press::Next, 30), 1);
        assert_eq!(grid.moved(1, Press::Previous, 30), 0);
        // Not mirrored: as before.
        assert_eq!(self::grid().moved(1, Press::Left, 30), 0);
    }

    #[test]
    fn a_tap_finds_the_tool_under_it_on_the_page() {
        let grid = grid();
        assert_eq!(grid.hit(Point::new(15, 55), 0, 30), Some(0));
        assert_eq!(grid.hit(Point::new(115, 135), 0, 30), Some(5));
        assert_eq!(grid.hit(Point::new(15, 55), 1, 30), Some(20));
        // No tool there on the last page, none outside the area.
        assert_eq!(grid.hit(Point::new(315, 55), 1, 22), None);
        assert_eq!(grid.hit(Point::new(5, 55), 0, 30), None);
        assert_eq!(grid.hit(Point::new(15, 55), 0, 0), None);
    }

    #[test]
    fn the_focus_moves_by_one_a_row_or_a_page_and_stays_at_the_ends() {
        let grid = grid();
        let go = |at, press| grid.moved(at, press, 30);
        assert_eq!(go(0, Press::Right), 1);
        assert_eq!(go(0, Press::Left), 0);
        assert_eq!(go(1, Press::Down), 5);
        assert_eq!(go(5, Press::Up), 1);
        assert_eq!(go(2, Press::Up), 0);
        assert_eq!(go(29, Press::Next), 29);
        assert_eq!(go(27, Press::Down), 29);
        assert_eq!(grid.paged(3, true, 30), 23);
        assert_eq!(grid.paged(23, false, 30), 3);
        assert_eq!(grid.paged(20, true, 30), 29);
        assert_eq!(grid.moved(0, Press::Right, 0), 0);
        assert_eq!(grid.paged(0, true, 0), 0);
    }
}
