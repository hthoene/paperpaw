//! The year so far: how much of it has passed, as a bar, a ring or a dot
//! for each month, with the percent, the day of the year, the week and
//! the days left, as wished.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::time::{self, LocalTime};
use paperpaw_sdk::widgets::{self, Align, Drawn, Frame, Padding};
use paperpaw_sdk::{
    Arg, Canvas, Color, Env, FontHandle, Id, Point, Rect, RegisterError, Registrar,
};

use paperpaw_sdk::SettingSpec;

use super::{chosen_name, dates, frame_height, line, ui};

const ID: Id = Id::new_static("widgets.year-progress");

/// The values of the style setting: a bar, a ring, a dot for each month.
const STYLES: [&str; 3] = [
    "widgets.year.bar",
    "widgets.year.ring",
    "widgets.year.months",
];

/// Its settings, kept in the program.
static SETTINGS: [SettingSpec; 11] = [
    widgets::align_spec("widgets.year-progress.align", ID, Align::Left),
    widgets::frame_spec("widgets.year-progress.frame", ID, Frame::None),
    widgets::padding_spec("widgets.year-progress.padding", ID, Padding::Auto),
    widgets::paper_spec("widgets.year-progress.paper", ID),
    widgets::ink_spec("widgets.year-progress.ink", ID),
    SettingSpec::choice(
        "widgets.year-progress.style",
        "widgets.setting.style",
        &STYLES,
        STYLES[0],
        ID,
    )
    .description("widgets.year-progress.style.help"),
    widgets::part_spec(
        "widgets.year-progress.year",
        ID,
        "widgets.setting.year",
        true,
    ),
    widgets::part_spec(
        "widgets.year-progress.percent",
        ID,
        "widgets.setting.percent",
        true,
    ),
    widgets::part_spec(
        "widgets.year-progress.day",
        ID,
        "widgets.setting.day-of-year",
        true,
    ),
    widgets::part_spec(
        "widgets.year-progress.week",
        ID,
        "widgets.setting.week-number",
        false,
    ),
    widgets::part_spec(
        "widgets.year-progress.left",
        ID,
        "widgets.setting.days-left",
        false,
    ),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    super::add(
        r,
        {
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(ID, "widgets.year-progress");
            &SPEC
        },
        Drawn::new(height, draw).shows(shows),
    )?;
    r.add_settings(&SETTINGS)
}

/// How far the year is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Progress {
    /// Today, 1 to 366.
    pub day: u16,
    /// The year's days.
    pub days: u16,
    /// Of the year passed, in tenths of a percent (the time of day
    /// counted).
    pub permille: u16,
    pub week: u8,
    /// Days after today.
    pub left: u16,
}

pub(super) fn progress(t: &LocalTime) -> Progress {
    let day = dates::day_of_year(t.year, t.month, t.day);
    let days = dates::days_in_year(t.year);
    let minutes = (u32::from(day) - 1) * 1440 + u32::from(t.hour) * 60 + u32::from(t.minute);
    Progress {
        day,
        days,
        permille: (minutes * 1000 / (u32::from(days) * 1440)) as u16,
        week: dates::iso_week(t.year, t.month, t.day),
        left: days - day,
    }
}

fn shows(env: &Env<'_>) -> String {
    time::local_time(env).map_or_else(String::new, |t| {
        let p = progress(&t);
        format!("{}/{}", p.day, p.permille / 10)
    })
}

/// The notes below the bar, as the settings want them: "Day 287 of
/// 365", "Week 42", "78 days left"; with `short`, "Day 287".
fn notes(env: &Env<'_>, p: &Progress, short: bool) -> Vec<String> {
    let mut notes = Vec::new();
    if widgets::shows(env, ID.as_str(), "day") {
        notes.push(env.i18n().format(
            if short {
                "widgets.year.day-short"
            } else {
                "widgets.year.day"
            },
            &[
                ("day", Arg::Int(i64::from(p.day))),
                ("days", Arg::Int(i64::from(p.days))),
            ],
        ));
    }
    if widgets::shows(env, ID.as_str(), "week") {
        notes.push(env.i18n().format(
            "widgets.year.week",
            &[("week", Arg::Int(i64::from(p.week)))],
        ));
    }
    if widgets::shows(env, ID.as_str(), "left") {
        notes.push(
            env.i18n()
                .plural("widgets.year.left", i64::from(p.left), &[]),
        );
    }
    notes
}

