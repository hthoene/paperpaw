//! *Daily goal*: today's reading time as a ring that fills towards the daily
//! goal (`stats.goal`), the minutes in the middle and the streak below ("5
//! days in a row"), or what is left to read today.
//!
//! Without a goal the ring stays empty around the minutes read, and below it
//! the widget asks for a goal: a tap opens the statistics' settings, where the
//! goal is set (it is not measured against a made-up goal, which would tell of
//! a progress nobody asked for). In a place much wider than high the ring sits
//! on the left, the time and the note beside it.
//!
//! The canvas has no arc: a ring is a run of short thick segments along the
//! circle, with round ends made of discs.
use crate::declarations::RING_STYLES;
use crate::native_widgets::{
    caption_room, captioned, days, goal, middle, number_if_narrow, pick_caption, shade, sized,
    text_at,
};
use crate::texts::{duration, seconds_on};
use crate::totals;
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::Error;
use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::{Point, Rect};
use paperpaw_ui_kit::widget::text::{Wrapped, fit_line};
use paperpaw_ui_kit::widget::{Cx, Look};
use paperpaw_ui_kit::{Paint, geometry, paint, text};

/// What the ring shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Numbers {
    today: u64,
    /// Days in a row the goal was reached (any reading without a goal).
    streak: u32,
}

/// The numbers, if the device knows the time.
fn numbers(cx: &Cx<'_>, goal: Option<u64>) -> Result<Option<Numbers>, Error> {
    let Some((today, days)) = days(cx)? else {
        return Ok(None);
    };
    let least = goal.map_or(1, |g| g.min(u64::from(u32::MAX)) as u32);
    Ok(Some(Numbers {
        today: u64::from(seconds_on(&days, today)),
        streak: totals::streaks(&days, today, least).current,
    }))
}

/// The sizes of the style the layout uses.
#[derive(Debug, Clone, Copy)]
struct Sizes {
    title: f32,
    small: f32,
    gap: u16,
    line: u16,
}

impl Sizes {
    fn of(cx: &Cx<'_>) -> Self {
        let style = cx.style();
        Self {
            title: style.title,
            small: style.small,
            gap: style.gap,
            line: style.line,
        }
    }
}

/// The line a text of `size` takes.
fn line(cx: &Cx<'_>, size: f32) -> Result<u16, Error> {
    geometry::line_px(&cx.font, size)
}

/// The ring's largest diameter by the size setting (as large as fits: what a
/// medium one has, for the height it asks for).
fn largest(cx: &Cx<'_>) -> Result<u16, Error> {
    use paperpaw_ui_kit::widget::TextSize;
    let title = line(cx, cx.style().title)?;
    Ok(match cx.settings.size()? {
        TextSize::Small => title.saturating_mul(3),
        TextSize::Large => title.saturating_mul(6),
        _ => title.saturating_mul(9) / 2,
    })
}

/// The room the note takes below the ring.
fn note_height(cx: &Cx<'_>, goal: Option<u64>) -> Result<u16, Error> {
    if cx.settings.shows("note")? || goal.is_none() {
        Ok(line(cx, cx.style().small)?.saturating_add(cx.style().gap / 2))
    } else {
        Ok(0)
    }
}

/// The height the widget takes: the frame, the caption, the ring and the note.
///
/// # Errors
/// Font and host refusals.
pub fn height(cx: &mut Cx<'_>, width: u16) -> Result<u16, Error> {
    let goal = goal(cx.host)?;
    let ring = largest(cx)?.min(width);
    Ok(cx
        .shape
        .frame_height()
        .saturating_add(caption_room(cx)?)
        .saturating_add(ring)
        .saturating_add(note_height(cx, goal)?))
}

/// The note: the streak, what is left today, or the goal reached; without a
/// goal, the request for one.
fn note(cx: &mut Cx<'_>, numbers: Numbers, goal: Option<u64>) -> Result<Option<String>, Error> {
    let Some(goal) = goal else {
        return cx.tr("stats.widget.goal.set").map(Some);
    };
    if !cx.settings.shows("note")? {
        return Ok(None);
    }
    Ok(Some(if numbers.streak > 0 {
        cx.plural("stats.goal.streak", i64::from(numbers.streak), &[])?
    } else if numbers.today >= goal {
        cx.tr("stats.goal.reached")?
    } else {
        let left = duration(cx, (goal - numbers.today).max(60))?;
        cx.format(
            "stats.goal.to-go",
            &[Arg::Text {
                name: "time",
                value: &left,
            }],
        )?
    }))
}

