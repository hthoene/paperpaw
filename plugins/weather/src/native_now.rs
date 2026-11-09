//! The widget *Weather now* of the weather plugin: a symbol, the
//! temperature large, the condition's name, today's highest and lowest, and
//! what else the reader chooses (how warm it feels, wind, chance of rain,
//! humidity, sunrise and sunset), with the place's name small above.
//!
//! The symbol sits beside the text where the place is wide, above it where it
//! is tall (or as set), or small before the temperature; with the size "as
//! large as it fits", the temperature takes the room the other lines leave.
//! Lines that do not fit are left out, the least important first. Where
//! things go is decided in `layout` without a host; this file reads the
//! settings and the weather and draws the plan. The interface-6 code is
//! `now.rs`.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{
    Color, Rect,
    native::{Error, fonts::Font, formatting::Arg},
};
use paperpaw_ui_kit::{
    Paint,
    geometry::{clamp_u16, line_px},
    text,
    widget::{Cx, Look, TextSize},
};

use crate::declarations::{PLACINGS, SYMBOLS};
use crate::native_draw::{
    age_text, ceil, clock, digits_height, draw_digits, draw_symbol, draw_waiting_for, effective,
    ink_of, measure, sane, temperature_unit, text_from, wind_unit,
};
use crate::native_shown::Shown;
use crate::native_symbol;
use crate::native_weather::{Current, Day, Temperature, condition, degrees_text};

pub(crate) mod layout;
#[cfg(test)]
mod tests;

use layout::{Fonts, Line, Placing, Plan, Setup, inline_symbol};

impl Fonts for Font<'_> {
    fn width(&self, size: f32, text: &str) -> Result<f32, Error> {
        measure(self, size, text)
    }

    fn line(&self, size: f32) -> Result<u16, Error> {
        line_px(self, sane(size))
    }

    fn effective(&self, size: f32) -> Result<f32, Error> {
        effective(self, size)
    }
}

/// What the style and the settings say about the layout of the widget `cx`.
fn setup(cx: &Cx<'_>) -> Result<Setup, Error> {
    let style = cx.style();
    let title = style.title;
    let fixed = match cx.settings.size()? {
        TextSize::Small => Some(title * 1.6),
        TextSize::Medium => Some(title * 2.4),
        TextSize::Large => Some(title * 3.4),
        _ => None,
    };
    let symbol = cx
        .settings
        .with_text("symbol", |v| SYMBOLS.iter().position(|s| *s == v))?
        .unwrap_or(0);
    let placing = cx
        .settings
        .with_text("placing", |v| PLACINGS.iter().position(|p| *p == v))?
        .unwrap_or(0);
    Ok(Setup {
        text: style.text,
        small: style.small,
        title,
        gap: style.gap,
        fixed,
        symbol,
        placing,
    })
}

fn put<T>(list: &mut Vec<T>, item: T) -> Result<(), Error> {
    list.try_reserve(1).map_err(|_| Error::NoMemory)?;
    list.push(item);
    Ok(())
}

/// How high the widget is: the temperature, three small lines and the frame.
///
/// # Errors
/// `NoMemory`, or what the host says.
pub fn height(cx: &mut Cx<'_>, shown: &Shown<'_>, width: u16) -> Result<u16, Error> {
    let _ = (shown, width);
    let set = setup(cx)?;
    let size = set.fixed.unwrap_or(set.title * 2.4);
    let lines = line_px(&cx.font, sane(set.small))?.saturating_mul(3);
    Ok(cx
        .shape
        .frame_height()
        .saturating_add(digits_height(size))
        .saturating_add(lines))
}

/// The lines below the temperature and the caption above it, as set.
fn lines(
    cx: &mut Cx<'_>,
    shown: &Shown<'_>,
    current: Current,
    today: Option<&Day>,
) -> Result<Vec<Line>, Error> {
    let (text_size, small) = (cx.style().text, cx.style().small);
    let unit = temperature_unit(cx)?;
    let with_unit = cx.settings.shows("unit")?;
    let mut out: Vec<Line> = Vec::new();
    let mut caption: Vec<String> = Vec::new();
    if cx.settings.shows("place")? && !shown.place.is_empty() {
        put(&mut caption, text::copy(shown.place)?)?;
    }
    if cx.settings.shows("age")?
        && let Some(age) = age_text(cx, shown.old)?
    {
        put(&mut caption, age)?;
    }
    if !caption.is_empty() {
        put(
            &mut out,
            Line::new(&text::join_notes(&caption)?, small, true, 3)?,
        )?;
    }
    if cx.settings.shows("condition")? {
        let words = cx.tr(condition(current.code).0)?;
        put(&mut out, Line::new(&words, text_size, false, 5)?)?;
    }
    if cx.settings.shows("range")?
        && let Some(day) = today
    {
        let (high, low) = (
            degrees_text(unit, day.high, with_unit)?,
            degrees_text(unit, day.low, with_unit)?,
        );
        let words = cx.format(
            "weather.range",
            &[
                Arg::Text {
                    name: "high",
                    value: &high,
                },
                Arg::Text {
                    name: "low",
                    value: &low,
                },
            ],
        )?;
        put(&mut out, Line::new(&words, small, false, 4)?)?;
    }
    details(cx, &mut out, current, today, (unit, with_unit))?;
    Ok(out)
}

