//! *Read today*: today's reading time and pages, a bar towards the daily goal
//! (`stats.goal`), and below them the week or the goal: "7 min to your daily
//! goal", "Daily goal reached, 3 days in a row".
use crate::files::DataFolder;
use crate::native_screens::utc_day;
use crate::native_widgets::{
    caption_room, captioned, days, goal, mm, note_lines, pick_caption, shade, text_size, weekday_of,
};
use crate::overview_rows::distinct_pages;
use crate::texts::{duration, seconds_on};
use crate::totals;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::Rect;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::storage::DataStorage;
use paperpaw_ui_kit::widget::Cx;
use paperpaw_ui_kit::widget::layout::label;
use paperpaw_ui_kit::{Paint, geometry, paint, text};

/// What the widget shows.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Numbers {
    today: u64,
    pages: u32,
    week: u64,
    /// Days in a row the goal was reached, up to today (or yesterday, while
    /// today is not done yet).
    streak: u32,
}

/// The numbers, if the device knows the time.
fn numbers(cx: &mut Cx<'_>, goal: Option<u64>) -> Result<Option<Numbers>, Error> {
    let Some((today, days)) = days(cx)? else {
        return Ok(None);
    };
    let weekday = weekday_of(today);
    let since = today - i32::from((7 + weekday - cx.week_start()?) % 7);
    let streak = goal.map_or(0, |goal| {
        let least = goal.min(u64::from(u32::MAX)) as u32;
        totals::streaks(&days, today, least).current
    });
    let pages = if cx.settings.shows("pages")? {
        let storage = DataStorage::new(cx.host);
        distinct_pages(&DataFolder(&storage), today, utc_day)?
    } else {
        0
    };
    Ok(Some(Numbers {
        today: u64::from(seconds_on(&days, today)),
        pages,
        week: (since..=today)
            .map(|day| u64::from(seconds_on(&days, day)))
            .sum(),
        streak,
    }))
}

/// `text` kept on one line where the note is broken into two.
fn unbroken(text: String) -> String {
    text.replace(' ', "\u{a0}")
}

/// The height of the goal's bar with the room around it, if it shows.
fn bar_band(cx: &Cx<'_>, goal: Option<u64>) -> Result<u16, Error> {
    Ok(if goal.is_some() && cx.settings.shows("bar")? {
        mm(cx, 30)
    } else {
        0
    })
}

/// "23 min · 41 pages", and below it the week or the goal: "7 min to your
/// daily goal", "Goal reached, 3 days in a row".
fn lines(cx: &mut Cx<'_>, numbers: &Numbers, goal: Option<u64>) -> Result<(String, String), Error> {
    let time = duration(cx, numbers.today)?;
    let first = if cx.settings.shows("pages")? {
        let pages = cx.plural("stats.pages", i64::from(numbers.pages), &[])?;
        text::concat(&[&time, " · ", &pages])?
    } else {
        time
    };
    let second = match goal {
        None => {
            let week = unbroken(duration(cx, numbers.week)?);
            cx.format(
                "stats.widget.this-week",
                &[Arg::Text {
                    name: "time",
                    value: &week,
                }],
            )?
        }
        Some(goal) if numbers.today < goal => {
            let left = unbroken(duration(cx, (goal - numbers.today).max(60))?);
            let line = cx.format(
                "stats.goal.left",
                &[Arg::Text {
                    name: "time",
                    value: &left,
                }],
            )?;
            match numbers.streak {
                0 => line,
                days => {
                    let streak = cx.plural("stats.goal.streak", i64::from(days), &[])?;
                    text::concat(&[&line, " · ", &streak])?
                }
            }
        }
        Some(_) => match numbers.streak {
            0 | 1 => cx.tr("stats.goal.reached")?,
            days => cx.plural("stats.goal.reached-days", i64::from(days), &[])?,
        },
    };
    Ok((first, second))
}

/// The height the widget takes: the frame, the caption, a line of the numbers,
/// the bar and the note.
///
/// # Errors
/// Font and host refusals.
pub fn height(cx: &mut Cx<'_>, _width: u16) -> Result<u16, Error> {
    let style = cx.style();
    let size = cx.settings.size()?.text(style);
    let note = if cx.settings.shows("note")? {
        geometry::line_px(&cx.font, style.small)?
    } else {
        0
    };
    let goal = goal(cx.host)?;
    Ok(cx
        .shape
        .frame_height()
        .saturating_add(caption_room(cx)?)
        .saturating_add(geometry::line_px(&cx.font, size)?)
        .saturating_add(bar_band(cx, goal)?)
        .saturating_add(note))
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
    let goal = goal(cx.host)?;
    let Some(numbers) = numbers(cx, goal)? else {
        return crate::native_widgets::message(cx, out, &look, "stats.no-clock");
    };
    body(cx, out, &look, &numbers, goal)
}

