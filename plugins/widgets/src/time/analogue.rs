//! The analogue clock: a round face with an hour and a minute hand,
//! ticks or numerals as the reader likes, a ring or two round it, and
//! the date below it or on the face.

use paperpaw_sdk::round::point_at;
use paperpaw_sdk::time::{self, LocalTime};
use paperpaw_sdk::widgets::{self, Align, Drawn, Frame, Padding};
use paperpaw_sdk::{Canvas, Color, Env, Id, Point, Rect, RegisterError, Registrar};

use paperpaw_sdk::SettingSpec;

use super::{chosen_name, frame_height, line, ui};

const ID: Id = Id::new_static("widgets.analogue-clock");

/// The values of the face setting: hands alone, ticks at the hours, ticks
/// at every minute, numerals at the quarters, all twelve numerals.
const FACES: [&str; 5] = [
    "widgets.face.minimal",
    "widgets.face.hours",
    "widgets.face.minutes",
    "widgets.face.quarters",
    "widgets.face.numerals",
];
/// The values of the ring setting: none, one, two.
const RINGS: [&str; 3] = ["widgets.ring.none", "widgets.ring.one", "widgets.ring.two"];
/// The values of the hands setting: slender, regular, bold.
const HANDS: [&str; 3] = [
    "widgets.hands.thin",
    "widgets.hands.regular",
    "widgets.hands.bold",
];
/// Where the date goes: nowhere, below the face, on it.
const DATES: [&str; 3] = [
    "widgets.clock-date.none",
    "widgets.clock-date.below",
    "widgets.clock-date.inside",
];

/// Its settings, kept in the program.
static SETTINGS: [SettingSpec; 14] = [
    widgets::align_spec("widgets.analogue-clock.align", ID, Align::Center),
    widgets::frame_spec("widgets.analogue-clock.frame", ID, Frame::None),
    widgets::padding_spec("widgets.analogue-clock.padding", ID, Padding::Auto),
    widgets::paper_spec("widgets.analogue-clock.paper", ID),
    widgets::ink_spec("widgets.analogue-clock.ink", ID),
    widgets::corners_spec("widgets.analogue-clock.corners", ID),
    widgets::frame_line_spec("widgets.analogue-clock.frame-line", ID),
    widgets::shadow_spec("widgets.analogue-clock.drop-shadow", ID),
    SettingSpec::choice(
        "widgets.analogue-clock.face",
        "widgets.setting.face",
        &FACES,
        FACES[1],
        ID,
    )
    .description("widgets.analogue-clock.face.help"),
    SettingSpec::choice(
        "widgets.analogue-clock.ring",
        "widgets.setting.ring",
        &RINGS,
        RINGS[1],
        ID,
    )
    .description("widgets.analogue-clock.ring.help"),
    SettingSpec::choice(
        "widgets.analogue-clock.hands",
        "widgets.setting.hands",
        &HANDS,
        HANDS[1],
        ID,
    )
    .description("widgets.analogue-clock.hands.help"),
    SettingSpec::choice(
        "widgets.analogue-clock.date",
        "widgets.setting.clock-date",
        &DATES,
        DATES[0],
        ID,
    )
    .description("widgets.analogue-clock.date.help"),
    super::date_format_spec(
        "widgets.analogue-clock.date-format",
        ID,
        super::DATE_FORMATS[1],
    ),
    widgets::part_spec(
        "widgets.analogue-clock.centre",
        ID,
        "widgets.setting.centre-dot",
        true,
    ),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    super::add(
        r,
        {
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(ID, "widgets.analogue-clock");
            &SPEC
        },
        Drawn::new(height, draw).shows(super::minute_shows),
    )?;
    r.add_settings(&SETTINGS)
}

fn height(env: &Env<'_>, width: u16) -> u16 {
    let style = paperpaw_sdk::ui::UiStyle::new(env);
    let face = width.min((u32::from(style.row()) * 4) as u16);
    face + frame_height(env, ID.as_str())
}

/// The angles of the hands, in degrees clockwise from twelve: the hour
/// hand moves on with the minutes.
pub(super) fn angles(hour: u8, minute: u8) -> (i32, i32) {
    let hour = i32::from(hour % 12) * 30 + i32::from(minute) / 2;
    (hour, i32::from(minute) * 6)
}

