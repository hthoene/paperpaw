//! The overview on interface 7: today, the week and the streak as tiles side by
//! side, the last two weeks as bars, the totals, and the ways on. Where the
//! tiles do not fit side by side (the largest text) or the chart has too little
//! room, the tiles are rows of a list above the ways on (the chart then goes).
//! The contents are `overview_rows`'; this lays them out and draws them.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::native::callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind};
use paperpaw_sdk::native::commands::{CommandSender, RedrawMode};
use paperpaw_sdk::native::fonts::{Font, FontRole};
use paperpaw_sdk::native::snapshot;
use paperpaw_sdk::native::ui::{Open, Ui};
use paperpaw_sdk::native::{Error, Host};
use paperpaw_sdk::{Color, Handled, Rect, Storage};
use paperpaw_ui_kit::list;
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Page, Paint, Style, header, paint, text,
};

use crate::native_chart;
use crate::native_screens::{failed, message, open_book, utc_day};
use crate::overview_rows::{self, Chart, Facts, Tile, Way};
use crate::store;
use crate::texts::{HostWords, Words};

/// Whether a way's screen is on interface 7; the other (the sync) is left out
/// until it is.
fn shown(way: &Way) -> bool {
    matches!(way, Way::ThisBook(_) | Way::Calendar | Way::Books)
}

/// The overview's contents and what was last drawn of it.
pub(crate) struct OverviewView {
    tiles: Option<[Tile; 3]>,
    chart: Option<Chart>,
    totals: Option<String>,
    no_clock: String,
    title: String,
    ways: Vec<Way>,
    /// The tiles as rows and the ways, for the one column.
    list: ItemList,
    /// The focused way, where the tiles stand side by side.
    focus: usize,
    /// Whether the last frame was the one column.
    stacked: bool,
    /// The way rows of the last frame, and the header's regions.
    hits: Vec<(Rect, usize)>,
    header: Page,
}

impl OverviewView {
    /// Reads the figures over `data`; `today` is the local day (none until the
    /// device knows the time) and `current` the book being read.
    pub(crate) fn new(
        host: &Host<'_>,
        data: &dyn Storage,
        today: Option<i32>,
        current: Option<u16>,
    ) -> Result<Self, Error> {
        Self::build(&mut HostWords::new(host)?, data, today, current)
    }

    /// As [`Self::new`], with the texts given.
    pub(crate) fn build(
        words: &mut impl Words,
        data: &dyn Storage,
        today: Option<i32>,
        current: Option<u16>,
    ) -> Result<Self, Error> {
        let days = store::days(data, utc_day).map_err(failed)?;
        let books = store::load_books(data).map_err(failed)?.len();
        let today_pages = match today {
            Some(day) => overview_rows::distinct_pages(data, day, utc_day)?,
            None => 0,
        };
        let facts = Facts {
            today,
            days: &days,
            books,
            today_pages,
        };
        let tiles = overview_rows::tiles(words, &facts)?;
        let chart = overview_rows::chart(words, &facts)?;
        let totals = overview_rows::totals_text(words, &facts)?;
        let mut ways = overview_rows::ways(current)?;
        ways.retain(shown);
        let mut items = Vec::new();
        items
            .try_reserve_exact(ways.len() + 3)
            .map_err(|_| Error::NoMemory)?;
        if let Some(tiles) = &tiles {
            for tile in tiles {
                items.push(Item::noted(&tile.label, &tile.value, &tile.note)?);
            }
        }
        for &way in &ways {
            items.push(overview_rows::way_row(words, way)?);
        }
        let mut list = ItemList::new(items);
        list.set_focus(if tiles.is_some() { 3 } else { 0 });
        Ok(Self {
            no_clock: words.copy("stats.no-clock")?,
            title: words.copy("stats.title")?,
            tiles,
            chart,
            totals,
            ways,
            list,
            focus: 0,
            stacked: false,
            hits: Vec::new(),
            header: Page::default(),
        })
    }

