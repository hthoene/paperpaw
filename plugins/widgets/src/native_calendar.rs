//! The calendar: this month as a grid of weeks, the month's name above it,
//! the weekdays' initials, today marked; the weeks' numbers and the next
//! month as well, if wished.
use crate::native_dates::{MonthGrid, civil_from_days, days_from_civil};
use crate::native_draw::{no_time, today};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::Host;
use paperpaw_sdk::{Color, Point, Rect, native::Error};
use paperpaw_ui_kit::widget::Cx;
use paperpaw_ui_kit::widget::names::{chosen_name, month_name, month_short, weekday_name};
use paperpaw_ui_kit::{
    Paint, paint,
    widget::{
        Look,
        layout::{self, Part, cap_height, centred, label, line, share},
        shape::{fill_circle, stroke_circle},
        text::largest_size,
    },
};

/// The height: one month (or two) of six rows, and the frame.
///
/// # Errors
/// Host refusals.
pub fn height(cx: &mut Cx<'_>, width: u16) -> Result<u16, Error> {
    let months: u16 = if cx.settings.shows("next")? { 2 } else { 1 };
    let row = cx.style().row;
    Ok((width * 6 / 7)
        .min((u32::from(row) * 6) as u16)
        .saturating_mul(months)
        .saturating_add(cx.shape.frame_height()))
}

/// What it shows, to tell when it changed: the day.
///
/// # Errors
/// Host refusals.
pub fn shows(host: &Host<'_>, _widget: &str) -> Result<u64, Error> {
    today(host)
}

/// The first day of the week, 0 (Sunday) or 1 (Monday), as the widget's
/// `week-start` says, or as set for the device.
///
/// # Errors
/// Host refusals.
pub fn week_start(cx: &mut Cx<'_>) -> Result<u8, Error> {
    match chosen_name(cx, "week-start")?.as_str() {
        "sunday" => Ok(0),
        "monday" => Ok(1),
        _ => cx.week_start(),
    }
}

/// Draws the widget in `area`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let Some(now) = cx.local_time()? else {
        return no_time(cx, out, &look);
    };
    let area = look.area;
    let start = week_start(cx)?;
    let month = MonthGrid::new(now.year, now.month, start);
    // Two months: side by side where the place is wide, else one below the
    // other; this one alone where two have no room for a whole month each.
    let gap = cx.style().gap * 2;
    let wide = area.width > area.height * 5 / 4;
    let two = if cx.settings.shows("next")? {
        Some(if wide {
            let w = share(area.width, gap, 2);
            (
                Rect::new(area.x, area.y, w, area.height),
                Rect::new(area.x + w + gap, area.y, w, area.height),
            )
        } else {
            let h = share(area.height, gap, 2);
            (
                Rect::new(area.x, area.y, area.width, h),
                Rect::new(area.x, area.y + h + gap, area.width, h),
            )
        })
    } else {
        None
    };
    let weeks = month.weeks.max(month.next().weeks);
    if let Some((first, second)) = two
        && grid(cx, first, weeks, true)?.is_some()
    {
        let one = Place {
            look: &look,
            area: first,
            month: &month,
            today: Some(now.day),
            weeks_shown: weeks,
        };
        draw_month(cx, out, &one)?;
        let next = month.next();
        let other = Place {
            look: &look,
            area: second,
            month: &next,
            today: None,
            weeks_shown: weeks,
        };
        return draw_month(cx, out, &other);
    }
    draw_month(
        cx,
        out,
        &Place {
            look: &look,
            area,
            month: &month,
            today: Some(now.day),
            weeks_shown: month.weeks,
        },
    )
}

/// How a month's grid is laid out in a place: which parts show, the cells'
/// size and the numbers' size (one the font draws in a cell).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Grid {
    heading: bool,
    initials: bool,
    weeks: bool,
    cell_w: f32,
    cell_h: f32,
    size: f32,
}

