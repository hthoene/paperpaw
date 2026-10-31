//! The overview: today, the last seven days and the streak at a glance,
//! the last two weeks as bars, the totals, and the way to the calendar,
//! the books and the book being read.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::ui::{self, ListChange, ListView, UiStyle};
use paperpaw_sdk::widgets;
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect,
    RefreshMode, Screen,
};

use crate::store::{self, Day};
use crate::{BOOK, BOOKS, CALENDAR, chart, fmt, totals};

/// Days in the chart.
const CHART_DAYS: i32 = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    ThisBook(u16),
    Calendar,
    Books,
    Sync,
}

/// What a row of the list shows: a way on, or, where the tiles do not fit
/// side by side (the largest text), a tile, the chart or the totals, one
/// below the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entry {
    Tile(usize),
    Chart,
    Totals,
    Go(Row),
}

pub(crate) struct Overview {
    style: UiStyle,
    header: ui::Header,
    tiles: Rect,
    chart: Rect,
    totals: Rect,
    list: ListView,
    entries: Vec<Entry>,
    /// Tiles, chart and totals in the list, one below the other.
    stacked: bool,
    today: Option<i32>,
    days: Vec<Day>,
    books: usize,
    /// Distinct pages read today.
    today_pages: u32,
    /// The size the tiles' values are drawn at (the largest at which each
    /// fits), found once.
    value_size: f32,
}

