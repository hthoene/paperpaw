//! The year so far: how much of it has passed, as a bar, a ring or a dot for
//! each month, with the percent, the day of the year, the week and the days
//! left, as wished.
use crate::native_dates::{day_of_year, days_in_month, days_in_year, iso_week};
use crate::native_draw::no_time;
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::Host;
use paperpaw_sdk::{
    Color, Point, Rect,
    native::{Error, formatting::Arg},
    time::LocalTime,
};
use paperpaw_ui_kit::widget::names::chosen_name;
use paperpaw_ui_kit::widget::settings::Stack;
use paperpaw_ui_kit::widget::{Cx, cx::local_time_now};
use paperpaw_ui_kit::{
    Paint, paint,
    widget::{
        Align, Look,
        layout::{
            self, Part, cap_height, caption, centred, fit_size, label, line, pack, stack_height,
        },
        shape::{fill_circle, fill_ring, stroke_circle},
        text::largest_size,
    },
};

/// How far the year is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Today, 1 to 366.
    pub day: u16,
    /// The year's days.
    pub days: u16,
    /// Of the year passed, in tenths of a percent (the time of day counted).
    pub permille: u16,
    pub week: u8,
    /// Days after today.
    pub left: u16,
}

/// How far the year is at `t`.
#[must_use]
pub fn progress(t: &LocalTime) -> Progress {
    let day = day_of_year(t.year, t.month, t.day);
    let days = days_in_year(t.year);
    let minutes = (u32::from(day) - 1) * 1440 + u32::from(t.hour) * 60 + u32::from(t.minute);
    Progress {
        day,
        days,
        permille: (minutes * 1000 / (u32::from(days) * 1440)) as u16,
        week: iso_week(t.year, t.month, t.day),
        left: days - day,
    }
}

/// What it shows, to tell when it changed.
///
/// # Errors
/// Host refusals.
pub fn shows(host: &Host<'_>, _widget: &str) -> Result<u64, Error> {
    Ok(match local_time_now(host)? {
        Some(t) => {
            let p = progress(&t);
            u64::from(p.day) * 1_000 + u64::from(p.permille / 10) + 1
        }
        None => 0,
    })
}

/// The notes below the bar, as the settings want them: "Day 287 of 365",
/// "Week 42", "78 days left"; with `short`, "Day 287".
fn notes(cx: &mut Cx<'_>, p: &Progress, short: bool) -> Result<Vec<String>, Error> {
    let mut notes: Vec<String> = Vec::new();
    notes.try_reserve(3).map_err(|_| Error::NoMemory)?;
    if cx.settings.shows("day")? {
        let key = if short {
            "widgets.year.day-short"
        } else {
            "widgets.year.day"
        };
        notes.push(cx.format(
            key,
            &[
                Arg::Int {
                    name: "day",
                    value: i64::from(p.day),
                },
                Arg::Int {
                    name: "days",
                    value: i64::from(p.days),
                },
            ],
        )?);
    }
    if cx.settings.shows("week")? {
        notes.push(cx.format(
            "widgets.year.week",
            &[Arg::Int {
                name: "week",
                value: i64::from(p.week),
            }],
        )?);
    }
    if cx.settings.shows("left")? {
        notes.push(cx.plural("widgets.year.left", i64::from(p.left), &[])?);
    }
    Ok(notes)
}

fn percent_text(cx: &mut Cx<'_>, p: &Progress) -> Result<String, Error> {
    let text = Stack::<16>::from_args(format_args!("{} %", p.permille / 10))?;
    cx.messages.digits(cx.host, text.as_str())
}

/// The height: the percent large, two small lines, two gaps, the frame.
///
/// # Errors
/// Font refusals.
pub fn height(cx: &mut Cx<'_>, _: u16) -> Result<u16, Error> {
    let (title, small, gap) = (cx.style().title, cx.style().small, cx.style().gap);
    let big = paperpaw_ui_kit::geometry::line_px(&cx.font, title * 1.6)?;
    let note = paperpaw_ui_kit::geometry::line_px(&cx.font, small)?;
    Ok(big
        .saturating_add(note.saturating_mul(2))
        .saturating_add(gap.saturating_mul(2))
        .saturating_add(cx.shape.frame_height()))
}