/// "4 min", or with a goal "4 of 30 min".
fn time_text(cx: &mut Cx<'_>, numbers: Numbers, goal: Option<u64>) -> Result<String, Error> {
    let time = duration(cx, numbers.today)?;
    let Some(goal) = goal else {
        return Ok(time);
    };
    let goal = duration(cx, goal)?;
    cx.format(
        "stats.widget.goal.of",
        &[
            Arg::Text {
                name: "time",
                value: &time,
            },
            Arg::Text {
                name: "goal",
                value: &goal,
            },
        ],
    )
}

/// The least diameter of a ring with the minutes inside it: smaller, the
/// widget shows the time as text.
fn least_ring(cx: &Cx<'_>) -> Result<u16, Error> {
    Ok(line(cx, cx.style().text)?.saturating_mul(3) / 2)
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
    body(cx, out, &look, numbers, goal)
}

/// How the pieces go where the area is wide: the ring on the left, text beside.
struct Beside<'a> {
    first: &'a str,
    size: f32,
    note: Option<&'a str>,
}

#[expect(
    clippy::too_many_lines,
    reason = "one layout, a branch for each shape of the place"
)]
fn body(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    numbers: Numbers,
    goal: Option<u64>,
) -> Result<(), Error> {
    use paperpaw_ui_kit::widget::{Align, TextSize};
    let style = Sizes::of(cx);
    let title = cx.tr("stats.widget.goal.caption")?;
    let caption = pick_caption(cx, look.area, &[&title])?;
    let used = crate::native_widgets::caption_used(cx, caption, look.area)?;
    let area = Rect::new(
        look.area.x,
        look.area.y.saturating_add(used),
        look.area.width,
        look.area.height.saturating_sub(used),
    );
    let note = note(cx, numbers, goal)?;
    let limit = if cx.settings.size()? == TextSize::Fit {
        u16::MAX
    } else {
        largest(cx)?
    };
    let align = cx.settings.align()?;
    let progress = goal.map(|goal| (numbers.today * 1000 / goal.max(1)).min(1000) as u32);
    let small = line(cx, style.small)?;
    let least = least_ring(cx)?;
    let first = time_text(cx, numbers, goal)?;
    if area.width >= area.height.saturating_mul(2) && area.height >= least {
        // Wide: the ring, the time and the note beside it, placed together as
        // the alignment says.
        let area = captioned(cx, out, look, caption, area.height)?;
        let d = area.height.min(area.width.saturating_mul(2) / 5).min(limit);
        let gap = style.gap;
        let room = area.width.saturating_sub(d.saturating_add(gap));
        let size = sized(
            cx,
            &first,
            (room, area.height / 2),
            (style.title * 3.0, style.small),
        )?
        .min(style.title);
        let big = line(cx, size)?;
        let (_, shown) = fit_line(&cx.font, size, size, &first, f32::from(room))?;
        // The note where it fits whole below the time; else, where it fits
        // whole on two lines, the note instead of the time (the ring shows the
        // minutes).
        let mut below: Option<String> = None;
        let mut instead: Option<Wrapped> = None;
        if let Some(n) = &note {
            if big.saturating_add(small) <= area.height
                && cx.font.measure_text(style.small, n)? <= f32::from(room)
            {
                below = Some(text::copy(n)?);
            } else {
                instead = crate::native_widgets::note_lines(cx, n, (room, area.height), 2)?
                    .filter(Wrapped::whole);
            }
        }
        let mut wide = 0.0_f32;
        if let Some(lines) = &instead {
            for one in lines.lines() {
                wide = wide.max(cx.font.measure_text(style.small, one)?);
            }
        } else {
            wide = cx.font.measure_text(size, &shown)?;
            if let Some(n) = &below {
                wide = wide.max(cx.font.measure_text(style.small, n)?);
            }
        }
        let group = d
            .saturating_add(gap)
            .saturating_add(((wide + 0.99) as u16).min(room));
        let left = geometry::clamp_u16(align.x(area, f32::from(group)) as i64);
        let ring = middle(Rect::new(left, area.y, d, area.height), d);
        let text_area = Rect::new(
            left.saturating_add(d).saturating_add(gap),
            area.y,
            room,
            area.height,
        );
        draw_ring(
            cx,
            out,
            look,
            ring,
            progress,
            numbers.today,
            instead.is_some(),
        )?;
        if let Some(lines) = instead {
            lines.draw(out, &cx.font, text_area, Align::Left, look.faint)?;
        } else {
            let beside = Beside {
                first: &shown,
                size,
                note: below.as_deref(),
            };
            draw_beside(cx, out, look, text_area, &beside)?;
        }
        return Ok(());
    }
    // The ring with the note below, together in the middle; where the note
    // would leave the ring small, the ring alone. The note on two lines where
    // it needs them and they fit whole, else left out.
    let gap = style.gap / 2;
    let ring_in = |note_h: u16| {
        area.width
            .min(area.height.saturating_sub(note_h))
            .min(limit)
    };
    let lines = match &note {
        Some(n) => crate::native_widgets::note_lines(cx, n, (area.width, area.height), 2)?,
        None => None,
    };
    let with_note = match &lines {
        Some(l) => Some(l.height(&cx.font)?.saturating_add(gap)).filter(|&h| {
            let small_ring = progress.is_some() && ring_in(h) < area.width.saturating_mul(2) / 5;
            !small_ring && ring_in(h) >= least
        }),
        None => None,
    };
    let note_h = with_note.unwrap_or(0);
    let d = ring_in(note_h);
    if d < least {
        // No room for a ring: the time as text, the note below it.
        return draw_time(cx, out, (look, caption), area, (&first, lines.as_ref()));
    }
    let block = captioned(cx, out, look, caption, d.saturating_add(note_h))?;
    let x = geometry::clamp_u16(align.x(area, f32::from(d)) as i64);
    let ring = Rect::new(x, block.y, d, d);
    draw_ring(cx, out, look, ring, progress, numbers.today, true)?;
    if let Some(lines) = lines.filter(|_| note_h > 0) {
        let row = Rect::new(
            area.x,
            block.y.saturating_add(d).saturating_add(gap),
            area.width,
            note_h.saturating_sub(gap),
        );
        lines.draw(out, &cx.font, row, align, look.faint)?;
    }
    Ok(())
}