#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
    let area = look.area;
    let (style, font) = ui(env);
    let now = time::local_time(env);
    if now.is_none() {
        super::draw_no_time(canvas, env, &look);
        return;
    }
    let date = chosen_name(env, ID.as_str(), "date");
    let date_text = now.map(|t| super::date_text(env, ID.as_str(), t.year, t.month, t.day, false));
    // The date below takes a line, where it fits whole and leaves the
    // face two thirds of the height at least; the face the rest, round.
    let below = match (&font, date, &date_text) {
        (Some(font), "below", Some(text))
            if font.measure(style.small(), text) <= f32::from(area.width) =>
        {
            let below = line(font, style.small()) as u16 + style.gap() / 2;
            if below * 3 <= area.height { below } else { 0 }
        }
        _ => 0,
    };
    let room = area.height.saturating_sub(below).min(area.width);
    let radius = u32::from(room / 2);
    if radius < 8 {
        return;
    }
    let align = widgets::align(env, ID.as_str());
    let left = align.x(area, f32::from(room)) as i32;
    let top = i32::from(area.y) + i32::from(area.height.saturating_sub(room + below) / 2);
    let center = Point::new(left + radius as i32, top + radius as i32);
    let r = radius as i32;
    let ink = look.ink;

    // Rings round the face.
    let ring = (radius / 40).max(1) + u32::from(u32::from(style.line()) > 1);
    match chosen_name(env, ID.as_str(), "ring") {
        "one" => canvas.stroke_circle(center, radius, ring, ink),
        "two" => {
            canvas.stroke_circle(center, radius, ring, ink);
            canvas.stroke_circle(center, radius - ring * 3, (ring / 2).max(1), ink);
        }
        _ => {}
    }
    // The ticks start inside the rings.
    let edge = match chosen_name(env, ID.as_str(), "ring") {
        "one" => r - ring as i32 * 3,
        "two" => r - ring as i32 * 6,
        _ => r - (r / 30).max(1),
    };
    let face = chosen_name(env, ID.as_str(), "face");
    let tick = (radius / 28).max(2);
    match face {
        "hours" | "minutes" => {
            for n in 0..60 {
                let hour = n % 5 == 0;
                if !hour && face != "minutes" {
                    continue;
                }
                let (length, width) = match (hour, n % 15 == 0) {
                    (true, true) => (r / 7, tick + tick / 2),
                    (true, false) => (r / 9, tick),
                    _ => (r / 22, (tick / 2).max(1)),
                };
                let outer = point_at(center, edge - width as i32 / 2, n * 6);
                let inner = point_at(center, edge - length, n * 6);
                draw_capsule(canvas, inner, outer, width, ink);
            }
        }
        "quarters" | "numerals" => {
            if let Some(font) = &font {
                let size = (r as f32 * if face == "quarters" { 0.3 } else { 0.24 }).max(6.0);
                let at = edge - (size * 0.62) as i32;
                for hour in 1..=12u8 {
                    let degrees = i32::from(hour) * 30;
                    if face == "quarters" && hour % 3 != 0 {
                        // Dots between the numerals.
                        let dot = point_at(center, at, degrees);
                        canvas.fill_circle(dot, (tick * 2 / 3).max(1), ink);
                        continue;
                    }
                    let p = point_at(center, at, degrees);
                    let text = env.i18n().number(i64::from(hour));
                    let (x, y) = (p.x as f32, p.y as f32);
                    super::draw_centred(canvas, font, size, x, y, &text, ink);
                }
            }
        }
        _ => {}
    }

    // The day on the face, in a window towards three as on a watch
    // (towards six where numerals are).
    if let (Some(font), "inside", Some(now)) = (&font, date, &now) {
        let text = env.i18n().number(i64::from(now.day));
        let numerals = matches!(face, "quarters" | "numerals");
        let size = (r as f32 * if numerals { 0.14 } else { 0.17 }).max(6.0);
        let (x, y) = if numerals {
            (center.x as f32, center.y as f32 + r as f32 * 0.38)
        } else {
            (center.x as f32 + r as f32 * 0.56, center.y as f32)
        };
        let w = font.measure(size, "00") + size * 0.5;
        let h = super::cap_height(font, size) + size * 0.55;
        let window = Rect::new(
            (x - w / 2.0) as u16,
            (y - h / 2.0) as u16,
            w as u16,
            h as u16,
        );
        let thick = (r as u32 / 60).max(1);
        canvas.stroke_round(window, thick, (h / 5.0) as u32, look.faint);
        super::draw_centred(canvas, font, size, x, y, &text, ink);
    }

    // The hands, the minute hand over the hour hand.
    if let Some(now) = now {
        draw_hands(canvas, env, center, r, &now, ink);
    }

    // The date below, placed as the face is (it fits whole: see above).
    if let (true, Some(text)) = (below > 0, &date_text) {
        let gap = style.gap() / 2;
        let row = Rect::new(area.x, (top + 2 * r) as u16 + gap, area.width, below - gap);
        let small = style.small();
        widgets::draw_line(canvas, env, row, align, small, small, text, look.faint);
    }
}