/// The lines of the details the reader chose (how warm it feels, wind, rain,
/// humidity, sunrise and sunset), after `out`'s others.
fn details(
    cx: &mut Cx<'_>,
    out: &mut Vec<Line>,
    current: Current,
    today: Option<&Day>,
    (unit, with_unit): (Temperature, bool),
) -> Result<(), Error> {
    let small = cx.style().small;
    if cx.settings.shows("feels")?
        && let Some(feels) = current.feels
    {
        let degrees = degrees_text(unit, feels, with_unit)?;
        let words = cx.format(
            "weather.feels",
            &[Arg::Text {
                name: "degrees",
                value: &degrees,
            }],
        )?;
        put(out, Line::new(&words, small, true, 2)?)?;
    }
    if cx.settings.shows("wind")?
        && let Some(wind) = current.wind
    {
        let wind_unit = wind_unit(cx)?;
        let speed = cx.format(
            wind_unit.key(),
            &[Arg::Int {
                name: "speed",
                value: i64::from(wind_unit.speed(wind)),
            }],
        )?;
        let words = cx.format(
            "weather.wind",
            &[Arg::Text {
                name: "speed",
                value: &speed,
            }],
        )?;
        put(out, Line::new(&words, small, true, 2)?)?;
    }
    if cx.settings.shows("rain")?
        && let Some(rain) = today.and_then(|d| d.rain)
    {
        let words = cx.format(
            "weather.rain",
            &[Arg::Int {
                name: "percent",
                value: i64::from(rain),
            }],
        )?;
        put(out, Line::new(&words, small, true, 2)?)?;
    }
    if cx.settings.shows("humidity")?
        && let Some(humidity) = current.humidity
    {
        let words = cx.format(
            "weather.humidity",
            &[Arg::Int {
                name: "percent",
                value: i64::from(humidity),
            }],
        )?;
        put(out, Line::new(&words, small, true, 1)?)?;
    }
    if cx.settings.shows("sun")?
        && let Some((rise, set)) = today.and_then(|d| d.sunrise.zip(d.sunset))
    {
        let (rise, set) = (clock(cx, rise)?, clock(cx, set)?);
        let words = cx.format(
            "weather.sun",
            &[
                Arg::Text {
                    name: "rise",
                    value: &rise,
                },
                Arg::Text {
                    name: "set",
                    value: &set,
                },
            ],
        )?;
        put(out, Line::new(&words, small, true, 1)?)?;
    }
    Ok(())
}

/// Lays the widget out in `area` as drawing operations: the frame, then the
/// weather, or why there is none.
///
/// # Errors
/// `NoMemory`, or what the host says.
pub fn draw(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    shown: &Shown<'_>,
) -> Result<(), Error> {
    // The snapshot does not tell the gray levels: a card is taken to be drawn
    // on a panel with grays.
    let look = cx.shape.paint(out, area, 256)?;
    let Some(current) = shown.weather.and_then(|w| w.current) else {
        return draw_waiting_for(cx, out, area, shown.waiting, shown.place);
    };
    let today = shown.weather.and_then(|w| w.days_from(shown.today).first());
    let set = setup(cx)?;
    let unit = temperature_unit(cx)?;
    let temperature = degrees_text(unit, current.temperature, cx.settings.shows("unit")?)?;
    let all = lines(cx, shown, current, today)?;
    let plan = layout::plan(&cx.font, &set, look.area, &temperature, &all)?;
    draw_now(cx, out, &look, current, (&temperature, &plan, &set))
}

/// A row of the text: its height, its width and its line (none for the
/// temperature).
type RowOf<'a> = (u16, f32, Option<&'a Line>);