/// The time `size` pixels large in `first` beside the ring, the note below it,
/// together in the middle of its height.
fn draw_beside(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    at: Rect,
    beside: &Beside<'_>,
) -> Result<(), Error> {
    let small = cx.style().small;
    let (big, little) = (line(cx, beside.size)?, line(cx, small)?);
    let note_h = if beside.note.is_some() { little } else { 0 };
    let block = middle(at, big.saturating_add(note_h));
    let x = f32::from(at.x);
    text_at(
        cx,
        out,
        beside.first,
        (beside.size, look.ink),
        (x, Rect::new(at.x, block.y, at.width, big)),
    )?;
    if let Some(note) = beside.note {
        let row = Rect::new(at.x, block.y.saturating_add(big), at.width, little);
        text_at(cx, out, note, (small, look.faint), (x, row))?;
    }
    Ok(())
}

/// The time read as text where there is no room for a ring, as large as fits,
/// with the note below it where that fits too.
fn draw_time(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    (look, caption): (&Look, Option<&str>),
    area: Rect,
    (first, note): (&str, Option<&Wrapped>),
) -> Result<(), Error> {
    let style = cx.style();
    let note_h = match note {
        Some(n) => n.height(&cx.font)?,
        None => 0,
    };
    // The time's size in `area` with the note below it (if `with`), and
    // whether the note keeps: not where it leaves no room or makes the time
    // smaller than the body's size.
    let lay = |area: Rect, with: bool| -> Result<(f32, u16, bool), Error> {
        let fits = |note_h: u16| -> Result<(f32, u16), Error> {
            let room = area.height.saturating_sub(note_h);
            let size = crate::native_widgets::text_size(cx, first, area.width, room)?;
            Ok((size, line(cx, size)?))
        };
        let (size, big) = fits(note_h)?;
        let alone = fits(0)?;
        Ok(
            if !with
                || big.saturating_add(note_h) > area.height
                || (size < style.text && alone.0 > size)
            {
                (alone.0, alone.1, false)
            } else {
                (size, big, true)
            },
        )
    };
    let (mut size, mut big, mut keep) = lay(area, note.is_some())?;
    let (mut area, mut caption) = (area, caption);
    // The caption goes before the note.
    if note.is_some() && !keep && caption.is_some() {
        let without = lay(look.area, true)?;
        if without.2 {
            (size, big, keep) = without;
            (area, caption) = (look.area, None);
        }
    }
    let note = note.filter(|_| keep);
    let note_h = match note {
        Some(n) => n.height(&cx.font)?,
        None => 0,
    };
    let align = cx.settings.align()?;
    let block = captioned(cx, out, look, caption, big.saturating_add(note_h))?;
    let shown = number_if_narrow(cx, size, first, area.width)?;
    let (_, shown) = fit_line(&cx.font, size, size, shown, f32::from(area.width))?;
    let x = align.x(area, cx.font.measure_text(size, &shown)?);
    text_at(
        cx,
        out,
        &shown,
        (size, look.ink),
        (x, Rect::new(area.x, block.y, area.width, big)),
    )?;
    if let Some(note) = note {
        let row = Rect::new(area.x, block.y.saturating_add(big), area.width, note_h);
        note.draw(out, &cx.font, row, align, look.faint)?;
    }
    Ok(())
}

