//! The day and the date large, a calm alternative to the home screen's date:
//! the weekday large with the date below it, the day's number large beside
//! the weekday and the month, or a sheet of a tear-off calendar.
use crate::native_cx::Cx;
use crate::native_names::{
    chosen_name, date_text, month_name, month_short, weekday_name, weekday_short,
};
use alloc::{format, string::String, vec::Vec};
use paperpaw_sdk::{
    Rect,
    native::{Error, formatting::Arg},
    time::{LocalTime, iso_week},
};
use paperpaw_ui_kit::{
    Paint, paint,
    widget::{
        Align, TextSize,
        layout::{self, Part, cap_height, centred, fit_size, label, line},
        text::largest_size,
    },
};

/// The height of the widget: what is large, two lines below it, the frame.
///
/// # Errors
/// Font and host refusals.
pub fn height(cx: &mut Cx<'_>, _width: u16) -> Result<u16, Error> {
    let size = big_size(cx, None)?;
    let frame = cx.shape.frame_height();
    let big = paperpaw_ui_kit::geometry::line_px(&cx.font, size)?;
    let text = paperpaw_ui_kit::geometry::line_px(&cx.font, cx.style().text)?;
    Ok(big
        .saturating_add(text.saturating_mul(2))
        .saturating_add(frame))
}

/// What the widget shows, to tell when it changed: the day.
///
/// # Errors
/// Host refusals.
pub fn shows(cx: &Cx<'_>) -> Result<String, Error> {
    Ok(match cx.local_time()? {
        Some(t) => format!("{}-{}-{}", t.year, t.month, t.day),
        None => String::new(),
    })
}

/// The size of what is large: as set, or as fits `room` (height).
fn big_size(cx: &Cx<'_>, room: Option<f32>) -> Result<f32, Error> {
    let style = cx.style();
    Ok(match (cx.settings.size()?, room) {
        (TextSize::Fit, Some(room)) => room,
        (TextSize::Fit, None) => style.title * 2.0,
        (size, _) => size.text(style) * 1.6,
    })
}

/// Draws the widget in `area`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect) -> Result<(), Error> {
    // The snapshot does not tell the gray levels (the same gap as the soft focus):
    // a card is taken to be drawn on a panel with grays.
    let grays = 256;
    let look = cx.shape.paint(out, area, grays)?;
    let Some(t) = cx.local_time()? else {
        return crate::native_draw::no_time(cx, out, &look);
    };
    let align = cx.settings.align()?;
    let with_year = cx.settings.shows("year")?;
    let weekday = weekday_name(cx, t.weekday)?;
    let month = month_name(cx, t.month)?;
    let week = if cx.settings.shows("week")? {
        let number = iso_week(t.year, t.month, t.day);
        Some(cx.format(
            "widgets.year.week",
            &[Arg::Int {
                name: "week",
                value: i64::from(number),
            }],
        )?)
    } else {
        None
    };
    let style = chosen_name(cx, "style")?;
    match style.as_str() {
        "number" => number_style(
            cx,
            out,
            &look,
            (&weekday, &month),
            week.as_deref(),
            &t,
            with_year,
        ),
        "sheet" => sheet(cx, out, &look, &t, with_year, week.as_deref()),
        _ => weekday_style(
            cx,
            out,
            &look,
            align,
            &weekday,
            week.as_deref(),
            &t,
            with_year,
        ),
    }
}

