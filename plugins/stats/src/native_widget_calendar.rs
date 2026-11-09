//! *Reading calendar*: the days of this month, or the last weeks as a heat map
//! (a column a week, a row a weekday), each day shaded by how long one read
//! (`level`: with a daily goal by its share, else by minutes), today marked;
//! below, the streak and the time read in the days shown.
use crate::declarations::{CALENDAR_CELLS, CALENDAR_STYLES};
use crate::native_widgets::{
    caption_room, captioned, days, goal, mm, pick_caption, shade, stamp, text_at, today_of,
};
use crate::texts::{date_of, duration, month, month_start, seconds_on, weekday_short};
use crate::totals;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::{Color, Rect};
use paperpaw_ui_kit::widget::text::{ink_baseline, largest_size};
use paperpaw_ui_kit::widget::{Cx, Look};
use paperpaw_ui_kit::{Paint, geometry, paint, text};

/// What the calendar shows.
#[derive(Debug, PartialEq, Eq)]
struct Shown {
    /// The first day shown: the start of a week.
    first: i32,
    today: i32,
    /// The heat map's weeks; 0 for the month.
    weeks: u8,
    /// Each day's shade from `first` on, to the month's end or today.
    levels: Vec<u8>,
    /// Seconds read in the days shown.
    total: u64,
    streak: u32,
}

/// What the figures were worked out for: the stamp, the goal, the style, the
/// weeks and the first day of the week.
type Key = (u64, u64, bool, u8, u8);

/// What a calendar widget worked out, and what for: the day log is read again
/// only when that changed, never at each draw.
#[derive(Debug, Default)]
pub(crate) struct Cache(core::cell::RefCell<Option<(Key, Shown)>>);

/// How dark a day of `seconds` reading is shaded, 0 (none) to 4: with a daily
/// goal by its share (a third, two thirds, nearly, reached), else by minutes
/// (under 15, under 30, under 60, an hour or more).
pub(crate) fn level(seconds: u32, goal: Option<u64>) -> u8 {
    let seconds = u64::from(seconds);
    if seconds == 0 {
        return 0;
    }
    match goal.filter(|&g| g > 0) {
        Some(goal) if seconds >= goal => 4,
        Some(goal) => 1 + (seconds * 3 / goal) as u8,
        None => match seconds / 60 {
            0..=14 => 1,
            15..=29 => 2,
            30..=59 => 3,
            _ => 4,
        },
    }
}

/// Whether text on a shade of `level` is drawn in the paper's colour.
fn on_dark(level: u8) -> bool {
    level >= 3
}

/// The start of the week `day` is in.
fn week_start_of(day: i32, first_day: u8) -> i32 {
    let weekday = i32::from(date_of(day).weekday);
    day - (weekday - i32::from(first_day)).rem_euclid(7)
}

/// The last day of the month of `day`.
fn month_end(day: i32) -> i32 {
    month_start(month_start(day).saturating_add(31)) - 1
}

/// The heat map's weeks, by its setting.
fn weeks_set(cx: &Cx<'_>) -> Result<u8, Error> {
    cx.settings.with_text("weeks", |v| {
        v.rsplit('.')
            .next()
            .and_then(|n| n.parse::<u8>().ok())
            .map_or(12, |w| w.clamp(1, 53))
    })
}

/// Each day's shade from `first` to `last`, the seconds read then.
fn levels(
    days: &[crate::store::Day],
    (first, last): (i32, i32),
    goal: Option<u64>,
) -> Result<(Vec<u8>, u64), Error> {
    let mut total = 0;
    let mut out = Vec::new();
    out.try_reserve_exact((last - first + 1).max(0) as usize)
        .map_err(|_| Error::NoMemory)?;
    for day in first..=last {
        let seconds = seconds_on(days, day);
        total += u64::from(seconds);
        out.push(level(seconds, goal));
    }
    Ok((out, total))
}