pub(super) fn percent_text(env: &Env<'_>, p: &Progress) -> String {
    env.i18n()
        .localize_digits(&format!("{} %", p.permille / 10))
        .into_owned()
}

fn height(env: &Env<'_>, _: u16) -> u16 {
    let (style, _) = ui(env);
    widgets::line_height(env, style.title() * 1.6)
        + widgets::line_height(env, style.small()) * 2
        + style.gap() * 2
        + frame_height(env, ID.as_str())
}

/// The track's colour: a light gray on light paper, a dark one on dark.
fn track(look: &widgets::WidgetLook) -> Color {
    if look.paper.luma() < 128 {
        Color::gray(80)
    } else {
        Color::gray(205)
    }
}

fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
    let (_, font) = ui(env);
    let Some(now) = time::local_time(env) else {
        super::draw_no_time(canvas, env, &look);
        return;
    };
    let Some(font) = font else {
        return;
    };
    let p = progress(&now);
    let year =
        widgets::shows(env, ID.as_str(), "year").then(|| env.i18n().number(i64::from(now.year)));
    match chosen_name(env, ID.as_str(), "style") {
        "ring" => {
            let mut area = look.area;
            if let Some(year) = &year {
                area = widgets::draw_caption(canvas, env, ID.as_str(), &look, year);
            }
            draw_ring(canvas, env, &font, &look, area, &p);
        }
        "months" => draw_bar(canvas, env, &font, &look, year.as_deref(), &p, Some(&now)),
        _ => draw_bar(canvas, env, &font, &look, year.as_deref(), &p, None),
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

/// How notes may be laid out: whole in up to two lines, whole in one,
/// in their short forms on one ("Day 287"), shortened at the end of one,
/// or not at all.
#[derive(Clone, Copy, PartialEq)]
enum Notes {
    Two,
    One,
    Short,
    Cut,
    None,
}

/// The year as `caption` above, the percent large, a bar (or a dot for
/// each month of `months`) and the notes below, half a gap between their
/// ink; where they have no room together, a second line of notes, the
/// percent, the caption and the notes are left out in turn (the notes
/// shortened before they go), never drawn over each other.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn draw_bar(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    font: &FontHandle<'_>,
    look: &widgets::WidgetLook,
    caption: Option<&str>,
    p: &Progress,
    months: Option<&LocalTime>,
) {
    let (style, _) = ui(env);
    let align = widgets::align(env, ID.as_str());
    let gap = style.gap() as f32 / 2.0;
    let area = look.area;
    let width = f32::from(area.width);
    let height = f32::from(area.height);
    let items = notes(env, p, false);
    let short = notes(env, p, true);
    let text = percent_text(env, p);
    let percent = widgets::shows(env, ID.as_str(), "percent");
    let ink = |lines: Vec<&str>, size: f32| {
        let part = super::Part {
            lines,
            size,
            color: look.ink,
            font: None,
        };
        super::stack_height(font, &[part], 0.0)
    };
    let caption_h = caption.map_or(0.0, |c| ink(alloc::vec![c], style.small()));
    let bar_h = |height: f32| {
        if months.is_some() {
            (width / 12.0 * 0.7).min(height * 0.3)
        } else {
            // Four lines thick at least, but no thicker than one and a
            // half gaps (where thick lines want more, the gaps win).
            (height * 0.1)
                .max(style.line() as f32 * 4.0)
                .min(style.gap() as f32 * 1.5)
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
    let lay = |(with_caption, kind, with_percent): (bool, Notes, bool)| -> Option<BarParts> {
        if with_caption && caption.is_none() || with_percent && !percent {
            return None;
        }
        if kind != Notes::None && items.is_empty() {
            return None;
        }
        let notes = match kind {
            Notes::Two => super::pack(font, style.small(), &items, width, 2),
            Notes::One | Notes::Cut => super::pack(font, style.small(), &items, width, 1),
            Notes::Short => super::pack(font, style.small(), &short, width, 1),
            Notes::None => Vec::new(),
        };
        // Whole notes only, but where they may be shortened.
        if kind != Notes::Cut && notes.iter().any(|n| n.ends_with('…')) {
            return None;
        }
        let note_h = if notes.is_empty() {
            0.0
        } else {
            ink(notes.iter().map(String::as_str).collect(), style.small()) + gap
        };
        let top_h = if with_caption { caption_h + gap } else { 0.0 };
        let bar = bar_h(height);
        let room = height - top_h - note_h - bar;
        if room < 0.0 {
            return None;
        }
        let size = if with_percent {
            let most = (room - gap).min(style.title() * 2.2);
            let size = widgets::largest_size(most, style.text(), |s| {
                font.measure(s, &text) <= width && super::cap_height(font, s) + gap <= room
            })?;
            Some(size)
        } else {
            None
        };
        Some(BarParts {
            caption: with_caption,
            percent: size,
            notes,
            bar,
        })
    };
    let Some(parts) = ways.into_iter().find_map(lay) else {
        return;
    };
    let big = parts
        .percent
        .map_or(0.0, |size| ink(alloc::vec![text.as_str()], size) + gap);
    let note_h = if parts.notes.is_empty() {
        0.0
    } else {
        ink(
            parts.notes.iter().map(String::as_str).collect(),
            style.small(),
        ) + gap
    };
    let top_h = if parts.caption { caption_h + gap } else { 0.0 };
    let bar_h = parts.bar;
    // All of it together in the middle by its ink.
    let total = top_h + big + bar_h + note_h;
    let mut y = f32::from(area.y) + (height - total).max(0.0) / 2.0;
    // Draws one part's ink from `y`, `h` high.
    let draw = |canvas: &mut Canvas<'_>, y: f32, h: f32, part: super::Part<'_>| {
        let band = Rect::new(area.x, y as u16, area.width, (h + 0.99) as u16);
        super::draw_stack(canvas, font, band, align, &[part], 0.0);
    };
    if let (true, Some(caption)) = (parts.caption, caption) {
        let part = super::Part {
            lines: alloc::vec![caption],
            size: style.small(),
            color: look.faint,
            font: None,
        };
        draw(canvas, y, caption_h, part);
        y += top_h;
    }
    if let Some(size) = parts.percent {
        let part = super::Part {
            lines: alloc::vec![text.as_str()],
            size,
            color: look.ink,
            font: None,
        };
        draw(canvas, y, big - gap, part);
        y += big;
    }
    match months {
        Some(t) => draw_months(canvas, look, area, y, bar_h, t),
        None => {
            let bar = Rect::new(area.x, y as u16, area.width, bar_h as u16);
            let radius = bar_h as u32 / 2;
            canvas.fill_round_shaded(bar, radius, track(look));
            let done = (u32::from(area.width) * u32::from(p.permille) / 1000) as u16;
            if done > 0 {
                let filled = Rect::new(bar.x, bar.y, done.max(bar.height), bar.height);
                canvas.fill_round(filled, radius, look.ink);
            }
        }
    }
    y += bar_h + gap;
    if !parts.notes.is_empty() {
        let part = super::Part {
            lines: parts.notes.iter().map(String::as_str).collect(),
            size: style.small(),
            color: look.faint,
            font: None,
        };
        draw(canvas, y, note_h - gap, part);
    }
}

/// A dot for each month: those passed filled, this one as far as it
/// went, those to come as rings.
fn draw_months(
    canvas: &mut Canvas<'_>,
    look: &widgets::WidgetLook,
    area: Rect,
    y: f32,
    height: f32,
    t: &LocalTime,
) {
    let step = f32::from(area.width) / 12.0;
    let radius = (height / 2.0).min(step * 0.42).max(2.0) as u32;
    let thick = (radius / 6).max(1);
    let days = u32::from(dates::days_in_month(t.year, t.month));
    for month in 1..=12u8 {
        let x = f32::from(area.x) + step * (f32::from(month) - 0.5);
        let center = Point::new(x as i32, (y + height / 2.0) as i32);
        if month < t.month {
            canvas.fill_circle(center, radius, look.ink);
        } else if month == t.month {
            canvas.fill_circle(center, radius, track(look));
            let sweep = (u32::from(t.day) - 1) * 360 / days + 360 / days / 2;
            canvas.fill_ring(center, radius, radius, 0, sweep.max(8), look.ink);
            canvas.stroke_circle(center, radius, thick, look.ink);
        } else {
            canvas.stroke_circle(center, radius, thick, look.ink);
        }
    }
}

/// A ring as far round as the year went, the percent in it, the notes
/// beside it where there is room.
fn draw_ring(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    font: &FontHandle<'_>,
    look: &widgets::WidgetLook,
    area: Rect,
    p: &Progress,
) {
    let (style, _) = ui(env);
    let align = widgets::align(env, ID.as_str());
    let notes = notes(env, p, false);
    let small = line(font, style.small());
    let side = area.height.min(area.width);
    // Beside it where the notes have room there, else below it.
    let widest = notes
        .iter()
        .map(|n| font.measure(style.text(), n))
        .fold(0.0, f32::max);
    let rest = f32::from(area.width) - f32::from(side) - style.gap() as f32 * 2.0;
    let beside = !notes.is_empty() && widest <= rest;
    // Not beside: below it, the notes in a line.
    let below = if beside || notes.is_empty() {
        0
    } else {
        (small + style.gap() as f32 / 2.0) as u16
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
        return;
    }
    let top = area.y + area.height.saturating_sub(side + below) / 2;
    let center = Point::new(
        i32::from(ring_x) + radius as i32,
        i32::from(top) + radius as i32,
    );
    canvas.fill_ring(center, radius, thick, 0, 360, track(look));
    let sweep = u32::from(p.permille) * 360 / 1000;
    canvas.fill_ring(center, radius, thick, 0, sweep, look.ink);
    if widgets::shows(env, ID.as_str(), "percent") {
        let text = percent_text(env, p);
        let inner = (radius - thick) as f32 * 2.0 * 0.78;
        let size = super::fit_size(font, inner * 0.42, &text, inner);
        super::draw_centred(
            canvas,
            font,
            size,
            center.x as f32,
            center.y as f32,
            &text,
            look.ink,
        );
    }
    if !beside {
        if below > 0 {
            let row = Rect::new(
                area.x,
                top + side + style.gap() / 2,
                area.width,
                small as u16,
            );
            let text = notes.join(" · ");
            super::label(canvas, font, style.small(), align, row, &text, look.faint);
        }
        return;
    }
    let gap = style.gap() * 2;
    let text_x = if align == Align::Right {
        area.x
    } else {
        area.x + side + gap
    };
    let room = Rect::new(text_x, area.y, area.width - side - gap, area.height);
    let small = line(font, style.text());
    let total = small * notes.len() as f32;
    let mut y = f32::from(room.y) + (f32::from(room.height) - total).max(0.0) / 2.0;
    let text_align = if align == Align::Right {
        Align::Right
    } else {
        Align::Left
    };
    for note in &notes {
        if y + small > f32::from(room.y) + f32::from(room.height) + 1.0 {
            break;
        }
        let row = Rect::new(room.x, y as u16, room.width, small as u16);
        super::label(canvas, font, style.text(), text_align, row, note, look.ink);
        y += small;
    }
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
}
