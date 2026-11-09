//! The widget *Forecast* of the weather plugin: the next days, each with its
//! weekday, a symbol and its highest and lowest temperature (and the chance
//! of rain, if chosen); in a row of columns where the place is wide, as a
//! list where it is tall (or as set). In the list, a bar shows each day's
//! range within the week's. Where things go is decided in `layout` without a
//! host. The interface-6 code is `forecast.rs`.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{
    Color, Point, Rect,
    native::{Error, formatting::Arg},
};
use paperpaw_ui_kit::{
    Paint,
    geometry::{clamp_u16, line_px},
    text,
    widget::{
        Cx, Look, TextSize,
        layout::caption,
        shape::{capsule, fill},
    },
};

use crate::declarations::{LAYOUTS, NAMES, STARTS};
use crate::native_draw::{
    age_text, cap, digits_height, draw_digits, draw_symbol, draw_waiting_for, measure, sane,
    temperature_unit, text_from,
};
use crate::native_shown::Shown;
use crate::native_symbol::Style;
use crate::native_weather::{Day, Temperature, condition, degrees_text, weekday};

mod layout;
#[cfg(test)]
mod tests;

use layout::{Parts, Row, Setup, Texts, Wants};

/// What the style and the settings say about the layout of the widget `cx`.
fn setup(cx: &Cx<'_>) -> Result<Setup, Error> {
    let style = cx.style();
    let fixed = match cx.settings.size()? {
        TextSize::Small => Some(style.text),
        TextSize::Medium => Some(style.title),
        TextSize::Large => Some(style.title * 1.4),
        _ => None,
    };
    let dpi = u32::from(cx.snapshot.display_dpi());
    Ok(Setup {
        text: style.text,
        small: style.small,
        title: style.title,
        gap: style.gap,
        rule: style.line.max(1),
        mm6: clamp_u16(i64::from((60 * dpi + 127) / 254)),
        fixed,
    })
}

/// How high the widget is: three small lines, two lines of digits, the frame.
///
/// # Errors
/// `NoMemory`, or what the host says.
pub fn height(cx: &mut Cx<'_>, shown: &Shown<'_>, width: u16) -> Result<u16, Error> {
    let _ = (shown, width);
    let set = setup(cx)?;
    let small = line_px(&cx.font, sane(set.small))?.saturating_mul(3);
    let digits = digits_height(set.title).saturating_mul(2);
    Ok(cx
        .shape
        .frame_height()
        .saturating_add(small)
        .saturating_add(digits))
}

fn put<T>(list: &mut Vec<T>, item: T) -> Result<(), Error> {
    list.try_reserve(1).map_err(|_| Error::NoMemory)?;
    list.push(item);
    Ok(())
}

/// The days shown: from today or tomorrow, as many as set.
fn shown_days(cx: &Cx<'_>, days: &[Day]) -> Result<Vec<Day>, Error> {
    let skip = usize::from(cx.settings.with_text("start", |v| v == STARTS[1])?);
    let count = usize::try_from(cx.settings.int("days", 5)?.clamp(1, 7)).unwrap_or(5);
    let mut out = Vec::new();
    out.try_reserve(count.min(days.len()))
        .map_err(|_| Error::NoMemory)?;
    out.extend(days.iter().skip(skip).take(count).copied());
    Ok(out)
}

/// A weekday's name as set: `long` "Monday", short "Mon", or its initial.
fn day_name(
    cx: &mut Cx<'_>,
    day: &Day,
    today: Option<i32>,
    (long, initial): (bool, bool),
) -> Result<String, Error> {
    let weekday = weekday(day.date);
    if today == Some(day.date) && cx.settings.shows("today")? && !initial {
        return cx.tr("weather.today");
    }
    if long {
        return cx.tr(&text::format(format_args!("core.weekday.{weekday}"))?);
    }
    let short = cx.tr(&text::format(format_args!("weather.weekday.{weekday}"))?)?;
    if initial {
        let mut first = String::new();
        if let Some(c) = short.chars().next() {
            first.try_reserve(4).map_err(|_| Error::NoMemory)?;
            first.push(c);
        }
        return Ok(first);
    }
    Ok(short)
}

/// How the weekdays are named: (long, initial).
fn naming(cx: &Cx<'_>) -> Result<(bool, bool), Error> {
    cx.settings
        .with_text("names", |v| (v == NAMES[2], v == NAMES[1]))
}

/// Whether the days go in a list, as `cx` and the place say.
fn as_list(cx: &Cx<'_>, area: Rect, count: usize) -> Result<bool, Error> {
    let layout = cx
        .settings
        .with_text("layout", |v| LAYOUTS.iter().position(|l| *l == v))?;
    Ok(layout::as_list(layout, area.width, area.height, count))
}