/// Makes the cache hold the figures for what the calendar is drawn for (worked
/// out again only where something changed); whether it holds them (not where
/// the device does not know the time). Nothing is copied: the draw reads them
/// in the cache.
fn refresh(cx: &mut Cx<'_>, cache: &Cache) -> Result<bool, Error> {
    let heat = cx
        .settings
        .with_text("style", |v| v == CALENDAR_STYLES[1])?;
    let (weeks, first_day) = (weeks_set(cx)?, cx.week_start()?);
    let goal = goal(cx.host)?;
    let key = (stamp(cx.host)?, goal.unwrap_or(0), heat, weeks, first_day);
    if cache
        .0
        .borrow()
        .as_ref()
        .is_some_and(|(kept, _)| *kept == key)
    {
        return Ok(true);
    }
    let Some(made) = work_out(cx, goal, heat, (weeks, first_day))? else {
        return Ok(false);
    };
    *cache.0.borrow_mut() = Some((key, made));
    Ok(true)
}

fn work_out(
    cx: &Cx<'_>,
    goal: Option<u64>,
    heat: bool,
    (weeks, first_day): (u8, u8),
) -> Result<Option<Shown>, Error> {
    let Some((today, days)) = days(cx)? else {
        return Ok(None);
    };
    let (first, last, weeks) = if heat {
        let first = week_start_of(today, first_day) - 7 * (i32::from(weeks) - 1);
        (first, today, weeks)
    } else {
        let start = month_start(today);
        (week_start_of(start, first_day), month_end(today), 0)
    };
    let (levels, mut total) = levels(&days, (first, last), goal)?;
    // The total of the month itself, or of the weeks shown: of the month the
    // days of the week before its first are left out (at most six).
    if !heat {
        total -= (first..month_start(today))
            .map(|day| u64::from(seconds_on(&days, day)))
            .sum::<u64>();
    }
    Ok(Some(Shown {
        first,
        today,
        weeks,
        levels,
        total,
        streak: totals::streak(&days, today),
    }))
}

/// The line a text of `size` takes.
fn line(cx: &Cx<'_>, size: f32) -> Result<u16, Error> {
    geometry::line_px(&cx.font, size)
}

/// The height of the row of weekdays above the month, if it shows.
fn weekdays_height(cx: &Cx<'_>) -> Result<u16, Error> {
    if cx.settings.shows("weekdays")? {
        line(cx, cx.style().small)
    } else {
        Ok(0)
    }
}

/// The height of the footer, if either of its parts shows.
fn footer_height(cx: &Cx<'_>) -> Result<u16, Error> {
    if cx.settings.shows("streak")? || cx.settings.shows("total")? {
        Ok(line(cx, cx.style().small)?.saturating_add(cx.style().gap / 2))
    } else {
        Ok(0)
    }
}

/// The height the widget takes at `width`: the frame, the caption, the
/// weekdays, the days and the footer.
///
/// # Errors
/// Font and host refusals.
pub fn height(cx: &mut Cx<'_>, width: u16) -> Result<u16, Error> {
    let heat = cx
        .settings
        .with_text("style", |v| v == CALENDAR_STYLES[1])?;
    let inner = width.saturating_sub(cx.shape.frame_height());
    // Square days across the width, at most twice a line high.
    let cell = (inner / 7).min(line(cx, cx.style().small)?.saturating_mul(2));
    let today = today_of(cx.host)?;
    let (rows, header, cell) = match today {
        Some(_) if heat => {
            let weeks = u16::from(weeks_set(cx)?);
            (7, 0, (inner / (weeks + 1)).min(cell))
        }
        Some(today) => {
            let first_day = cx.week_start()?;
            let first = week_start_of(month_start(today), first_day);
            let days = month_end(today) - first + 1;
            ((days.max(0) as u16).div_ceil(7), weekdays_height(cx)?, cell)
        }
        None => (7, 0, cell),
    };
    Ok(cx
        .shape
        .frame_height()
        .saturating_add(caption_room(cx)?)
        .saturating_add(header)
        .saturating_add(rows.saturating_mul(cell))
        .saturating_add(footer_height(cx)?))
}

/// "5 days in a row", "6 h 20 min in October", as far as these show.
fn footer(cx: &mut Cx<'_>, shown: &Shown) -> Result<Vec<String>, Error> {
    let mut parts: Vec<String> = Vec::new();
    parts.try_reserve_exact(2).map_err(|_| Error::NoMemory)?;
    if cx.settings.shows("streak")? && shown.streak > 0 {
        parts.push(cx.plural("stats.goal.streak", i64::from(shown.streak), &[])?);
    }
    if cx.settings.shows("total")? {
        let time = duration(cx, shown.total)?;
        parts.push(if shown.weeks == 0 {
            let name = month(cx, date_of(shown.today).month)?;
            cx.format(
                "stats.widget.calendar.month-total",
                &[
                    Arg::Text {
                        name: "time",
                        value: &time,
                    },
                    Arg::Text {
                        name: "month",
                        value: &name,
                    },
                ],
            )?
        } else {
            cx.format(
                "stats.widget.calendar.weeks-total",
                &[
                    Arg::Text {
                        name: "time",
                        value: &time,
                    },
                    Arg::Int {
                        name: "count",
                        value: i64::from(shown.weeks),
                    },
                ],
            )?
        });
    }
    Ok(parts)
}

