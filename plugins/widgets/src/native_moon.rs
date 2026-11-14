//! The moon's phase: today's moon drawn, its lit part bounded by the
//! terminator as it is seen, with the phase's name and when the moon is full
//! next.
//!
//! The new and full moons are reckoned as astronomers do (Meeus,
//! *Astronomical Algorithms*, chapter 49: the mean lunation and its largest
//! periodic terms), in whole numbers: within minutes of the true times,
//! enough for the day.
use crate::native_dates::days_from_civil;
use crate::native_draw::{light_and_dark, no_time, today};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::Host;
use paperpaw_sdk::{
    Color, Point, Rect,
    native::{Error, formatting::Arg},
    round::{UNIT, sin_cos},
    time::LocalTime,
};
use paperpaw_ui_kit::widget::Cx;
use paperpaw_ui_kit::widget::names::{chosen_name, date_text};
use paperpaw_ui_kit::widget::settings::Stack;
use paperpaw_ui_kit::{
    Paint, paint,
    widget::{
        Align,
        layout::{label, line, wrap_whole},
        shape::{fill_circle, spans, stroke_circle},
    },
};

/// A lunation, in milliseconds (29.530588861 days).
const LUNATION: i64 = 2_551_442_877;
/// The mean new moon of 6 January 2000 (lunation 0), in milliseconds since
/// 1970 (UTC).
const EPOCH: i64 = 947_168_373_824;

