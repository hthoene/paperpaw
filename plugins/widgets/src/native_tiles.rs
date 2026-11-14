//! The clock in tiles, as a flip clock looks: each digit (or each pair) on a
//! rounded tile of its own, dark with light digits, light, or outlined; twelve
//! or twenty-four hours as the clock is set, the date below if wished.
use crate::native_draw::{light_and_dark, no_time, twelve};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::Host;
use paperpaw_sdk::{
    Color, Point, Rect,
    native::{Error, formatting::Arg},
    time::LocalTime,
};
use paperpaw_ui_kit::widget::Cx;
use paperpaw_ui_kit::widget::names::{chosen_name, date_text};
use paperpaw_ui_kit::widget::settings::Stack;
use paperpaw_ui_kit::{
    Paint, paint,
    widget::{
        Align, Look,
        layout::{cap_height, centred, label},
        shape::{fill, fill_circle},
        text::largest_size,
    },
};

/// The tiles at `t`: the hours' and the minutes' (each a digit, or both
/// digits on one).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn tiles(
    t: &LocalTime,
    twelve_hours: bool,
    pairs: bool,
) -> Result<(Vec<String>, Vec<String>), Error> {
    let hours = if twelve_hours {
        Stack::<4>::from_args(format_args!("{}", twelve(t.hour)))?
    } else {
        Stack::<4>::from_args(format_args!("{:02}", t.hour))?
    };
    let minutes = Stack::<4>::from_args(format_args!("{:02}", t.minute))?;
    let split = |s: &str| -> Result<Vec<String>, Error> {
        let mut out: Vec<String> = Vec::new();
        if pairs {
            out.try_reserve(1).map_err(|_| Error::NoMemory)?;
            out.push(paperpaw_ui_kit::text::copy(s)?);
        } else {
            out.try_reserve(s.len()).map_err(|_| Error::NoMemory)?;
            for c in s.chars() {
                let mut one = String::new();
                one.try_reserve(4).map_err(|_| Error::NoMemory)?;
                one.push(c);
                out.push(one);
            }
        }
        Ok(out)
    };
    Ok((split(hours.as_str())?, split(minutes.as_str())?))
}

/// The height: the tiles, the date line if wished, the frame.
///
/// # Errors
/// Font and host refusals.
pub fn height(cx: &mut Cx<'_>, width: u16) -> Result<u16, Error> {
    let (row, small, gap) = {
        let style = cx.style();
        (u32::from(style.row), style.small, style.gap)
    };
    let tiles = (u32::from(width) * 2 / 5).min(row * 3) as u16;
    let date = if cx.settings.shows("date")? {
        paperpaw_ui_kit::geometry::line_px(&cx.font, small)?.saturating_add(gap)
    } else {
        0
    };
    Ok(tiles
        .saturating_add(date)
        .saturating_add(cx.shape.frame_height()))
}

/// What it shows, to tell when it changed: the minute.
///
/// # Errors
/// Host refusals.
pub fn shows(host: &Host<'_>, _widget: &str) -> Result<u64, Error> {
    crate::native_draw::minute(host)
}

fn localised(cx: &mut Cx<'_>, parts: Vec<String>) -> Result<Vec<String>, Error> {
    let mut out: Vec<String> = Vec::new();
    out.try_reserve(parts.len()).map_err(|_| Error::NoMemory)?;
    for part in parts {
        out.push(cx.messages.digits(cx.host, &part)?);
    }
    Ok(out)
}

/// Each tile `w` wide (pairs wider), `w * 1.4` high.
fn tile_w(text: &str) -> f32 {
    if text.chars().count() > 1 { wide() } else { 1.0 }
}

/// A pair's width. Apart on the S3: LLVM's Xtensa backend cannot select the
/// constant pool of two floats it makes of `1.55` and `1.0` ("Cannot select:
/// XtensaISD::PCREL_WRAPPER ... [2 x float]"), which stops the plugin building (#263).
#[cfg_attr(target_arch = "xtensa", inline(never))]
fn wide() -> f32 {
    // `black_box`: without it the compiler sees the constant through the call.
    #[cfg(target_arch = "xtensa")]
    return core::hint::black_box(1.55);
    #[cfg(not(target_arch = "xtensa"))]
    1.55
}

