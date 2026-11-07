//! The countdown: the days until a date the reader sets, once (`2026-12-24`)
//! or each year (`12-24`, e.g. a birthday), with a title ("12 days until
//! Holiday"). On the day it says so; a day once passed is counted since. A tap
//! opens its settings.
use crate::native_dates::{days_from_civil, days_in_month};
use crate::native_draw::{fnv, message_in, no_time, today};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::Host;
use paperpaw_sdk::{
    Rect,
    native::{Error, formatting::Arg},
};
use paperpaw_ui_kit::widget::names::{chosen_name, date_text, month_name};
use paperpaw_ui_kit::widget::{Cx, Settings};
use paperpaw_ui_kit::{
    Paint,
    text::{concat, copy},
    widget::{
        TextSize,
        layout::{
            self, Part, balanced, cap_height, label, largest, line, stack_height, wrap, wrap_whole,
        },
        text::largest_size,
    },
};

/// A date to count to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Once(i32, u8, u8),
    /// Each year: month, day.
    Yearly(u8, u8),
}

/// The date in `text`: `YYYY-MM-DD`, `MM-DD`, `DD.MM.YYYY` or `DD.MM.`.
#[must_use]
pub fn parse(text: &str) -> Option<Target> {
    let text = text.trim().trim_start_matches("--");
    let numbers = |sep: char| -> Option<([u32; 3], usize)> {
        let mut out = [0u32; 3];
        let mut count = 0;
        for part in text.split(sep).filter(|p| !p.is_empty()) {
            if count == 3 {
                return None;
            }
            out[count] = part.trim().parse().ok()?;
            count += 1;
        }
        Some((out, count))
    };
    let (year, month, day) = if text.contains('.') {
        match numbers('.')? {
            ([d, m, y], 3) => (Some(y), m, d),
            ([d, m, _], 2) => (None, m, d),
            _ => return None,
        }
    } else {
        match numbers('-')? {
            ([y, m, d], 3) => (Some(y), m, d),
            ([m, d, _], 2) => (None, m, d),
            _ => return None,
        }
    };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (month, day) = (month as u8, day as u8);
    match year {
        Some(y) if (1..=9999).contains(&y) => {
            let y = y as i32;
            (day <= days_in_month(y, month)).then_some(Target::Once(y, month, day))
        }
        Some(_) => None,
        // Each year: 29 February too (the 28th where there is none).
        None => (day <= days_in_month(2024, month)).then_some(Target::Yearly(month, day)),
    }
}

/// The days from `today` to the target's next day (negative once passed) and
/// that day.
#[must_use]
pub fn days_until(today: (i32, u8, u8), target: Target) -> (i64, (i32, u8, u8)) {
    let from = days_from_civil(today.0, today.1, today.2);
    let day = match target {
        Target::Once(y, m, d) => (y, m, d),
        Target::Yearly(m, d) => {
            let in_year = |y: i32| (y, m, d.min(days_in_month(y, m)));
            let this = in_year(today.0);
            if days_from_civil(this.0, this.1, this.2) >= from {
                this
            } else {
                in_year(today.0 + 1)
            }
        }
    };
    (days_from_civil(day.0, day.1, day.2) - from, day)
}

/// What it shows, to tell when it changed: the day, the date and the title.
///
/// # Errors
/// Host refusals.
pub fn shows(host: &Host<'_>, widget: &str) -> Result<u64, Error> {
    let settings = Settings::new(host, widget);
    let mark = fnv(today(host)?, settings.string("date", 64)?.as_bytes());
    Ok(fnv(mark, settings.string("title", 128)?.as_bytes()))
}

/// The height: the number, two small lines, the frame.
///
/// # Errors
/// Font refusals.
pub fn height(cx: &mut Cx<'_>, _: u16) -> Result<u16, Error> {
    let (title, small) = (cx.style().title, cx.style().small);
    let big = paperpaw_ui_kit::geometry::line_px(&cx.font, title * 1.6)?;
    let notes = paperpaw_ui_kit::geometry::line_px(&cx.font, small)?;
    Ok(big
        .saturating_add(notes.saturating_mul(2))
        .saturating_add(cx.shape.frame_height()))
}

/// What it shows: the number (or "Today"), and what it counts to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    /// The number of days, or "Today", "Tomorrow", "Yesterday".
    pub big: String,
    /// "days until Holiday".
    pub note: String,
    /// The whole as a sentence: "12 days until Holiday".
    pub sentence: String,
    /// What the number counts where the note has no room: "days".
    pub short: String,
}

