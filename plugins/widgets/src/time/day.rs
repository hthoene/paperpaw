//! The day and the date large, a calm alternative to the home screen's
//! date: the weekday large with the date below it, the day's number large
//! beside the weekday and the month, or a sheet of a tear-off calendar.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::time;
use paperpaw_sdk::widgets::{self, Align, Drawn, Frame, Padding, TextSize};
use paperpaw_sdk::{Arg, Canvas, Env, Id, Rect, RegisterError, Registrar};

use paperpaw_sdk::SettingSpec;

use super::{chosen_name, frame_height, line, ui};

const ID: Id = Id::new_static("widgets.day");

/// The values of the style setting: the weekday large, the number large,
/// a calendar sheet.
const STYLES: [&str; 3] = [
    "widgets.day.style.weekday",
    "widgets.day.style.number",
    "widgets.day.style.sheet",
];

/// Its settings, kept in the program.
static SETTINGS: [SettingSpec; 13] = [
    widgets::align_spec("widgets.day.align", ID, Align::Left),
    widgets::frame_spec("widgets.day.frame", ID, Frame::None),
    widgets::padding_spec("widgets.day.padding", ID, Padding::Auto),
    widgets::paper_spec("widgets.day.paper", ID),
    widgets::ink_spec("widgets.day.ink", ID),
    widgets::corners_spec("widgets.day.corners", ID),
    widgets::frame_line_spec("widgets.day.frame-line", ID),
    widgets::shadow_spec("widgets.day.drop-shadow", ID),
    SettingSpec::choice(
        "widgets.day.style",
        "widgets.setting.style",
        &STYLES,
        STYLES[0],
        ID,
    )
    .description("widgets.day.style.help"),
    widgets::size_spec("widgets.day.size", ID, TextSize::Fit, true),
    super::date_format_spec("widgets.day.date-format", ID, super::DATE_FORMATS[0]),
    widgets::part_spec("widgets.day.year", ID, "widgets.setting.year", true),
    widgets::part_spec("widgets.day.week", ID, "widgets.setting.week-number", false),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    super::add(
        r,
        {
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(ID, "widgets.day");
            &SPEC
        },
        Drawn::new(height, draw).shows(super::today_shows),
    )?;
    r.add_settings(&SETTINGS)
}

fn height(env: &Env<'_>, _: u16) -> u16 {
    let (style, _) = ui(env);
    let size = big_size(env, None);
    widgets::line_height(env, size)
        + widgets::line_height(env, style.text()) * 2
        + frame_height(env, ID.as_str())
}