/// The footer that fits `width` whole: all its parts, else the total alone,
/// else the streak alone; none where not even one fits.
fn footer_fitting(cx: &mut Cx<'_>, shown: &Shown, width: u16) -> Result<Option<String>, Error> {
    let small = cx.style().small;
    let parts = footer(cx, shown)?;
    let wide = f32::from(width);
    if parts.len() == 2 {
        let all = text::concat(&[&parts[0], " · ", &parts[1]])?;
        if cx.font.measure_text(small, &all)? <= wide {
            return Ok(Some(all));
        }
    } else if let Some(only) = parts.first()
        && cx.font.measure_text(small, only)? <= wide
    {
        return Ok(parts.into_iter().next());
    }
    for part in parts.into_iter().rev() {
        if cx.font.measure_text(small, &part)? <= wide {
            return Ok(Some(part));
        }
    }
    Ok(None)
}

/// Draws `text`, the footer, at the bottom of `block`.
fn draw_footer(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    block: Rect,
    words: &str,
) -> Result<(), Error> {
    let small_px = cx.style().small;
    let small = line(cx, small_px)?;
    let x = cx
        .settings
        .align()?
        .x(block, cx.font.measure_text(small_px, words)?);
    let row = Rect::new(
        block.x,
        block.y.saturating_add(block.height.saturating_sub(small)),
        block.width,
        small,
    );
    text_at(cx, out, words, (small_px, look.faint), (x, row))
}

/// The room a day's shade keeps from its cell's edges.
fn cell_pad(cell: u16) -> u16 {
    (cell / 12).max(1)
}

/// The size of the days' numbers in cells `cell` pixels wide: the body size,
/// smaller where the widest number is wider than seven tenths of the cell (so
/// that numbers keep apart) or its ink higher than three fifths of the day's
/// shade, measured as drawn; `None` where not even the least fits (the days
/// show as dots then).
fn number_size(cx: &Cx<'_>, cell: u16) -> Result<Option<f32>, Error> {
    let inner = cell.saturating_sub(2 * cell_pad(cell));
    let width = f32::from(cell) * 0.7;
    let height = i32::from(inner) * 3 / 5;
    largest_size(cx.style().text, cx.style().small * 0.7, |size| {
        let tall = match cx.font.ink(size, "30")? {
            Some(ink) => ink.bottom.saturating_sub(ink.top),
            None => i32::MAX,
        };
        Ok(cx.font.measure_text(size, "30")? <= width && tall <= height)
    })
}

/// A day: its cell, its shade, today's ring and its number (`size` large, or
/// a dot where there is no number and nothing was read).
struct Day {
    cell: Rect,
    level: u8,
    number: Option<(u8, f32)>,
    today: bool,
}

