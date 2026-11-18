//! The month as a grid of days: painted for the Month screen and for the
//! *Month* widget alike ([`paint_month`]), and the Month screen itself (a
//! header, the grid, and below it what the chosen day holds).
use alloc::{string::String, vec::Vec};
use paperpaw_caldav::date::{civil_from_days, days_from_civil, days_in_month, weekday};
use paperpaw_sdk::{
    Color, Point, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose},
        fonts::{Font, FontRole},
        snapshot,
    },
};
use paperpaw_ui_kit::{
    Messages, Page, Paint, Style,
    geometry::line_px,
    header,
    mem::push,
    paint,
    text::decimal,
    widget::{
        Align,
        layout::label,
        shape::{fill, fill_circle},
    },
};

/// Colours of what is painted.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub ink: Color,
    pub faint: Color,
    pub paper: Color,
}

/// What a month shows.
#[derive(Debug, Clone, Copy)]
pub struct Month<'a> {
    pub year: i32,
    pub month: u8,
    /// The first day of the week: 0 is Sunday, 1 Monday.
    pub week_start: u8,
    pub today: i32,
    pub selected: Option<i32>,
    /// How many events fall on each day of the month, the first day first.
    pub busy: &'a [u8],
    pub dots: bool,
    /// The short names of the weekdays from Sunday; none to leave the row out.
    pub names: &'a [String],
}

/// The first day of the grid of a month and how many weeks it has.
#[must_use]
pub fn grid_of(year: i32, month: u8, week_start: u8) -> (i32, u8) {
    let first = i32::try_from(days_from_civil(year, month, 1)).unwrap_or(0);
    let lead = (i32::from(weekday(i64::from(first))) - i32::from(week_start) + 7) % 7;
    let weeks = (lead + i32::from(days_in_month(year, month)) + 6) / 7;
    (first - lead, u8::try_from(weeks).unwrap_or(6))
}

/// Paints the month in `area`, the weekday names above the weeks; where each
/// day is, for taps.
///
/// # Errors
/// Font refusals, `NoMemory`.
pub fn paint_month(
    out: &mut Vec<Paint>,
    font: &Font<'_>,
    (size, line): (f32, u16),
    palette: Palette,
    area: Rect,
    shown: &Month<'_>,
) -> Result<Vec<(Rect, i32)>, Error> {
    let (start, weeks) = grid_of(shown.year, shown.month, shown.week_start);
    let head = if shown.names.is_empty() {
        0
    } else {
        line_px(font, size)?.saturating_add(line_px(font, size)? / 4)
    };
    let (columns, rows) = (7u32, u32::from(weeks));
    let cell_h = (u32::from(area.height.saturating_sub(head)) / rows).max(1);
    let mut cells = Vec::new();
    cells
        .try_reserve(usize::from(weeks) * 7)
        .map_err(|_| Error::NoMemory)?;
    if head > 0 {
        for column in 0..7u32 {
            let name = shown
                .names
                .get(((u32::from(shown.week_start) + column) % 7) as usize)
                .map_or("", String::as_str);
            let band = Rect::new(
                clip_x(area, column, columns),
                area.y,
                clip_w(area, column, columns),
                head,
            );
            label(
                out,
                font,
                (size * 0.8, Align::Center),
                band,
                name,
                palette.faint,
            )?;
        }
    }
    for row in 0..rows {
        for column in 0..columns {
            let index = i32::try_from(row * columns + column).unwrap_or(0);
            let day = start + index;
            let (_, month, date) = civil_from_days(i64::from(day));
            let cell = Rect::new(
                clip_x(area, column, columns),
                area.y
                    .saturating_add(head)
                    .saturating_add(u16::try_from(row * cell_h).unwrap_or(0)),
                clip_w(area, column, columns),
                u16::try_from(cell_h).unwrap_or(1),
            );
            push(&mut cells, (cell, day))?;
            let day = DayCell {
                day,
                date,
                inside: month == shown.month,
                chosen: shown.selected == Some(day),
                busy: shown
                    .busy
                    .get(usize::from(date).saturating_sub(1))
                    .is_some_and(|n| *n > 0)
                    && shown.dots,
            };
            paint_day(out, font, (size, line), palette, (cell, &day, shown.today))?;
        }
    }
    Ok(cells)
}

/// What a day of the grid is.
struct DayCell {
    day: i32,
    date: u8,
    inside: bool,
    chosen: bool,
    busy: bool,
}

/// Paints one day: its number (the chosen one in a bar, today in a frame) and
/// a dot under it where something falls on it.
fn paint_day(
    out: &mut Vec<Paint>,
    font: &Font<'_>,
    (size, line): (f32, u16),
    palette: Palette,
    (cell, day, today): (Rect, &DayCell, i32),
) -> Result<(), Error> {
    let mut digits = [0u8; 20];
    let number = decimal(usize::from(day.date), &mut digits);
    let mut color = if day.inside {
        palette.ink
    } else {
        palette.faint
    };
    let radius = u32::from(cell.width.min(cell.height)) / 6;
    if day.chosen {
        paint::push(out, Paint::Focus(inset(cell, 1), radius, None, palette.ink))?;
        color = palette.paper;
    } else if day.day == today {
        paint::push(
            out,
            Paint::Focus(
                inset(cell, 1),
                radius,
                Some(u32::from(line.max(1))),
                palette.ink,
            ),
        )?;
    }
    let band = Rect::new(
        cell.x,
        cell.y,
        cell.width,
        cell.height.saturating_sub(cell.height / 4),
    );
    label(out, font, (size, Align::Center), band, number, color)?;
    if day.inside && day.busy {
        let r = (size / 9.0).max(2.0) as u32;
        let centre = Point::new(
            i32::from(cell.x) + i32::from(cell.width) / 2,
            i32::from(cell.y) + i32::from(cell.height) - i32::try_from(r).unwrap_or(2) - 2,
        );
        let dot = if day.chosen {
            palette.paper
        } else {
            palette.ink
        };
        fill_circle(out, centre, r, dot)?;
    }
    Ok(())
}