/// The texts for `days` days to (or since) `title`.
///
/// # Errors
/// Host refusals and [`Error::NoMemory`].
pub fn shown(cx: &mut Cx<'_>, days: i64, title: &str) -> Result<Shown, Error> {
    let args = [Arg::Text {
        name: "title",
        value: title,
    }];
    let named = |cx: &mut Cx<'_>, word: &str, sentence: &str| -> Result<Shown, Error> {
        Ok(Shown {
            big: cx.tr(word)?,
            note: copy(title)?,
            sentence: cx.format(sentence, &args)?,
            short: String::new(),
        })
    };
    match days {
        0 => named(cx, "widgets.countdown.today", "widgets.countdown.today-is"),
        1 => named(
            cx,
            "widgets.countdown.tomorrow",
            "widgets.countdown.tomorrow-is",
        ),
        -1 => named(
            cx,
            "widgets.countdown.yesterday",
            "widgets.countdown.yesterday-was",
        ),
        2.. => Ok(Shown {
            big: cx.number(days)?,
            note: cx.plural("widgets.countdown.until", days, &args)?,
            sentence: cx.plural("widgets.countdown.days-until", days, &args)?,
            short: cx.plural("widgets.countdown.unit", days, &[])?,
        }),
        _ => Ok(Shown {
            big: cx.number(-days)?,
            note: cx.plural("widgets.countdown.since", -days, &args)?,
            sentence: cx.plural("widgets.countdown.days-ago", -days, &args)?,
            short: cx.plural("widgets.countdown.ago", -days, &[])?,
        }),
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
    let date_setting = cx.settings.string("date", 64)?;
    let Some(target) = parse(&date_setting) else {
        let message = cx.tr("widgets.countdown.no-date")?;
        return message_in(cx, out, &look, &message);
    };
    let (days, day) = days_until((now.year, now.month, now.day), target);
    let date = date_text(
        cx,
        day.0,
        day.1,
        day.2,
        matches!(target, Target::Once(..)) && day.0 != now.year,
    )?;
    let title_setting = cx.settings.string("title", 128)?;
    let title = title_setting.trim();
    let titled = !title.is_empty();
    // Without a title, the date in short: "24 December".
    let short = if day.0 == now.year {
        let month = month_name(cx, day.1)?;
        cx.format(
            "widgets.date.short",
            &[
                Arg::Int {
                    name: "day",
                    value: i64::from(day.2),
                },
                Arg::Text {
                    name: "month",
                    value: &month,
                },
            ],
        )?
    } else {
        copy(&date)?
    };
    let title = if titled { title } else { short.as_str() };
    let shown = shown(cx, days, title)?;
    // Notes below: the date (where the title is not it already) and the weeks.
    let mut notes: Vec<String> = Vec::new();
    notes.try_reserve(2).map_err(|_| Error::NoMemory)?;
    if cx.settings.shows("weeks")? && days.abs() >= 7 {
        let weeks = cx.plural("widgets.countdown.weeks", days.abs() / 7, &[])?;
        let rest = days.abs() % 7;
        notes.push(if rest == 0 {
            weeks
        } else {
            let rest = cx.plural("widgets.countdown.days", rest, &[])?;
            concat(&[&weeks, ", ", &rest])?
        });
    }
    if cx.settings.shows("show-date")? && titled {
        notes.push(copy(&date)?);
    }
    let mut joined = String::new();
    for (n, note) in notes.iter().enumerate() {
        joined
            .try_reserve(note.len() + 4)
            .map_err(|_| Error::NoMemory)?;
        if n > 0 {
            joined.push_str(" · ");
        }
        joined.push_str(note);
    }
    let notes = joined;
    let area = look.area;
    let align = cx.settings.align()?;
    let small_size = cx.style().small;
    let small = line(&cx.font, small_size)?;
    let notes_h = if notes.is_empty() { 0.0 } else { small };
    if chosen_name(cx, "style")?.as_str() == "sentence" {
        return sentence(cx, out, &look, &shown, &notes, notes_h);
    }
    number_style(cx, out, &look, &shown, &notes, align, area)
}

fn sentence(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &paperpaw_ui_kit::widget::Look,
    shown: &Shown,
    notes: &str,
    notes_h: f32,
) -> Result<(), Error> {
    let area = look.area;
    let align = cx.settings.align()?;
    let small_size = cx.style().small;
    let small = line(&cx.font, small_size)?;
    let room = Rect::new(
        area.x,
        area.y,
        area.width,
        (f32::from(area.height) - notes_h) as u16,
    );
    let size = text_size(cx, &shown.sentence, room)?;
    let mut lines = balanced(&cx.font, size, &shown.sentence, f32::from(room.width))?;
    let height = line(&cx.font, size)?;
    // No more lines than the place holds: the last one shortened.
    let fit = ((f32::from(room.height) / height.max(1.0)) as usize).max(1);
    if lines.len() > fit {
        lines = wrap(&cx.font, size, &shown.sentence, f32::from(room.width), fit)?;
    }
    // The notes where they fit below.
    let notes_h = if height * lines.len() as f32 + notes_h <= f32::from(area.height) {
        notes_h
    } else {
        0.0
    };
    let total = height * lines.len() as f32 + notes_h;
    let mut y = f32::from(area.y) + (f32::from(area.height) - total).max(0.0) / 2.0;
    for text in &lines {
        let row = Rect::new(area.x, y as u16, area.width, height as u16);
        label(out, &cx.font, (size, align), row, text, look.ink)?;
        y += height;
    }
    if notes_h > 0.0 {
        let row = Rect::new(area.x, y as u16, area.width, small as u16);
        label(out, &cx.font, (small_size, align), row, notes, look.faint)?;
    }
    Ok(())
}