/// When the moon is new (`half` even) or full (`half` odd) in half
/// lunation `half` since that of January 2000, in milliseconds since
/// 1970 (UTC).
pub fn phase_time(half: i64) -> i64 {
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
pub struct Moon {
    /// How far round its lunation it is, in degrees: 0 new, 180 full.
    pub degrees: i32,
    /// When it is full next (or was, today), in milliseconds since 1970.
    pub next_full: i64,
    /// Days since it was new.
    pub age: i64,
}

/// The moon at `unix_ms` milliseconds since 1970 (UTC).
pub fn moon(unix_ms: i64) -> Moon {
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
pub fn phase(degrees: i32) -> u8 {
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
pub fn lit(degrees: i32) -> u8 {
    let (_, cos) = sin_cos(degrees);
    ((UNIT - cos) * 100 / (2 * UNIT)) as u8
}

/// The lit part of each eighth of a row of a moon of `radius`: from the
/// terminator to the edge, in eighths of a pixel from the centre.
fn lit_spans(radius: u32, degrees: i32, south: bool) -> Result<Vec<(i32, i32)>, Error> {
    let (_, cos) = sin_cos(degrees);
    let waxing = degrees.rem_euclid(360) < 180;
    let right = waxing != south;
    let r8 = 8 * i64::from(radius);
    let rows = 2 * 4 * radius as i32;
    let mut out: Vec<(i32, i32)> = Vec::new();
    out.try_reserve(usize::try_from(rows).unwrap_or(0))
        .map_err(|_| Error::NoMemory)?;
    for sub in 0..rows {
        let y8 = 2 * i64::from(sub) + 1 - r8;
        let w = (r8 * r8 - y8 * y8).max(0).isqrt();
        let terminator = w * i64::from(cos) / i64::from(UNIT);
        let (lo, hi) = if right {
            (terminator, w)
        } else {
            (-w, -terminator)
        };
        out.push((lo as i32, hi as i32));
    }
    Ok(out)
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

/// What it shows, to tell when it changed: the day.
///
/// # Errors
/// Host refusals.
pub fn shows(host: &Host<'_>, _widget: &str) -> Result<u64, Error> {
    today(host)
}

/// The height: a moon as wide as half the place, no higher than three rows.
///
/// # Errors
/// Host refusals.
pub fn height(cx: &mut Cx<'_>, width: u16) -> Result<u16, Error> {
    Ok((width / 2)
        .min((u32::from(cx.style().row) * 3) as u16)
        .saturating_add(cx.shape.frame_height()))
}

/// The lines of text beside the moon.
fn lines(cx: &mut Cx<'_>, moon: &Moon, today: &LocalTime) -> Result<Vec<String>, Error> {
    let mut lines: Vec<String> = Vec::new();
    lines.try_reserve(3).map_err(|_| Error::NoMemory)?;
    if cx.settings.shows("name")? {
        let key =
            Stack::<48>::from_args(format_args!("widgets.moon.phase.{}", phase(moon.degrees)))?;
        lines.push(cx.tr(key.as_str())?);
    }
    if cx.settings.shows("full")? {
        let unix = moon.next_full.div_euclid(1000);
        let local = unix + i64::from(cx.zone_offset(unix)?);
        let day = LocalTime::from_seconds(local);
        let days = days_from_civil(day.year, day.month, day.day)
            - days_from_civil(today.year, today.month, today.day);
        let text = match days {
            0 => cx.tr("widgets.moon.full-today")?,
            1 => cx.tr("widgets.moon.full-tomorrow")?,
            _ => {
                let date = date_text(cx, day.year, day.month, day.day, false)?;
                cx.format(
                    "widgets.moon.full-on",
                    &[Arg::Text {
                        name: "date",
                        value: &date,
                    }],
                )?
            }
        };
        lines.push(text);
    }
    if cx.settings.shows("lit")? {
        lines.push(cx.format(
            "widgets.moon.lit",
            &[Arg::Int {
                name: "percent",
                value: i64::from(lit(moon.degrees)),
            }],
        )?);
    }
    Ok(lines)
}

/// A line of text beside or below the moon: its words, size and colour.
type Row = (String, f32, Color);

/// The text's rows beside a moon `side` pixels across (`beside`), or below
/// it in `share` of the height; and how many of the lines they show.
fn rows_in(
    cx: &Cx<'_>,
    lines: &[String],
    look: &paperpaw_ui_kit::widget::Look,
    (beside, share, side): (bool, f32, u16),
) -> Result<(Vec<Row>, usize), Error> {
    let area = look.area;
    let (first, small, gap) = (cx.style().text, cx.style().small, cx.style().gap);
    let (width, room, most) = if beside {
        (
            area.width.saturating_sub(side + gap),
            f32::from(area.height),
            2,
        )
    } else {
        (area.width, f32::from(area.height) * share, 3)
    };
    // Each line whole, broken between words: the first as large as the body
    // size down to the small one, the others small; those that have no room
    // left out from the last.
    let mut rows: Vec<Row> = Vec::new();
    let mut used = 0.0;
    let mut shown = 0;
    for (n, text) in lines.iter().enumerate() {
        let (max, color) = if n == 0 {
            (first, look.ink)
        } else {
            (small, look.faint)
        };
        // In as few lines as it fits.
        let mut fitted = None;
        for count in 1..=most {
            let place = (f32::from(width), room - used);
            if let Some(found) = wrap_whole(&cx.font, (max, small), text, place, count)? {
                fitted = Some(found);
                break;
            }
        }
        let Some((size, broken)) = fitted else {
            break;
        };
        used += line(&cx.font, size)? * broken.len() as f32;
        shown += 1;
        rows.try_reserve(broken.len())
            .map_err(|_| Error::NoMemory)?;
        rows.extend(broken.into_iter().map(|l| (l, size, color)));
    }
    Ok((rows, shown))
}

/// Draws the widget in `area`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
#[allow(clippy::too_many_lines, reason = "one layout")]
pub fn draw(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let (Some(unix), Some(today)) = (cx.snapshot.unix_seconds(), cx.local_time()?) else {
        return no_time(cx, out, &look);
    };
    let moon = moon(unix.saturating_mul(1000));
    let lines = lines(cx, &moon, &today)?;
    let area = look.area;
    let align = cx.settings.align()?;
    let gap = cx.style().gap;
    // Beside the text where the place is wide, else above it.
    let wide = area.width >= area.height * 3 / 2;
    // Beside it where the place is wide, the moon a little smaller where that
    // gives a long word room.
    let mut beside = wide && !lines.is_empty();
    let mut side = area.height;
    let mut rows: Vec<Row> = Vec::new();
    if beside {
        for part in [10, 9, 8, 7] {
            side = area.height * part / 10;
            rows = rows_in(cx, &lines, &look, (true, 1.0, side))?.0;
            if !rows.is_empty() {
                break;
            }
        }
    }
    if !beside || rows.is_empty() {
        beside = false;
        // Below it: the moon the larger, but not at the cost of a line that
        // has room where the moon is a little smaller.
        let mut best = rows_in(cx, &lines, &look, (false, 0.45, 0))?;
        for share in [0.55, 0.65] {
            let more = rows_in(cx, &lines, &look, (false, share, 0))?;
            if more.1 > best.1 {
                best = more;
            }
        }
        rows = best.0;
    }
    let mut heights: Vec<f32> = Vec::new();
    heights
        .try_reserve(rows.len())
        .map_err(|_| Error::NoMemory)?;
    for (_, size, _) in &rows {
        heights.push(line(&cx.font, *size)?);
    }
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
        return Ok(());
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
                let mut widest = 0.0_f32;
                for (l, size, _) in &rows {
                    widest = widest.max(cx.font.measure_text(*size, l)?);
                }
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
    let centre = Point::new(
        i32::from(moon_x) + i32::from(side / 2),
        i32::from(top) + i32::from(side / 2),
    );
    let south = chosen_name(cx, "hemisphere")?.as_str() == "south";
    let light = chosen_name(cx, "shadow")?.as_str() == "light";
    draw_moon(out, &look, centre, radius, (moon.degrees, south, light))?;

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
        label(out, &cx.font, (*size, text_align), row, text, *color)?;
        y += h;
    }
    Ok(())
}

/// Draws the moon `degrees` round its lunation as a disc of `radius` round
/// `centre`: its lit part light, the rest dark (or a light gray), a fine ring
/// round it.
fn draw_moon(
    out: &mut Vec<Paint>,
    look: &paperpaw_ui_kit::widget::Look,
    centre: Point,
    radius: u32,
    (degrees, south, light_shadow): (i32, bool, bool),
) -> Result<(), Error> {
    let (lit_color, dark) = light_and_dark(look);
    let r = radius as i32;
    if light_shadow {
        // A gray disc, row by row.
        let gray = if look.paper.luma() < 128 { 90 } else { 190 };
        for dy in -r..r {
            let y = 2 * dy + 1;
            let w = ((4 * r * r - y * y).max(0) as u32).isqrt() as i32 / 2;
            let row = Rect::new(
                (centre.x - w).max(0) as u16,
                (centre.y + dy).max(0) as u16,
                (2 * w) as u16,
                1,
            );
            paint::push(out, Paint::Fill(row, Color::gray(gray)))?;
        }
    } else {
        fill_circle(out, centre, radius, dark)?;
    }
    let lit = lit_spans(radius, degrees, south)?;
    // The pixels more than half lit, row by row as stretches.
    spans(
        out,
        (centre.x - r, centre.y - r),
        (2 * radius, 2 * radius),
        lit_color,
        |x, y| {
            let at = y as usize * 4;
            lit.get(at..at + 4)
                .is_some_and(|sub| lit_share(sub, 8 * x as i32 - 8 * r) >= 128)
        },
    )?;
    let ring = (radius / 30).max(1);
    stroke_circle(out, centre, radius, ring, look.ink)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Milliseconds since 1970 of a time (UTC).
    fn utc(year: i32, month: u8, day: u8, hour: i64, minute: i64) -> i64 {
        (days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60) * 1000
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
}
