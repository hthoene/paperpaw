//! The moon's phase: today's moon drawn, its lit part bounded by the
//! terminator as it is seen, with the phase's name and when the moon is
//! full next.
//!
//! The new and full moons are reckoned as astronomers do (Meeus,
//! *Astronomical Algorithms*, chapter 49: the mean lunation and its
//! largest periodic terms), in whole numbers: within minutes of the true
//! times, enough for the day.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::round::{UNIT, sin_cos};
use paperpaw_sdk::time::{self, LocalTime};
use paperpaw_sdk::widgets::{self, Align, Drawn, Frame, Padding};
use paperpaw_sdk::{Arg, Canvas, Color, Env, Id, Point, Rect, RegisterError, Registrar};

use paperpaw_sdk::SettingSpec;

use super::{chosen_name, dates, frame_height, line, ui};

const ID: Id = Id::new_static("widgets.moon");

/// The values of the hemisphere setting: the moon as seen from the north
/// (waxing lit on the right), or from the south.
const HEMISPHERES: [&str; 2] = ["widgets.moon.north", "widgets.moon.south"];
/// The values of the shadow setting: dark, light.
const SHADOWS: [&str; 2] = ["widgets.moon.shadow.dark", "widgets.moon.shadow.light"];

/// A lunation, in milliseconds (29.530588861 days).
const LUNATION: i64 = 2_551_442_877;
/// The mean new moon of 6 January 2000 (lunation 0), in milliseconds
/// since 1970 (UTC).
const EPOCH: i64 = 947_168_373_824;

/// Its settings, kept in the program.
static SETTINGS: [SettingSpec; 14] = [
    widgets::align_spec("widgets.moon.align", ID, Align::Left),
    widgets::frame_spec("widgets.moon.frame", ID, Frame::None),
    widgets::padding_spec("widgets.moon.padding", ID, Padding::Auto),
    widgets::paper_spec("widgets.moon.paper", ID),
    widgets::ink_spec("widgets.moon.ink", ID),
    widgets::corners_spec("widgets.moon.corners", ID),
    widgets::frame_line_spec("widgets.moon.frame-line", ID),
    widgets::shadow_spec("widgets.moon.drop-shadow", ID),
    widgets::part_spec("widgets.moon.name", ID, "widgets.setting.phase-name", true),
    widgets::part_spec(
        "widgets.moon.full",
        ID,
        "widgets.setting.next-full-moon",
        true,
    ),
    widgets::part_spec("widgets.moon.lit", ID, "widgets.setting.lit-percent", false),
    SettingSpec::choice(
        "widgets.moon.shadow",
        "widgets.setting.shadow",
        &SHADOWS,
        SHADOWS[0],
        ID,
    )
    .description("widgets.moon.shadow.help"),
    SettingSpec::choice(
        "widgets.moon.hemisphere",
        "widgets.setting.hemisphere",
        &HEMISPHERES,
        HEMISPHERES[0],
        ID,
    )
    .description("widgets.moon.hemisphere.help"),
    super::date_format_spec("widgets.moon.date-format", ID, super::DATE_FORMATS[0]),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    super::add(
        r,
        {
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(ID, "widgets.moon");
            &SPEC
        },
        Drawn::new(height, draw).shows(super::today_shows),
    )?;
    r.add_settings(&SETTINGS)
}