/// How the label below the number is laid: its size and lines, and whether
/// the notes go below it too.
type Laid = (f32, f32, Vec<String>, bool);

#[allow(clippy::too_many_lines, reason = "one layout")]
fn number_style(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &paperpaw_ui_kit::widget::Look,
    shown: &Shown,
    notes: &str,
    align: paperpaw_ui_kit::widget::Align,
    area: Rect,
) -> Result<(), Error> {
    // The number large, what it counts to below it (whole in as few lines as
    // fit, from the body size down to the small one, else on one line
    // shortened at its end), the notes below where they fit whole; a gap
    // between their ink.
    let (small_size, text_size, gap) = {
        let style = cx.style();
        (style.small, style.text, f32::from(style.gap))
    };
    let width = f32::from(area.width);
    let height = f32::from(area.height);
    let note_max = text_size.min(height / 5.0).max(small_size);
    let chosen = cx.settings.size()?;
    // The notes below whole on their line, or not at all; the label whole
    // before the notes, shortened only where it has no room whole.
    let with_notes = !notes.is_empty() && cx.font.measure_text(small_size, notes)? <= width;
    // The label whole at the largest size that has room (in fewer lines where
    // two counts give it the same), then shortened on one.
    let mut best: Option<(f32, usize)> = None;
    for most in 1..=3 {
        if let Some((size, lines)) = wrap_whole(
            &cx.font,
            (note_max, small_size),
            &shown.note,
            (width, height),
            most,
        )? {
            match best {
                Some((s, _)) if s >= size => {}
                _ => best = Some((size, lines.len())),
            }
        }
    }
    let best_lines = best.map(|(_, lines)| lines);
    let mut ways: Vec<(bool, Option<usize>)> = Vec::new();
    ways.try_reserve(11).map_err(|_| Error::NoMemory)?;
    for lines in [best_lines, Some(1), Some(2), Some(3), None] {
        for extra in [true, false] {
            if (!extra || with_notes) && (lines.is_some() || best_lines.is_none()) {
                ways.push((extra, lines));
            }
        }
    }
    ways.push((false, None));
    let mut laid: Option<Laid> = None;
    for (extra, lines) in ways {
        let (note_size, note_lines) = match lines {
            Some(most) => match wrap_whole(
                &cx.font,
                (note_max, small_size),
                &shown.note,
                (width, height),
                most,
            )? {
                Some(found) => found,
                None => continue,
            },
            // What the number counts, where that fits whole; else the note
            // shortened.
            None if !shown.short.is_empty()
                && cx.font.measure_text(small_size, &shown.short)? <= width =>
            {
                let mut one = Vec::new();
                one.try_reserve(1).map_err(|_| Error::NoMemory)?;
                one.push(copy(&shown.short)?);
                (small_size, one)
            }
            None => {
                let mut one = Vec::new();
                one.try_reserve(1).map_err(|_| Error::NoMemory)?;
                one.push(paperpaw_ui_kit::text::fit(
                    &cx.font,
                    small_size,
                    &shown.note,
                    width,
                )?);
                (small_size, one)
            }
        };
        let mut below = ink_of(&cx.font, &note_lines, note_size, look.ink)? + gap;
        if extra {
            below += ink_of(
                &cx.font,
                core::slice::from_ref(&copy(notes)?),
                small_size,
                look.faint,
            )? + gap;
        }
        // The number's ink: its digits, as high as capitals.
        let fits = |s: f32| -> Result<bool, Error> {
            Ok(cx.font.measure_text(s, &shown.big)? <= width
                && cap_height(&cx.font, s)? + below <= height)
        };
        let size = match chosen {
            TextSize::Fit => largest_size(height, text_size, fits)?,
            other => {
                let size = other.text(cx.style()) * 1.6;
                largest_size(size, text_size.min(size), fits)?
            }
        };
        if let Some(size) = size {
            laid = Some((size, note_size, note_lines, extra));
            break;
        }
    }
    // Not even with the label shortened: the number alone, shortened at the
    // small size where it is a word ("Tomorrow"), nothing where not even that
    // has room.
    let (mut size, note_size, note_lines, extra) = match laid {
        Some(found) => found,
        None => {
            let alone = |s: f32| -> Result<bool, Error> {
                Ok(cx.font.measure_text(s, &shown.big)? <= width
                    && cap_height(&cx.font, s)? <= height)
            };
            let size = largest_size(height, small_size, alone)?.unwrap_or(small_size);
            (size, small_size, Vec::new(), false)
        }
    };
    // The digits' ink measured where capitals were assumed: smaller where it
    // reaches further.
    let small = line(&cx.font, small_size)?;
    while size > small_size {
        let number = [copy(&shown.big)?];
        let mut parts = alloc::vec![Part {
            lines: number.iter().map(String::as_str).collect(),
            size,
            color: look.ink,
        }];
        if !note_lines.is_empty() {
            parts.push(Part {
                lines: note_lines.iter().map(String::as_str).collect(),
                size: note_size,
                color: look.ink,
            });
        }
        let total = stack_height(&cx.font, &parts, gap)? + if extra { small + gap } else { 0.0 };
        if total <= height {
            break;
        }
        size *= 0.92;
    }
    let number = paperpaw_ui_kit::text::fit(&cx.font, size, &shown.big, width)?;
    if cx.font.measure_text(size, &number)? > width || cap_height(&cx.font, size)? > height {
        return Ok(());
    }
    // Centred together by their ink, a gap between the parts.
    let mut parts = alloc::vec![Part {
        lines: alloc::vec![number.as_str()],
        size,
        color: look.ink,
    }];
    if !note_lines.is_empty() {
        parts.push(Part {
            lines: note_lines.iter().map(String::as_str).collect(),
            size: note_size,
            color: look.ink,
        });
    }
    if extra {
        parts.push(Part {
            lines: alloc::vec![notes],
            size: small_size,
            color: look.faint,
        });
    }
    layout::stack(out, &cx.font, area, align, &parts, gap)
}

