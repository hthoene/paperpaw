//! *Reading per day*: the last days (7, 14 or 28, or this week) as bars, today's
//! marked, the daily goal as a dashed line across, each day's initial below.
use crate::declarations::{PERIODS, SHAPES};
use crate::native_widgets::{
    caption_room, captioned, dashed, days, goal, pick_caption, shade, text_at, weekday_of,
};
use crate::texts::{duration, seconds_on, weekday_short};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::Rect;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_ui_kit::widget::{Cx, Look};
use paperpaw_ui_kit::{Paint, geometry, paint, text};

/// A day shown: its weekday, seconds read, and whether it is still to come
/// (later this week).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shown {
    weekday: u8,
    seconds: u32,
    to_come: bool,
    today: bool,
}

/// The index in [`PERIODS`] of the period chosen.
fn period(cx: &Cx<'_>) -> Result<usize, Error> {
    cx.settings.with_text("period", |value| {
        PERIODS.iter().position(|p| *p == value).unwrap_or(0)
    })
}

/// The days shown, oldest first (none where the device does not know the time).
fn shown(cx: &mut Cx<'_>) -> Result<Vec<Shown>, Error> {
    let Some((today, days)) = days(cx)? else {
        return Ok(Vec::new());
    };
    let range = match period(cx)? {
        1 => {
            let weekday = weekday_of(today);
            let start = today - i32::from((7 + weekday - cx.week_start()?) % 7);
            start..=start + 6
        }
        2 => today - 13..=today,
        3 => today - 27..=today,
        _ => today - 6..=today,
    };
    let mut shown = Vec::new();
    shown
        .try_reserve_exact(range.clone().count())
        .map_err(|_| Error::NoMemory)?;
    for day in range {
        shown.push(Shown {
            weekday: weekday_of(day),
            seconds: seconds_on(&days, day),
            to_come: day > today,
            today: day == today,
        });
    }
    Ok(shown)
}

/// The height of the row of weekdays below the bars, if it shows.
fn labels_height(cx: &Cx<'_>) -> Result<u16, Error> {
    Ok(if cx.settings.shows("labels")? {
        geometry::line_px(&cx.font, cx.style().small)?.saturating_add(cx.style().gap / 2)
    } else {
        0
    })
}

/// The height the widget takes: the frame, the caption, the weekdays and bars
/// of one and a half rows.
///
/// # Errors
/// Font and host refusals.
pub fn height(cx: &mut Cx<'_>, _width: u16) -> Result<u16, Error> {
    let bars = (f32::from(cx.style().row) * 1.4) as u16;
    Ok(cx
        .shape
        .frame_height()
        .saturating_add(caption_room(cx)?)
        .saturating_add(labels_height(cx)?)
        .saturating_add(bars))
}

/// The time read in `days`: "3 h 20 min".
fn total(cx: &mut Cx<'_>, days: &[Shown]) -> Result<String, Error> {
    let total: u64 = days.iter().map(|d| u64::from(d.seconds)).sum();
    duration(cx, total)
}

/// "Last 7 days: 3 h 20 min" or "This week: 2 h 05 min".
fn label(cx: &mut Cx<'_>, days: &[Shown]) -> Result<String, Error> {
    let time = total(cx, days)?;
    if days.iter().any(|d| d.to_come) || period(cx)? == 1 {
        cx.format(
            "stats.widget.this-week",
            &[Arg::Text {
                name: "time",
                value: &time,
            }],
        )
    } else {
        cx.format(
            "stats.widget.days-label",
            &[
                Arg::Int {
                    name: "count",
                    value: days.len() as i64,
                },
                Arg::Text {
                    name: "time",
                    value: &time,
                },
            ],
        )
    }
}

/// The first letter of a weekday's short name.
fn initial(cx: &mut Cx<'_>, weekday: u8) -> Result<String, Error> {
    let name = weekday_short(cx, weekday)?;
    let end = name.char_indices().nth(1).map_or(name.len(), |(at, _)| at);
    text::copy(name.get(..end).unwrap_or(&name))
}

/// Draws the widget in `area`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    _slot: &crate::native_widgets::Slot,
) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let days = shown(cx)?;
    if days.is_empty() {
        return crate::native_widgets::message(cx, out, &look, "stats.no-clock");
    }
    bars(cx, out, &look, &days)
}