/// "14" large; beside it the weekday, and the month below it.
fn number_style(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &paperpaw_ui_kit::widget::Look,
    (weekday, month): (&str, &str),
    week: Option<&str>,
    t: &LocalTime,
    with_year: bool,
) -> Result<(), Error> {
    let area = look.area;
    let align = cx.settings.align()?;
    let width = f32::from(area.width);
    let number = cx.number(i64::from(t.day))?;
    let month_line = if with_year {
        let year = cx.number(i64::from(t.year))?;
        paperpaw_ui_kit::text::concat(&[month, " ", &year])?
    } else {
        paperpaw_ui_kit::text::copy(month)?
    };
    let mut notes: Vec<&str> = Vec::new();
    notes.try_reserve(3).map_err(|_| Error::NoMemory)?;
    notes.push(weekday);
    notes.push(&month_line);
    if let Some(week) = week {
        notes.push(week);
    }
    let (small, title, gap) = {
        let style = cx.style();
        (style.small, style.title, f32::from(style.gap))
    };
    let room = f32::from(area.height);
    let size = fit_size(
        &cx.font,
        big_size(cx, Some(room * 0.95))?,
        &number,
        width * 0.45,
    )?;
    let number_w = cx.font.measure_text(size, &number)?;
    let gap = gap * 1.5;
    // The notes as large as fits beside it, the weekday larger.
    let rest = (width - number_w - gap).max(0.0);
    let mut note = title * 1.2;
    for held in &notes {
        note = note.min(fit_size(&cx.font, title * 1.2, held, rest)?);
    }
    let note = note.min(room / (notes.len() as f32 + 0.6)).max(small);
    let lines: f32 = line(&cx.font, note * 1.2)? + line(&cx.font, note)? * (notes.len() - 1) as f32;
    let mut widest = 0.0_f32;
    for (n, held) in notes.iter().enumerate() {
        let at = if n == 0 { note * 1.2 } else { note };
        widest = widest.max(cx.font.measure_text(at, held)?);
    }
    let total_w = number_w + gap + widest;
    let left = align.x(area, total_w.min(width));
    let big = line(&cx.font, size)?.min(room);
    let row = Rect::new(
        left as u16,
        area.y.saturating_add(((room - big) / 2.0) as u16),
        (number_w as u16).saturating_add(2),
        big as u16,
    );
    label(out, &cx.font, (size, Align::Left), row, &number, look.ink)?;
    let x = left + number_w + gap;
    let mut y = f32::from(area.y) + (room - lines).max(0.0) / 2.0;
    for (n, held) in notes.iter().enumerate() {
        let (s, color) = if n == 0 {
            (note * 1.2, look.ink)
        } else {
            (note, look.faint)
        };
        let h = line(&cx.font, s)?;
        let row = Rect::new(
            x as u16,
            y as u16,
            (f32::from(area.x) + width - x).max(0.0) as u16,
            h as u16,
        );
        label(out, &cx.font, (s, Align::Left), row, held, color)?;
        y += h;
    }
    Ok(())
}

/// The weekday's size and whether it is the short name, and the date with its
/// size and line height.
type Picked = ((f32, bool), Option<(f32, f32, String)>);

