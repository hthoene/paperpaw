//! *Reading streak*: the days read in a row (any reading, or the daily goal
//! reached), the longest streak so far, and the last 7 or 14 days as marks:
//! filled (with a check, if chosen) for the days that count, an outline for
//! the others, today's darker while it does not count yet.
use crate::declarations::{STREAK_COUNTS, STREAK_DAYS, STREAK_MARKS};
use crate::native_widgets::{
    caption_room, captioned, days, goal, middle, mm, number_if_narrow, pick_caption, text_at,
    text_size,
};
use crate::texts::{date_of, seconds_on, weekday_short};
use crate::totals::{self, Streaks};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::{Color, Point, Rect};
use paperpaw_ui_kit::widget::{Align, Cx, Look, TextSize};
use paperpaw_ui_kit::{Paint, geometry, paint, text};

/// A day of the marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mark {
    weekday: u8,
    counts: bool,
    today: bool,
}

/// What the widget shows.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Shown {
    streaks: Streaks,
    marks: Vec<Mark>,
}

/// The least seconds of a day that counts: the goal, if chosen and set.
fn least(cx: &Cx<'_>) -> Result<u32, Error> {
    if !cx.settings.with_text("counts", |v| v == STREAK_COUNTS[1])? {
        return Ok(1);
    }
    Ok(goal(cx.host)?.map_or(1, |g| g.min(u64::from(u32::MAX)) as u32))
}

/// How many days the marks show.
fn count(cx: &Cx<'_>) -> Result<usize, Error> {
    Ok(if cx.settings.with_text("days", |v| v == STREAK_DAYS[1])? {
        14
    } else {
        7
    })
}

fn shown(cx: &Cx<'_>) -> Result<Option<Shown>, Error> {
    let Some((today, days)) = days(cx)? else {
        return Ok(None);
    };
    let least = least(cx)?;
    let count = count(cx)?;
    let mut marks = Vec::new();
    marks
        .try_reserve_exact(count)
        .map_err(|_| Error::NoMemory)?;
    for day in today - count as i32 + 1..=today {
        marks.push(Mark {
            weekday: date_of(day).weekday,
            counts: seconds_on(&days, day) >= least,
            today: day == today,
        });
    }
    Ok(Some(Shown {
        streaks: totals::streaks(&days, today, least),
        marks,
    }))
}

/// The line a text of `size` takes.
fn line(cx: &Cx<'_>, size: f32) -> Result<u16, Error> {
    geometry::line_px(&cx.font, size)
}

/// The headline's size by the size setting (the body's where "as large as
/// fits").
fn headline(cx: &Cx<'_>) -> Result<f32, Error> {
    Ok(match cx.settings.size()? {
        TextSize::Fit => cx.style().text,
        size => size.text(cx.style()),
    })
}

/// The marks' place across `width` (as large as they may be for 0): how wide
/// each one's column is and how large the mark.
fn mark_size(cx: &Cx<'_>, width: u16, count: usize) -> (u32, u32) {
    let most = u32::from(mm(cx, 40));
    // A mark and a half for each, at most: together, not spread out.
    let column = match width {
        0 => most * 3 / 2,
        w => (u32::from(w) / count.max(1) as u32).min(most * 3 / 2),
    };
    (column, (column * 3 / 4).min(most))
}

/// The height of the marks across `width`, with the room above them and their
/// weekdays below if `weekdays`; 0 where they do not show.
fn marks_height(cx: &Cx<'_>, small: u16, width: u16, weekdays: bool) -> Result<u16, Error> {
    if !cx.settings.shows("marks")? {
        return Ok(0);
    }
    let (_, d) = mark_size(cx, width, count(cx)?);
    Ok((d as u16)
        .saturating_add(cx.style().gap / 2)
        .saturating_add(if weekdays { small } else { 0 }))
}

/// The height the widget takes: the frame, the caption, the streak, a line of
/// note and the marks.
///
/// # Errors
/// Font and host refusals.
pub fn height(cx: &mut Cx<'_>, _width: u16) -> Result<u16, Error> {
    let small = line(cx, cx.style().small)?;
    let weekdays = cx.settings.shows("weekdays")?;
    Ok(cx
        .shape
        .frame_height()
        .saturating_add(caption_room(cx)?)
        .saturating_add(line(cx, headline(cx)?)?)
        .saturating_add(small)
        .saturating_add(marks_height(cx, small, 0, weekdays)?))
}