/// When the moon is new (`half` even) or full (`half` odd) in half
/// lunation `half` since that of January 2000, in milliseconds since
/// 1970 (UTC).
pub(super) fn phase_time(half: i64) -> i64 {
    // The sun's and the moon's mean anomaly and the moon's argument of
    // latitude, in millionths of a degree.
    let angle = |at_zero: i64, per_lunation: i64| at_zero + per_lunation * half / 2;
    let m = angle(2_553_400, 29_105_357);
    let m1 = angle(201_564_300, 385_816_935);
    let f = angle(160_710_800, 390_670_503);
    let sin = |micro: i64| {
        let degrees = (micro.rem_euclid(360_000_000) + 500_000) / 1_000_000;
        i64::from(sin_cos(degrees as i32).0)
    };
    let full = half % 2 != 0;
    // The largest terms, in hundred-thousandths of a day.
    let terms: [(i64, i64); 10] = [
        (if full { -40_614 } else { -40_720 }, sin(m1)),
        (if full { 17_302 } else { 17_241 }, sin(m)),
        (if full { 1_614 } else { 1_608 }, sin(2 * m1)),
        (if full { 1_043 } else { 1_039 }, sin(2 * f)),
        (if full { 734 } else { 739 }, sin(m1 - m)),
        (if full { -515 } else { -514 }, sin(m1 + m)),
        (if full { 209 } else { 208 }, sin(2 * m)),
        (-111, sin(m1 - 2 * f)),
        (-57, sin(m1 + 2 * f)),
        (56, sin(2 * m1 + m)),
    ];
    let correction: i64 = terms.iter().map(|(c, s)| c * s).sum::<i64>() / i64::from(UNIT);
    EPOCH + LUNATION * half / 2 + correction * 864
}

/// The moon at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Moon {
    /// How far round its lunation it is, in degrees: 0 new, 180 full.
    pub degrees: i32,
    /// When it is full next (or was, today), in milliseconds since 1970.
    pub next_full: i64,
    /// Days since it was new.
    pub age: i64,
}

/// The moon at `unix_ms` milliseconds since 1970 (UTC).
pub(super) fn moon(unix_ms: i64) -> Moon {
    let mut k = (unix_ms - EPOCH).div_euclid(LUNATION);
    while phase_time(2 * k) > unix_ms {
        k -= 1;
    }
    while phase_time(2 * k + 2) <= unix_ms {
        k += 1;
    }
    let (new, next) = (phase_time(2 * k), phase_time(2 * k + 2));
    let degrees = ((unix_ms - new) * 360 / (next - new)) as i32;
    // The full moon of this lunation, or the next one once it passed
    // (but for the rest of its day).
    let full = phase_time(2 * k + 1);
    let next_full = if full + 86_400_000 > unix_ms {
        full
    } else {
        phase_time(2 * k + 3)
    };
    Moon {
        degrees,
        next_full,
        age: (unix_ms - new) / 86_400_000,
    }
}

/// The phase's name, one of eight: new moon, waxing crescent, first
/// quarter, waxing gibbous, full moon, waning gibbous, last quarter,
/// waning crescent; each principal phase for a day and a half round it.
pub(super) fn phase(degrees: i32) -> u8 {
    const PRINCIPAL: i32 = 18;
    let d = degrees.rem_euclid(360);
    let near = |at: i32| (d - at).abs() <= PRINCIPAL;
    if !(PRINCIPAL..360 - PRINCIPAL).contains(&d) {
        0
    } else if near(90) {
        2
    } else if near(180) {
        4
    } else if near(270) {
        6
    } else {
        // Between them: 1, 3, 5, 7.
        (d / 90) as u8 * 2 + 1
    }
}

/// How much of the moon is lit, in percent.
pub(super) fn lit(degrees: i32) -> u8 {
    let (_, cos) = sin_cos(degrees);
    ((UNIT - cos) * 100 / (2 * UNIT)) as u8
}

fn height(env: &Env<'_>, width: u16) -> u16 {
    let (style, _) = ui(env);
    (width / 2).min((u32::from(style.row()) * 3) as u16) + frame_height(env, ID.as_str())
}