/// A day's cell: its shade, today's ring, its number.
#[inline(never)]
fn draw_day(cx: &Cx<'_>, out: &mut Vec<Paint>, look: &Look, day: &Day) -> Result<(), Error> {
    let Day {
        cell,
        level,
        number,
        today,
    } = *day;
    let round = cx.settings.with_text("cells", |v| v == CALENDAR_CELLS[1])?;
    let pad = cell_pad(cell.width);
    let inner = Rect::new(
        cell.x.saturating_add(pad),
        cell.y.saturating_add(pad),
        cell.width.saturating_sub(2 * pad),
        cell.height.saturating_sub(2 * pad),
    );
    let radius = |r: Rect| {
        u32::from(if round {
            r.width.min(r.height) / 2
        } else {
            r.width.min(r.height) / 5
        })
    };
    // Numbers do not read on darker dithered shades: where the panel has few
    // grays, days with numbers get the lightest shade and a bar as long as
    // their level instead.
    let few = cx.snapshot.display_gray_levels().is_some_and(|g| g <= 4);
    let barred = number.is_some() && few && (1..4).contains(&level);
    let thick = (u32::from(inner.height) / 8).max(2);
    if level >= 4 {
        paint::push(out, Paint::Focus(inner, radius(inner), None, look.ink))?;
    } else if barred {
        paint::push(
            out,
            Paint::Focus(inner, radius(inner), None, shade(look.paper, 1)),
        )?;
        let full = u32::from(inner.width) * 3 / 5;
        let len = (full * u32::from(level) / 3).max(thick);
        let bar = Rect::new(
            inner.x.saturating_add((inner.width - len as u16) / 2),
            inner
                .y
                .saturating_add(inner.height)
                .saturating_sub((thick * 2) as u16),
            len as u16,
            thick as u16,
        );
        paint::push(out, Paint::Focus(bar, thick / 2, None, look.ink))?;
    } else if level > 0 {
        paint::push(
            out,
            Paint::Focus(inner, radius(inner), None, shade(look.paper, level)),
        )?;
    }
    if today {
        let ring = (u32::from(cx.style().line) * 2)
            .min(u32::from(pad.max(1)) * 2)
            .max(2);
        paint::push(out, Paint::Focus(cell, radius(cell), Some(ring), look.ink))?;
    }
    match number {
        Some(_) => number_in(cx, out, look, day, (inner, barred, thick))?,
        None if level == 0 && !today => {
            let r = u32::from(cell.width / 10).max(1);
            let at = Rect::new(
                (cell.x + cell.width / 2).saturating_sub(r as u16),
                (cell.y + cell.height / 2).saturating_sub(r as u16),
                (r * 2) as u16,
                (r * 2) as u16,
            );
            paint::push(out, Paint::Focus(at, r, None, look.faint))?;
        }
        None => {}
    }
    Ok(())
}

/// `day` as text, in `buf`: without a heap, as it is drawn for each day.
fn two_digits(day: u8, buf: &mut [u8; 2]) -> &str {
    let day = day.min(99);
    buf[0] = b'0' + day / 10;
    buf[1] = b'0' + day % 10;
    let digits: &[u8] = if day < 10 { &buf[1..] } else { &buf[..] };
    core::str::from_utf8(digits).unwrap_or("")
}

/// A day's number in the middle of its shade by its ink, above the bar if
/// there is one.
fn number_in(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    day: &Day,
    (inner, barred, thick): (Rect, bool, u32),
) -> Result<(), Error> {
    let Some((number, size)) = day.number else {
        return Ok(());
    };
    let mut digits = [0u8; 2];
    let words = two_digits(number, &mut digits);
    let w = cx.font.measure_text(size, words)?;
    let color: Color = if on_dark(day.level) && !barred {
        look.paper
    } else if day.level == 0 && !day.today {
        look.faint
    } else {
        look.ink
    };
    let x = f32::from(day.cell.x) + (f32::from(day.cell.width) - w) / 2.0;
    let lift = if barred { (thick * 2) as u16 } else { 0 };
    let band = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(lift),
    );
    let baseline = ink_baseline(&cx.font, size, words, band)?;
    paint::push(
        out,
        Paint::Text {
            text: text::copy(words)?,
            size,
            origin: paperpaw_sdk::native::fonts::TextOrigin::new(x, baseline),
            color,
            clip: None,
        },
    )
}

/// Draws the widget in `area`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    slot: &crate::native_widgets::Slot,
) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    if !refresh(cx, &slot.calendar)? {
        return crate::native_widgets::message(cx, out, &look, "stats.no-clock");
    }
    let kept = slot.calendar.0.borrow();
    let Some((_, shown)) = kept.as_ref() else {
        return Err(Error::State);
    };
    if shown.weeks == 0 {
        draw_month(cx, out, &look, shown)
    } else {
        draw_weeks(cx, out, &look, shown)
    }
}

/// The rectangle below the caption, if any, in `look`'s area.
fn below(cx: &Cx<'_>, caption: Option<&str>, area: Rect) -> Result<Rect, Error> {
    let used = crate::native_widgets::caption_used(cx, caption, area)?;
    Ok(Rect::new(
        area.x,
        area.y.saturating_add(used),
        area.width,
        area.height.saturating_sub(used),
    ))
}