/// The size of what is large: as set, or as fits `room` (height).
fn big_size(env: &Env<'_>, room: Option<f32>) -> f32 {
    let (style, _) = ui(env);
    match (widgets::size(env, ID.as_str()), room) {
        (TextSize::Fit, Some(room)) => room,
        (TextSize::Fit, None) => style.title() * 2.0,
        (size, _) => size.text(&style) * 1.6,
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
    let (style, font) = ui(env);
    let Some(t) = time::local_time(env) else {
        super::draw_no_time(canvas, env, &look);
        return;
    };
    let Some(font) = font else {
        return;
    };
    let area = look.area;
    let align = widgets::align(env, ID.as_str());
    let with_year = widgets::shows(env, ID.as_str(), "year");
    let weekday = super::weekday_name(env, t.weekday);
    let month = super::month_name(env, t.month);
    let week = widgets::shows(env, ID.as_str(), "week").then(|| {
        let week = super::dates::iso_week(t.year, t.month, t.day);
        env.i18n()
            .format("widgets.year.week", &[("week", Arg::Int(i64::from(week)))])
    });
    let width = f32::from(area.width);
    match chosen_name(env, ID.as_str(), "style") {
        "number" => {
            // "14" large; beside it the weekday, and the month below it.
            let number = env.i18n().number(i64::from(t.day));
            let month_line = if with_year {
                format!("{month} {}", env.i18n().number(i64::from(t.year)))
            } else {
                String::from(month)
            };
            let mut notes: Vec<&str> = alloc::vec![weekday, &month_line];
            if let Some(week) = &week {
                notes.push(week);
            }
            let room = f32::from(area.height);
            let size = super::fit_size(
                &font,
                big_size(env, Some(room * 0.95)),
                &number,
                width * 0.45,
            );
            let number_w = font.measure(size, &number);
            let gap = style.gap() as f32 * 1.5;
            // The notes as large as fits beside it, the weekday larger.
            let rest = (width - number_w - gap).max(0.0);
            let note = notes
                .iter()
                .map(|n| super::fit_size(&font, style.title() * 1.2, n, rest))
                .fold(style.title() * 1.2, f32::min)
                .min(room / (notes.len() as f32 + 0.6))
                .max(style.small());
            let lines: f32 = line(&font, note * 1.2) + line(&font, note) * (notes.len() - 1) as f32;
            let total_w = number_w
                + gap
                + notes
                    .iter()
                    .enumerate()
                    .map(|(n, text)| font.measure(if n == 0 { note * 1.2 } else { note }, text))
                    .fold(0.0, f32::max);
            let left = align.x(area, total_w.min(width));
            let big = line(&font, size).min(room);
            let row = Rect::new(
                left as u16,
                area.y + ((room - big) / 2.0) as u16,
                number_w as u16 + 2,
                big as u16,
            );
            super::label(canvas, &font, size, Align::Left, row, &number, look.ink);
            let x = left + number_w + gap;
            let mut y = f32::from(area.y) + (room - lines).max(0.0) / 2.0;
            for (n, text) in notes.iter().enumerate() {
                let (s, color) = if n == 0 {
                    (note * 1.2, look.ink)
                } else {
                    (note, look.faint)
                };
                let h = line(&font, s);
                let row = Rect::new(
                    x as u16,
                    y as u16,
                    (f32::from(area.x) + width - x).max(0.0) as u16,
                    h as u16,
                );
                super::label(canvas, &font, s, Align::Left, row, text, color);
                y += h;
            }
        }
        "sheet" => draw_sheet(canvas, env, &font, &look, &t, with_year, week.as_deref()),
        _ => {
            // "Wednesday" large, the date below it: each whole on its
            // line, the weekday's short name where the name has no room,
            // the date without its year, then with the month's short
            // name, then left out.
            let mut dates = alloc::vec![super::date_text(
                env,
                ID.as_str(),
                t.year,
                t.month,
                t.day,
                with_year
            )];
            if with_year {
                dates.push(super::date_text(
                    env,
                    ID.as_str(),
                    t.year,
                    t.month,
                    t.day,
                    false,
                ));
            }
            dates.push(env.i18n().format(
                "widgets.date.short",
                &[
                    ("day", Arg::Int(i64::from(t.day))),
                    ("month", Arg::Str(super::month_short(env, t.month))),
                ],
            ));
            let dates = dates.into_iter().map(|date| match &week {
                Some(week) => format!("{date} · {week}"),
                None => date,
            });
            let most = style
                .text()
                .min(f32::from(area.height) / 3.5)
                .max(style.small());
            let fits = |size: f32, text: &str| font.measure(size, text) <= width;
            let date = dates.into_iter().find_map(|date| {
                widgets::largest_size(most, style.small(), |s| fits(s, &date)).map(|s| (s, date))
            });
            let height = f32::from(area.height);
            // The weekday as large as set or as fits, a line at least
            // above the date; without the date where both have no room.
            let weekday_fits = |text: &str, note_h: f32, least: f32| {
                let max = big_size(env, Some((height - note_h) * 0.8)).max(least);
                widgets::largest_size(max, least, |s| {
                    fits(s, text) && line(&font, s) <= height - note_h
                })
            };
            let short = super::weekday_short(env, t.weekday);
            // The full name at the body size at least, as it heads the date;
            // else the short name.
            let pick = |note_h: f32| {
                weekday_fits(weekday, note_h, style.text())
                    .map(|s| (s, weekday))
                    .or_else(|| weekday_fits(short, note_h, style.small()).map(|s| (s, short)))
            };
            let with_date = date.and_then(|(note, date)| {
                let note_h = line(&font, note);
                pick(note_h).map(|big| (big, Some((note, note_h, date))))
            });
            let ((size, name), date) = match with_date.or_else(|| pick(0.0).map(|big| (big, None)))
            {
                Some(found) => found,
                None => ((style.small(), short), None),
            };
            // The date a step smaller than the weekday, which heads it.
            let date = date.map(|(note, h, date)| {
                let most = (size * 0.84).max(style.small());
                if note <= most {
                    return (note, h, date);
                }
                let note = widgets::largest_size(most, style.small(), |s| fits(s, &date))
                    .unwrap_or(style.small());
                (note, line(&font, note), date)
            });
            // The two centred together by their ink, half a gap apart.
            let mut parts = alloc::vec![super::Part {
                lines: alloc::vec![name],
                size,
                color: look.ink,
                font: None,
            }];
            if let Some((note, _, date)) = &date {
                parts.push(super::Part {
                    lines: alloc::vec![date.as_str()],
                    size: *note,
                    color: look.faint,
                    font: None,
                });
            }
            let gap = style.gap() as f32 / 2.0;
            super::draw_stack(canvas, &font, area, align, &parts, gap);
        }
    }
}

/// A sheet of a tear-off calendar: the month on a dark band at the top,
/// the day's number large, the weekday below it.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
fn draw_sheet(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    font: &paperpaw_sdk::FontHandle<'_>,
    look: &widgets::WidgetLook,
    t: &paperpaw_sdk::time::LocalTime,
    with_year: bool,
    week: Option<&str>,
) {
    let (style, _) = ui(env);
    let area = look.area;
    // As wide as about four fifths of its height, at most.
    let height = f32::from(area.height);
    let width = f32::from(area.width)
        .min(height * 0.95)
        .max(height * 0.6)
        .min(f32::from(area.width));
    let align = widgets::align(env, ID.as_str());
    let left = align.x(area, width);
    let sheet = Rect::new(left as u16, area.y, width as u16, area.height);
    let radius = u32::from(style.corner()).max((width * 0.06) as u32);
    // The month (with the year where set) on the band where it fits, else
    // in short, else shortened: never wider than the sheet.
    let room = width * 0.85;
    let band_max = (height * 0.11).max(style.small());
    let full = super::month_name(env, t.month);
    let short = super::month_short(env, t.month);
    let mut names = Vec::new();
    if with_year {
        names.push(format!("{full} {}", env.i18n().number(i64::from(t.year))));
    }
    names.push(String::from(full));
    names.push(String::from(short));
    let fits = |text: &String| {
        widgets::largest_size(band_max, style.small(), |s| font.measure(s, text) <= room).is_some()
    };
    let month = names
        .iter()
        .find(|text| fits(text))
        .cloned()
        .unwrap_or_else(|| paperpaw_sdk::ui::fit(font, style.small(), short, room));
    let band_size = super::fit_size(font, band_max, &month, room);
    let band_h = (line(font, band_size) * 1.25).min(height * 0.3);
    // The band in ink, the sheet's paper inside a line round it.
    let thick = u32::from(style.line()) * 2;
    canvas.fill_round(sheet, radius, look.ink);
    let inner = Rect::new(
        sheet.x + thick as u16,
        sheet.y + band_h as u16,
        sheet.width.saturating_sub(2 * thick as u16),
        sheet.height.saturating_sub(band_h as u16 + thick as u16),
    );
    canvas.fill_round(inner, radius.saturating_sub(thick), look.paper);
    // Square where it meets the band.
    canvas.fill(
        Rect::new(
            inner.x,
            inner.y,
            inner.width,
            (radius as u16).min(inner.height),
        ),
        look.paper,
    );
    let cx = left + width / 2.0;
    super::draw_centred(
        canvas,
        font,
        band_size,
        cx,
        f32::from(sheet.y) + band_h / 2.0,
        &month,
        look.paper,
    );
    let weekday = super::weekday_name(env, t.weekday);
    let mut notes: Vec<&str> = alloc::vec![weekday];
    if let Some(week) = week {
        notes.push(week);
    }
    let note = super::fit_size(
        font,
        (height * 0.1).clamp(style.small(), style.title()),
        weekday,
        width * 0.85,
    );
    let notes_h = line(font, note) * notes.len() as f32;
    let room = f32::from(inner.height) - notes_h;
    let number = env.i18n().number(i64::from(t.day));
    let size = super::fit_size(font, room * 0.72, &number, width * 0.8);
    let digits = super::cap_height(font, size);
    let gap = (room - digits) / 2.0 * 0.6;
    let total = digits + gap + notes_h;
    let top = f32::from(inner.y) + (f32::from(inner.height) - total) / 2.0;
    super::draw_centred(
        canvas,
        font,
        size,
        cx,
        top + digits / 2.0,
        &number,
        look.ink,
    );
    let mut y = top + digits + gap;
    for (n, text) in notes.iter().enumerate() {
        let h = line(font, note);
        let row = Rect::new(inner.x, y as u16, inner.width, h as u16);
        let color = if n == 0 { look.ink } else { look.faint };
        super::label(canvas, font, note, Align::Center, row, text, color);
        y += h;
    }
}