impl Overview {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let current = params.get(params::ID).and_then(|id| id.parse::<u16>().ok());
        let mut rows = Vec::new();
        if let Some(book) = current {
            rows.push(Row::ThisBook(book));
        }
        rows.extend([Row::Calendar, Row::Books, Row::Sync]);
        let row = style.row();
        let gap = style.gap();
        let list_height = row * rows.len() as u16;
        // The lowest the rows may start.
        let lowest = display.height.saturating_sub(list_height + gap);
        let tiles = Rect::new(0, header.area().height + gap, display.width, row * 8 / 5);
        let chart_top = tiles.y + tiles.height + gap * 2;
        // The chart takes the room left, up to a third of the screen; the
        // totals right below it.
        let room = lowest.saturating_sub(chart_top + row + gap * 2);
        let chart = Rect::new(
            style.margin(),
            chart_top,
            display.width - 2 * style.margin(),
            room.min(display.height / 3),
        );
        let totals = Rect::new(0, chart.y + chart.height + gap, display.width, row);
        // The ways on right below.
        let list_top = (totals.y + totals.height + gap * 2).min(lowest);
        let today = fmt::today(env);
        let days = store::days(env.storage(), |unix| fmt::day_of(env, unix));
        let today_pages = today.map_or(0, |day| distinct_pages(env, day));
        let mut overview = Self {
            list: ListView::new(
                Rect::new(0, list_top, display.width, list_height),
                u32::from(style.row()),
            )
            .focus_on_keys(env),
            entries: rows.into_iter().map(Entry::Go).collect(),
            stacked: false,
            style,
            header,
            tiles,
            chart,
            totals,
            today,
            days,
            books: store::books(env.storage()).len(),
            today_pages,
            value_size: style.title(),
        };
        overview.value_size = overview.value_size(env);
        // One column also where the chart has too little room between the
        // tiles and the rows (the largest text): its title, the days'
        // names and bars to see.
        let least = (style.small() * 6.0) as u16;
        if overview.today.is_some() && (!overview.tiles_fit(env) || overview.chart.height < least) {
            overview.stack(env);
        }
        overview
    }

    /// Whether each tile's words fit whole side by side (as small as
    /// [`Overview::draw_tiles`] sets them).
    fn tiles_fit(&self, env: &Env<'_>) -> bool {
        let style = self.style;
        let (Some(font), Some(tiles)) = (style.font(env), self.tiles(env)) else {
            return true;
        };
        let least = style.small() * 0.85;
        tiles.iter().enumerate().all(|(i, (label, value, note))| {
            let room = self.tile_room(i) + 1.0;
            font.measure(least, env.i18n().tr(label)) <= room
                && font.measure(self.value_size, value) <= room
                && font.measure(least, note) <= room
        })
    }

    /// One column: the tiles, the chart and the totals as rows of the list
    /// above the ways on, the list across the screen below the header.
    fn stack(&mut self, env: &Env<'_>) {
        let style = self.style;
        let display = env.device().display;
        let top = self.header.area().height;
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let mut entries = alloc::vec![Entry::Tile(0), Entry::Tile(1), Entry::Tile(2), Entry::Chart];
        if !self.days.is_empty() {
            entries.push(Entry::Totals);
        }
        entries.extend(self.entries.iter().copied());
        self.entries = entries;
        self.list = ListView::new(area, style.noted_row()).focus_on_keys(env);
        self.stacked = true;
    }

    /// One column: the list of tiles, chart, totals and ways on.
    fn draw_stacked(&self, ui: &mut ui::Ui<'_>) {
        let style = self.style;
        let env = ui.env();
        let tiles = self.tiles(env);
        let totals = self.totals_text(env);
        self.list.draw(
            ui,
            self.entries.len(),
            |ui, index, rect, focused| match self.entries[index] {
                Entry::Tile(i) => {
                    if let Some((label, value, note)) = tiles.as_ref().and_then(|t| t.get(i)) {
                        let label = env.i18n().tr(label);
                        ui::Row::value(label, value)
                            .note(note)
                            .focused(focused)
                            .draw(ui, rect);
                    }
                }
                Entry::Chart => {
                    let margin = style.margin();
                    let gap = style.gap();
                    let area = Rect::new(
                        rect.x + margin,
                        rect.y + gap,
                        rect.width.saturating_sub(2 * margin),
                        rect.height.saturating_sub(2 * gap),
                    );
                    self.draw_chart(ui, area);
                }
                Entry::Totals => ui.message(rect, &totals),
                Entry::Go(row) => draw_way(ui, rect, focused, row),
            },
        );
    }

    /// The way on in row `index`, if it is one.
    fn row_at(&self, index: usize) -> Option<Row> {
        match self.entries.get(index) {
            Some(Entry::Go(row)) => Some(*row),
            _ => None,
        }
    }

    /// The tiles: their names, values and notes; none without the time.
    fn tiles(&self, env: &Env<'_>) -> Option<[(&'static str, String, String); 3]> {
        let today = self.today?;
        let week: u32 = (0..7).map(|d| self.seconds_on(today - d)).sum();
        let streak = totals::streak(&self.days, today);
        let pages = env
            .i18n()
            .plural("stats.pages", i64::from(self.today_pages), &[]);
        Some([
            (
                "stats.today",
                fmt::duration(env, u64::from(self.seconds_on(today))),
                pages,
            ),
            (
                "stats.week",
                fmt::duration(env, u64::from(week)),
                env.i18n().tr("stats.last-seven-days").into(),
            ),
            (
                "stats.streak",
                env.i18n().plural("stats.days", i64::from(streak), &[]),
                env.i18n().tr("stats.in-a-row").into(),
            ),
        ])
    }

    /// The room for the text of the tile `i`: up to the next tile's line,
    /// a gap before it; the last up to the margin.
    fn tile_room(&self, i: usize) -> f32 {
        let style = self.style;
        let right = if i + 1 < 3 {
            u32::from(style.gap())
        } else {
            u32::from(style.margin())
        };
        (f32::from(self.tiles.width / 3) - style.margin() as f32 - right as f32).max(0.0)
    }

    /// The largest size at which each tile's value fits, down to the small
    /// one.
    fn value_size(&self, env: &Env<'_>) -> f32 {
        let style = self.style;
        let (Some(font), Some(tiles)) = (style.font(env), self.tiles(env)) else {
            return style.title();
        };
        tiles
            .iter()
            .enumerate()
            .map(|(i, (_, value, _))| {
                widgets::largest_size(style.title(), style.small(), |size| {
                    font.measure(size, value) <= self.tile_room(i)
                })
                .unwrap_or(style.small())
            })
            .fold(style.title(), f32::min)
    }

    fn seconds_on(&self, day: i32) -> u32 {
        fmt::seconds_on(&self.days, day)
    }

    fn open(&self, row: Row, cx: &mut Cx<'_, '_>) {
        cx.command(match row {
            Row::ThisBook(book) => {
                Command::OpenScreen(BOOK, Params::new().with(params::ID, format!("{book}")))
            }
            Row::Calendar => Command::OpenScreen(CALENDAR, Params::new()),
            Row::Books => Command::OpenScreen(BOOKS, Params::new()),
            Row::Sync => Command::OpenScreen(crate::sync::SCREEN, Params::new()),
        });
    }

    fn draw_tiles(&self, ui: &mut ui::Ui<'_>) {
        let style = self.style;
        let env = ui.env();
        let (Some(font), Some(tiles)) = (ui.font(), self.tiles(env)) else {
            return;
        };
        let t = self.tiles;
        let width = t.width / 3;
        let third = t.height / 3;
        let pad = style.margin() as f32;
        let value_size = self.value_size;
        for (i, (label, value, note)) in tiles.iter().enumerate() {
            let x = t.x + width * i as u16;
            let left = f32::from(x) + pad;
            let room = self.tile_room(i);
            let cell = |row: u16| Rect::new(x, t.y + third * row, (pad + room) as u16, third);
            // A little smaller where a text does not fit whole (a longer
            // language), only then shortened.
            let least = style.small() * 0.85;
            let lines = [
                (
                    env.i18n().tr(label),
                    style.small(),
                    least,
                    style.faint(ui::Ink::MUTED),
                ),
                (value.as_str(), value_size, value_size, Color::BLACK),
                (
                    note.as_str(),
                    style.small(),
                    least,
                    style.faint(ui::Ink::MUTED),
                ),
            ];
            for (row, (text, size, least, color)) in lines.into_iter().enumerate() {
                let (size, line) = widgets::fit_line(&font, size, least, text, room);
                ui::Label::new(&line)
                    .px(size)
                    .color(color)
                    .draw_from(ui, left, cell(row as u16));
            }
            if i > 0 {
                ui.canvas().fill_tint(
                    Rect::new(x, t.y, u32::from(style.line()).max(1) as u16, t.height),
                    Color::gray(190),
                );
            }
        }
    }

    fn draw_chart(&self, ui: &mut ui::Ui<'_>, c: Rect) {
        let style = self.style;
        let env = ui.env();
        if ui.font().is_none() {
            return;
        }
        let title_h = (style.small() * 1.6) as u16;
        let title = Rect::new(c.x, c.y, c.width, title_h);
        let label = env.i18n().format(
            "stats.last-days",
            &[("count", Arg::Int(i64::from(CHART_DAYS)))],
        );
        ui::Label::new(&label)
            .px(style.small())
            .color(style.faint(ui::Ink::MUTED))
            .draw_from(ui, f32::from(c.x), title);
        let Some(today) = self.today else {
            return;
        };
        let first = today - CHART_DAYS + 1;
        let values: Vec<u32> = (first..=today).map(|d| self.seconds_on(d)).collect();
        let names: Vec<&str> = (first..=today)
            .map(|d| fmt::weekday_short(env, fmt::date_of(d).weekday))
            .collect();
        let max = values.iter().copied().max().unwrap_or(0);
        let top = if max > 0 {
            fmt::duration(env, u64::from(max))
        } else {
            String::new()
        };
        let area = Rect::new(
            c.x,
            c.y + title_h,
            c.width,
            c.height.saturating_sub(title_h),
        );
        chart::bars(ui, area, &values, &names, (Some(values.len() - 1), &top));
    }

    /// All reading: the time, the books and the days.
    fn totals_text(&self, env: &Env<'_>) -> String {
        let total: u64 = self.days.iter().map(|d| u64::from(d.seconds)).sum();
        env.i18n().format(
            "stats.totals",
            &[
                ("time", Arg::Str(&fmt::duration(env, total))),
                (
                    "books",
                    Arg::Str(
                        &env.i18n()
                            .plural("stats.books-count", self.books as i64, &[]),
                    ),
                ),
                (
                    "days",
                    Arg::Str(
                        &env.i18n()
                            .plural("stats.reading-days", self.days.len() as i64, &[]),
                    ),
                ),
            ],
        )
    }

    /// The totals on a line below the chart.
    fn draw_totals(&self, ui: &mut ui::Ui<'_>) {
        let style = self.style;
        let Some(font) = ui.font() else {
            return;
        };
        let text = self.totals_text(ui.env());
        let w = font.measure(style.small(), &text);
        let x = (f32::from(self.totals.width) - w).max(0.0) / 2.0;
        ui::Label::new(&text)
            .px(style.small())
            .color(style.faint(ui::Ink::NOTE))
            .draw_from(ui, x, self.totals);
    }
}