/// The sine and cosine of `degrees` clockwise from the top, in thousandths
/// (Bhaskara's approximation, within two thousandths).
fn sin_cos(degrees: i32) -> (i32, i32) {
    let sine = |d: i32| -> i32 {
        let d = d.rem_euclid(360);
        let (d, sign) = if d > 180 { (d - 180, -1) } else { (d, 1) };
        let p = d * (180 - d);
        sign * (4000 * p / (40500 - p))
    };
    (sine(degrees), sine(degrees + 90))
}

/// The point at `degrees` (clockwise from the top) on the circle of `radius`
/// around `center`.
fn point_at(center: Point, radius: i32, degrees: i32) -> Point {
    let (sin, cos) = sin_cos(degrees);
    Point::new(
        center.x + (sin * radius + 500).div_euclid(1000),
        center.y - (cos * radius + 500).div_euclid(1000),
    )
}

/// A disc of `diameter` around `at`.
fn disc(
    out: &mut Vec<Paint>,
    at: Point,
    diameter: u32,
    color: paperpaw_sdk::Color,
) -> Result<(), Error> {
    let radius = diameter / 2;
    let rect = Rect::new(
        geometry::clamp_u16(i64::from(at.x) - i64::from(radius)),
        geometry::clamp_u16(i64::from(at.y) - i64::from(radius)),
        geometry::clamp_u16(i64::from(diameter)),
        geometry::clamp_u16(i64::from(diameter)),
    );
    paint::push(out, Paint::Focus(rect, radius, None, color))
}

/// An arc of the ring around `center` of the outer `radius`, `thick` wide,
/// from `from` degrees `sweep` degrees on, as segments each a little shorter
/// than the line is thick (at most [`MOST`]).
fn arc(
    out: &mut Vec<Paint>,
    (center, radius, thick): (Point, u32, u32),
    (from, sweep): (i32, u32),
    color: paperpaw_sdk::Color,
) -> Result<(), Error> {
    /// The most segments of one arc.
    const MOST: u32 = 120;
    let mid = radius.saturating_sub(thick / 2) as i32;
    // The length of the arc in pixels over a half of the thickness.
    let length = u64::from(mid.unsigned_abs()) * 6283 * u64::from(sweep) / 360_000;
    let steps = (length * 2 / u64::from(thick.max(1)) + 1).clamp(2, u64::from(MOST)) as u32;
    let mut last = point_at(center, mid, from);
    for step in 1..=steps {
        let degrees = from + (sweep * step / steps) as i32;
        let next = point_at(center, mid, degrees);
        paint::push(
            out,
            Paint::Line {
                from: last,
                to: next,
                width: thick,
                color,
            },
        )?;
        last = next;
    }
    Ok(())
}