/// The grid of `rows` weeks in `area`, its parts as wished; where the numbers
/// have no room, the week numbers, the heading, the initials left out in turn
/// (the initials only with `all`). `None` where the numbers do not fit even
/// so: the font draws none that small.
fn grid(cx: &Cx<'_>, area: Rect, rows: u8, all: bool) -> Result<Option<Grid>, Error> {
    let (small, title) = (cx.style().small, cx.style().title);
    let mut grid = Grid {
        heading: cx.settings.shows("heading")?,
        initials: cx.settings.shows("weekdays")?,
        weeks: cx.settings.shows("weeks")?,
        cell_w: 0.0,
        cell_h: 0.0,
        size: 0.0,
    };
    loop {
        let columns = 7 + u16::from(grid.weeks);
        let lines = u16::from(rows) + u16::from(grid.initials);
        // The heading as high as a row would be, a little more.
        let lines = f32::from(lines) + if grid.heading { 1.3 } else { 0.0 };
        grid.cell_h = f32::from(area.height) / lines;
        grid.cell_w = f32::from(area.width) / f32::from(columns);
        // Two digits with room round them in each cell, as drawn, and no
        // smaller than four fifths of the small size.
        let (w, h) = (grid.cell_w, grid.cell_h);
        let most = (h * 0.5).min(w * 0.42);
        let size = largest_size(most, small * 0.8, |size| {
            Ok(cx.font.measure_text(size, "00")? <= w * 0.8
                && cap_height(&cx.font, size)? <= h * 0.72)
        })?;
        // The heading's line in its row.
        let heading_fits = match size {
            Some(size) => {
                let heading = (size * 1.15).min(title * 1.6);
                line(&cx.font, heading)? * 0.8 <= h * 1.3
            }
            None => false,
        };
        match size {
            Some(size) if !grid.heading || heading_fits => {
                grid.size = size;
                return Ok(Some(grid));
            }
            _ if grid.weeks => grid.weeks = false,
            _ if grid.heading => grid.heading = false,
            _ if grid.initials && all => grid.initials = false,
            _ => return Ok(None),
        }
    }
}

/// A month in a place, with the look it is drawn in.
struct Place<'a> {
    look: &'a Look,
    area: Rect,
    month: &'a MonthGrid,
    today: Option<u8>,
    /// Room for this many weeks (so that two months side by side line up).
    weeks_shown: u8,
}

