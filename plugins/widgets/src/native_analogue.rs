//! The analogue clock: a round face with an hour and a minute hand, ticks or
//! numerals as the reader likes, a ring or two round it, and the date below it
//! or on the face.
use crate::native_cx::Cx;
use crate::native_draw::{minute, no_time};
use crate::native_names::{chosen_name, date_text};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{Point, Rect, native::Error, round::point_at, time::LocalTime};
use paperpaw_ui_kit::{
    Paint, paint,
    widget::{
        layout::{cap_height, centred, line},
        shape::{capsule, fill_circle, stroke_circle},
        text,
    },
};

/// The height: the face, as wide as the place and no higher than four rows.
///
/// # Errors
/// Host refusals.
pub fn height(cx: &mut Cx<'_>, width: u16) -> Result<u16, Error> {
    let face = width.min((u32::from(cx.style().row) * 4) as u16);
    Ok(face.saturating_add(cx.shape.frame_height()))
}

/// What it shows, to tell when it changed: the minute.
///
/// # Errors
/// Host refusals.
pub fn shows(cx: &Cx<'_>) -> Result<String, Error> {
    minute(cx)
}

/// The angles of the hands, in degrees clockwise from twelve: the hour hand
/// moves on with the minutes.
#[must_use]
pub fn angles(hour: u8, minute: u8) -> (i32, i32) {
    let hour = i32::from(hour % 12) * 30 + i32::from(minute) / 2;
    (hour, i32::from(minute) * 6)
}

/// Draws the widget in `area`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
#[allow(clippy::too_many_lines, reason = "one drawing")]
pub fn draw(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let area = look.area;
    let Some(now) = cx.local_time()? else {
        return no_time(cx, out, &look);
    };
    let (small, gap, line_px) = {
        let style = cx.style();
        (style.small, style.gap, style.line)
    };
    let date = chosen_name(cx, "date")?;
    let date_text = date_text(cx, now.year, now.month, now.day, false)?;
    // The date below takes a line, where it fits whole and leaves the face
    // two thirds of the height at least; the face the rest, round.
    let below = if date.as_str() == "below"
        && cx.font.measure_text(small, &date_text)? <= f32::from(area.width)
    {
        let below = line(&cx.font, small)? as u16 + gap / 2;
        if below * 3 <= area.height { below } else { 0 }
    } else {
        0
    };
    let room = area.height.saturating_sub(below).min(area.width);
    let radius = u32::from(room / 2);
    if radius < 8 {
        return Ok(());
    }
    let align = cx.settings.align()?;
    let left = align.x(area, f32::from(room)) as i32;
    let top = i32::from(area.y) + i32::from(area.height.saturating_sub(room + below) / 2);
    let centre = Point::new(left + radius as i32, top + radius as i32);
    let r = radius as i32;
    let ink = look.ink;

    // Rings round the face.
    let ring = (radius / 40).max(1) + u32::from(u32::from(line_px) > 1);
    let rings = chosen_name(cx, "ring")?;
    match rings.as_str() {
        "one" => stroke_circle(out, centre, radius, ring, ink)?,
        "two" => {
            stroke_circle(out, centre, radius, ring, ink)?;
            stroke_circle(out, centre, radius - ring * 3, (ring / 2).max(1), ink)?;
        }
        _ => {}
    }
    // The ticks start inside the rings.
    let edge = match rings.as_str() {
        "one" => r - ring as i32 * 3,
        "two" => r - ring as i32 * 6,
        _ => r - (r / 30).max(1),
    };
    let face = chosen_name(cx, "face")?;
    let tick = (radius / 28).max(2);
    match face.as_str() {
        "hours" | "minutes" => {
            for n in 0..60 {
                let hour = n % 5 == 0;
                if !hour && face.as_str() != "minutes" {
                    continue;
                }
                let (length, width) = match (hour, n % 15 == 0) {
                    (true, true) => (r / 7, tick + tick / 2),
                    (true, false) => (r / 9, tick),
                    _ => (r / 22, (tick / 2).max(1)),
                };
                let outer = point_at(centre, edge - width as i32 / 2, n * 6);
                let inner = point_at(centre, edge - length, n * 6);
                capsule(out, inner, outer, width, ink)?;
            }
        }
        "quarters" | "numerals" => {
            let size = (r as f32
                * if face.as_str() == "quarters" {
                    0.3
                } else {
                    0.24
                })
            .max(6.0);
            let at = edge - (size * 0.62) as i32;
            for hour in 1..=12u8 {
                let degrees = i32::from(hour) * 30;
                if face.as_str() == "quarters" && hour % 3 != 0 {
                    // Dots between the numerals.
                    let dot = point_at(centre, at, degrees);
                    fill_circle(out, dot, (tick * 2 / 3).max(1), ink)?;
                    continue;
                }
                let p = point_at(centre, at, degrees);
                let number = cx.number(i64::from(hour))?;
                centred(out, &cx.font, size, (p.x as f32, p.y as f32), &number, ink)?;
            }
        }
        _ => {}
    }

    // The day on the face, in a window towards three as on a watch (towards
    // six where numerals are).
    if date.as_str() == "inside" {
        let number = cx.number(i64::from(now.day))?;
        let numerals = matches!(face.as_str(), "quarters" | "numerals");
        let size = (r as f32 * if numerals { 0.14 } else { 0.17 }).max(6.0);
        let (x, y) = if numerals {
            (centre.x as f32, centre.y as f32 + r as f32 * 0.38)
        } else {
            (centre.x as f32 + r as f32 * 0.56, centre.y as f32)
        };
        let w = cx.font.measure_text(size, "00")? + size * 0.5;
        let h = cap_height(&cx.font, size)? + size * 0.55;
        let window = Rect::new(
            (x - w / 2.0) as u16,
            (y - h / 2.0) as u16,
            w as u16,
            h as u16,
        );
        let thick = (r as u32 / 60).max(1);
        paint::push(
            out,
            Paint::Focus(window, (h / 5.0) as u32, Some(thick), look.faint),
        )?;
        centred(out, &cx.font, size, (x, y), &number, ink)?;
    }

    // The hands, the minute hand over the hour hand.
    draw_hands(cx, out, centre, r, &now, ink)?;

    // The date below, placed as the face is (it fits whole: see above).
    if below > 0 {
        let gap = gap / 2;
        let row = Rect::new(area.x, (top + 2 * r) as u16 + gap, area.width, below - gap);
        text::line(
            out,
            &cx.font,
            row,
            align,
            (small, small),
            &date_text,
            look.faint,
        )?;
    }
    Ok(())
}

fn draw_hands(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    centre: Point,
    r: i32,
    now: &LocalTime,
    ink: paperpaw_sdk::Color,
) -> Result<(), Error> {
    let (hour, minute) = angles(now.hour, now.minute);
    let scale = match chosen_name(cx, "hands")?.as_str() {
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
        let tip = point_at(centre, length, degrees);
        let back = point_at(centre, tail, degrees + 180);
        capsule(out, back, tip, w, ink)?;
    }
    if cx.settings.shows("centre")? {
        fill_circle(out, centre, hour_width * 3 / 4 + 1, ink)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hands_point_where_a_clock_shows() {
        assert_eq!(angles(0, 0), (0, 0));
        assert_eq!(angles(3, 0), (90, 0));
        assert_eq!(angles(15, 30), (105, 180));
        assert_eq!(angles(10, 41), (320, 246));
        assert_eq!(angles(23, 59), (359, 354));
    }
}