/// "Wednesday" large, the date below it: each whole on its line, the
/// weekday's short name where the name has no room, the date without its
/// year, then with the month's short name, then left out.
#[allow(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "one layout; its inputs are separate things"
)]
fn weekday_style(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &paperpaw_ui_kit::widget::Look,
    align: Align,
    weekday: &str,
    week: Option<&str>,
    t: &LocalTime,
    with_year: bool,
) -> Result<(), Error> {
    let area = look.area;
    let width = f32::from(area.width);
    let height = f32::from(area.height);
    let (small, text_size, gap) = {
        let style = cx.style();
        (style.small, style.text, f32::from(style.gap))
    };
    let mut dates: Vec<String> = Vec::new();
    dates.try_reserve(3).map_err(|_| Error::NoMemory)?;
    dates.push(date_text(cx, t.year, t.month, t.day, with_year)?);
    if with_year {
        dates.push(date_text(cx, t.year, t.month, t.day, false)?);
    }
    let short_month = month_short(cx, t.month)?;
    dates.push(cx.format(
        "widgets.date.short",
        &[
            Arg::Int {
                name: "day",
                value: i64::from(t.day),
            },
            Arg::Text {
                name: "month",
                value: &short_month,
            },
        ],
    )?);
    if let Some(week) = week {
        for date in &mut dates {
            *date = paperpaw_ui_kit::text::concat(&[date, " · ", week])?;
        }
    }
    let most = text_size.min(height / 3.5).max(small);
    let mut date: Option<(f32, String)> = None;
    for candidate in dates {
        if let Some(size) = largest_size(most, small, |s| {
            Ok(cx.font.measure_text(s, &candidate)? <= width)
        })? {
            date = Some((size, candidate));
            break;
        }
    }
    let short = weekday_short(cx, t.weekday)?;
    // The weekday as large as set or as fits, a line at least above the
    // date; without the date where both have no room.
    let weekday_fits = |cx: &Cx<'_>,
                        name: &str,
                        note_h: f32,
                        least: f32|
     -> Result<Option<f32>, Error> {
        let max = big_size(cx, Some((height - note_h) * 0.8))?.max(least);
        largest_size(max, least, |s| {
            Ok(cx.font.measure_text(s, name)? <= width && line(&cx.font, s)? <= height - note_h)
        })
    };
    // The full name at the body size at least, as it heads the date; else
    // the short name.
    let pick = |cx: &Cx<'_>, note_h: f32| -> Result<Option<(f32, bool)>, Error> {
        if let Some(size) = weekday_fits(cx, weekday, note_h, text_size)? {
            return Ok(Some((size, false)));
        }
        Ok(weekday_fits(cx, &short, note_h, small)?.map(|size| (size, true)))
    };
    let mut chosen: Option<Picked> = None;
    if let Some((note, text)) = date {
        let note_h = line(&cx.font, note)?;
        if let Some(big) = pick(cx, note_h)? {
            chosen = Some((big, Some((note, note_h, text))));
        }
    }
    if chosen.is_none()
        && let Some(big) = pick(cx, 0.0)?
    {
        chosen = Some((big, None));
    }
    let ((size, use_short), date) = chosen.unwrap_or(((small, true), None));
    let name: &str = if use_short { &short } else { weekday };
    // The date a step smaller than the weekday, which heads it.
    let date = match date {
        Some((note, h, text)) => {
            let cap = (size * 0.84).max(small);
            if note <= cap {
                Some((note, h, text))
            } else {
                let note =
                    largest_size(cap, small, |s| Ok(cx.font.measure_text(s, &text)? <= width))?
                        .unwrap_or(small);
                Some((note, line(&cx.font, note)?, text))
            }
        }
        None => None,
    };
    // The two centred together by their ink, half a gap apart.
    let mut parts: Vec<Part<'_>> = Vec::new();
    parts.try_reserve(2).map_err(|_| Error::NoMemory)?;
    let mut first: Vec<&str> = Vec::new();
    first.try_reserve(1).map_err(|_| Error::NoMemory)?;
    first.push(name);
    parts.push(Part {
        lines: first,
        size,
        color: look.ink,
    });
    if let Some((note, _, text)) = &date {
        let mut second: Vec<&str> = Vec::new();
        second.try_reserve(1).map_err(|_| Error::NoMemory)?;
        second.push(text.as_str());
        parts.push(Part {
            lines: second,
            size: *note,
            color: look.faint,
        });
    }
    layout::stack(out, &cx.font, area, align, &parts, gap / 2.0)
}