/// Draws a month into its place, `today` marked. Where the whole month has no
/// room, the week of today alone; where that has none either, today's number
/// large under the month's name.
#[allow(clippy::too_many_lines, reason = "one drawing")]
fn draw_month(cx: &mut Cx<'_>, out: &mut Vec<Paint>, place: &Place<'_>) -> Result<(), Error> {
    let Place {
        look,
        area,
        month,
        today,
        weeks_shown,
    } = *place;
    let whole = grid(cx, area, weeks_shown, true)?;
    let (grid, rows) = match (whole, today) {
        (Some(grid), _) => (grid, 0..month.weeks),
        (None, Some(day)) => match self::grid(cx, area, 1, false)? {
            Some(grid) => {
                let week = month.week_of(day);
                (grid, week..week + 1)
            }
            None => return draw_today(cx, out, place, day),
        },
        (None, None) => return Ok(()),
    };
    let title = cx.style().title;
    let Grid {
        heading,
        initials,
        weeks,
        cell_w,
        cell_h,
        size,
    } = grid;
    let small = size * 0.82;
    // The size the font draws at (the nearest it has).
    let drawn = cap_height(&cx.font, size)? / 0.74;
    // The cells fill the place from top to bottom.
    let mut top = f32::from(area.y);
    let left = f32::from(area.x) + if weeks { cell_w } else { 0.0 };
    if heading {
        let name = month_name(cx, month.month)?;
        let text = if cx.settings.shows("year")? {
            let year = cx.number(i64::from(month.year))?;
            paperpaw_ui_kit::text::concat(&[&name, " ", &year])?
        } else {
            name
        };
        let heading_size = (size * 1.15).min(title * 1.6);
        // In line with the numbers' ink below, not the cells' edges.
        let inset = ((cell_w - cx.font.measure_text(size, "00")?) / 2.0).max(0.0);
        let row = Rect::new(
            (left + inset) as u16,
            top as u16,
            (f32::from(area.width) - (left - f32::from(area.x)) - 2.0 * inset).max(0.0) as u16,
            (cell_h * 1.3 * 0.8) as u16,
        );
        let align = cx.settings.align()?;
        label(out, &cx.font, (heading_size, align), row, &text, look.ink)?;
        top += cell_h * 1.3;
    }
    if initials {
        let two = chosen_name(cx, "initials")?.as_str() == "two";
        for column in 0..7u8 {
            let name = weekday_name(cx, month.weekday_of(column))?;
            let mut initial = String::new();
            initial.try_reserve(8).map_err(|_| Error::NoMemory)?;
            initial.extend(name.chars().take(if two { 2 } else { 1 }));
            let x = left + cell_w * (f32::from(column) + 0.5);
            centred(
                out,
                &cx.font,
                small,
                (x, top + cell_h / 2.0),
                &initial,
                look.faint,
            )?;
        }
        top += cell_h;
    }
    let others = cx.settings.shows("others")?;
    let weekends = cx.settings.shows("weekends")?;
    let mark = chosen_name(cx, "mark")?;
    let first = days_from_civil(month.year, month.month, 1);
    for (row, week) in rows.enumerate() {
        let y = top + cell_h * (row as f32 + 0.5);
        if weeks {
            let number = cx.number(i64::from(month.week_number(week)))?;
            let x = f32::from(area.x) + cell_w / 2.0;
            centred(out, &cx.font, small, (x, y), &number, look.faint)?;
        }
        for column in 0..7u8 {
            let day = month.day_at(week, column);
            let x = left + cell_w * (f32::from(column) + 0.5);
            let inside = (0..i32::from(month.days)).contains(&day);
            if !inside && !others {
                continue;
            }
            let date = civil_from_days(first + i64::from(day));
            let text = cx.number(i64::from(date.2))?;
            let mut color = if inside { look.ink } else { look.faint };
            if weekends && matches!(month.weekday_of(column), 0 | 6) {
                color = look.faint;
            }
            if inside && today == Some((day + 1) as u8) {
                let wide = cx.font.measure_text(size, &text)?;
                color = draw_mark(
                    out,
                    look,
                    mark.as_str(),
                    (x, y),
                    (cell_w, cell_h),
                    (drawn, wide),
                )?;
            }
            let shown = if inside { size } else { small };
            centred(out, &cx.font, shown, (x, y), &text, color)?;
        }
    }
    Ok(())
}