/// The first letter of a weekday's short name.
fn initial(cx: &mut Cx<'_>, weekday: u8) -> Result<String, Error> {
    let name = weekday_short(cx, weekday)?;
    let end = name.char_indices().nth(1).map_or(name.len(), |(at, _)| at);
    text::copy(name.get(..end).unwrap_or(&name))
}

#[inline(never)]
fn draw_month(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    shown: &Shown,
) -> Result<(), Error> {
    let small_px = cx.style().small;
    let date = date_of(shown.today);
    let name = month(cx, date.month)?;
    let title = text::format(format_args!("{name} {}", date.year))?;
    // "October 2026", or where that is too long the month alone.
    let caption = pick_caption(cx, look.area, &[&title, &name])?;
    let area = below(cx, caption, look.area)?;
    let mut header = weekdays_height(cx)?;
    let footer_text = footer_fitting(cx, shown, area.width)?;
    let mut footer = if footer_text.is_some() {
        footer_height(cx)?
    } else {
        0
    };
    let rows = shown.levels.len().div_ceil(7) as u16;
    let cell_of = |header: u16, footer: u16| {
        let room = area.height.saturating_sub(header.saturating_add(footer));
        (area.width / 7).min(room / rows.max(1))
    };
    // Where the days would be small, the footer goes, then the weekdays.
    let small = line(cx, small_px)?;
    if cell_of(header, footer) < small {
        footer = 0;
    }
    if cell_of(header, footer) < small * 3 / 4 {
        header = 0;
    }
    let cell = cell_of(header, footer);
    if cell < 4 {
        return Ok(());
    }
    let block = captioned(
        cx,
        out,
        look,
        caption,
        header.saturating_add(cell * rows).saturating_add(footer),
    )?;
    let left = geometry::clamp_u16(cx.settings.align()?.x(area, f32::from(cell * 7)) as i64);
    if header > 0 {
        weekday_row(cx, out, look, (block.y, left, cell, header))?;
    }
    let size = if cx.settings.shows("numbers")? {
        number_size(cx, cell)?
    } else {
        None
    };
    for (i, level) in shown.levels.iter().enumerate() {
        let day = shown.first + i as i32;
        let d = date_of(day);
        if d.month != date.month {
            continue;
        }
        let (row, col) = (i as u16 / 7, i as u16 % 7);
        let at = Rect::new(
            left + col * cell,
            block.y.saturating_add(header).saturating_add(row * cell),
            cell,
            cell,
        );
        let one = Day {
            cell: at,
            level: *level,
            number: size.map(|size| (d.day, size)),
            today: day == shown.today,
        };
        draw_day(cx, out, look, &one)?;
    }
    if let Some(words) = footer_text.filter(|_| footer > 0) {
        draw_footer(cx, out, look, block, &words)?;
    }
    Ok(())
}

/// The names of the weekdays above the month: two letters where every one has
/// room, else one.
fn weekday_row(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    (y, left, cell, header): (u16, u16, u16, u16),
) -> Result<(), Error> {
    let small_px = cx.style().small;
    let from = cx.week_start()?;
    let mut two = true;
    for w in 0..7 {
        let short = weekday_short(cx, w)?;
        two &= cx.font.measure_text(small_px, &short)? * 1.3 <= f32::from(cell);
    }
    for col in 0..7u8 {
        let weekday = (from + col) % 7;
        let name = if two {
            weekday_short(cx, weekday)?
        } else {
            initial(cx, weekday)?
        };
        let x = left + cell * u16::from(col);
        let w = cx.font.measure_text(small_px, &name)?;
        let at = f32::from(x) + (f32::from(cell) - w) / 2.0;
        text_at(
            cx,
            out,
            &name,
            (small_px, look.faint),
            (at, Rect::new(x, y, cell, header)),
        )?;
    }
    Ok(())
}