/// A sheet of a tear-off calendar: the month on a dark band at the top, the
/// day's number large, the weekday below it.
#[allow(clippy::too_many_lines, reason = "one drawing")]
fn sheet(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &paperpaw_ui_kit::widget::Look,
    t: &LocalTime,
    with_year: bool,
    week: Option<&str>,
) -> Result<(), Error> {
    let area = look.area;
    let (small, title, corner, line_px) = {
        let style = cx.style();
        (
            style.small,
            style.title,
            style.corner,
            u32::from(style.line),
        )
    };
    // As wide as about four fifths of its height, at most.
    let height = f32::from(area.height);
    let width = f32::from(area.width)
        .min(height * 0.95)
        .max(height * 0.6)
        .min(f32::from(area.width));
    let align = cx.settings.align()?;
    let left = align.x(area, width);
    let sheet = Rect::new(left as u16, area.y, width as u16, area.height);
    let radius = corner.max((width * 0.06) as u32);
    // The month (with the year where set) on the band where it fits, else in
    // short, else shortened: never wider than the sheet.
    let room = width * 0.85;
    let band_max = (height * 0.11).max(small);
    let full = month_name(cx, t.month)?;
    let short = month_short(cx, t.month)?;
    let mut names: Vec<String> = Vec::new();
    names.try_reserve(3).map_err(|_| Error::NoMemory)?;
    if with_year {
        let year = cx.number(i64::from(t.year))?;
        names.push(paperpaw_ui_kit::text::concat(&[&full, " ", &year])?);
    }
    names.push(paperpaw_ui_kit::text::copy(&full)?);
    names.push(paperpaw_ui_kit::text::copy(&short)?);
    let mut month = None;
    for candidate in &names {
        if largest_size(band_max, small, |s| {
            Ok(cx.font.measure_text(s, candidate)? <= room)
        })?
        .is_some()
        {
            month = Some(paperpaw_ui_kit::text::copy(candidate)?);
            break;
        }
    }
    let month = match month {
        Some(month) => month,
        None => paperpaw_ui_kit::text::fit(&cx.font, small, &short, room)?,
    };
    let band_size = fit_size(&cx.font, band_max, &month, room)?;
    let band_h = (line(&cx.font, band_size)? * 1.25).min(height * 0.3);
    // The band in ink, the sheet's paper inside a line round it.
    let thick = line_px * 2;
    paint::push(out, Paint::Focus(sheet, radius, None, look.ink))?;
    let inner = Rect::new(
        sheet.x.saturating_add(thick as u16),
        sheet.y.saturating_add(band_h as u16),
        sheet.width.saturating_sub(2 * thick as u16),
        sheet.height.saturating_sub(band_h as u16 + thick as u16),
    );
    paint::push(
        out,
        Paint::Focus(inner, radius.saturating_sub(thick), None, look.paper),
    )?;
    // Square where it meets the band.
    paint::push(
        out,
        Paint::Fill(
            Rect::new(
                inner.x,
                inner.y,
                inner.width,
                (radius as u16).min(inner.height),
            ),
            look.paper,
        ),
    )?;
    let cx_mid = left + width / 2.0;
    centred(
        out,
        &cx.font,
        band_size,
        (cx_mid, f32::from(sheet.y) + band_h / 2.0),
        &month,
        look.paper,
    )?;
    let weekday = weekday_name(cx, t.weekday)?;
    let mut notes: Vec<&str> = Vec::new();
    notes.try_reserve(2).map_err(|_| Error::NoMemory)?;
    notes.push(&weekday);
    if let Some(week) = week {
        notes.push(week);
    }
    let note = fit_size(
        &cx.font,
        (height * 0.1).clamp(small, title),
        &weekday,
        width * 0.85,
    )?;
    let notes_h = line(&cx.font, note)? * notes.len() as f32;
    let room = f32::from(inner.height) - notes_h;
    let number = cx.number(i64::from(t.day))?;
    let size = fit_size(&cx.font, room * 0.72, &number, width * 0.8)?;
    let digits = cap_height(&cx.font, size)?;
    let gap = (room - digits) / 2.0 * 0.6;
    let total = digits + gap + notes_h;
    let top = f32::from(inner.y) + (f32::from(inner.height) - total) / 2.0;
    centred(
        out,
        &cx.font,
        size,
        (cx_mid, top + digits / 2.0),
        &number,
        look.ink,
    )?;
    let mut y = top + digits + gap;
    for (n, held) in notes.iter().enumerate() {
        let h = line(&cx.font, note)?;
        let row = Rect::new(inner.x, y as u16, inner.width, h as u16);
        let color = if n == 0 { look.ink } else { look.faint };
        label(out, &cx.font, (note, Align::Center), row, held, color)?;
        y += h;
    }
    Ok(())
}
