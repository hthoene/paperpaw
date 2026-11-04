//! The calendar: this month as a grid of weeks, the month's name above
//! it, the weekdays' initials, today marked; the weeks' numbers and the
//! next month as well, if wished.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::time;
use paperpaw_sdk::widgets::{self, Align, Drawn, Frame, Padding};
use paperpaw_sdk::{Canvas, Color, Env, FontHandle, Id, Point, Rect, RegisterError, Registrar};

use paperpaw_sdk::SettingSpec;

use super::dates::MonthGrid;
use super::{chosen_name, frame_height, ui};

const ID: Id = Id::new_static("widgets.calendar");

/// The values of the week start setting: as set for the device, Monday,
/// Sunday.
pub(super) const WEEK_STARTS: [&str; 3] = [
    "widgets.week-start.auto",
    "widgets.week-start.monday",
    "widgets.week-start.sunday",
];
/// How today is marked: a dark circle, a dark square, a ring, a line
/// below.
const MARKS: [&str; 4] = [
    "widgets.mark.circle",
    "widgets.mark.square",
    "widgets.mark.ring",
    "widgets.mark.underline",
];
/// The weekdays' names above the columns: one letter, two.
const INITIALS: [&str; 2] = ["widgets.initials.one", "widgets.initials.two"];

/// Its settings, kept in the program.
static SETTINGS: [SettingSpec; 15] = [
    widgets::align_spec("widgets.calendar.align", ID, Align::Center),
    widgets::frame_spec("widgets.calendar.frame", ID, Frame::None),
    widgets::padding_spec("widgets.calendar.padding", ID, Padding::Auto),
    widgets::paper_spec("widgets.calendar.paper", ID),
    widgets::ink_spec("widgets.calendar.ink", ID),
    SettingSpec::choice(
        "widgets.calendar.week-start",
        "widgets.setting.week-start",
        &WEEK_STARTS,
        WEEK_STARTS[0],
        ID,
    )
    .description("widgets.calendar.week-start.help"),
    SettingSpec::choice(
        "widgets.calendar.mark",
        "widgets.setting.today-mark",
        &MARKS,
        MARKS[0],
        ID,
    )
    .description("widgets.calendar.mark.help"),
    SettingSpec::choice(
        "widgets.calendar.initials",
        "widgets.setting.initials",
        &INITIALS,
        INITIALS[0],
        ID,
    )
    .description("widgets.calendar.initials.help"),
    widgets::part_spec(
        "widgets.calendar.heading",
        ID,
        "widgets.setting.month-name",
        true,
    ),
    widgets::part_spec("widgets.calendar.year", ID, "widgets.setting.year", false),
    widgets::part_spec(
        "widgets.calendar.weekdays",
        ID,
        "widgets.setting.weekdays",
        true,
    ),
    widgets::part_spec(
        "widgets.calendar.weeks",
        ID,
        "widgets.setting.week-numbers",
        false,
    ),
    widgets::part_spec(
        "widgets.calendar.others",
        ID,
        "widgets.setting.other-days",
        false,
    ),
    widgets::part_spec(
        "widgets.calendar.weekends",
        ID,
        "widgets.setting.weekends-faint",
        false,
    ),
    widgets::part_spec(
        "widgets.calendar.next",
        ID,
        "widgets.setting.next-month",
        false,
    ),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    super::add(
        r,
        {
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(ID, "widgets.calendar");
            &SPEC
        },
        Drawn::new(height, draw).shows(super::today_shows),
    )?;
    r.add_settings(&SETTINGS)
}

/// The first day of the week, 0 (Sunday) or 1 (Monday), as `widget`'s
/// setting `week-start` says, or as set for the device.
pub(super) fn week_start(env: &Env<'_>, widget: &str) -> u8 {
    match chosen_name(env, widget, "week-start") {
        "sunday" => 0,
        "monday" => 1,
        _ => paperpaw_sdk::time::week_start(env),
    }
}

fn height(env: &Env<'_>, width: u16) -> u16 {
    let (style, _) = ui(env);
    let months = if widgets::shows(env, ID.as_str(), "next") {
        2
    } else {
        1
    };
    (width * 6 / 7).min((u32::from(style.row()) * 6) as u16) * months
        + frame_height(env, ID.as_str())
}

fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
    let (style, font) = ui(env);
    let Some(today) = time::local_time(env) else {
        super::draw_no_time(canvas, env, &look);
        return;
    };
    let Some(font) = font else {
        return;
    };
    let area = look.area;
    let month = MonthGrid::new(today.year, today.month, week_start(env, ID.as_str()));
    // Two months: side by side where the place is wide, else one below
    // the other; this one alone where two have no room for a whole
    // month each.
    let gap = style.gap() * 2;
    let wide = area.width > area.height * 5 / 4;
    let two = widgets::shows(env, ID.as_str(), "next").then(|| {
        if wide {
            let w = super::share(area.width, gap, 2);
            (
                Rect::new(area.x, area.y, w, area.height),
                Rect::new(area.x + w + gap, area.y, w, area.height),
            )
        } else {
            let h = super::share(area.height, gap, 2);
            (
                Rect::new(area.x, area.y, area.width, h),
                Rect::new(area.x, area.y + h + gap, area.width, h),
            )
        }
    });
    let weeks = month.weeks.max(month.next().weeks);
    match two {
        Some((first, second)) if grid(env, &font, first, weeks, true).is_some() => {
            draw_month(
                canvas,
                env,
                &font,
                &look,
                first,
                &month,
                Some(today.day),
                weeks,
            );
            draw_month(
                canvas,
                env,
                &font,
                &look,
                second,
                &month.next(),
                None,
                weeks,
            );
        }
        _ => draw_month(
            canvas,
            env,
            &font,
            &look,
            area,
            &month,
            Some(today.day),
            month.weeks,
        ),
    }
}

/// How a month's grid is laid out in a place: which parts show, the
/// cells' size and the numbers' size (one the font draws in a cell).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Grid {
    heading: bool,
    initials: bool,
    weeks: bool,
    cell_w: f32,
    cell_h: f32,
    size: f32,
}

/// The grid of `rows` weeks in `area`, its parts as wished; where the
/// numbers have no room, the week numbers, the heading, the initials left
/// out in turn (the initials only with `all`). `None` where the numbers
/// do not fit even so: the font draws none that small.
fn grid(env: &Env<'_>, font: &FontHandle<'_>, area: Rect, rows: u8, all: bool) -> Option<Grid> {
    let (style, _) = ui(env);
    let widget = ID;
    let id = widget.as_str();
    let mut grid = Grid {
        heading: widgets::shows(env, id, "heading"),
        initials: widgets::shows(env, id, "weekdays"),
        weeks: widgets::shows(env, id, "weeks"),
        cell_w: 0.0,
        cell_h: 0.0,
        size: 0.0,
    };
    loop {
        let columns = 7 + u16::from(grid.weeks);
        let lines = u16::from(rows) + u16::from(grid.initials);
        // The heading as high as a row would be, a little more.
        let lines = f32::from(lines) + if grid.heading { 1.3 } else { 0.0 };
        grid.cell_h = f32::from(area.height) / lines;
        grid.cell_w = f32::from(area.width) / f32::from(columns);
        // Two digits with room round them in each cell, as drawn, and no
        // smaller than four fifths of the small size (else a week is
        // easier to read than a month).
        let (w, h) = (grid.cell_w, grid.cell_h);
        let most = (h * 0.5).min(w * 0.42);
        let size = widgets::largest_size(most, style.small() * 0.8, |size| {
            font.measure(size, "00") <= w * 0.8 && super::cap_height(font, size) <= h * 0.72
        });
        // The heading's line in its row.
        let heading_fits = size.is_some_and(|size| {
            let heading = (size * 1.15).min(style.title() * 1.6);
            super::line(font, heading) * 0.8 <= h * 1.3
        });
        match size {
            Some(size) if !grid.heading || heading_fits => {
                grid.size = size;
                return Some(grid);
            }
            _ if grid.weeks => grid.weeks = false,
            _ if grid.heading => grid.heading = false,
            _ if grid.initials && all => grid.initials = false,
            _ => return None,
        }
    }
}