#[inline(never)]
fn draw_weeks(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    shown: &Shown,
) -> Result<(), Error> {
    let small_px = cx.style().small;
    let gap = cx.style().gap;
    let title = cx.format(
        "stats.widget.calendar.weeks",
        &[Arg::Int {
            name: "count",
            value: i64::from(shown.weeks),
        }],
    )?;
    let caption = pick_caption(cx, look.area, &[&title])?;
    let area = below(cx, caption, look.area)?;
    let footer_text = footer_fitting(cx, shown, area.width)?;
    let mut footer = if footer_text.is_some() {
        footer_height(cx)?
    } else {
        0
    };
    let small = line(cx, small_px)?;
    // Where the days would be small, the footer goes.
    if area.height.saturating_sub(footer) / 7 < small / 2 {
        footer = 0;
    }
    let from = cx.week_start()?;
    let labels = cx.settings.shows("weekdays")?;
    let mut label_w = 0;
    if labels {
        let mut widest = 0.0_f32;
        for w in 0..7 {
            let one = initial(cx, w)?;
            widest = widest.max(cx.font.measure_text(small_px, &one)?);
        }
        label_w = (widest as u16).saturating_add(gap / 2);
    }
    let room = area.height.saturating_sub(footer);
    // As many weeks as fit days at least 1.5 mm wide, the newest.
    let least = mm(cx, 15).max(4);
    let across = area.width.saturating_sub(label_w);
    let weeks = u16::from(shown.weeks).min(across / least).max(1);
    let cell = (across / weeks).min(room / 7);
    if cell < 3 {
        return Ok(());
    }
    let width = label_w + cell * weeks;
    let block = captioned(cx, out, look, caption, (cell * 7).saturating_add(footer))?;
    let left = geometry::clamp_u16(cx.settings.align()?.x(area, f32::from(width)) as i64);
    let skipped = (u16::from(shown.weeks) - weeks) * 7;
    if labels {
        // Every other weekday where the rows are low.
        let every = if cell >= small { 1 } else { 2 };
        for row in (0..7u8).step_by(every) {
            let name = initial(cx, (from + row) % 7)?;
            let r = Rect::new(left, block.y + cell * u16::from(row), label_w, cell);
            text_at(cx, out, &name, (small_px, look.faint), (f32::from(left), r))?;
        }
    }
    for (i, level) in shown.levels.iter().enumerate().skip(usize::from(skipped)) {
        let day = shown.first + i as i32;
        let n = i as u16 - skipped;
        let (col, row) = (n / 7, n % 7);
        let one = Day {
            cell: Rect::new(
                left + label_w + col * cell,
                block.y + row * cell,
                cell,
                cell,
            ),
            level: *level,
            number: None,
            today: day == shown.today,
        };
        draw_day(cx, out, look, &one)?;
    }
    if let Some(words) = footer_text.filter(|_| footer > 0) {
        draw_footer(cx, out, look, block, &words)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Day as Stored;

    /// The local day of a date.
    fn day(year: i32, month: u8, d: u8) -> i32 {
        (19_000..22_000)
            .find(|&n| {
                let t = date_of(n);
                (t.year, t.month, t.day) == (year, month, d)
            })
            .unwrap_or(0)
    }

    #[test]
    fn months_have_their_ends() {
        assert_eq!(month_end(day(2026, 10, 14)), day(2026, 10, 31));
        assert_eq!(month_end(day(2028, 2, 3)), day(2028, 2, 29));
    }

    #[test]
    fn weeks_start_on_the_chosen_day() {
        // 2026-10-14 is a Wednesday.
        assert_eq!(week_start_of(day(2026, 10, 14), 1), day(2026, 10, 12));
        assert_eq!(week_start_of(day(2026, 10, 14), 0), day(2026, 10, 11));
        assert_eq!(week_start_of(day(2026, 10, 11), 1), day(2026, 10, 5));
    }

    #[test]
    fn days_are_shaded_in_order() {
        let days: Vec<Stored> = [(10, 600), (12, 4000)]
            .into_iter()
            .map(|(day, seconds)| Stored {
                day,
                seconds,
                stays: 1,
            })
            .collect();
        let (shades, total) = levels(&days, (9, 13), None).unwrap();
        assert_eq!(shades, [0, 1, 0, 4, 0]);
        assert_eq!(total, 4600);
        // With a goal of an hour, ten minutes are a sixth of it.
        let (shades, _) = levels(&days, (10, 12), Some(3600)).unwrap();
        assert_eq!(shades, [1, 0, 4]);
    }

    #[test]
    fn numbers_are_one_or_two_digits() {
        let mut buf = [0; 2];
        assert_eq!(two_digits(7, &mut buf), "7");
        assert_eq!(two_digits(30, &mut buf), "30");
        assert_eq!(two_digits(200, &mut buf), "99");
    }
}