/// The ink height of lines drawn at `size` stacked alone.
fn ink_of(
    font: &paperpaw_sdk::native::fonts::Font<'_>,
    lines: &[String],
    size: f32,
    color: paperpaw_sdk::Color,
) -> Result<f32, Error> {
    let part = Part {
        lines: lines.iter().map(String::as_str).collect(),
        size,
        color,
    };
    stack_height(font, core::slice::from_ref(&part), 0.0)
}

/// The size of the sentence: as large as fits, or as set.
fn text_size(cx: &Cx<'_>, text: &str, room: Rect) -> Result<f32, Error> {
    let style = cx.style();
    match cx.settings.size()? {
        TextSize::Fit => largest(
            &cx.font,
            text,
            (room.width, room.height),
            (style.small, f32::from(room.height).min(style.title * 2.4)),
        ),
        size => Ok(size.text(style)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_read() {
        assert_eq!(parse("2026-12-24"), Some(Target::Once(2026, 12, 24)));
        assert_eq!(parse(" 12-24 "), Some(Target::Yearly(12, 24)));
        assert_eq!(parse("--02-29"), Some(Target::Yearly(2, 29)));
        assert_eq!(parse("24.12.2026"), Some(Target::Once(2026, 12, 24)));
        assert_eq!(parse("24.12."), Some(Target::Yearly(12, 24)));
        assert_eq!(parse("2026-02-29"), None);
        assert_eq!(parse("2028-02-29"), Some(Target::Once(2028, 2, 29)));
        assert_eq!(parse("13-01"), None);
        // Not cut to a day that fits in a byte (280 is 24 there).
        assert_eq!(parse("12-280"), None);
        assert_eq!(parse("2026-12-280"), None);
        assert_eq!(parse("280.12."), None);
        assert_eq!(parse("Christmas"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn days_are_counted() {
        let today = (2026, 10, 14);
        assert_eq!(
            days_until(today, Target::Yearly(12, 24)),
            (71, (2026, 12, 24))
        );
        assert_eq!(days_until(today, Target::Yearly(10, 14)).0, 0);
        assert_eq!(days_until(today, Target::Yearly(10, 15)).0, 1);
        // Passed this year: next year's.
        assert_eq!(
            days_until(today, Target::Yearly(10, 13)),
            (364, (2027, 10, 13))
        );
        // Once, passed: counted since.
        assert_eq!(days_until(today, Target::Once(2026, 10, 11)).0, -3);
        assert_eq!(days_until(today, Target::Once(2027, 1, 1)).0, 79);
        // 29 February each year: the 28th where there is none.
        assert_eq!(days_until(today, Target::Yearly(2, 29)).1, (2027, 2, 28));
        assert_eq!(
            days_until((2027, 3, 1), Target::Yearly(2, 29)).1,
            (2028, 2, 29)
        );
    }
}