/// The lines of text beside the moon.
fn lines(env: &Env<'_>, moon: &Moon, today: &LocalTime) -> Vec<String> {
    let mut lines = Vec::new();
    if widgets::shows(env, ID.as_str(), "name") {
        let key = format!("widgets.moon.phase.{}", phase(moon.degrees));
        lines.push(String::from(env.i18n().tr(&key)));
    }
    if widgets::shows(env, ID.as_str(), "full") {
        let local = time::local_seconds(env, moon.next_full.div_euclid(1000));
        let day = LocalTime::from_seconds(local);
        let days = dates::days_from_civil(day.year, day.month, day.day)
            - dates::days_from_civil(today.year, today.month, today.day);
        let text = match days {
            0 => String::from(env.i18n().tr("widgets.moon.full-today")),
            1 => String::from(env.i18n().tr("widgets.moon.full-tomorrow")),
            _ => {
                let date = super::date_text(env, ID.as_str(), day.year, day.month, day.day, false);
                env.i18n()
                    .format("widgets.moon.full-on", &[("date", Arg::Str(&date))])
            }
        };
        lines.push(text);
    }
    if widgets::shows(env, ID.as_str(), "lit") {
        lines.push(env.i18n().format(
            "widgets.moon.lit",
            &[("percent", Arg::Int(i64::from(lit(moon.degrees))))],
        ));
    }
    lines
}

#[expect(
    clippy::too_many_lines,
    reason = "longer than the limit from before it was set; split it when it is next changed"
)]
fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
    let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
    let (style, font) = ui(env);
    let (Some(unix), Some(today)) = (time::unix_seconds(env), time::local_time(env)) else {
        super::draw_no_time(canvas, env, &look);
        return;
    };
    let moon = moon(unix * 1000);
    let lines = lines(env, &moon, &today);
    let area = look.area;
    let align = widgets::align(env, ID.as_str());
    let first = style.text();
    let small = style.small();
    let gap = style.gap();
    // Beside the text where the place is wide, else above it.
    let wide = area.width >= area.height * 3 / 2;
    // The text's rows beside the moon, or below it in `share` of the
    // height; and how many of the lines they show.
    // Beside a moon `side` pixels across, or below it in `share` of the
    // height.
    let rows_in = |beside: bool, share: f32, side: u16| -> (Vec<(String, f32, Color)>, usize) {
        let Some(font) = &font else {
            return (Vec::new(), 0);
        };
        let (width, room, most) = if beside {
            (
                area.width.saturating_sub(side + gap),
                f32::from(area.height),
                2,
            )
        } else {
            (area.width, f32::from(area.height) * share, 3)
        };
        // Each line whole, broken between words: the first as large as
        // the body size down to the small one, the others small; those
        // that have no room left out from the last.
        let mut rows = Vec::new();
        let mut used = 0.0;
        let mut shown = 0;
        for (n, text) in lines.iter().enumerate() {
            let (max, color) = if n == 0 {
                (first, look.ink)
            } else {
                (small, look.faint)
            };
            // In as few lines as it fits.
            let fitted = (1..=most).find_map(|lines| {
                let place = (f32::from(width), room - used);
                super::wrap_whole(font, (max, small), text, place, lines)
            });
            let Some((size, broken)) = fitted else {
                break;
            };
            used += line(font, size) * broken.len() as f32;
            shown += 1;
            rows.extend(broken.into_iter().map(|l| (l, size, color)));
        }
        (rows, shown)
    };
    // Beside it where the place is wide, the moon a little smaller where
    // that gives a long word room.
    let mut beside = wide && !lines.is_empty();
    let mut side = area.height;
    let mut rows = Vec::new();
    if beside {
        for part in [10, 9, 8, 7] {
            side = area.height * part / 10;
            rows = rows_in(true, 1.0, side).0;
            if !rows.is_empty() {
                break;
            }
        }
    }
    if !beside || rows.is_empty() {
        beside = false;
        // Below it: the moon the larger, but not at the cost of a line
        // that has room where the moon is a little smaller.
        let mut best = rows_in(false, 0.45, 0);
        for share in [0.55, 0.65] {
            let more = rows_in(false, share, 0);
            if more.1 > best.1 {
                best = more;
            }
        }
        rows = best.0;
    }
    let heights: Vec<f32> = match &font {
        Some(font) => rows.iter().map(|(_, size, _)| line(font, *size)).collect(),
        None => Vec::new(),
    };
    let text_h: f32 = heights.iter().sum();
    let below_gap = if rows.is_empty() { 0 } else { gap / 2 };
    let side = if beside {
        side
    } else {
        area.height
            .saturating_sub(text_h as u16 + below_gap)
            .min(area.width)
    };
    let radius = u32::from(side / 2).saturating_sub(1);
    if radius < 4 {
        return;
    }
    let (moon_x, text_room) = if beside {
        let rest = area.width.saturating_sub(side + gap);
        match align {
            Align::Right => (
                area.x + area.width - side,
                Rect::new(area.x, area.y, rest, area.height),
            ),
            Align::Center => {
                // The moon and the text together in the middle.
                let widest = font.as_ref().map_or(0.0, |font| {
                    rows.iter()
                        .map(|(l, size, _)| font.measure(*size, l))
                        .fold(0.0, f32::max)
                });
                let used = (f32::from(side + gap) + widest).min(f32::from(area.width)) as u16;
                let left = area.x + (area.width - used) / 2;
                (
                    left,
                    Rect::new(left + side + gap, area.y, rest, area.height),
                )
            }
            // Left, and alignments to come.
            _ => (
                area.x,
                Rect::new(area.x + side + gap, area.y, rest, area.height),
            ),
        }
    } else if rows.is_empty() {
        // The moon alone: in the middle, as a sign.
        (Align::Center.x(area, f32::from(side)) as u16, area)
    } else {
        (align.x(area, f32::from(side)) as u16, area)
    };
    let top = if beside {
        area.y + (area.height - side) / 2
    } else {
        area.y + area.height.saturating_sub(side + below_gap + text_h as u16) / 2
    };
    let center = Point::new(
        i32::from(moon_x) + i32::from(side / 2),
        i32::from(top) + i32::from(side / 2),
    );
    let south = chosen_name(env, ID.as_str(), "hemisphere") == "south";
    let light = chosen_name(env, ID.as_str(), "shadow") == "light";
    draw_moon(canvas, &look, center, radius, moon.degrees, south, light);

    let Some(font) = font else {
        return;
    };
    let text_room = if beside {
        text_room
    } else {
        Rect::new(
            text_room.x,
            top + side + below_gap,
            text_room.width,
            text_h as u16,
        )
    };
    let text_align = if beside && align == Align::Center {
        Align::Left
    } else {
        align
    };
    let mut y = f32::from(text_room.y) + (f32::from(text_room.height) - text_h).max(0.0) / 2.0;
    for ((text, size, color), h) in rows.iter().zip(&heights) {
        let row = Rect::new(text_room.x, y as u16, text_room.width, *h as u16);
        super::label(canvas, &font, *size, text_align, row, text, *color);
        y += h;
    }
}