/// Draws `month` into `area`, `today` marked, with room for
/// `weeks_shown` weeks (so that two months shown side by side line up).
/// Where the whole month has no room, the week of today alone; where that
/// has none either, today's number large under the month's name.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
fn draw_month(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    font: &FontHandle<'_>,
    look: &widgets::WidgetLook,
    area: Rect,
    month: &MonthGrid,
    today: Option<u8>,
    weeks_shown: u8,
) {
    let label = format!(
        "{} {}",
        super::month_name(env, month.month),
        env.i18n().number(i64::from(month.year))
    );
    canvas.describe(paperpaw_sdk::semantics::Role::Text, &label, area);
    let whole = grid(env, font, area, weeks_shown, true);
    let (grid, rows) = match (whole, today) {
        (Some(grid), _) => (grid, 0..month.weeks),
        (None, Some(day)) => match grid(env, font, area, 1, false) {
            Some(grid) => {
                let week = month.week_of(day);
                (grid, week..week + 1)
            }
            None => {
                draw_today(canvas, env, font, look, area, month, day);
                return;
            }
        },
        (None, None) => return,
    };
    let (style, _) = ui(env);
    let widget = ID;
    let id = widget.as_str();
    let Grid {
        heading,
        initials,
        weeks,
        cell_w,
        cell_h,
        size,
    } = grid;
    let small = size * 0.82;
    // The size the font draws at (the nearest it has).
    let drawn = super::cap_height(font, size) / 0.74;
    // The cells fill the place from top to bottom.
    let mut top = f32::from(area.y);
    let left = f32::from(area.x) + if weeks { cell_w } else { 0.0 };

    if heading {
        let name = super::month_name(env, month.month);
        let text = if widgets::shows(env, id, "year") {
            format!("{name} {}", env.i18n().number(i64::from(month.year)))
        } else {
            String::from(name)
        };
        let heading_size = (size * 1.15).min(style.title() * 1.6);
        // In line with the numbers' ink below, not the cells' edges.
        let inset = ((cell_w - font.measure(size, "00")) / 2.0).max(0.0);
        let row = Rect::new(
            (left + inset) as u16,
            top as u16,
            (f32::from(area.width) - (left - f32::from(area.x)) - 2.0 * inset).max(0.0) as u16,
            (cell_h * 1.3 * 0.8) as u16,
        );
        let align = widgets::align(env, id);
        super::label(canvas, font, heading_size, align, row, &text, look.ink);
        top += cell_h * 1.3;
    }
    if initials {
        let two = chosen_name(env, id, "initials") == "two";
        for column in 0..7u8 {
            let name = super::weekday_name(env, month.weekday_of(column));
            let initial: String = name.chars().take(if two { 2 } else { 1 }).collect();
            let x = left + cell_w * (f32::from(column) + 0.5);
            let y = top + cell_h / 2.0;
            super::draw_centred(canvas, font, small, x, y, &initial, look.faint);
        }
        top += cell_h;
    }
    let others = widgets::shows(env, id, "others");
    let weekends = widgets::shows(env, id, "weekends");
    let mark = chosen_name(env, id, "mark");
    let first = super::dates::days_from_civil(month.year, month.month, 1);
    for (row, week) in rows.enumerate() {
        let y = top + cell_h * (row as f32 + 0.5);
        if weeks {
            let number = env.i18n().number(i64::from(month.week_number(week)));
            let x = f32::from(area.x) + cell_w / 2.0;
            super::draw_centred(canvas, font, small, x, y, &number, look.faint);
        }
        for column in 0..7u8 {
            let day = month.day_at(week, column);
            let x = left + cell_w * (f32::from(column) + 0.5);
            let inside = (0..i32::from(month.days)).contains(&day);
            if !inside && !others {
                continue;
            }
            let date = super::dates::civil_from_days(first + i64::from(day));
            let text = env.i18n().number(i64::from(date.2));
            let mut color = if inside { look.ink } else { look.faint };
            if weekends && matches!(month.weekday_of(column), 0 | 6) {
                color = look.faint;
            }
            if inside && today == Some((day + 1) as u8) {
                let wide = font.measure(size, &text);
                let cell = (cell_w, cell_h);
                color = draw_mark(canvas, look, mark, x, y, cell, drawn, wide);
            }
            let shown = if inside { size } else { small };
            super::draw_centred(canvas, font, shown, x, y, &text, color);
        }
    }
}