/// "5 days", and below it "in a row, longest 12" (or how to begin one).
fn texts(cx: &mut Cx<'_>, streaks: Streaks) -> Result<(String, String), Error> {
    let first = cx.plural("stats.days", i64::from(streaks.current), &[])?;
    let second = if streaks.current == 0 {
        cx.tr("stats.widget.streak.start")?
    } else if cx.settings.shows("longest")? {
        cx.format(
            "stats.widget.streak.longest",
            &[Arg::Int {
                name: "count",
                value: i64::from(streaks.longest),
            }],
        )?
    } else {
        cx.tr("stats.in-a-row")?
    };
    Ok((first, second))
}

/// Draws the widget in `area`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let Some(shown) = shown(cx)? else {
        return crate::native_widgets::message(cx, out, &look, "stats.no-clock");
    };
    body(cx, out, &look, &shown)
}

/// The streak and the note below it laid out for an area.
struct Texts {
    size: f32,
    first: String,
    /// The note, where it has room below the streak.
    second: Option<String>,
    /// How wide and high the two are.
    width: u16,
    height: u16,
}

/// The streak as large as the size says (or fits) in `area`, the note below
/// it where there is room for both (else the streak alone), each shortened
/// where it is too wide.
fn lay_texts(cx: &mut Cx<'_>, area: Rect, (first, second): (&str, &str)) -> Result<Texts, Error> {
    let (text_px, small_px) = (cx.style().text, cx.style().small);
    let small = line(cx, small_px)?;
    let width = f32::from(area.width);
    let room = area.height.saturating_sub(small);
    let with_note = text_size(cx, first, area.width, room)?;
    let alone = text_size(cx, first, area.width, area.height)?;
    // The note where there is room for it below the streak, and where it does
    // not make the streak smaller than the body's size.
    let note = line(cx, with_note)?.saturating_add(small) <= area.height
        && (with_note >= text_px || with_note >= alone);
    let size = if note { with_note } else { alone };
    let shown = text::fit(
        &cx.font,
        size,
        number_if_narrow(cx, size, first, area.width)?,
        width,
    )?;
    // The longest streak left out where the note is too long; how to begin one
    // in short.
    let mut kept: Option<String> = None;
    if note {
        let in_a_row = cx.tr("stats.in-a-row")?;
        let start = cx.tr("stats.widget.streak.start")?;
        let wide = cx.font.measure_text(small_px, second)? > width;
        let chosen = if wide && second.starts_with(in_a_row.as_str()) {
            in_a_row
        } else if wide && second == start {
            cx.tr("stats.widget.streak.start-short")?
        } else {
            text::copy(second)?
        };
        // Left out where not even that fits whole: a note shortened says
        // little.
        if cx.font.measure_text(small_px, &chosen)? <= width {
            kept = Some(chosen);
        }
    }
    let mut wide = cx.font.measure_text(size, &shown)?;
    if let Some(s) = &kept {
        wide = wide.max(cx.font.measure_text(small_px, s)?);
    }
    let height = line(cx, size)?.saturating_add(if kept.is_some() { small } else { 0 });
    Ok(Texts {
        size,
        first: shown,
        second: kept,
        width: ((wide + 0.99) as u16).min(area.width),
        height: height.min(area.height),
    })
}

/// Draws the streak and the note below it, as laid out, in the middle of
/// `area` as `align` places them.
fn draw_texts(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    area: Rect,
    laid: &Texts,
    align: Align,
) -> Result<(), Error> {
    let small_px = cx.style().small;
    let small = line(cx, small_px)?;
    let big = line(cx, laid.size)?;
    let block = middle(area, laid.height);
    let x = align.x(area, cx.font.measure_text(laid.size, &laid.first)?);
    text_at(
        cx,
        out,
        &laid.first,
        (laid.size, look.ink),
        (x, Rect::new(area.x, block.y, area.width, big)),
    )?;
    if let Some(second) = &laid.second {
        let x = align.x(area, cx.font.measure_text(small_px, second)?);
        let row = Rect::new(area.x, block.y.saturating_add(big), area.width, small);
        text_at(cx, out, second, (small_px, look.faint), (x, row))?;
    }
    Ok(())
}

/// The first letter of a weekday's short name.
fn initial(cx: &mut Cx<'_>, weekday: u8) -> Result<String, Error> {
    let name = weekday_short(cx, weekday)?;
    let end = name.char_indices().nth(1).map_or(name.len(), |(at, _)| at);
    text::copy(name.get(..end).unwrap_or(&name))
}