fn inset(rect: Rect, by: u16) -> Rect {
    Rect::new(
        rect.x.saturating_add(by),
        rect.y.saturating_add(by),
        rect.width.saturating_sub(by * 2),
        rect.height.saturating_sub(by * 2),
    )
}

/// The left edge and the width of column `n` of `count` in `area`, so that
/// the columns fill it exactly.
fn clip_x(area: Rect, n: u32, count: u32) -> u16 {
    area.x
        .saturating_add(u16::try_from(u32::from(area.width) * n / count).unwrap_or(0))
}

fn clip_w(area: Rect, n: u32, count: u32) -> u16 {
    let end = u32::from(area.width) * (n + 1) / count;
    let begin = u32::from(area.width) * n / count;
    u16::try_from(end - begin).unwrap_or(1)
}

/// The Month screen as it was drawn last: its header and where each day is.
#[derive(Debug, Default)]
pub struct Grid {
    pub page: Page,
    pub cells: Vec<(Rect, i32)>,
}

impl Grid {
    /// The day under a tap.
    #[must_use]
    pub fn day_at(&self, point: Point) -> Option<i32> {
        let (x, y) = (point.x, point.y);
        self.cells.iter().find_map(|(rect, day)| {
            let inside = x >= i32::from(rect.x)
                && x < i32::from(rect.x) + i32::from(rect.width)
                && y >= i32::from(rect.y)
                && y < i32::from(rect.y) + i32::from(rect.height);
            inside.then_some(*day)
        })
    }
}

/// What the Month screen shows.
#[derive(Debug, Clone, Copy)]
pub struct Shown<'a> {
    /// The header: the month and the year.
    pub title: &'a str,
    pub month: Month<'a>,
    /// What the chosen day holds, a line each (ended by a line saying there
    /// is more, where there is).
    pub lines: &'a [String],
    /// What to say where the day holds nothing.
    pub empty: &'a str,
}

/// Draws the Month screen.
///
/// # Errors
/// Font and host refusals, `NoMemory`.
pub fn draw(
    host: &Host<'_>,
    frame: &mut DrawFrame<'_>,
    grid: &mut Grid,
    shown: &Shown<'_>,
) -> Result<(), Error> {
    if !matches!(frame.purpose, DrawPurpose::Screen) {
        return Ok(());
    }
    let display = snapshot::capture_snapshot(host)?;
    let font = Font::acquire_role(host, FontRole::Ui)?;
    let result = paint_screen(host, frame, grid, shown, &display, &font);
    font.release()?;
    result
}

fn paint_screen(
    host: &Host<'_>,
    frame: &mut DrawFrame<'_>,
    grid: &mut Grid,
    shown: &Shown<'_>,
    display: &snapshot::Snapshot,
    font: &Font<'_>,
) -> Result<(), Error> {
    let mut style = Style::read(host, display, font)?;
    style.describing = paint::asking(frame);
    let (width, height) = (display.display_width(), display.display_height());
    let mut messages = Messages::new()?;
    let mut page = Page::default();
    header::prepare(
        &mut page,
        host,
        font,
        &style,
        &mut messages,
        shown.title,
        width,
        display.has_touch(),
        0,
        1,
    )?;
    let top = style.header.saturating_add(style.gap);
    let margin = style.margin.max(style.gap);
    let body = Rect::new(
        margin,
        top,
        width.saturating_sub(margin.saturating_mul(2)),
        height.saturating_sub(top).saturating_sub(margin),
    );
    let mut ops: Vec<Paint> = Vec::new();
    fill(
        &mut ops,
        Rect::new(0, top, width, height.saturating_sub(top)),
        style.paper,
    )?;
    // The grid takes two thirds of what is there; the day's events the rest.
    let line = line_px(font, style.text)?;
    let below = line.saturating_mul(4).saturating_add(style.gap);
    let grid_h = body.height.saturating_sub(below).max(body.height / 2);
    let area = Rect::new(body.x, body.y, body.width, grid_h);
    let palette = Palette {
        ink: style.ink,
        faint: style.muted,
        paper: style.paper,
    };
    let cells = paint_month(
        &mut ops,
        font,
        (style.text, style.line),
        palette,
        area,
        &shown.month,
    )?;
    let mut y = body.y.saturating_add(grid_h).saturating_add(style.gap);
    let end = body.y.saturating_add(body.height);
    let rule = Rect::new(
        body.x,
        y.saturating_sub(style.gap / 2),
        body.width,
        style.line.max(1),
    );
    fill(&mut ops, rule, style.rule)?;
    if shown.lines.is_empty() {
        let row = Rect::new(body.x, y, body.width, line);
        label(
            &mut ops,
            font,
            (style.small, Align::Left),
            row,
            shown.empty,
            style.muted,
        )?;
    }
    for text in shown.lines {
        if y.saturating_add(line) > end {
            break;
        }
        let row = Rect::new(body.x, y, body.width, line);
        label(
            &mut ops,
            font,
            (style.text, Align::Left),
            row,
            text,
            style.ink,
        )?;
        y = y.saturating_add(line);
    }
    paint::replay(frame, font, page.paint.iter().chain(ops.iter()))?;
    page.paint = Vec::new();
    grid.page = page;
    grid.cells = cells;
    Ok(())
}