/// Today alone, where not even a week has room: the month's name (or
/// its short name) small above the day's number as large as fits, the
/// two in the middle; the name left out where both have no room.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
fn draw_today(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    font: &FontHandle<'_>,
    look: &widgets::WidgetLook,
    area: Rect,
    month: &MonthGrid,
    day: u8,
) {
    let (style, _) = ui(env);
    let width = f32::from(area.width);
    let long = super::month_name(env, month.month);
    let short = super::month_short(env, month.month);
    let name = [long, short]
        .into_iter()
        .find(|name| font.measure(style.small(), name) <= width);
    let name_h = super::line(font, style.small());
    let number = env.i18n().number(i64::from(day));
    let room = |name: bool| f32::from(area.height) - if name { name_h } else { 0.0 };
    let fits = |size: f32, name: bool| {
        font.measure(size, &number) <= width && super::line(font, size) <= room(name)
    };
    let most = f32::from(area.height).min(style.title() * 3.0);
    // With the name where the number keeps the body size at least.
    let with_name = name.and_then(|_| widgets::largest_size(most, style.text(), |s| fits(s, true)));
    let (size, name) = match with_name {
        Some(size) => (size, name),
        None => match widgets::largest_size(most, style.small(), |s| fits(s, false)) {
            Some(size) => (size, None),
            // Not even the number has room: nothing rather than a part.
            None => return,
        },
    };
    // The two centred together by their ink, half a gap apart.
    let mut parts = Vec::new();
    if let Some(name) = name {
        parts.push(super::Part {
            lines: alloc::vec![name],
            size: style.small(),
            color: look.faint,
            font: None,
        });
    }
    parts.push(super::Part {
        lines: alloc::vec![number.as_str()],
        size,
        color: look.ink,
        font: None,
    });
    let align = widgets::align(env, ID.as_str());
    let gap = style.gap() as f32 / 2.0;
    super::draw_stack(canvas, font, area, align, &parts, gap);
}

/// Draws today's mark round the number at `x`, `y` and returns the
/// number's colour on it.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
fn draw_mark(
    canvas: &mut Canvas<'_>,
    look: &widgets::WidgetLook,
    mark: &str,
    x: f32,
    y: f32,
    (cell_w, cell_h): (f32, f32),
    size: f32,
    wide: f32,
) -> Color {
    // Round the number, but never into the next row or column.
    let radius = (cell_w.min(cell_h) * 0.46)
        .min(size * 1.15)
        .max(size * 0.78)
        .min(cell_w.min(cell_h) * 0.5);
    let center = Point::new(x as i32, y as i32);
    // The number's ink with a little room round it.
    let pad = (size * 0.12).max(1.0);
    let cap = size * 0.74;
    let cramped = wide / 2.0 + pad > radius || cap / 2.0 + pad > radius;
    match mark {
        "square" => {
            let r = radius as u16;
            let at = Rect::new((x - radius) as u16, (y - radius) as u16, r * 2, r * 2);
            canvas.fill_round(at, (radius * 0.3) as u32, look.ink);
            look.paper
        }
        "ring" => {
            let thick = (radius / 7.0).max(1.5) as u32;
            canvas.stroke_circle(center, radius as u32, thick, look.ink);
            look.ink
        }
        "underline" => {
            let thick = (size / 9.0).max(2.0) as u16;
            let w = (size * 1.1) as u16;
            let at = Rect::new(
                (x - f32::from(w) / 2.0) as u16,
                (y + size * 0.48) as u16,
                w,
                thick,
            );
            canvas.fill_round(at, u32::from(thick / 2), look.ink);
            look.ink
        }
        // Where a disc would touch the number, a box round it: as wide
        // as the number and a little room, never into the next column.
        _ if cramped => {
            let w = (wide + 2.0 * pad).min(cell_w - 2.0);
            let h = (cap + 2.0 * pad).min(cell_h - 2.0);
            let at = Rect::new(
                (x - w / 2.0) as u16,
                (y - h / 2.0) as u16,
                w as u16,
                h as u16,
            );
            canvas.fill_round(at, (h * 0.2) as u32, look.ink);
            look.paper
        }
        _ => {
            canvas.fill_circle(center, radius as u32, look.ink);
            look.paper
        }
    }
}