fn draw_hands(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    center: Point,
    r: i32,
    now: &LocalTime,
    ink: Color,
) {
    let (hour, minute) = angles(now.hour, now.minute);
    let scale = match chosen_name(env, ID.as_str(), "hands") {
        "thin" => 6,
        "bold" => 15,
        _ => 10,
    };
    let width = |share: i32| ((r * scale / share) as u32).max(2);
    let hour_width = width(130);
    let minute_width = width(190);
    let tail = r / 9;
    for (degrees, length, w) in [
        (hour, r * 52 / 100, hour_width),
        (minute, r * 78 / 100, minute_width),
    ] {
        let tip = point_at(center, length, degrees);
        let back = point_at(center, tail, degrees + 180);
        draw_capsule(canvas, back, tip, w, ink);
    }
    if widgets::shows(env, ID.as_str(), "centre") {
        canvas.fill_circle(center, hour_width * 3 / 4 + 1, ink);
    }
}

/// Draws a line from `a` to `b`, `width` pixels thick, with round ends
/// and smooth edges where the panel has grays.
pub(super) fn draw_capsule(canvas: &mut Canvas<'_>, a: Point, b: Point, width: u32, color: Color) {
    // In eighths of a pixel.
    let half = i64::from(width.max(1)) * 4;
    let (ax, ay) = (i64::from(a.x) * 8 + 4, i64::from(a.y) * 8 + 4);
    let (bx, by) = (i64::from(b.x) * 8 + 4, i64::from(b.y) * 8 + 4);
    let (dx, dy) = (bx - ax, by - ay);
    let length2 = dx * dx + dy * dy;
    // How far a point is from the line, squared.
    let distance2 = |px: i64, py: i64| -> i64 {
        let (qx, qy) = (px - ax, py - ay);
        let along = qx * dx + qy * dy;
        if length2 == 0 || along <= 0 {
            qx * qx + qy * qy
        } else if along >= length2 {
            let (rx, ry) = (px - bx, py - by);
            rx * rx + ry * ry
        } else {
            let across = qx * dy - qy * dx;
            across * across / length2
        }
    };
    let pad = width as i32 / 2 + 2;
    let left = a.x.min(b.x) - pad;
    let top = a.y.min(b.y) - pad;
    let w = ((a.x - b.x).unsigned_abs() + 2 * pad as u32) + 1;
    let h = ((a.y - b.y).unsigned_abs() + 2 * pad as u32) + 1;
    let inner = (half - 6).max(0);
    let outer = half + 6;
    canvas.draw_mask(left, top, w, h, color, |x, y| {
        let px = (i64::from(left) + i64::from(x)) * 8 + 4;
        let py = (i64::from(top) + i64::from(y)) * 8 + 4;
        let d = distance2(px, py);
        if d <= inner * inner {
            return 255;
        }
        if d > outer * outer {
            return 0;
        }
        let mut inside = 0u32;
        for sy in [-3, -1, 1, 3] {
            for sx in [-3, -1, 1, 3] {
                if distance2(px + sx, py + sy) <= half * half {
                    inside += 1;
                }
            }
        }
        (inside * 255 / 16) as u8
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_face_stays_in_its_place() {
        assert_eq!(super::super::word_clock::drawn_outside(ID.as_str()), None);
    }

    #[test]
    fn hands_point_where_a_clock_shows() {
        assert_eq!(angles(0, 0), (0, 0));
        assert_eq!(angles(3, 0), (90, 0));
        assert_eq!(angles(15, 30), (105, 180));
        assert_eq!(angles(10, 41), (320, 246));
        assert_eq!(angles(23, 59), (359, 354));
    }
}