/// Draws the moon `degrees` round its lunation as a disc of `radius`
/// round `center`: its lit part light, the rest dark (or a light gray),
/// a fine ring round it.
fn draw_moon(
    canvas: &mut Canvas<'_>,
    look: &widgets::WidgetLook,
    center: Point,
    radius: u32,
    degrees: i32,
    south: bool,
    light_shadow: bool,
) {
    let (lit_color, dark) = super::light_and_dark(look);
    let r = radius as i32;
    if light_shadow {
        // Shaded where the panel has few grays: row by row.
        let gray = if look.paper.luma() < 128 { 90 } else { 190 };
        for dy in -r..r {
            let y = 2 * dy + 1;
            let w = ((4 * r * r - y * y).max(0) as u32).isqrt() as i32 / 2;
            let row = Rect::new(
                (center.x - w).max(0) as u16,
                (center.y + dy).max(0) as u16,
                (2 * w) as u16,
                1,
            );
            canvas.fill_shaded(row, Color::gray(gray));
        }
    } else {
        canvas.fill_circle(center, radius, dark);
    }
    let spans = lit_spans(radius, degrees, south);
    lit_part(canvas, &spans, center, radius, lit_color);
    let ring = (radius / 30).max(1);
    canvas.stroke_circle(center, radius, ring, look.ink);
}