    /// How many rows of the list are tiles.
    fn tile_rows(&self) -> usize {
        if self.tiles.is_some() { 3 } else { 0 }
    }

    pub(crate) fn draw(&mut self, host: &Host<'_>, frame: &mut DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let style = Style::read(host, &display, &font)?;
        let (width, height) = (display.display_width(), display.display_height());
        let layout = Layout::of(&style, width, height, self.ways.len());
        if self.tiles.is_some() && !self.fits(&font, &style, &layout)? {
            font.release()?;
            self.stacked = true;
            let footer = self.totals.as_deref().map_or(Footer::None, Footer::Note);
            return self.list.draw_with(host, frame, &self.title, true, footer);
        }
        self.stacked = false;
        let mut header = Page::default();
        header::prepare(
            &mut header,
            host,
            &font,
            &style,
            &mut Messages::new()?,
            &self.title,
            width,
            display.has_touch(),
            0,
            1,
        )?;
        let mut body = Vec::new();
        let below = style.header.saturating_add(style.gap);
        paint::push(
            &mut body,
            Paint::Fill(
                Rect::new(0, below, width, height.saturating_sub(below)),
                style.paper,
            ),
        )?;
        match (&self.tiles, &self.chart) {
            (Some(tiles), chart) => {
                self.draw_tiles(&mut body, &font, &style, &layout, tiles)?;
                if let Some(chart) = chart {
                    self.draw_chart(&mut body, &font, &style, &layout, chart)?;
                }
            }
            (None, _) => {
                let area = Rect::new(
                    0,
                    layout.tiles.y,
                    layout.tiles.width,
                    layout.totals.y.saturating_sub(layout.tiles.y),
                );
                message(&mut body, &font, &style, area, &self.no_clock)?;
            }
        }
        if let Some(totals) = &self.totals {
            let w = font.measure_text(style.small, totals)?;
            let x = (f32::from(layout.totals.width) - w).max(0.0) / 2.0;
            text::label(
                &mut body,
                &font,
                totals,
                style.small,
                x,
                layout.totals,
                style.note,
                None,
            )?;
        }
        let hits = self.draw_ways(&mut body, &font, &style, &layout, width)?;
        paint::replay(frame, &font, header.paint.iter().chain(body.iter()))?;
        font.release()?;
        header.paint = Vec::new();
        self.header = header;
        self.hits = hits;
        Ok(())
    }

    /// Whether each tile's words fit whole side by side (as small as they are
    /// drawn), and the chart has room for its title, the names and the bars.
    fn fits(&self, font: &Font<'_>, style: &Style, layout: &Layout) -> Result<bool, Error> {
        let Some(tiles) = &self.tiles else {
            return Ok(true);
        };
        let least = style.small * 0.85;
        let value_size = value_size(font, style, layout, tiles)?;
        for (i, tile) in tiles.iter().enumerate() {
            let room = layout.tile_room(style, i) + 1.0;
            if font.measure_text(least, &tile.label)? > room
                || font.measure_text(value_size, &tile.value)? > room
                || font.measure_text(least, &tile.note)? > room
            {
                return Ok(false);
            }
        }
        Ok(f32::from(layout.chart.height) >= style.small * 6.0)
    }

    fn draw_tiles(
        &self,
        body: &mut Vec<Paint>,
        font: &Font<'_>,
        style: &Style,
        layout: &Layout,
        tiles: &[Tile; 3],
    ) -> Result<(), Error> {
        let area = layout.tiles;
        let column = area.width / 3;
        let third = area.height / 3;
        let pad = f32::from(style.margin);
        let value_size = value_size(font, style, layout, tiles)?;
        let least = style.small * 0.85;
        for (i, tile) in tiles.iter().enumerate() {
            let x = area.x.saturating_add(column.saturating_mul(i as u16));
            let left = f32::from(x) + pad;
            let room = layout.tile_room(style, i);
            let cell = |row: u16| {
                Rect::new(
                    x,
                    area.y.saturating_add(third.saturating_mul(row)),
                    (pad + room) as u16,
                    third,
                )
            };
            let lines = [
                (&tile.label, style.small, least, style.muted),
                (&tile.value, value_size, value_size, style.ink),
                (&tile.note, style.small, least, style.muted),
            ];
            for (row, (words, size, smallest, color)) in lines.into_iter().enumerate() {
                let (size, line) = fit_line(font, size, smallest, words, room)?;
                text::label(body, font, &line, size, left, cell(row as u16), color, None)?;
            }
            if i > 0 {
                paint::push(
                    body,
                    Paint::Fill(
                        Rect::new(x, area.y, style.line.max(1), area.height),
                        Color::gray(190),
                    ),
                )?;
            }
        }
        Ok(())
    }