/// The track's colour: a light gray on light paper, a dark one on dark.
fn track(look: &Look) -> Color {
    if look.paper.luma() < 128 {
        Color::gray(80)
    } else {
        Color::gray(205)
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
    let p = progress(&now);
    let year = if cx.settings.shows("year")? {
        Some(cx.number(i64::from(now.year))?)
    } else {
        None
    };
    match chosen_name(cx, "style")?.as_str() {
        "ring" => {
            let mut area = look.area;
            if let Some(year) = &year {
                let (small, gap) = (cx.style().small, cx.style().gap);
                let align = cx.settings.align()?;
                area = caption(out, &cx.font, (small, gap), (area, align, look.faint), year)?;
            }
            draw_ring(cx, out, &look, area, &p)
        }
        "months" => draw_bar(cx, out, &look, year.as_deref(), &p, Some(&now)),
        _ => draw_bar(cx, out, &look, year.as_deref(), &p, None),
    }
}

/// What the bar styles show in a place: the parts that fit, the least
/// important left out first.
struct BarParts {
    caption: bool,
    /// The percent's size, where it shows.
    percent: Option<f32>,
    notes: Vec<String>,
    bar: f32,
}

/// How notes may be laid out: whole in up to two lines, whole in one, in
/// their short forms on one ("Day 287"), shortened at the end of one, or not
/// at all.
#[derive(Clone, Copy, PartialEq)]
enum Notes {
    Two,
    One,
    Short,
    Cut,
    None,
}

fn ink(
    font: &paperpaw_sdk::native::fonts::Font<'_>,
    lines: &[&str],
    size: f32,
) -> Result<f32, Error> {
    let part = Part {
        lines: paperpaw_ui_kit::mem::copy_strs(lines)?,
        size,
        color: Color::BLACK,
    };
    stack_height(font, core::slice::from_ref(&part), 0.0)
}

/// The year as `caption` above, the percent large, a bar (or a dot for each
/// month of `months`) and the notes below, half a gap between their ink;
/// where they have no room together, a second line of notes, the percent, the
/// caption and the notes are left out in turn (the notes shortened before
/// they go), never drawn over each other.
#[allow(clippy::too_many_lines, reason = "one layout")]
fn draw_bar(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    caption_text: Option<&str>,
    p: &Progress,
    months: Option<&LocalTime>,
) -> Result<(), Error> {
    let (small, text_size, title, gap_px, line_px) = {
        let style = cx.style();
        (
            style.small,
            style.text,
            style.title,
            f32::from(style.gap),
            f32::from(style.line),
        )
    };
    let align = cx.settings.align()?;
    let gap = gap_px / 2.0;
    let area = look.area;
    let width = f32::from(area.width);
    let height = f32::from(area.height);
    let items = notes(cx, p, false)?;
    let shorts = notes(cx, p, true)?;
    let text = percent_text(cx, p)?;
    let percent = cx.settings.shows("percent")?;
    let caption_h = match caption_text {
        Some(c) => ink(&cx.font, &[c], small)?,
        None => 0.0,
    };
    let bar_h = |height: f32| {
        if months.is_some() {
            (width / 12.0 * 0.7).min(height * 0.3)
        } else {
            // Four lines thick at least, but no thicker than one and a half
            // gaps (where thick lines want more, the gaps win).
            (height * 0.1)
                .max(line_px * 4.0)
                .min(gap_px * 1.5)
                .min(height)
        }
    };
    // Each way to show it, from all parts to the bar alone.
    let ways = [
        (true, Notes::Two, true),
        (true, Notes::One, true),
        (true, Notes::Two, false),
        (true, Notes::One, false),
        (true, Notes::Short, false),
        (true, Notes::Cut, false),
        (false, Notes::Two, false),
        (false, Notes::One, false),
        (false, Notes::Short, false),
        (false, Notes::Cut, false),
        (false, Notes::None, true),
        (false, Notes::None, false),
    ];
    let mut found: Option<BarParts> = None;
    for (with_caption, kind, with_percent) in ways {
        if (with_caption && caption_text.is_none()) || (with_percent && !percent) {
            continue;
        }
        if kind != Notes::None && items.is_empty() {
            continue;
        }
        let lines = match kind {
            Notes::Two => pack(&cx.font, small, &items, width, 2)?,
            Notes::One | Notes::Cut => pack(&cx.font, small, &items, width, 1)?,
            Notes::Short => pack(&cx.font, small, &shorts, width, 1)?,
            Notes::None => Vec::new(),
        };
        // Whole notes only, but where they may be shortened.
        if kind != Notes::Cut && lines.iter().any(|n| n.ends_with('…')) {
            continue;
        }
        let note_h = if lines.is_empty() {
            0.0
        } else {
            ink(&cx.font, &paperpaw_ui_kit::mem::strs(&lines)?, small)? + gap
        };
        let top_h = if with_caption { caption_h + gap } else { 0.0 };
        let bar = bar_h(height);
        let room = height - top_h - note_h - bar;
        if room < 0.0 {
            continue;
        }
        let size = if with_percent {
            let most = (room - gap).min(title * 2.2);
            let size = largest_size(most, text_size, |s| {
                Ok(cx.font.measure_text(s, &text)? <= width
                    && cap_height(&cx.font, s)? + gap <= room)
            })?;
            match size {
                Some(size) => Some(size),
                None => continue,
            }
        } else {
            None
        };
        found = Some(BarParts {
            caption: with_caption,
            percent: size,
            notes: lines,
            bar,
        });
        break;
    }
    let Some(parts) = found else {
        return Ok(());
    };
    let big = match parts.percent {
        Some(size) => ink(&cx.font, &[text.as_str()], size)? + gap,
        None => 0.0,
    };
    let note_refs: Vec<&str> = paperpaw_ui_kit::mem::strs(&parts.notes)?;
    let note_h = if parts.notes.is_empty() {
        0.0
    } else {
        ink(&cx.font, &note_refs, small)? + gap
    };
    let top_h = if parts.caption { caption_h + gap } else { 0.0 };
    let bar_height = parts.bar;
    // All of it together in the middle by its ink.
    let total = top_h + big + bar_height + note_h;
    let mut y = f32::from(area.y) + (height - total).max(0.0) / 2.0;
    // Draws one part's ink from `y`, `h` high.
    let part_at = |cx: &Cx<'_>, out: &mut Vec<Paint>, y: f32, h: f32, part: Part<'_>| {
        let band = Rect::new(area.x, y as u16, area.width, (h + 0.99) as u16);
        layout::stack(out, &cx.font, band, align, &[part], 0.0)
    };
    if let (true, Some(c)) = (parts.caption, caption_text) {
        part_at(
            cx,
            out,
            y,
            caption_h,
            Part {
                lines: paperpaw_ui_kit::mem::one(c)?,
                size: small,
                color: look.faint,
            },
        )?;
        y += top_h;
    }
    if let Some(size) = parts.percent {
        part_at(
            cx,
            out,
            y,
            big - gap,
            Part {
                lines: paperpaw_ui_kit::mem::one(text.as_str())?,
                size,
                color: look.ink,
            },
        )?;
        y += big;
    }
    match months {
        Some(t) => draw_months(out, look, area, (y, bar_height), t)?,
        None => {
            let bar = Rect::new(area.x, y as u16, area.width, bar_height as u16);
            let radius = bar_height as u32 / 2;
            paint::push(out, Paint::Focus(bar, radius, None, track(look)))?;
            let done = (u32::from(area.width) * u32::from(p.permille) / 1000) as u16;
            if done > 0 {
                let filled = Rect::new(bar.x, bar.y, done.max(bar.height), bar.height);
                paint::push(out, Paint::Focus(filled, radius, None, look.ink))?;
            }
        }
    }
    y += bar_height + gap;
    if !parts.notes.is_empty() {
        part_at(
            cx,
            out,
            y,
            note_h - gap,
            Part {
                lines: note_refs,
                size: small,
                color: look.faint,
            },
        )?;
    }
    Ok(())
}

/// A dot for each month: those passed filled, this one as far as it went,
/// those to come as rings.
fn draw_months(
    out: &mut Vec<Paint>,
    look: &Look,
    area: Rect,
    (y, height): (f32, f32),
    t: &LocalTime,
) -> Result<(), Error> {
    let step = f32::from(area.width) / 12.0;
    let radius = (height / 2.0).min(step * 0.42).max(2.0) as u32;
    let thick = (radius / 6).max(1);
    let days = u32::from(days_in_month(t.year, t.month));
    for month in 1..=12u8 {
        let x = f32::from(area.x) + step * (f32::from(month) - 0.5);
        let centre = Point::new(x as i32, (y + height / 2.0) as i32);
        if month < t.month {
            fill_circle(out, centre, radius, look.ink)?;
        } else if month == t.month {
            fill_circle(out, centre, radius, track(look))?;
            let sweep = (u32::from(t.day) - 1) * 360 / days + 360 / days / 2;
            fill_ring(out, centre, radius, radius, (0, sweep.max(8)), look.ink)?;
            stroke_circle(out, centre, radius, thick, look.ink)?;
        } else {
            stroke_circle(out, centre, radius, thick, look.ink)?;
        }
    }
    Ok(())
}

/// A ring as far round as the year went, the percent in it, the notes beside
/// it where there is room.
#[allow(clippy::too_many_lines, reason = "one drawing")]
fn draw_ring(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    area: Rect,
    p: &Progress,
) -> Result<(), Error> {
    let (small_size, text_size, gap) = {
        let style = cx.style();
        (style.small, style.text, style.gap)
    };
    let align = cx.settings.align()?;
    let items = notes(cx, p, false)?;
    let small = line(&cx.font, small_size)?;
    let side = area.height.min(area.width);
    // Beside it where the notes have room there, else below it.
    let mut widest = 0.0_f32;
    for note in &items {
        widest = widest.max(cx.font.measure_text(text_size, note)?);
    }
    let rest = f32::from(area.width) - f32::from(side) - f32::from(gap) * 2.0;
    let beside = !items.is_empty() && widest <= rest;
    // Not beside: below it, the notes in a line.
    let below = if beside || items.is_empty() {
        0
    } else {
        (small + f32::from(gap) / 2.0) as u16
    };
    let side = side.min(area.height.saturating_sub(below));
    let ring_x = if beside {
        match align {
            Align::Right => area.x + area.width - side,
            _ => area.x,
        }
    } else {
        align.x(area, f32::from(side)) as u16
    };
    let radius = u32::from(side / 2);
    let thick = (radius / 7).max(3);
    if radius <= thick {
        // No room for a ring.
        return Ok(());
    }
    let top = area.y + area.height.saturating_sub(side + below) / 2;
    let centre = Point::new(
        i32::from(ring_x) + radius as i32,
        i32::from(top) + radius as i32,
    );
    fill_ring(out, centre, radius, thick, (0, 360), track(look))?;
    let sweep = u32::from(p.permille) * 360 / 1000;
    fill_ring(out, centre, radius, thick, (0, sweep), look.ink)?;
    if cx.settings.shows("percent")? {
        let text = percent_text(cx, p)?;
        let inner = (radius - thick) as f32 * 2.0 * 0.78;
        let size = fit_size(&cx.font, inner * 0.42, &text, inner)?;
        centred(
            out,
            &cx.font,
            size,
            (centre.x as f32, centre.y as f32),
            &text,
            look.ink,
        )?;
    }
    if !beside {
        if below > 0 {
            let row = Rect::new(area.x, top + side + gap / 2, area.width, small as u16);
            let joined = paperpaw_ui_kit::text::join_notes(&items)?;
            label(out, &cx.font, (small_size, align), row, &joined, look.faint)?;
        }
        return Ok(());
    }
    let gap2 = gap * 2;
    let text_x = if align == Align::Right {
        area.x
    } else {
        area.x + side + gap2
    };
    let room = Rect::new(text_x, area.y, area.width - side - gap2, area.height);
    let line_h = line(&cx.font, text_size)?;
    let total = line_h * items.len() as f32;
    let mut y = f32::from(room.y) + (f32::from(room.height) - total).max(0.0) / 2.0;
    let text_align = if align == Align::Right {
        Align::Right
    } else {
        Align::Left
    };
    for note in &items {
        if y + line_h > f32::from(room.y) + f32::from(room.height) + 1.0 {
            break;
        }
        let row = Rect::new(room.x, y as u16, room.width, line_h as u16);
        label(out, &cx.font, (text_size, text_align), row, note, look.ink)?;
        y += line_h;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(year: i32, month: u8, day: u8, hour: u8) -> LocalTime {
        LocalTime::new(year, month, day, hour, 0, 0)
    }

    #[test]
    fn the_year_so_far() {
        let p = progress(&at(2026, 10, 14, 10));
        assert_eq!((p.day, p.days, p.week, p.left), (287, 365, 42, 78));
        assert_eq!(p.permille / 10, 78);
        let first = progress(&at(2026, 1, 1, 0));
        assert_eq!((first.day, first.permille, first.left), (1, 0, 364));
        let last = progress(&at(2024, 12, 31, 23));
        assert_eq!((last.day, last.days, last.left), (366, 366, 0));
        assert_eq!(last.permille, 999);
        // Half the year: 2 July at noon, in a common year.
        assert_eq!(progress(&at(2026, 7, 2, 12)).permille, 500);
    }

    #[test]
    fn leap_days_and_the_last_minute_of_a_common_year() {
        let leap = progress(&at(2028, 2, 29, 0));
        assert_eq!((leap.day, leap.days, leap.left), (60, 366, 306));
        let last = progress(&LocalTime::new(2026, 12, 31, 23, 59, 59));
        assert_eq!((last.day, last.days, last.left), (365, 365, 0));
        assert_eq!(last.permille, 999);
        // The week of the first days of a year may belong to the one before.
        assert_eq!(progress(&at(2027, 1, 1, 0)).week, 53);
        assert_eq!(progress(&at(2026, 12, 31, 0)).week, 53);
    }
}