/// Lays the widget out in `area` as drawing operations: the frame, then the
/// days, or why there are none.
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
    let days = match shown.weather {
        Some(weather) => shown_days(cx, weather.days_from(shown.today))?,
        None => Vec::new(),
    };
    if days.is_empty() {
        return draw_waiting_for(cx, out, area, shown.waiting, shown.place);
    }
    draw_forecast(cx, out, &look, shown, &days)
}

fn draw_forecast(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    look: &Look,
    shown: &Shown<'_>,
    days: &[Day],
) -> Result<(), Error> {
    let set = setup(cx)?;
    let mut notes: Vec<String> = Vec::new();
    if cx.settings.shows("place")? && !shown.place.is_empty() {
        put(&mut notes, text::copy(shown.place)?)?;
    }
    if let Some(age) = age_text(cx, shown.old)? {
        put(&mut notes, age)?;
    }
    let area = if notes.is_empty() {
        look.area
    } else {
        let align = cx.settings.align()?;
        caption(
            out,
            &cx.font,
            (set.small, set.gap),
            (look.area, align, look.faint),
            &text::join_notes(&notes)?,
        )?
    };
    let days = if as_list(cx, area, days.len())? {
        days
    } else {
        // As many columns as have room.
        let fit = layout::row_days(&cx.font, &set, area.width, days.len())?;
        days.get(..fit).unwrap_or(days)
    };
    let unit = temperature_unit(cx)?;
    let symbols = crate::native_draw::style(cx)?;
    let context = Context {
        set,
        look,
        unit,
        symbols,
        today: shown.today,
    };
    if as_list(cx, area, days.len())? {
        context.list(cx, out, area, days)
    } else {
        context.row(cx, out, area, days)
    }
}

/// What drawing the days needs.
struct Context<'a> {
    set: Setup,
    look: &'a Look,
    unit: Temperature,
    symbols: Style,
    today: Option<i32>,
}

