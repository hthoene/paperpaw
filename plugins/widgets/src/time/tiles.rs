//! The clock in tiles, as a flip clock looks: each digit (or each pair)
//! on a rounded tile of its own, dark with light digits, light, or
//! outlined; twelve or twenty-four hours as the clock is set, the date
//! below if wished.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::time::{self, LocalTime};
use paperpaw_sdk::widgets::{self, Align, Drawn, Frame, Padding};
use paperpaw_sdk::{Canvas, Color, Env, Id, Point, Rect, RegisterError, Registrar};

use paperpaw_sdk::SettingSpec;

use super::{chosen_name, frame_height, line, ui};

const ID: Id = Id::new_static("widgets.tile-clock");

/// The values of the tiles' look: dark, light, outlined.
const LOOKS: [&str; 3] = [
    "widgets.tiles.dark",
    "widgets.tiles.light",
    "widgets.tiles.outline",
];
/// What goes on a tile: a digit, or the hours and the minutes.
const GROUPS: [&str; 2] = ["widgets.tiles.digits", "widgets.tiles.pairs"];

/// Its settings, kept in the program.
static SETTINGS: [SettingSpec; 14] = [
    widgets::align_spec("widgets.tile-clock.align", ID, Align::Center),
    widgets::frame_spec("widgets.tile-clock.frame", ID, Frame::None),
    widgets::padding_spec("widgets.tile-clock.padding", ID, Padding::Auto),
    widgets::paper_spec("widgets.tile-clock.paper", ID),
    widgets::ink_spec("widgets.tile-clock.ink", ID),
    widgets::corners_spec("widgets.tile-clock.corners", ID),
    widgets::frame_line_spec("widgets.tile-clock.frame-line", ID),
    widgets::shadow_spec("widgets.tile-clock.drop-shadow", ID),
    SettingSpec::choice(
        "widgets.tile-clock.tiles",
        "widgets.setting.tiles",
        &LOOKS,
        LOOKS[0],
        ID,
    )
    .description("widgets.tile-clock.tiles.help"),
    SettingSpec::choice(
        "widgets.tile-clock.group",
        "widgets.setting.tile-group",
        &GROUPS,
        GROUPS[0],
        ID,
    )
    .description("widgets.tile-clock.group.help"),
    widgets::part_spec(
        "widgets.tile-clock.hinge",
        ID,
        "widgets.setting.hinge",
        true,
    ),
    widgets::part_spec(
        "widgets.tile-clock.colon",
        ID,
        "widgets.setting.colon",
        false,
    ),
    widgets::part_spec(
        "widgets.tile-clock.date",
        ID,
        "widgets.setting.show-date",
        false,
    ),
    super::date_format_spec("widgets.tile-clock.date-format", ID, super::DATE_FORMATS[1]),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    super::add(
        r,
        {
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(ID, "widgets.tile-clock");
            &SPEC
        },
        Drawn::new(height, draw).shows(super::minute_shows),
    )?;
    r.add_settings(&SETTINGS)
}

fn height(env: &Env<'_>, width: u16) -> u16 {
    let (style, _) = ui(env);
    let tiles = (u32::from(width) * 2 / 5).min(u32::from(style.row()) * 3) as u16;
    let date = if widgets::shows(env, ID.as_str(), "date") {
        widgets::line_height(env, style.small()) + style.gap()
    } else {
        0
    };
    tiles + date + frame_height(env, ID.as_str())
}

/// The tiles at `t`: the hours' and the minutes' (each a digit, or both
/// digits on one), and "AM" or "PM" on a twelve-hour clock.
pub(super) fn tiles(t: &LocalTime, twelve: bool, pairs: bool) -> (Vec<String>, Vec<String>) {
    let hours = if twelve {
        format!("{}", super::twelve(t.hour))
    } else {
        format!("{:02}", t.hour)
    };
    let minutes = format!("{:02}", t.minute);
    let split = |s: &str| -> Vec<String> {
        if pairs {
            alloc::vec![String::from(s)]
        } else {
            s.chars().map(String::from).collect()
        }
    };
    (split(&hours), split(&minutes))
}