/// The measures of the row of tiles in an area.
struct Plan {
    units: f32,
    gaps: f32,
    group_gap: f32,
}

impl Plan {
    fn total(&self, w: f32) -> f32 {
        w * (self.units + self.gaps)
    }
}

/// Draws the widget in `area`.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
#[allow(clippy::too_many_lines, reason = "one drawing")]
pub fn draw(cx: &mut Cx<'_>, out: &mut Vec<Paint>, area: Rect) -> Result<(), Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let Some(now) = cx.local_time()? else {
        return no_time(cx, out, &look);
    };
    let area = look.area;
    let twelve_hours = cx.twelve_hours()?;
    let pairs = chosen_name(cx, "group")?.as_str() == "pairs";
    let (hours, minutes) = tiles(&now, twelve_hours, pairs)?;
    let (hours, minutes) = (localised(cx, hours)?, localised(cx, minutes)?);
    let colon = cx.settings.shows("colon")?;
    let (small_size, gap_px, line_px) = {
        let style = cx.style();
        (style.small, f32::from(style.gap), style.line)
    };
    let small = paperpaw_ui_kit::widget::layout::line(&cx.font, small_size)?;
    let mut date = if cx.settings.shows("date")? {
        Some(date_text(cx, now.year, now.month, now.day, false)?)
    } else {
        None
    };
    let mut meridiem = if twelve_hours {
        let key = if now.hour < 12 {
            "core.clock.am"
        } else {
            "core.clock.pm"
        };
        let text = cx.format(
            key,
            &[Arg::Text {
                name: "time",
                value: "",
            }],
        )?;
        Some(paperpaw_ui_kit::text::copy(text.trim())?)
    } else {
        None
    };
    let units: f32 = hours.iter().chain(&minutes).map(|t| tile_w(t)).sum();
    let gap = 0.08;
    let group_gap = if colon { 0.42 } else { 0.24 };
    let plan = Plan {
        units,
        gaps: gap * (hours.len() + minutes.len() - 2) as f32 + group_gap,
        group_gap,
    };
    let area_h = f32::from(area.height);
    let tile = |cx: &Cx<'_>, below: f32| -> Result<f32, Error> {
        let room_h = (area_h - below).max(0.0);
        let w = (f32::from(area.width) / (plan.units + plan.gaps)).min(room_h / 1.4);
        // Fonts of fixed sizes have a largest: the tiles no larger than its
        // digits fill well.
        let digits = cap_height(&cx.font, w * 1.4 * 0.66)?;
        Ok(w.min(digits / 0.48 / 1.4))
    };
    // The line below (the date, "AM" or "PM") where it fits whole under the
    // tiles and leaves them two thirds of the height; the date is left out
    // first.
    let line_below = small + gap_px / 2.0;
    let meridiem_width = |cx: &Cx<'_>, m: &Option<String>| -> Result<f32, Error> {
        Ok(match m {
            Some(m) => cx.font.measure_text(small_size, m)? + gap_px,
            None => 0.0,
        })
    };
    let room_below =
        |cx: &Cx<'_>, date: &Option<String>, m: &Option<String>| -> Result<bool, Error> {
            let total = plan.total(tile(cx, line_below)?);
            let date_width = match date {
                Some(d) => cx.font.measure_text(small_size, d)?,
                None => 0.0,
            };
            Ok(line_below * 3.0 <= area_h && date_width + meridiem_width(cx, m)? <= total + 0.5)
        };
    if date.is_some() && !room_below(cx, &date, &meridiem)? {
        date = None;
    }
    if meridiem.is_some() && !room_below(cx, &date, &meridiem)? {
        meridiem = None;
    }
    let below = if date.is_some() || meridiem.is_some() {
        line_below
    } else {
        0.0
    };
    let w = tile(cx, below)?;
    if w < 6.0 {
        return Ok(());
    }
    let h = w * 1.4;
    let total = plan.total(w);
    let align = cx.settings.align()?;
    let left = align.x(area, total);
    let top = f32::from(area.y) + (area_h - h - below).max(0.0) / 2.0;
    let size = h * 0.66;
    let radius = (w * 0.14) as u32;
    let kind = chosen_name(cx, "tiles")?;
    let hinge = cx.settings.shows("hinge")?;
    let (light, dark) = light_and_dark(&look);
    let on_dark = look.paper.luma() < 128;
    let mut x = left;
    for (group, texts) in [&hours, &minutes].into_iter().enumerate() {
        if group == 1 {
            let space = w * plan.group_gap;
            if colon {
                let dot = (w * 0.07).max(1.5) as u32;
                let centre = (x + space / 2.0) as i32;
                for dy in [-0.17, 0.17] {
                    let cy = (top + h / 2.0 + h * dy) as i32;
                    fill_circle(out, Point::new(centre, cy), dot, look.ink)?;
                }
            }
            x += space;
        }
        for (n, text) in texts.iter().enumerate() {
            if n > 0 {
                x += w * gap;
            }
            let width = w * tile_w(text);
            let rect = Rect::new(x as u16, top as u16, width as u16, h as u16);
            let digits = match kind.as_str() {
                "light" => {
                    let gray = if on_dark { 70 } else { 215 };
                    paint::push(out, Paint::Focus(rect, radius, None, Color::gray(gray)))?;
                    look.ink
                }
                "outline" => {
                    let thick = ((w / 30.0) as u32).max(u32::from(line_px) * 2);
                    paint::push(out, Paint::Focus(rect, radius, Some(thick), look.ink))?;
                    look.ink
                }
                _ => {
                    paint::push(out, Paint::Focus(rect, radius, None, look.ink))?;
                    look.paper
                }
            };
            let digit_size = largest_size(size, 1.0, |s| {
                Ok(cx.font.measure_text(s, text)? <= width * 0.86)
            })?
            .unwrap_or(1.0);
            let (cxm, cym) = (x + width / 2.0, top + h / 2.0);
            centred(out, &cx.font, digit_size, (cxm, cym), text, digits)?;
            if hinge {
                // The line where a flip clock's leaves meet.
                let thick = ((h / 70.0) as u16).max(1);
                let line = Rect::new(
                    rect.x,
                    (cym as u16).saturating_sub(thick / 2),
                    rect.width,
                    thick,
                );
                let color = match kind.as_str() {
                    "outline" => look.ink,
                    "light" if on_dark => dark,
                    "light" => light,
                    _ => look.paper,
                };
                fill(out, line, color)?;
            }
            x += width;
        }
    }
    below_line(
        cx,
        out,
        &look,
        (left, top + h, total),
        small,
        &meridiem,
        &date,
    )
}