    fn draw_chart(
        &self,
        body: &mut Vec<Paint>,
        font: &Font<'_>,
        style: &Style,
        layout: &Layout,
        chart: &Chart,
    ) -> Result<(), Error> {
        let area = layout.chart;
        let title_height = (style.small * 1.6) as u16;
        text::label(
            body,
            font,
            &chart.title,
            style.small,
            f32::from(area.x),
            Rect::new(area.x, area.y, area.width, title_height),
            style.muted,
            None,
        )?;
        let mut names: Vec<&str> = Vec::new();
        names
            .try_reserve_exact(chart.names.len())
            .map_err(|_| Error::NoMemory)?;
        names.extend(chart.names.iter().map(String::as_str));
        let bars = Rect::new(
            area.x,
            area.y.saturating_add(title_height),
            area.width,
            area.height.saturating_sub(title_height),
        );
        native_chart::bars(
            body,
            font,
            style,
            bars,
            &chart.values,
            &names,
            (chart.values.len().checked_sub(1), &chart.top),
        )
    }

    /// The ways on, one row each; their regions for taps.
    fn draw_ways(
        &self,
        body: &mut Vec<Paint>,
        font: &Font<'_>,
        style: &Style,
        layout: &Layout,
        width: u16,
    ) -> Result<Vec<(Rect, usize)>, Error> {
        let mut hits = Vec::new();
        hits.try_reserve_exact(self.ways.len())
            .map_err(|_| Error::NoMemory)?;
        for (index, item) in self.list.items().iter().skip(self.tile_rows()).enumerate() {
            let Item::Row(row) = item else { continue };
            let rect = Rect::new(
                0,
                layout
                    .list_top
                    .saturating_add(style.row.saturating_mul(index as u16)),
                width,
                style.row,
            );
            list::row(
                body,
                font,
                style,
                rect,
                &row.title,
                &row.value,
                index == self.focus,
            )?;
            hits.push((rect, index));
        }
        Ok(hits)
    }

    pub(crate) fn input(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        if self.stacked {
            let step = self.list.input(host, input)?;
            if let ItemStep::Chosen(index) = step
                && let Some(way) = index
                    .checked_sub(self.tile_rows())
                    .and_then(|at| self.ways.get(at).copied())
            {
                open_way(host, ui, way)?;
            }
            return Ok(ItemList::handled(step));
        }
        match *input {
            Input::Action { action, .. } => {
                let last = self.ways.len().saturating_sub(1);
                let to = match action {
                    "core.up" | "core.left" | "core.previous" => self.focus.saturating_sub(1),
                    "core.down" | "core.right" | "core.next" => {
                        self.focus.saturating_add(1).min(last)
                    }
                    "core.confirm" => {
                        if let Some(&way) = self.ways.get(self.focus) {
                            open_way(host, ui, way)?;
                        }
                        return Ok(Handled::Yes);
                    }
                    _ => return Ok(Handled::No),
                };
                self.focus_on(host, to)?;
                Ok(Handled::Yes)
            }
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => {
                if let Some(action) = self.header.action(position) {
                    let mut scratch = [0; 128];
                    CommandSender::new(host, &mut scratch).run_action(action)?;
                    return Ok(Handled::Yes);
                }
                let Some(&(_, index)) = self.hits.iter().find(|(rect, _)| rect.contains(position))
                else {
                    return Ok(Handled::No);
                };
                self.focus_on(host, index)?;
                if let Some(&way) = self.ways.get(index) {
                    open_way(host, ui, way)?;
                }
                Ok(Handled::Yes)
            }
            _ => Ok(Handled::No),
        }
    }