#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
    let (style, font) = ui(env);
    let Some(now) = time::local_time(env) else {
        super::draw_no_time(canvas, env, &look);
        return;
    };
    let Some(font) = font else {
        return;
    };
    let area = look.area;
    let twelve = paperpaw_sdk::time::twelve_hours(env);
    let pairs = chosen_name(env, ID.as_str(), "group") == "pairs";
    let (hours, minutes) = tiles(&now, twelve, pairs);
    let local = |parts: Vec<String>| -> Vec<String> {
        parts
            .iter()
            .map(|p| env.i18n().localize_digits(p).into_owned())
            .collect()
    };
    let (hours, minutes) = (local(hours), local(minutes));
    let colon = widgets::shows(env, ID.as_str(), "colon");
    let small = line(&font, style.small());
    let mut date = widgets::shows(env, ID.as_str(), "date")
        .then(|| super::date_text(env, ID.as_str(), now.year, now.month, now.day, false));
    let meridiem = twelve.then(|| {
        let key = if now.hour < 12 {
            "core.clock.am"
        } else {
            "core.clock.pm"
        };
        String::from(
            env.i18n()
                .format(key, &[("time", paperpaw_sdk::Arg::Str(""))])
                .trim(),
        )
    });
    // Each tile `w` wide (pairs wider), `w * 1.4` high; tiles of a group
    // apart by a small gap, the groups by a larger one.
    let tile_w = |text: &str| if text.chars().count() > 1 { 1.55 } else { 1.0 };
    let units: f32 = hours.iter().chain(&minutes).map(|t| tile_w(t)).sum();
    let gap = 0.08;
    let group_gap = if colon { 0.42 } else { 0.24 };
    let gaps = gap * (hours.len() + minutes.len() - 2) as f32 + group_gap;
    let tile = |below: f32| {
        let room_h = (f32::from(area.height) - below).max(0.0);
        let w = (f32::from(area.width) / (units + gaps)).min(room_h / 1.4);
        // Fonts of fixed sizes have a largest: the tiles no larger than
        // its digits fill well.
        let digits = super::cap_height(&font, w * 1.4 * 0.66);
        w.min(digits / 0.48 / 1.4)
    };
    // The line below (the date, "AM" or "PM") where it fits whole under
    // the tiles and leaves them two thirds of the height; the date is
    // left out first.
    let line_below = small + style.gap() as f32 / 2.0;
    let meridiem_width = |m: &Option<String>| {
        m.as_ref()
            .map_or(0.0, |m| font.measure(style.small(), m) + style.gap() as f32)
    };
    let room_below = |date: &Option<String>, meridiem: &Option<String>| {
        let total = tile(line_below) * (units + gaps);
        let date_width = date
            .as_ref()
            .map_or(0.0, |d| font.measure(style.small(), d));
        line_below * 3.0 <= f32::from(area.height)
            && date_width + meridiem_width(meridiem) <= total + 0.5
    };
    let mut meridiem = meridiem;
    if date.is_some() && !room_below(&date, &meridiem) {
        date = None;
    }
    if meridiem.is_some() && !room_below(&date, &meridiem) {
        meridiem = None;
    }
    let below = if date.is_some() || meridiem.is_some() {
        line_below
    } else {
        0.0
    };
    let w = tile(below);
    if w < 6.0 {
        return;
    }
    let h = w * 1.4;
    let total = w * (units + gaps);
    let align = widgets::align(env, ID.as_str());
    let left = align.x(area, total);
    let top = f32::from(area.y) + (f32::from(area.height) - h - below).max(0.0) / 2.0;
    let size = h * 0.66;
    let radius = (w * 0.14) as u32;
    let kind = chosen_name(env, ID.as_str(), "tiles");
    let hinge = widgets::shows(env, ID.as_str(), "hinge");
    let (light, dark) = super::light_and_dark(&look);
    let on_dark = look.paper.luma() < 128;

    let mut x = left;
    for (group, texts) in [&hours, &minutes].into_iter().enumerate() {
        if group == 1 {
            let space = w * group_gap;
            if colon {
                let dot = (w * 0.07).max(1.5) as u32;
                let cx = (x + space / 2.0) as i32;
                for dy in [-0.17, 0.17] {
                    let cy = (top + h / 2.0 + h * dy) as i32;
                    canvas.fill_circle(Point::new(cx, cy), dot, look.ink);
                }
            }
            x += space;
        }
        for (n, text) in texts.iter().enumerate() {
            if n > 0 {
                x += w * gap;
            }
            let width = w * tile_w(text);
            let tile = Rect::new(x as u16, top as u16, width as u16, h as u16);
            let digits = match kind {
                "light" => {
                    let gray = if on_dark { 70 } else { 215 };
                    canvas.fill_round_shaded(tile, radius, Color::gray(gray));
                    look.ink
                }
                "outline" => {
                    let thick = ((w / 30.0) as u32).max(u32::from(style.line()) * 2);
                    canvas.stroke_round(tile, thick, radius, look.ink);
                    look.ink
                }
                _ => {
                    canvas.fill_round(tile, radius, look.ink);
                    look.paper
                }
            };
            let digit_size =
                widgets::largest_size(size, 1.0, |s| font.measure(s, text) <= width * 0.86)
                    .unwrap_or(1.0);
            let cx = x + width / 2.0;
            let cy = top + h / 2.0;
            super::draw_centred(canvas, &font, digit_size, cx, cy, text, digits);
            if hinge {
                // The line where a flip clock's leaves meet.
                let thick = ((h / 70.0) as u16).max(1);
                let line = Rect::new(
                    tile.x,
                    (cy as u16).saturating_sub(thick / 2),
                    tile.width,
                    thick,
                );
                let color = match kind {
                    "outline" => look.ink,
                    "light" if on_dark => dark,
                    "light" => light,
                    _ => look.paper,
                };
                canvas.fill(line, color);
            }
            x += width;
        }
    }
    canvas.describe(
        paperpaw_sdk::semantics::Role::Text,
        &super::clock_text(env, &now),
        Rect::new(left as u16, top as u16, total as u16, h as u16),
    );

    // The date below, and "AM" or "PM" at the right end.
    let row = Rect::new(
        left as u16,
        (top + h + style.gap() as f32 / 2.0) as u16,
        total as u16,
        small as u16,
    );
    let meridiem_width = meridiem_width(&meridiem);
    if let Some(m) = &meridiem {
        super::label(
            canvas,
            &font,
            style.small(),
            Align::Right,
            row,
            m,
            look.faint,
        );
    }
    if let Some(date) = &date {
        let room = Rect::new(
            row.x,
            row.y,
            (f32::from(row.width) - meridiem_width).max(0.0) as u16,
            row.height,
        );
        let place = if meridiem.is_some() {
            Align::Left
        } else {
            Align::Center
        };
        super::label(canvas, &font, style.small(), place, room, date, look.faint);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tiles_stay_in_their_place() {
        assert_eq!(super::super::word_clock::drawn_outside(ID.as_str()), None);
    }

    fn at(hour: u8, minute: u8) -> LocalTime {
        LocalTime::new(2026, 10, 14, hour, minute, 0)
    }

    #[test]
    fn digits_on_tiles() {
        let (h, m) = tiles(&at(9, 5), false, false);
        assert_eq!((h.join("|"), m.join("|")), ("0|9".into(), "0|5".into()));
        // Twelve hours: no leading zero, one tile fewer.
        let (h, m) = tiles(&at(21, 41), true, false);
        assert_eq!((h.join("|"), m.join("|")), ("9".into(), "4|1".into()));
        let (h, _) = tiles(&at(0, 0), true, false);
        assert_eq!(h.join("|"), "1|2");
        // In pairs.
        let (h, m) = tiles(&at(10, 41), false, true);
        assert_eq!((h.join("|"), m.join("|")), ("10".into(), "41".into()));
    }
}