/// Today alone, where not even a week has room: the month's name (or its
/// short name) small above the day's number as large as fits, the two in the
/// middle; the name left out where both have no room.
fn draw_today(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    place: &Place<'_>,
    day: u8,
) -> Result<(), Error> {
    let area = place.area;
    let (small, text_size, title, gap) = {
        let style = cx.style();
        (style.small, style.text, style.title, f32::from(style.gap))
    };
    let width = f32::from(area.width);
    let long = month_name(cx, place.month.month)?;
    let short = month_short(cx, place.month.month)?;
    let name = if cx.font.measure_text(small, &long)? <= width {
        Some(long)
    } else if cx.font.measure_text(small, &short)? <= width {
        Some(short)
    } else {
        None
    };
    let name_h = line(&cx.font, small)?;
    let number = cx.number(i64::from(day))?;
    let fits = |size: f32, name: bool| -> Result<bool, Error> {
        let room = f32::from(area.height) - if name { name_h } else { 0.0 };
        Ok(cx.font.measure_text(size, &number)? <= width && line(&cx.font, size)? <= room)
    };
    let most = f32::from(area.height).min(title * 3.0);
    // With the name where the number keeps the body size at least.
    let with_name = if name.is_some() {
        largest_size(most, text_size, |s| fits(s, true))?
    } else {
        None
    };
    let (size, name) = match with_name {
        Some(size) => (size, name),
        None => match largest_size(most, small, |s| fits(s, false))? {
            Some(size) => (size, None),
            // Not even the number has room: nothing rather than a part.
            None => return Ok(()),
        },
    };
    // The two centred together by their ink, half a gap apart.
    let mut parts: Vec<Part<'_>> = Vec::new();
    parts.try_reserve(2).map_err(|_| Error::NoMemory)?;
    if let Some(name) = &name {
        let mut lines: Vec<&str> = Vec::new();
        lines.try_reserve(1).map_err(|_| Error::NoMemory)?;
        lines.push(name);
        parts.push(Part {
            lines,
            size: small,
            color: place.look.faint,
        });
    }
    let mut lines: Vec<&str> = Vec::new();
    lines.try_reserve(1).map_err(|_| Error::NoMemory)?;
    lines.push(&number);
    parts.push(Part {
        lines,
        size,
        color: place.look.ink,
    });
    let align = cx.settings.align()?;
    layout::stack(out, &cx.font, area, align, &parts, gap / 2.0)
}

/// Draws today's mark round the number at `(x, y)` and returns the number's
/// colour on it.
fn draw_mark(
    out: &mut Vec<Paint>,
    look: &Look,
    mark: &str,
    (x, y): (f32, f32),
    (cell_w, cell_h): (f32, f32),
    (size, wide): (f32, f32),
) -> Result<Color, Error> {
    // Round the number, but never into the next row or column.
    let radius = (cell_w.min(cell_h) * 0.46)
        .min(size * 1.15)
        .max(size * 0.78)
        .min(cell_w.min(cell_h) * 0.5);
    let centre = Point::new(x as i32, y as i32);
    // The number's ink with a little room round it.
    let pad = (size * 0.12).max(1.0);
    let cap = size * 0.74;
    let cramped = wide / 2.0 + pad > radius || cap / 2.0 + pad > radius;
    match mark {
        "square" => {
            let r = radius as u16;
            let at = Rect::new((x - radius) as u16, (y - radius) as u16, r * 2, r * 2);
            paint::push(out, Paint::Focus(at, (radius * 0.3) as u32, None, look.ink))?;
            Ok(look.paper)
        }
        "ring" => {
            let thick = (radius / 7.0).max(1.5) as u32;
            stroke_circle(out, centre, radius as u32, thick, look.ink)?;
            Ok(look.ink)
        }
        "underline" => {
            let thick = (size / 9.0).max(2.0) as u16;
            let w = (size * 1.1) as u16;
            let at = Rect::new(
                (x - f32::from(w) / 2.0) as u16,
                (y + size * 0.48) as u16,
                w,
                thick,
            );
            paint::push(out, Paint::Focus(at, u32::from(thick / 2), None, look.ink))?;
            Ok(look.ink)
        }
        // Where a disc would touch the number, a box round it: as wide as the
        // number and a little room, never into the next column.
        _ if cramped => {
            let w = (wide + 2.0 * pad).min(cell_w - 2.0);
            let h = (cap + 2.0 * pad).min(cell_h - 2.0);
            let at = Rect::new(
                (x - w / 2.0) as u16,
                (y - h / 2.0) as u16,
                w as u16,
                h as u16,
            );
            paint::push(out, Paint::Focus(at, (h * 0.2) as u32, None, look.ink))?;
            Ok(look.paper)
        }
        _ => {
            fill_circle(out, centre, radius as u32, look.ink)?;
            Ok(look.paper)
        }
    }
}