/// The caption, then a bar a day with its weekday below; today's bar in the
/// ink and the others a shade lighter (if today is marked); the daily goal, if
/// set, as a dashed line across.
#[expect(
    clippy::too_many_lines,
    reason = "one layout, a step to each part of the chart"
)]
fn bars(cx: &mut Cx<'_>, out: &mut Vec<Paint>, look: &Look, days: &[Shown]) -> Result<(), Error> {
    let (small, line_w) = (cx.style().small, u32::from(cx.style().line).max(1));
    // The caption, or where it is too long the time alone.
    let (full, time) = (label(cx, days)?, total(cx, days)?);
    let caption = pick_caption(cx, look.area, &[&full, &time])?;
    let used = crate::native_widgets::caption_used(cx, caption, look.area)?;
    let room = look.area.height.saturating_sub(used);
    let area = captioned(cx, out, look, caption, room)?;
    let line = geometry::line_px(&cx.font, small)?;
    let labels = labels_height(cx)?;
    // Where the bars would be low, the weekdays go.
    let labels = if area.height.saturating_sub(labels) < line.saturating_mul(2) {
        0
    } else {
        labels
    };
    let thin = line_w;
    // Room above the bars for the goal's line and rounded tops.
    let top = (thin * 2) as u16;
    let chart = Rect::new(
        area.x,
        area.y.saturating_add(top),
        area.width,
        area.height.saturating_sub(labels.saturating_add(top)),
    );
    let goal = goal(cx.host)?.filter(|_| cx.settings.shows("goal").unwrap_or(false));
    let most = days
        .iter()
        .map(|d| u64::from(d.seconds))
        .max()
        .unwrap_or(0)
        .max(goal.unwrap_or(0))
        .max(60);
    let round = cx.settings.with_text("shape", |v| v != SHAPES[1])?;
    let mark_today = cx.settings.shows("today")?;
    let n = days.len() as u32;
    let width = u32::from(area.width);
    let column_of = |i: u32| (width * i / n, width * (i + 1) / n - width * i / n);
    // Two letters where every column has room for them, else one.
    let (_, narrowest) = column_of(0);
    let mut two = true;
    let mut every = true;
    if labels > 0 {
        for weekday in 0..7 {
            let short = weekday_short(cx, weekday)?;
            two &= cx.font.measure_text(small, &short)? * 1.4 <= narrowest as f32;
            let one = initial(cx, weekday)?;
            every &= cx.font.measure_text(small, &one)? * 1.6 <= narrowest as f32;
        }
    }
    let first_day = cx.week_start()?;
    let base = i32::from(chart.y) + i32::from(chart.height);
    let mm3 = u32::from(crate::native_widgets::mm(cx, 30)).max(2);
    for (i, day) in days.iter().enumerate() {
        let (left, column) = column_of(i as u32);
        let centre = i32::from(area.x) + (left + column / 2) as i32;
        let bar_w = (column * 2 / 5).clamp(2, mm3);
        let bx = centre - (bar_w / 2) as i32;
        let radius = if round { bar_w / 4 } else { 0 };
        if day.seconds > 0 {
            let h = (u64::from(chart.height) * u64::from(day.seconds) / most) as u32;
            let h = h.max(if round { radius * 2 } else { thin * 2 }).max(2);
            let bar = Rect::new(
                geometry::clamp_u16(i64::from(bx)),
                geometry::clamp_u16(i64::from(base) - i64::from(h)),
                geometry::clamp_u16(i64::from(bar_w)),
                geometry::clamp_u16(i64::from(h)),
            );
            let color = if mark_today && !day.today {
                shade(look.paper, 3)
            } else {
                look.ink
            };
            paint::push(out, Paint::Focus(bar, radius.min(h / 4), None, color))?;
        } else if !day.to_come {
            // A day without reading: a light stub on the base.
            let h = (thin * 2).max(3);
            let stub = Rect::new(
                geometry::clamp_u16(i64::from(bx)),
                geometry::clamp_u16(i64::from(base) - i64::from(h)),
                geometry::clamp_u16(i64::from(bar_w)),
                geometry::clamp_u16(i64::from(h)),
            );
            paint::push(
                out,
                Paint::Focus(stub, radius.min(h / 2), None, shade(look.paper, 1)),
            )?;
        }
        if labels > 0 && (every || day.today || day.weekday == first_day) {
            let name = if two {
                weekday_short(cx, day.weekday)?
            } else {
                initial(cx, day.weekday)?
            };
            let w = cx.font.measure_text(small, &name)?;
            // Under its bar, but inside the area (the first and last columns'
            // names may be wider than half a column).
            let right = f32::from(area.x) + f32::from(area.width) - w;
            let at = (centre as f32 - w / 2.0).min(right).max(f32::from(area.x));
            let row = Rect::new(
                area.x,
                geometry::clamp_u16(
                    i64::from(chart.y) + i64::from(chart.height) + i64::from(labels)
                        - i64::from(line),
                ),
                area.width,
                line,
            );
            let color = if day.today && mark_today {
                look.ink
            } else {
                look.faint
            };
            text_at(cx, out, &name, (small, color), (at, row))?;
            if day.today && mark_today {
                // Underlined: seen also where faint text is drawn black.
                let y = i32::from(row.y) + i32::from(row.height) - thin as i32;
                let rule = Rect::new(
                    geometry::clamp_u16(at as i64),
                    geometry::clamp_u16(i64::from(y)),
                    geometry::clamp_u16(w as i64),
                    geometry::clamp_u16(i64::from(thin)),
                );
                paint::push(out, Paint::Fill(rule, look.ink))?;
            }
        }
    }
    if let Some(goal) = goal {
        let y = base - (u64::from(chart.height) * goal / most) as i32;
        let end = i32::from(area.x) + i32::from(area.width);
        dashed(out, (i32::from(area.x), end, y), thin, look.faint)?;
    }
    Ok(())
}