/// The lit part of each eighth of a row of a moon of `radius`: from the
/// terminator to the edge, in eighths of a pixel from the centre.
fn lit_spans(radius: u32, degrees: i32, south: bool) -> Vec<(i32, i32)> {
    let (_, cos) = sin_cos(degrees);
    let waxing = degrees.rem_euclid(360) < 180;
    let right = waxing != south;
    let r8 = 8 * i64::from(radius);
    (0..2 * 4 * radius as i32)
        .map(|sub| {
            let y8 = 2 * i64::from(sub) + 1 - r8;
            let w = (r8 * r8 - y8 * y8).max(0).isqrt();
            let terminator = w * i64::from(cos) / i64::from(UNIT);
            let (lo, hi) = if right {
                (terminator, w)
            } else {
                (-w, -terminator)
            };
            (lo as i32, hi as i32)
        })
        .collect()
}

/// How much of the pixel at `left` (in eighths from the centre) the
/// `spans` of its row light, 0 to 255.
fn lit_share(spans: &[(i32, i32)], left: i32) -> u8 {
    let covered: i32 = spans
        .iter()
        .map(|&(lo, hi)| (hi.min(left + 8) - lo.max(left)).max(0))
        .sum();
    (covered * 255 / 32) as u8
}

/// Draws the lit part, given as `spans` for each eighth of a row: row by
/// row, what is lit through its whole width as a stretch, only the
/// pixels at its ends by how much of them is lit.
fn lit_part(
    canvas: &mut Canvas<'_>,
    spans: &[(i32, i32)],
    center: Point,
    radius: u32,
    color: Color,
) {
    let r = radius as i32;
    let size = 2 * radius;
    for row in 0..size as i32 {
        let sub = &spans[row as usize * 4..row as usize * 4 + 4];
        let (lo_min, lo_max) = sub.iter().fold((i32::MAX, i32::MIN), |(a, b), &(lo, _)| {
            (a.min(lo), b.max(lo))
        });
        let (hi_min, hi_max) = sub.iter().fold((i32::MAX, i32::MIN), |(a, b), &(_, hi)| {
            (a.min(hi), b.max(hi))
        });
        // The pixels any of the four lights, and those they all do
        // through the whole width.
        let from = (lo_min + 8 * r).div_euclid(8).max(0);
        let to = (hi_max + 8 * r + 7).div_euclid(8).min(size as i32);
        let whole_from = (lo_max + 8 * r + 7).div_euclid(8).clamp(from, to);
        let whole_to = (hi_min + 8 * r).div_euclid(8).clamp(whole_from, to);
        let (x, y) = (center.x - r, center.y - r + row);
        let ends = |canvas: &mut Canvas<'_>, a: i32, b: i32| {
            if a < b {
                canvas.draw_mask(x + a, y, (b - a) as u32, 1, color, |dx, _| {
                    lit_share(sub, 8 * (a + dx as i32) - 8 * r)
                });
            }
        };
        ends(canvas, from, whole_from);
        if whole_from < whole_to {
            canvas.fill_rect(x + whole_from, y, (whole_to - whole_from) as u32, 1, color);
        }
        ends(canvas, whole_to, to);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Milliseconds since 1970 of a time (UTC).
    fn utc(year: i32, month: u8, day: u8, hour: i64, minute: i64) -> i64 {
        (dates::days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60) * 1000
    }

    /// Whether `at` lies within `hours` of `expected`.
    fn near(at: i64, expected: i64, hours: i64) -> bool {
        (at - expected).abs() <= hours * 3_600_000
    }

    #[test]
    fn new_and_full_moons_on_known_dates() {
        // New moons with solar eclipses, full moons with lunar ones.
        for (expected, full) in [
            (utc(2024, 4, 8, 18, 21), false),
            (utc(2026, 2, 17, 12, 1), false),
            (utc(2026, 8, 12, 17, 37), false),
            (utc(2025, 3, 14, 6, 55), true),
            (utc(2026, 3, 3, 11, 38), true),
            (utc(2026, 8, 28, 4, 18), true),
            (utc(2000, 1, 6, 18, 14), false),
        ] {
            let k = (expected - EPOCH + LUNATION / 4).div_euclid(LUNATION / 2);
            assert_eq!(k % 2 != 0, full);
            let at = phase_time(k);
            assert!(near(at, expected, 1), "{k}: {}", (at - expected) / 60_000);
        }
    }

    #[test]
    fn the_moon_on_known_days() {
        // 14 October 2026, a few days after the new moon of 10 October:
        // a waxing crescent, full on 26 October.
        let m = moon(utc(2026, 10, 14, 8, 41));
        assert_eq!(phase(m.degrees), 1);
        assert_eq!(m.age, 3);
        let full = LocalTime::from_seconds(m.next_full / 1000);
        assert_eq!((full.month, full.day), (10, 26));
        assert!(lit(m.degrees) < 25);
        // The night of the full moon of 3 March 2026.
        let m = moon(utc(2026, 3, 3, 12, 0));
        assert_eq!(phase(m.degrees), 4);
        assert!(lit(m.degrees) >= 99);
        assert_eq!(LocalTime::from_seconds(m.next_full / 1000).day, 3);
        // A week after: waning gibbous, then last quarter.
        assert_eq!(phase(moon(utc(2026, 3, 7, 12, 0)).degrees), 5);
        assert_eq!(phase(moon(utc(2026, 3, 11, 12, 0)).degrees), 6);
        // Before the new moon of 17 February: a waning crescent.
        assert_eq!(phase(moon(utc(2026, 2, 14, 12, 0)).degrees), 7);
        assert_eq!(phase(moon(utc(2026, 2, 17, 13, 0)).degrees), 0);
    }

    #[test]
    fn phases_and_light() {
        assert_eq!(phase(0), 0);
        assert_eq!(phase(359), 0);
        assert_eq!(phase(45), 1);
        assert_eq!(phase(90), 2);
        assert_eq!(phase(135), 3);
        assert_eq!(phase(180), 4);
        assert_eq!(phase(225), 5);
        assert_eq!(phase(270), 6);
        assert_eq!(phase(315), 7);
        assert_eq!(lit(0), 0);
        assert_eq!(lit(90), 50);
        assert_eq!(lit(180), 100);
    }

    /// The lit part drawn by stretches is the one drawn pixel by pixel.
    #[test]
    fn the_lit_part_by_stretches_is_the_one_by_pixels() {
        use paperpaw_graphics::Framebuffer;
        use paperpaw_sdk::PixelFormat;
        for format in [PixelFormat::Gray4, PixelFormat::Mono1] {
            for radius in [5_u32, 17, 40, 101] {
                for degrees in (0..360).step_by(23) {
                    for south in [false, true] {
                        let spans = lit_spans(radius, degrees, south);
                        let size = 2 * radius as u16 + 8;
                        let center = Point::new(i32::from(size / 2), i32::from(size / 2));
                        let draw = |by_pixels: bool| {
                            let mut fb = Framebuffer::new(size, size, format).unwrap();
                            let mut canvas = fb.canvas();
                            canvas.clear(Color::gray(40));
                            if by_pixels {
                                let r = radius as i32;
                                canvas.draw_mask(
                                    center.x - r,
                                    center.y - r,
                                    2 * radius,
                                    2 * radius,
                                    Color::WHITE,
                                    |x, y| {
                                        super::lit_share(
                                            &spans[y as usize * 4..y as usize * 4 + 4],
                                            8 * x as i32 - 8 * r,
                                        )
                                    },
                                );
                            } else {
                                lit_part(&mut canvas, &spans, center, radius, Color::WHITE);
                            }
                            let view = fb.view();
                            (0..size)
                                .flat_map(|y| (0..size).map(move |x| (x, y)))
                                .map(|(x, y)| view.pixel(x, y))
                                .collect::<Vec<_>>()
                        };
                        assert!(
                            draw(true) == draw(false),
                            "{format:?} r{radius} {degrees} south {south}"
                        );
                    }
                }
            }
        }
    }
}