/// A mark a day in `area`, placed as `align` says, the weekdays below if
/// `weekdays`.
fn draw_marks(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    area: Rect,
    marks: &[Mark],
    (align, weekdays): (Align, bool),
) -> Result<(), Error> {
    let small_px = cx.style().small;
    let small = line(cx, small_px)?;
    let n = marks.len().max(1) as u32;
    let (column, d) = mark_size(cx, area.width, marks.len());
    // The weekdays where their letters have room.
    let mut weekdays = weekdays;
    if weekdays {
        for w in 0..7 {
            let one = initial(cx, w)?;
            weekdays &= cx.font.measure_text(small_px, &one)? * 1.3 <= column as f32;
        }
    }
    let labels = if weekdays { small } else { 0 };
    let d = d.min(u32::from(area.height.saturating_sub(labels)));
    if d < 4 {
        return Ok(());
    }
    let checks = cx.settings.with_text("style", |v| v == STREAK_MARKS[1])?;
    let thin = u32::from(cx.style().line).max(1);
    // Flush with the text: the outer marks' edges on its margins.
    let inset = (column - d) / 2;
    let span = (column * n - 2 * inset) as f32;
    let left = align.x(area, span) as i32 - inset as i32;
    let radius = d / 2;
    for (i, mark) in marks.iter().enumerate() {
        let cx_x = left + (column * i as u32 + column / 2) as i32;
        let center = Point::new(cx_x, i32::from(area.y) + radius as i32);
        let disc = Rect::new(
            geometry::clamp_u16(i64::from(center.x) - i64::from(radius)),
            geometry::clamp_u16(i64::from(center.y) - i64::from(radius)),
            geometry::clamp_u16(i64::from(d)),
            geometry::clamp_u16(i64::from(d)),
        );
        if mark.counts {
            paint::push(out, Paint::Focus(disc, radius, None, look.ink))?;
            if checks && d >= 10 {
                check(out, center, radius, look.paper)?;
            }
        } else if mark.today {
            let width = (thin * 2).min(radius / 3).max(1);
            paint::push(out, Paint::Focus(disc, radius, Some(width), look.ink))?;
        } else {
            paint::push(out, Paint::Focus(disc, radius, Some(thin), look.faint))?;
        }
        if weekdays {
            let name = initial(cx, mark.weekday)?;
            let w = cx.font.measure_text(small_px, &name)?;
            let row = Rect::new(area.x, area.y.saturating_add(d as u16), area.width, labels);
            let color = if mark.today { look.ink } else { look.faint };
            text_at(
                cx,
                out,
                &name,
                (small_px, color),
                (cx_x as f32 - w / 2.0, row),
            )?;
        }
    }
    Ok(())
}