/// The rows of the text, from the top: the height, the width and the line
/// (none for the temperature).
fn rows<'a>(
    font: &Font<'_>,
    plan: &'a Plan,
    temperature_width: f32,
) -> Result<Vec<RowOf<'a>>, Error> {
    let (head, rest) = plan
        .lines
        .split_at(usize::from(plan.caption).min(plan.lines.len()));
    let mut rows = Vec::new();
    rows.try_reserve(plan.lines.len() + 1)
        .map_err(|_| Error::NoMemory)?;
    for line in head {
        rows.push((
            font.line(line.size)?,
            measure(font, line.size, &line.text)?,
            Some(line),
        ));
    }
    rows.push((digits_height(plan.size), temperature_width, None));
    for line in rest {
        rows.push((
            font.line(line.size)?,
            measure(font, line.size, &line.text)?,
            Some(line),
        ));
    }
    Ok(rows)
}

/// Draws `plan`: the symbol (placed by its ink, not its square), the
/// caption, the temperature and the lines.
fn draw_now(
    cx: &Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    current: Current,
    (temperature, plan, set): (&str, &Plan, &Setup),
) -> Result<(), Error> {
    let font = &cx.font;
    let area = look.area;
    let gap = set.gap;
    let align = cx.settings.align()?;
    let symbol_style = crate::native_draw::style(cx)?;
    let (_, symbol) = condition(current.code);
    let mark = (symbol, current.day);
    let color = |line: &Line| -> Color {
        if line.faint { look.faint } else { look.ink }
    };

    let (inline, inline_gap) = if plan.placing == Placing::Inline {
        inline_symbol(plan.size, gap)
    } else {
        (0, 0)
    };
    let temperature_width = measure(font, plan.size, temperature)?
        + f32::from(inline)
        + f32::from(inline_gap);
    let rows = rows(font, plan, temperature_width)?;
    let total = rows
        .iter()
        .fold(0u16, |sum, (height, _, _)| sum.saturating_add(*height));
    let widest = rows
        .iter()
        .map(|(_, width, _)| *width)
        .fold(0.0, f32::max)
        .min(f32::from(plan.text.width));

    // Beside: the symbol and the text as one group, placed as set; the text
    // starts at one edge.
    let (text_area, group_x) = match plan.placing {
        Placing::Beside => {
            let side = plan.symbol.width;
            let (ink_x, ink_y, ink_w, ink_h) = ink_of(symbol, current.day, side);
            let group = f32::from(ink_w.saturating_add(gap)) + widest;
            let x = align.x(area, group) as i32;
            let y = i32::from(area.y) + (i32::from(area.height) - i32::from(ink_h)) / 2;
            let at = Rect::new(
                clamp_u16(i64::from(x - ink_x)),
                clamp_u16(i64::from(y - ink_y)),
                side,
                side,
            );
            native_symbol::draw(out, symbol, current.day, at, symbol_style, look.ink)?;
            let text_x = clamp_u16(i64::from(x) + i64::from(ink_w) + i64::from(gap));
            (
                Rect::new(text_x, plan.text.y, ceil(widest), plan.text.height),
                Some(f32::from(text_x)),
            )
        }
        Placing::Above => {
            let side = plan.symbol.width;
            let (ink_x, ink_y, ink_w, ink_h) = ink_of(symbol, current.day, side);
            let whole = ink_h.saturating_add(gap).saturating_add(total);
            let top = area.y.saturating_add(area.height.saturating_sub(whole) / 2);
            let x = align.x(area, f32::from(ink_w)) as i32;
            let at = Rect::new(
                clamp_u16(i64::from(x - ink_x)),
                clamp_u16(i64::from(top) - i64::from(ink_y)),
                side,
                side,
            );
            native_symbol::draw(out, symbol, current.day, at, symbol_style, look.ink)?;
            let below = top.saturating_add(ink_h).saturating_add(gap);
            (
                Rect::new(
                    area.x,
                    below,
                    area.width,
                    area.height.saturating_sub(below.saturating_sub(area.y)),
                ),
                None,
            )
        }
        _ => (plan.text, None),
    };
    let mut y = match plan.placing {
        Placing::Above => text_area.y,
        _ => text_area
            .y
            .saturating_add(text_area.height.saturating_sub(total) / 2),
    };
    for (height, width, line) in rows {
        let row = Rect::new(text_area.x, y, text_area.width.max(1), height);
        let x = group_x.unwrap_or_else(|| align.x(text_area, width));
        match line {
            Some(line) => text_from(out, font, line.size, x, row, &line.text, color(line))?,
            None => {
                if inline > 0 {
                    let top = row.y.saturating_add(row.height.saturating_sub(inline) / 2);
                    let at = Rect::new(clamp_u16(x as i64), top, inline, inline);
                    draw_symbol(out, at, mark, symbol_style, look.ink)?;
                }
                let x = x + f32::from(inline) + f32::from(inline_gap);
                draw_digits(out, plan.size, x, row, temperature, look.ink)?;
            }
        }
        y = y.saturating_add(height);
    }
    Ok(())
}