/// A way on in `rect`: its name and "›".
fn draw_way(ui: &mut ui::Ui<'_>, rect: Rect, focused: bool, row: Row) {
    let key = match row {
        Row::ThisBook(_) => "stats.this-book",
        Row::Calendar => "stats.calendar",
        Row::Books => "stats.books",
        Row::Sync => "stats.sync.title",
    };
    ui::Row::value(ui.tr(key), "›")
        .focused(focused)
        .draw(ui, rect);
}

/// Distinct pages read on local day `day`.
pub(crate) fn distinct_pages(env: &Env<'_>, day: i32) -> u32 {
    let since = fmt::unix_near(day).saturating_sub(14 * 3600).max(0) as u32;
    let mut pages: Vec<(u16, u32)> = Vec::new();
    store::scan_recent(env.storage(), since, |stat| {
        if fmt::day_of(env, stat.start) == day {
            pages.push((stat.book, stat.page));
        }
    });
    pages.sort_unstable();
    pages.dedup();
    pages.len() as u32
}

impl Screen for Overview {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let title = env.i18n().tr("stats.title");
        let mut ui = ui::Ui::new(canvas, env, &style);
        if self.stacked {
            let paging = self.list.paging(self.entries.len());
            self.header.draw(&mut ui, title, None, paging);
            return self.draw_stacked(&mut ui);
        }
        self.header.draw(&mut ui, title, None, None);
        if self.today.is_some() {
            self.draw_tiles(&mut ui);
            self.draw_chart(&mut ui, self.chart);
        } else {
            // Without the time no day is today: what needs it gives way to
            // a hint (and the totals, if anything was read).
            let area = Rect::new(
                0,
                self.tiles.y,
                self.tiles.width,
                self.totals.y - self.tiles.y,
            );
            ui.message(area, env.i18n().tr("stats.no-clock"));
        }
        if self.today.is_some() || !self.days.is_empty() {
            self.draw_totals(&mut ui);
        }
        let entries = &self.entries;
        self.list
            .draw(&mut ui, entries.len(), |ui, index, rect, focused| {
                if let Entry::Go(row) = entries[index] {
                    draw_way(ui, rect, focused, row);
                }
            });
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        // In one column the buttons page as in other lists.
        if self.stacked
            && *a != actions::CONFIRM
            && let Some(change) = self.list.navigate(a, self.entries.len())
        {
            if change != ListChange::None {
                cx.request_redraw();
                cx.request_refresh(RefreshMode::Fast);
            }
            return Handled::Yes;
        }
        let delta = if [actions::UP, actions::LEFT, actions::PREVIOUS].contains(a) {
            -1
        } else if [actions::DOWN, actions::RIGHT, actions::NEXT].contains(a) {
            1
        } else if *a == actions::CONFIRM {
            if let Some(row) = self.row_at(self.list.focus()) {
                self.open(row, cx);
            }
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        self.list.move_focus(delta, self.entries.len()).request(cx);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        if let Some(action) = self.header.tap(cx.env(), &self.style, gesture.start, None) {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        let Some(index) = self.list.hit(gesture.start, self.entries.len()) else {
            return Handled::No;
        };
        self.list.set_focus(index);
        cx.request_redraw_area(self.list.area());
        if let Some(row) = self.row_at(index) {
            self.open(row, cx);
        }
        Handled::Yes
    }
}