impl Context<'_> {
    /// Draws a light line in `area`: a gray; on a dark card the faint colour.
    fn rule(&self, out: &mut Vec<Paint>, area: Rect) -> Result<(), Error> {
        let color = if self.look.paper.luma() < 128 {
            self.look.faint
        } else {
            Color::rgb(150, 150, 150)
        };
        fill(out, area, color)
    }

    fn degrees(&self, tenths: i32) -> Result<String, Error> {
        degrees_text(self.unit, tenths, false)
    }

    fn rain(&self, cx: &mut Cx<'_>, day: &Day) -> Result<Option<String>, Error> {
        let Some(rain) = day
            .rain
            .filter(|_| cx.settings.shows("rain").unwrap_or(false))
        else {
            return Ok(None);
        };
        cx.format(
            "weather.percent",
            &[Arg::Int {
                name: "percent",
                value: i64::from(rain),
            }],
        )
        .map(Some)
    }

    /// The days side by side, each a column: its name, symbol, highest, lowest
    /// and chance of rain, one below the other.
    fn row(
        &self,
        cx: &mut Cx<'_>,
        out: &mut Vec<Paint>,
        area: Rect,
        days: &[Day],
    ) -> Result<(), Error> {
        let set = &self.set;
        let mut rains: Vec<Option<String>> = Vec::new();
        for day in days {
            put(&mut rains, self.rain(cx, day)?)?;
        }
        let parts = Parts {
            symbol: cx.settings.shows("symbol")?,
            low: cx.settings.shows("low")?,
            rain: rains.iter().any(Option::is_some),
        };
        let plan = layout::row(&cx.font, set, (area.width, area.height), days.len(), parts)?;
        let names = naming(cx)?;
        let lines = cx.settings.shows("lines")?;
        let Row { column, top, .. } = plan;
        let top = area.y.saturating_add(top);
        for (i, day) in days.iter().enumerate() {
            let at = u16::try_from(i).unwrap_or(u16::MAX);
            let cell = Rect::new(
                area.x.saturating_add(column.saturating_mul(at)),
                area.y,
                column,
                area.height,
            );
            let centre = |w: f32| f32::from(cell.x) + (f32::from(cell.width) - w) / 2.0;
            let mut y = top;
            // The name: shorter where it does not fit.
            let mut label = day_name(cx, day, self.today, names)?;
            if measure(&cx.font, plan.name, &label)? > f32::from(column) {
                label = day_name(cx, day, None, (false, false))?;
            }
            if measure(&cx.font, plan.name, &label)? > f32::from(column) {
                label = day_name(cx, day, None, (false, true))?;
            }
            let font = &cx.font;
            let color = if self.today == Some(day.date) {
                self.look.ink
            } else {
                self.look.faint
            };
            let row = Rect::new(cell.x, y, cell.width, plan.name_h);
            let x = centre(measure(font, plan.name, &label)?);
            text_from(out, font, plan.name, x, row, &label, color)?;
            y = y.saturating_add(plan.name_h);
            if plan.side > 0 {
                y = y.saturating_add(plan.gap);
                let x = cell
                    .x
                    .saturating_add(cell.width.saturating_sub(plan.side) / 2);
                let (_, symbol) = condition(day.code);
                let square = Rect::new(x, y, plan.side, plan.side);
                draw_symbol(out, square, (symbol, true), self.symbols, self.look.ink)?;
                y = y.saturating_add(plan.side).saturating_add(plan.gap);
            }
            self.temperatures(out, font, (cell, y), day, &plan)?;
            y = y.saturating_add(plan.high_h);
            if plan.with_low {
                let text = self.degrees(day.low)?;
                let row = Rect::new(cell.x, y, cell.width, plan.low_h);
                let x = centre(measure(font, plan.low, &text)?);
                draw_digits(out, plan.low, x, row, &text, self.look.faint)?;
                y = y.saturating_add(plan.low_h);
            }
            if let Some(rain) = rains.get(i).and_then(Option::as_ref) {
                let row = Rect::new(cell.x, y, cell.width, plan.rain_h);
                let x = centre(measure(font, set.small, rain)?);
                text_from(out, font, set.small, x, row, rain, self.look.faint)?;
            }
            if lines && i > 0 {
                let inset = plan.total / 8;
                let line = Rect::new(
                    cell.x,
                    top.saturating_add(inset),
                    set.rule,
                    plan.total.saturating_sub(2 * inset),
                );
                self.rule(out, line)?;
            }
        }
        Ok(())
    }

    /// The highest of `day` in its column at `y`, with the lowest beside it
    /// where there is no room below.
    fn temperatures(
        &self,
        out: &mut Vec<Paint>,
        font: &paperpaw_sdk::native::fonts::Font<'_>,
        (cell, y): (Rect, u16),
        day: &Day,
        plan: &Row,
    ) -> Result<(), Error> {
        let centre = |w: f32| f32::from(cell.x) + (f32::from(cell.width) - w) / 2.0;
        let text = self.degrees(day.high)?;
        let row = Rect::new(cell.x, y, cell.width, plan.high_h);
        let high_w = measure(font, plan.high, &text)?;
        let beside = self.degrees(day.low)?;
        let space = f32::from(plan.gap);
        let beside_w = measure(font, plan.low, &beside)?;
        if plan.low_beside && high_w + space + beside_w <= f32::from(plan.column) * 0.95 {
            let x = centre(high_w + space + beside_w);
            draw_digits(out, plan.high, x, row, &text, self.look.ink)?;
            draw_digits(
                out,
                plan.low,
                x + high_w + space,
                row,
                &beside,
                self.look.faint,
            )
        } else {
            draw_digits(out, plan.high, centre(high_w), row, &text, self.look.ink)
        }
    }

    /// The names of `days`: as set, then short, then the initials.
    fn names(&self, cx: &mut Cx<'_>, days: &[Day]) -> Result<[Vec<String>; 3], Error> {
        let (long, initial) = naming(cx)?;
        let mut names: [Vec<String>; 3] = [Vec::new(), Vec::new(), Vec::new()];
        for (list, way) in names
            .iter_mut()
            .zip([(long, initial), (false, initial), (false, true)])
        {
            for day in days {
                put(list, day_name(cx, day, self.today, way)?)?;
            }
        }
        Ok(names)
    }

    /// The days one below the other, each a row: its name, symbol, chance of
    /// rain, lowest, a bar of its range within the week's, highest.
    fn list(
        &self,
        cx: &mut Cx<'_>,
        out: &mut Vec<Paint>,
        area: Rect,
        days: &[Day],
    ) -> Result<(), Error> {
        let set = &self.set;
        let count = layout::list_days(&cx.font, set, area.height, days.len())?;
        let days = days.get(..count).unwrap_or(days);
        let mut highs: Vec<String> = Vec::new();
        let mut lows: Vec<String> = Vec::new();
        let mut rains: Vec<Option<String>> = Vec::new();
        for day in days {
            put(&mut highs, self.degrees(day.high)?)?;
            put(&mut lows, self.degrees(day.low)?)?;
            put(&mut rains, self.rain(cx, day)?)?;
        }
        let names = self.names(cx, days)?;
        let wants = Wants {
            symbol: cx.settings.shows("symbol")?,
            low: cx.settings.shows("low")?,
            bars: cx.settings.shows("bars")?,
        };
        let texts = Texts {
            highs: &highs,
            lows: &lows,
            rains: &rains,
            names: [&names[0], &names[1], &names[2]],
        };
        let plan = layout::list(&cx.font, set, (area.width, area.height), &texts, wants)?;
        let lines = cx.settings.shows("lines")?;
        let font = &cx.font;
        let shown_names = names.get(plan.names).map_or(&[][..], Vec::as_slice);
        // The week's range, for the bars.
        let coldest = days.iter().map(|d| d.low).min().unwrap_or(0);
        let warmest = days
            .iter()
            .map(|d| d.high)
            .max()
            .unwrap_or(1)
            .max(coldest.saturating_add(1));
        let (bar_x, bar_w) = plan.bars.unwrap_or((0, 0));
        let at_x = |x: u16| area.x.saturating_add(x);
        let track_x = at_x(bar_x);
        let place = |t: i32| {
            i64::from(track_x)
                + i64::from(bar_w) * (i64::from(t) - i64::from(coldest))
                    / (i64::from(warmest) - i64::from(coldest))
        };
        for (i, day) in days.iter().enumerate() {
            let step = u16::try_from(i).unwrap_or(u16::MAX);
            let row = Rect::new(
                area.x,
                area.y.saturating_add(plan.height.saturating_mul(step)),
                area.width,
                plan.height,
            );
            let color = if self.today == Some(day.date) {
                self.look.ink
            } else {
                self.look.faint
            };
            if let Some(name) = shown_names.get(i) {
                text_from(out, font, plan.size, f32::from(area.x), row, name, color)?;
            }
            if plan.side > 0 {
                let top = row
                    .y
                    .saturating_add(plan.height.saturating_sub(plan.side) / 2);
                let square = Rect::new(at_x(plan.symbol_x), top, plan.side, plan.side);
                let (_, symbol) = condition(day.code);
                draw_symbol(out, square, (symbol, true), self.symbols, self.look.ink)?;
            }
            if let Some(rain) = rains
                .get(i)
                .and_then(Option::as_ref)
                .filter(|_| plan.rain_w > 0)
            {
                let w = measure(font, plan.small, rain)?;
                let x = f32::from(at_x(plan.rain_x)) + f32::from(plan.rain_w) - w;
                text_from(out, font, plan.small, x, row, rain, self.look.faint)?;
            }
            if plan.low_w > 0
                && let Some(low) = lows.get(i)
            {
                let w = measure(font, plan.size, low)?;
                let x = f32::from(at_x(plan.low_x)) + f32::from(plan.low_w) - w;
                draw_digits(out, plan.size, x, row, low, self.look.faint)?;
            }
            if let Some(high) = highs.get(i) {
                let w = measure(font, plan.size, high)?;
                let x = f32::from(at_x(plan.high_x)) + f32::from(plan.high_w) - w;
                draw_digits(out, plan.size, x, row, high, self.look.ink)?;
            }
            if plan.bars.is_some() {
                self.bar(out, (row, track_x, bar_w), plan.size, (day, &place))?;
            }
            if lines && i > 0 {
                let line = Rect::new(area.x, row.y, area.width, set.rule);
                self.rule(out, line)?;
            }
        }
        Ok(())
    }

    /// The track of the week (from `track_x`, `bar_w` wide) and the bar of
    /// `day`'s range on it, in `row`; `place` says where a temperature lies.
    fn bar(
        &self,
        out: &mut Vec<Paint>,
        (row, track_x, bar_w): (Rect, u16, u16),
        size: f32,
        (day, place): (&Day, &dyn Fn(i32) -> i64),
    ) -> Result<(), Error> {
        let mid = i32::from(row.y) + i32::from(row.height) / 2;
        let track = self.set.rule;
        let y = clamp_u16(i64::from(mid) - i64::from(track) / 2);
        self.rule(out, Rect::new(track_x, y, bar_w, track))?;
        let thick = (cap(size) * 0.25).max(3.0) as i64;
        let from = place(day.low);
        let to = place(day.high).max(from + thick);
        let end = |x: i64| i32::try_from(x).unwrap_or(i32::MAX);
        capsule(
            out,
            Point::new(end(from + thick / 2), mid),
            Point::new(end(to - thick / 2), mid),
            u32::try_from(thick).unwrap_or(3),
            self.look.ink,
        )
    }
}