/// How full the goal's bar is, in thousandths.
fn progress(numbers: &Numbers, goal: Option<u64>) -> u64 {
    goal.map_or(0, |goal| (numbers.today * 1000 / goal.max(1)).min(1000))
}

fn body(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &paperpaw_ui_kit::widget::Look,
    numbers: &Numbers,
    goal: Option<u64>,
) -> Result<(), Error> {
    let text_px = cx.style().text;
    let label_text = cx.tr("stats.today")?;
    let mut caption = pick_caption(cx, look.area, &[&label_text])?;
    let below = |cx: &Cx<'_>, caption: Option<&str>, area: Rect| -> Result<Rect, Error> {
        let cap = crate::native_widgets::caption_used(cx, caption, area)?;
        Ok(Rect::new(
            area.x,
            area.y.saturating_add(cap),
            area.width,
            area.height.saturating_sub(cap),
        ))
    };
    let mut area = below(cx, caption, look.area)?;
    let (first, second) = lines(cx, numbers, goal)?;
    let bar = bar_band(cx, goal)?;
    let note_on = cx.settings.shows("note")?;
    // Where they do not all fit, the note goes, then the bar.
    let least = geometry::line_px(&cx.font, text_px)?;
    // The caption goes before the note: where the note is whole only without
    // it.
    let whole = |cx: &Cx<'_>, area: Rect| -> Result<bool, Error> {
        let room = area.height.saturating_sub(least.saturating_add(bar));
        Ok(note_lines(cx, &second, (area.width, room), 3)?.is_some_and(|n| n.whole()))
    };
    if caption.is_some() && note_on && !whole(cx, area)? && whole(cx, look.area)? {
        caption = None;
        area = look.area;
    }
    let band = if least.saturating_add(bar) > area.height {
        0
    } else {
        bar
    };
    // The note on as many lines as it needs (three at most), shortened where
    // they do not hold it.
    let mut note = if note_on {
        let room = area.height.saturating_sub(least.saturating_add(band));
        note_lines(cx, &second, (area.width, room), 3)?
    } else {
        None
    };
    let mut note_h = match &note {
        Some(note) => note.height(&cx.font)?,
        None => 0,
    };
    let room = area.height.saturating_sub(band.saturating_add(note_h));
    let mut size = text_size(cx, &first, area.width, room)?;
    // The note goes where it makes the numbers smaller than the body's size.
    let alone = text_size(cx, &first, area.width, area.height.saturating_sub(band))?;
    if size < text_px && alone > size {
        note = None;
        note_h = 0;
        size = alone;
    }
    let big = geometry::line_px(&cx.font, size)?;
    // Too wide with the pages: the time alone.
    let width = f32::from(area.width);
    let first = if cx.font.measure_text(size, &first)? > width {
        duration(cx, numbers.today)?
    } else {
        first
    };
    // The lines together in the middle, the caption above them.
    let block = captioned(
        cx,
        out,
        look,
        caption,
        big.saturating_add(band).saturating_add(note_h),
    )?;
    let align = cx.settings.align()?;
    let row = |y: u16, h: u16| Rect::new(area.x, y, area.width, h);
    label(
        out,
        &cx.font,
        (size, align),
        row(block.y, big),
        &first,
        look.ink,
    )?;
    let mut y = block.y.saturating_add(big);
    if band > 0 {
        // A bar filled up to today's share of the goal, its ends round.
        let height = mm(cx, 10).max(4);
        let track = Rect::new(
            area.x,
            y.saturating_add(band.saturating_sub(height) / 2),
            area.width,
            height,
        );
        let radius = u32::from(height / 2);
        paint::push(out, Paint::Focus(track, radius, None, shade(look.paper, 1)))?;
        let filled = (u64::from(area.width) * progress(numbers, goal) / 1000) as u16;
        if filled > 0 {
            let filled = filled.max(height);
            let done = Rect::new(track.x, track.y, filled, height);
            paint::push(out, Paint::Focus(done, radius, None, look.ink))?;
        }
        y = y.saturating_add(band);
    }
    if let Some(note) = note {
        note.draw(out, &cx.font, row(y, note_h), align, look.faint)?;
    }
    Ok(())
}