    fn focus_on(&mut self, host: &Host<'_>, to: usize) -> Result<(), Error> {
        if to != self.focus {
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)?;
            self.focus = to;
        }
        Ok(())
    }
}

/// Opens the screen a way leads to.
fn open_way(host: &Host<'_>, ui: &Ui, way: Way) -> Result<(), Error> {
    match way {
        Way::ThisBook(id) => open_book(host, ui, id),
        Way::Books => {
            let mut scratch = [0; 256];
            CommandSender::new(host, &mut scratch).open(Open::screen("stats.books")?)
        }
        Way::Calendar => {
            let mut scratch = [0; 256];
            CommandSender::new(host, &mut scratch).open(Open::screen("stats.calendar")?)
        }
        // Its screen is not on interface 7 yet; its row is not shown.
        Way::Sync => Ok(()),
    }
}

/// Where the parts of the screen go, from the stock sizes.
struct Layout {
    tiles: Rect,
    chart: Rect,
    totals: Rect,
    list_top: u16,
}

impl Layout {
    fn of(style: &Style, width: u16, height: u16, ways: usize) -> Self {
        let (row, gap, margin) = (style.row, style.gap, style.margin);
        let list_height = row.saturating_mul(ways as u16);
        // The lowest the rows may start.
        let lowest = height.saturating_sub(list_height.saturating_add(gap));
        let tiles = Rect::new(0, style.header.saturating_add(gap), width, row * 8 / 5);
        let chart_top = tiles.y.saturating_add(tiles.height).saturating_add(gap * 2);
        // The chart takes the room left, up to a third of the screen; the
        // totals right below it.
        let room = lowest.saturating_sub(chart_top.saturating_add(row).saturating_add(gap * 2));
        let chart = Rect::new(
            margin,
            chart_top,
            width.saturating_sub(2 * margin),
            room.min(height / 3),
        );
        let totals = Rect::new(
            0,
            chart.y.saturating_add(chart.height).saturating_add(gap),
            width,
            row,
        );
        let list_top = totals
            .y
            .saturating_add(totals.height)
            .saturating_add(gap * 2)
            .min(lowest);
        Self {
            tiles,
            chart,
            totals,
            list_top,
        }
    }

    /// The room for the text of tile `i`: up to the next tile's line, a gap
    /// before it; the last up to the margin.
    fn tile_room(&self, style: &Style, i: usize) -> f32 {
        let right = if i + 1 < 3 { style.gap } else { style.margin };
        (f32::from(self.tiles.width / 3) - f32::from(style.margin) - f32::from(right)).max(0.0)
    }
}

/// The largest size at which each tile's value fits, down to the small one.
fn value_size(
    font: &Font<'_>,
    style: &Style,
    layout: &Layout,
    tiles: &[Tile; 3],
) -> Result<f32, Error> {
    let mut size = style.title;
    for (i, tile) in tiles.iter().enumerate() {
        let room = layout.tile_room(style, i);
        let mut fitted = style.title;
        while fitted > style.small && font.measure_text(fitted, &tile.value)? > room {
            fitted -= 1.0;
        }
        size = size.min(fitted.max(style.small));
    }
    Ok(size)
}

/// `words` at `size`, a little smaller where they do not fit whole (down to
/// `least`), only then shortened.
pub(crate) fn fit_line(
    font: &Font<'_>,
    size: f32,
    least: f32,
    words: &str,
    room: f32,
) -> Result<(f32, String), Error> {
    let mut size = size;
    while size > least && font.measure_text(size, words)? > room {
        size = (size - 1.0).max(least);
    }
    Ok((size, text::fit(font, size, words, room)?))
}

#[cfg(test)]
mod tests;