/// The date below, and "AM" or "PM" at the right end.
fn below_line(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    (left, bottom, total): (f32, f32, f32),
    small: f32,
    meridiem: &Option<String>,
    date: &Option<String>,
) -> Result<(), Error> {
    let (small_size, gap) = (cx.style().small, f32::from(cx.style().gap));
    let row = Rect::new(
        left as u16,
        (bottom + gap / 2.0) as u16,
        total as u16,
        small as u16,
    );
    let meridiem_width = match meridiem {
        Some(m) => cx.font.measure_text(small_size, m)? + gap,
        None => 0.0,
    };
    if let Some(m) = meridiem {
        label(
            out,
            &cx.font,
            (small_size, Align::Right),
            row,
            m,
            look.faint,
        )?;
    }
    if let Some(date) = date {
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
        label(out, &cx.font, (small_size, place), room, date, look.faint)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(hour: u8, minute: u8) -> LocalTime {
        LocalTime::new(2026, 10, 14, hour, minute, 0)
    }

    #[test]
    fn digits_on_tiles() {
        let (h, m) = tiles(&at(9, 5), false, false).unwrap();
        assert_eq!((h.join("|"), m.join("|")), ("0|9".into(), "0|5".into()));
        // Twelve hours: no leading zero, one tile fewer.
        let (h, m) = tiles(&at(21, 41), true, false).unwrap();
        assert_eq!((h.join("|"), m.join("|")), ("9".into(), "4|1".into()));
        let (h, _) = tiles(&at(0, 0), true, false).unwrap();
        assert_eq!(h.join("|"), "1|2");
        // In pairs.
        let (h, m) = tiles(&at(10, 41), false, true).unwrap();
        assert_eq!((h.join("|"), m.join("|")), ("10".into(), "41".into()));
    }
}