/// Draws the ring into the square `at`: a thin track all round (three quarters
/// for a gauge), over it the share of the goal reached (`progress` in
/// thousandths) with round ends, and the minutes read inside (with "min"
/// below them if `unit` and the part shows).
fn draw_ring(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    at: Rect,
    progress: Option<u32>,
    seconds: u64,
    unit: bool,
) -> Result<(), Error> {
    let style = Sizes::of(cx);
    let d = u32::from(at.width.min(at.height));
    if d < 8 {
        return Ok(());
    }
    let radius = d / 2;
    let thick = cx
        .settings
        .with_text("line", |v| match v {
            "stats.line.thin" => d / 20,
            "stats.line.thick" => d / 8,
            _ => d / 12,
        })?
        // Small rings thicker, to tell the share from the track.
        .max(3)
        .max(if d < 80 { d / 8 } else { 0 })
        .clamp(1, radius);
    let gauge = cx.settings.with_text("style", |v| v == RING_STYLES[1])?;
    let (from, sweep) = if gauge { (225, 270) } else { (0, 360) };
    let center = Point::new(
        i32::from(at.x) + radius as i32,
        i32::from(at.y) + radius as i32,
    );
    // The track, thin, in the middle of where the ring is drawn.
    let track = u32::from(style.line).max(1).min(thick);
    let track_radius = radius - (thick - track) / 2;
    arc(
        out,
        (center, track_radius, track),
        (from, sweep),
        shade(look.paper, 2),
    )?;
    if let Some(progress) = progress.filter(|p| *p > 0) {
        let done = (sweep * progress.min(1000) / 1000).max(1);
        arc(out, (center, radius, thick), (from, done), look.ink)?;
        // Round ends, but for a whole ring.
        if done < 360 {
            let mid = (radius - thick / 2) as i32;
            for degrees in [from, from + done as i32] {
                disc(out, point_at(center, mid, degrees), thick, look.ink)?;
            }
        }
    }
    // The minutes inside.
    let inner = d.saturating_sub(2 * thick + 2 * track) as u16;
    let minutes = text::format(format_args!("{}", seconds / 60))?;
    let unit = unit && cx.settings.shows("unit")?;
    let room = (u32::from(inner) * 45 / 100) as u16;
    let size = sized(
        cx,
        "000",
        (inner.saturating_mul(3) / 5, room),
        (style.title * 3.0, style.small),
    )?;
    let big = line(cx, size)?;
    let unit_h = if unit { line(cx, style.small)? } else { 0 };
    let unit_h = if unit_h.saturating_add(big) <= inner {
        unit_h
    } else {
        0
    };
    let shown = middle(
        Rect::new(
            at.x,
            (center.y as u16).saturating_sub(inner / 2),
            at.width,
            inner,
        ),
        big.saturating_add(unit_h),
    );
    let w = cx.font.measure_text(size, &minutes)?;
    if w > f32::from(inner) * 0.9 || big > inner {
        // A ring too small for the minutes: the ring alone.
        return Ok(());
    }
    let cx_x = center.x as f32;
    text_at(
        cx,
        out,
        &minutes,
        (size, look.ink),
        (cx_x - w / 2.0, Rect::new(at.x, shown.y, at.width, big)),
    )?;
    if unit_h > 0 {
        let words = cx.tr("stats.unit.minutes")?;
        let w = cx.font.measure_text(style.small, &words)?;
        let row = Rect::new(at.x, shown.y.saturating_add(big), at.width, unit_h);
        text_at(
            cx,
            out,
            &words,
            (style.small, look.faint),
            (cx_x - w / 2.0, row),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sine_is_close_at_every_degree() {
        for degrees in -360..720 {
            let (sin, cos) = sin_cos(degrees);
            let rad = f64::from(degrees).to_radians();
            assert!(
                (f64::from(sin) / 1000.0 - rad.sin()).abs() < 0.003,
                "{degrees}"
            );
            assert!(
                (f64::from(cos) / 1000.0 - rad.cos()).abs() < 0.003,
                "{degrees}"
            );
        }
    }

    #[test]
    fn degrees_run_clockwise_from_the_top() {
        let center = Point::new(100, 100);
        assert_eq!(point_at(center, 50, 0), Point::new(100, 50));
        assert_eq!(point_at(center, 50, 90), Point::new(150, 100));
        assert_eq!(point_at(center, 50, 180), Point::new(100, 150));
        assert_eq!(point_at(center, 50, 270), Point::new(50, 100));
    }

    #[test]
    fn an_arc_is_a_run_of_segments_never_more_than_the_most() {
        let mut out = Vec::new();
        arc(
            &mut out,
            (Point::new(500, 500), 400, 4),
            (0, 360),
            paperpaw_sdk::Color::BLACK,
        )
        .unwrap();
        assert_eq!(out.len(), 120);
        let mut few = Vec::new();
        arc(
            &mut few,
            (Point::new(30, 30), 20, 8),
            (225, 270),
            paperpaw_sdk::Color::BLACK,
        )
        .unwrap();
        assert!(few.len() >= 2 && few.len() < 30, "{}", few.len());
    }
}