/// A check in the paper's colour on a filled mark.
fn check(out: &mut Vec<Paint>, center: Point, radius: u32, paper: Color) -> Result<(), Error> {
    let r = radius as i32;
    let at = |x: i32, y: i32| Point::new(center.x + x * r / 100, center.y + y * r / 100);
    let width = (radius / 4).max(1);
    let (a, b, c) = (at(-42, 2), at(-12, 32), at(44, -30));
    for (from, to) in [(a, b), (b, c)] {
        paint::push(
            out,
            Paint::Line {
                from,
                to,
                width,
                color: paper,
            },
        )?;
    }
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "one layout, a branch for each shape of the place"
)]
fn body(cx: &mut Cx<'_>, out: &mut Vec<Paint>, look: &Look, shown: &Shown) -> Result<(), Error> {
    let (gap_px, small_px) = (cx.style().gap, cx.style().small);
    let title = cx.tr("stats.widget.streak.caption")?;
    let (first, second) = texts(cx, shown.streaks)?;
    let mut caption = pick_caption(cx, look.area, &[&title])?;
    let below = |cx: &Cx<'_>, caption: Option<&str>, area: Rect| -> Result<Rect, Error> {
        let used = crate::native_widgets::caption_used(cx, caption, area)?;
        Ok(Rect::new(
            area.x,
            area.y.saturating_add(used),
            area.width,
            area.height.saturating_sub(used),
        ))
    };
    let mut area = below(cx, caption, look.area)?;
    // The caption goes before the note below the streak.
    let has_note = |cx: &mut Cx<'_>, area: Rect| -> Result<bool, Error> {
        Ok(lay_texts(cx, area, (&first, &second))?.second.is_some())
    };
    if caption.is_some() && !has_note(cx, area)? && has_note(cx, look.area)? {
        caption = None;
        area = look.area;
    }
    let small = line(cx, small_px)?;
    let gap = gap_px / 2;
    let weekdays = cx.settings.shows("weekdays")?;
    let align = cx.settings.align()?;
    let head = headline(cx)?;
    let least = line(cx, head)?.saturating_add(small);
    let count = shown.marks.len();
    let mm3 = u32::from(mm(cx, 30));
    let roomy = u32::from(area.width) * 3 / 5 >= mm3 * count as u32;
    if area.width >= area.height.saturating_mul(3) && roomy {
        // Wide and low: the marks beside the text, if there is room, the two
        // placed together as the alignment says. The text two fifths (more
        // where the streak needs it), the marks the rest; the gap between
        // them taken from the marks where the two would not fit together
        // otherwise.
        let space = gap_px;
        let needs = cx.font.measure_text(head, &first)? as u16 + 1;
        let least_marks = (mm3 * count as u32) as u16;
        let side = needs
            .max(area.width * 2 / 5)
            .min(area.width.saturating_sub(least_marks.saturating_add(space)));
        let rest = area.width.saturating_sub(side);
        let fits = |cx: &Cx<'_>, weekdays: bool| -> Result<Option<(u16, bool)>, Error> {
            let h = marks_height(cx, small, rest, weekdays)?;
            Ok((h > 0 && h - gap <= area.height).then_some((h - gap, weekdays)))
        };
        let found = match fits(cx, weekdays)? {
            Some(found) => Some(found),
            None => fits(cx, false)?,
        };
        if let Some((marks, weekdays)) = found {
            let area = captioned(cx, out, look, caption, area.height)?;
            let side_area = Rect::new(area.x, area.y, side, area.height);
            let laid = lay_texts(cx, side_area, (&first, &second))?;
            let (mut column, mut d) = mark_size(cx, rest, count);
            let spans = |column: u32, d: u32| column * count as u32 - (column - d);
            if u32::from(laid.width) + u32::from(space) + spans(column, d) > u32::from(area.width) {
                (column, d) = mark_size(cx, rest.saturating_sub(space), count);
            }
            let inset = (column - d) / 2;
            let span = column * count as u32 - 2 * inset;
            let group = u32::from(laid.width) + u32::from(space) + span;
            let left = geometry::clamp_u16(align.x(area, group as f32) as i64);
            let text_area = Rect::new(left, area.y, laid.width, area.height);
            draw_texts(cx, out, look, text_area, &laid, Align::Left)?;
            let x = left.saturating_add(laid.width).saturating_add(space);
            let width = geometry::clamp_u16(i64::from(column * count as u32));
            let at = middle(Rect::new(x, area.y, width, area.height), marks);
            return draw_marks(cx, out, look, at, &shown.marks, (Align::Left, weekdays));
        }
    }
    // The marks below where there is room for them besides the text, without
    // the weekdays if need be, and without the caption rather than without the
    // marks.
    let in_area = |cx: &Cx<'_>, area: Rect| -> Result<Option<(u16, bool)>, Error> {
        let fits = |weekdays: bool| -> Result<Option<(u16, bool)>, Error> {
            let h = marks_height(cx, small, area.width, weekdays)?;
            Ok((h > 0 && h.saturating_add(least) <= area.height).then_some((h, weekdays)))
        };
        match fits(weekdays)? {
            Some(found) => Ok(Some(found)),
            None => fits(false),
        }
    };
    if caption.is_some() && in_area(cx, area)?.is_none() && in_area(cx, look.area)?.is_some() {
        caption = None;
        area = look.area;
    }
    let (marks, weekdays) = in_area(cx, area)?.unwrap_or((0, false));
    let room = Rect::new(
        area.x,
        area.y,
        area.width,
        area.height.saturating_sub(marks),
    );
    let laid = lay_texts(cx, room, (&first, &second))?;
    // The text and the marks below it, together in the middle.
    let block = captioned(cx, out, look, caption, laid.height.saturating_add(marks))?;
    let at = Rect::new(area.x, block.y, area.width, laid.height);
    draw_texts(cx, out, look, at, &laid, align)?;
    if marks > 0 {
        let at = Rect::new(
            area.x,
            block.y.saturating_add(laid.height).saturating_add(gap),
            area.width,
            marks.saturating_sub(gap),
        );
        draw_marks(cx, out, look, at, &shown.marks, (align, weekdays))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_is_two_segments_in_the_paper_colour_inside_the_mark() {
        let mut out = Vec::new();
        check(&mut out, Point::new(50, 50), 20, Color::WHITE).unwrap();
        assert_eq!(out.len(), 2);
        for op in &out {
            let Paint::Line {
                from,
                to,
                width,
                color,
            } = op
            else {
                panic!("{op:?}");
            };
            assert_eq!(*color, Color::WHITE);
            assert!(*width >= 1);
            for p in [from, to] {
                assert!((p.x - 50).abs() <= 20 && (p.y - 50).abs() <= 20, "{p:?}");
            }
        }
    }
}
