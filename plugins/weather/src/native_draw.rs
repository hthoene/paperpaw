//! What both widgets draw with: the style setting, the large digits and the
//! waiting message. Owner: the *Weather now* port. The interface-6 code is
//! `draw.rs`.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Rect,
    native::{Error, fonts::Font, fonts::TextOrigin, formatting::Arg},
};
use paperpaw_ui_kit::{
    Paint,
    geometry::{clamp_u16, line_metrics, line_px},
    paint,
    widget::{Align, Cx, Look, settings::text_of, text::Wrapped, text::ink_room},
};

use crate::declarations::STYLES;
use crate::native_shown::Waiting;
use crate::native_symbol::{self, Style};
use crate::native_weather::{Symbol, Temperature, Wind};

/// How the widget `cx` draws its symbols (its setting `style`).
///
/// # Errors
/// What the settings read says.
pub fn style(cx: &Cx<'_>) -> Result<Style, Error> {
    cx.settings.with_text("style", |value| {
        if value == STYLES[1] {
            Style::Solid
        } else {
            Style::Line
        }
    })
}

/// `size` as a size the host measures and draws at: finite, a pixel at least.
#[must_use]
pub fn sane(size: f32) -> f32 {
    if size.is_finite() { size.max(1.0) } else { 1.0 }
}

/// How wide `text` is at `size` pixels (`size` made [`sane`]).
///
/// # Errors
/// What the font says.
pub fn measure(font: &Font<'_>, size: f32, text: &str) -> Result<f32, Error> {
    font.measure_text(sane(size), text)
}

/// How high digits `size` pixels large are (their ink, about the
/// capitals' height).
#[must_use]
pub fn cap(size: f32) -> f32 {
    size * 0.72
}

/// The room above and below large digits at most, together.
const ROOM: f32 = 24.0;

/// The height of a line of digits `size` pixels large, with a little room
/// above and below.
#[must_use]
pub fn digits_height(size: f32) -> u16 {
    let cap = cap(size);
    clamp_u16((cap + (cap * 0.3).min(ROOM)) as i64)
}

/// The size of digits whose line is `height` pixels high
/// ([`digits_height`] the other way).
#[must_use]
pub fn digits_size(height: f32) -> f32 {
    let cap = if height >= ROOM / 0.3 * 1.3 {
        height - ROOM
    } else {
        height / 1.3
    };
    cap / cap_ratio()
}

fn cap_ratio() -> f32 {
    cap(1.0)
}

/// A line of `text` `size` pixels large from `x`, centred in `row` by the
/// font's line; wider than `row`, it is cut at the sides.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn text_from(
    out: &mut Vec<Paint>,
    font: &Font<'_>,
    size: f32,
    x: f32,
    row: Rect,
    text: &str,
    color: Color,
) -> Result<(), Error> {
    let size = sane(size);
    let (ascent, descent, _) = line_metrics(font, size)?;
    let baseline = f32::from(row.y) + (f32::from(row.height) + ascent - descent) / 2.0;
    push_text(out, size, (x, baseline), row, text, color)
}

/// Draws `text` `size` pixels large with its digits in the middle of `row`
/// from top to bottom, from `x`.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn draw_digits(
    out: &mut Vec<Paint>,
    size: f32,
    x: f32,
    row: Rect,
    text: &str,
    color: Color,
) -> Result<(), Error> {
    let size = sane(size);
    let baseline = f32::from(row.y) + (f32::from(row.height) + cap(size)) / 2.0;
    push_text(out, size, (x, baseline), row, text, color)
}

/// The run itself: cut at the sides of `row` (with the glyphs' overhang),
/// not above or below it, as a line of text is taller than its digits.
fn push_text(
    out: &mut Vec<Paint>,
    size: f32,
    (x, baseline): (f32, f32),
    row: Rect,
    text: &str,
    color: Color,
) -> Result<(), Error> {
    let above = clamp_u16(size as i64);
    let tall = Rect::new(
        row.x,
        row.y.saturating_sub(above),
        row.width,
        row.height.saturating_add(above.saturating_mul(2)),
    );
    paint::push(
        out,
        Paint::Text {
            text: paperpaw_ui_kit::text::copy(text)?,
            size,
            origin: TextOrigin::new(x, baseline as i32),
            color,
            clip: Some(ink_room(tall)),
        },
    )
}

/// The size digits `size` pixels large are drawn in: smaller where the
/// font has sizes up to a limit (packs of small devices).
///
/// # Errors
/// What the font says.
pub fn effective(font: &Font<'_>, size: f32) -> Result<f32, Error> {
    const PROBE: f32 = 20.0;
    let base = measure(font, PROBE, "0000")?;
    if base <= 0.0 {
        return Ok(size);
    }
    Ok((PROBE * measure(font, size, "0000")? / base).min(size))
}

/// `x` rounded up to whole pixels.
#[must_use]
pub fn ceil(x: f32) -> u16 {
    let whole = clamp_u16(x.max(0.0) as i64);
    if f32::from(whole) < x {
        whole.saturating_add(1)
    } else {
        whole
    }
}

/// Where the ink of `symbol` (by day or night) drawn in the square
/// `side` pixels large lies in it: left, top, width, height in pixels.
#[must_use]
pub fn ink_of(symbol: Symbol, day: bool, side: u16) -> (i32, i32, u16, u16) {
    let (left, top, right, bottom) = native_symbol::ink_box(symbol, day);
    let at = |units: i32| i32::from(side) * units / 1000;
    let (x, y) = (at(left), at(top));
    let span = |to: i32, from: i32| clamp_u16(i64::from(at(to) - from));
    (x, y, span(right, x), span(bottom, y))
}

/// Draws `symbol` (by day or night) into the square `area` with its ink in
/// the middle of it, rather than where its shapes lie in their square (most
/// leave more room on one side than the other).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn draw_symbol(
    out: &mut Vec<Paint>,
    area: Rect,
    (symbol, day): (Symbol, bool),
    style: Style,
    color: Color,
) -> Result<(), Error> {
    let side = area.width.min(area.height);
    let (x, y, w, h) = ink_of(symbol, day, side);
    let dx = (i32::from(side) - i32::from(w)) / 2 - x;
    let dy = (i32::from(side) - i32::from(h)) / 2 - y;
    let at = Rect::new(
        clamp_u16(i64::from(area.x) + i64::from(dx)),
        clamp_u16(i64::from(area.y) + i64::from(dy)),
        side,
        side,
    );
    native_symbol::draw(out, symbol, day, at, style, color)
}

/// The short form of what a widget says while it has no weather, for
/// where the whole sentence does not fit.
const fn short_key(waiting: Waiting) -> &'static str {
    match waiting {
        Waiting::NoPlace => "weather.no-place.short",
        Waiting::Fetching => "weather.fetching.short",
        Waiting::Failed => "weather.failed.short",
        Waiting::NoNetwork => "weather.no-network.short",
    }
}

/// What a widget says while it has no weather: why, under a faint symbol
/// where there is room, in `place`'s area (the widget's whole place, its
/// frame painted already). Where there is no network and the place is set
/// (`place` is its name), the sentence names it. Where the sentence does not
/// fit whole, the symbol goes, then the sentence gives way to its short form.
///
/// # Errors
/// `NoMemory`, or what the host says.
pub fn draw_waiting_for(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    place: Rect,
    waiting: Waiting,
    name: &str,
) -> Result<(), Error> {
    let look = cx.shape.look(place);
    let area = look.area;
    let (text, small, gap) = {
        let style = cx.style();
        (style.text, style.small, style.gap)
    };
    let align = cx.settings.align()?;
    let long = match waiting {
        // The place is set: its name with the message.
        Waiting::NoNetwork if !name.is_empty() => cx.format(
            "weather.no-network-for",
            &[Arg::Text {
                name: "place",
                value: name,
            }],
        )?,
        waiting => cx.tr(waiting.key())?,
    };
    let short = cx.tr(short_key(waiting))?;
    let line = line_px(&cx.font, text)?;
    // The short form alone may take half the spacing above and below: the
    // lines' own room above and below the letters keeps them clear of the
    // edges (two lines fit a low place then).
    let lines = Rect::new(area.x, place.y, area.width, place.height);
    let tall = ((u32::from(area.height) + u32::from(place.height)) / 2) as u16;
    // The symbol's ink at least this high, else none.
    let least = line.saturating_mul(3) / 2;
    let below = area.height.saturating_sub(least.saturating_add(gap));
    let lay = |text_: &str, height: u16| {
        Wrapped::fitting(&cx.font, text_, (text, small), (area.width, height), false)
    };
    let tries: [(&str, u16, bool); 3] = [
        (&long, below, true),
        (&long, area.height, false),
        (&short, below, true),
    ];
    let mut chosen = None;
    for (words, height, symbol) in tries {
        if height == 0 {
            continue;
        }
        let laid = lay(words, height)?;
        if laid.whole() {
            chosen = Some((laid, symbol));
            break;
        }
    }
    let (words, symbol) = match chosen {
        Some(found) => found,
        None => (lay(&short, tall)?, false),
    };
    let words_h = words.height(&cx.font)?;
    let (left, top, right, bottom) = native_symbol::ink_box(Symbol::PartlyCloudy, true);
    // The ink as high as the room left (three lines at most), not wider
    // than half the area; its square larger by what it leaves blank.
    let room = area
        .height
        .saturating_sub(words_h.saturating_add(gap))
        .min(line.saturating_mul(3));
    let high = u32::try_from(bottom - top).unwrap_or(1).max(1);
    let wide = u32::try_from(right - left).unwrap_or(1).max(1);
    let by_width = clamp_u16(i64::from(u32::from(area.width / 2) * high / wide));
    let ink_h = room.min(by_width);
    if !symbol || ink_h < least {
        words.draw(out, &cx.font, lines, align, look.ink)?;
        return Ok(());
    }
    draw_waiting_symbol(out, &cx.font, &look, align, (&words, ink_h), gap)
}

/// The faint symbol `ink_h` high over `words`, the two centred together in
/// `look`'s area, placed as `align` says.
fn draw_waiting_symbol(
    out: &mut Vec<Paint>,
    font: &Font<'_>,
    look: &Look,
    align: Align,
    (words, ink_h): (&Wrapped, u16),
    gap: u16,
) -> Result<(), Error> {
    let area = look.area;
    let words_h = words.height(font)?;
    let (left, top, right, bottom) = native_symbol::ink_box(Symbol::PartlyCloudy, true);
    let high = (bottom - top).max(1);
    let size = clamp_u16(i64::from(ink_h) * 1000 / i64::from(high));
    let unit = |v: i32| i32::from(size) * v / 1000;
    let ink_w = unit(right - left);
    let total = u32::from(ink_h) + u32::from(gap) + u32::from(words_h);
    let y = i64::from(area.y) + i64::from(u32::from(area.height).saturating_sub(total) / 2);
    let x = align.x(area, ink_w as f32) as i32;
    // The square placed so that its ink, not the square, sits there.
    let at = Rect::new(
        clamp_u16(i64::from(x - unit(left))),
        clamp_u16(y - i64::from(unit(top))),
        size,
        size,
    );
    native_symbol::draw(out, Symbol::PartlyCloudy, true, at, Style::Line, look.faint)?;
    let rest = Rect::new(
        area.x,
        clamp_u16(y + i64::from(ink_h) + i64::from(gap)),
        area.width,
        words_h,
    );
    words.draw(out, font, rest, align, look.ink)?;
    Ok(())
}

/// A time of day (minutes after midnight) as the clock shows it.
///
/// # Errors
/// `NoMemory`, or what the host says.
pub fn clock(cx: &mut Cx<'_>, minutes: u16) -> Result<String, Error> {
    let hour = u8::try_from(minutes / 60 % 24).unwrap_or(0);
    let minute = u8::try_from(minutes % 60).unwrap_or(0);
    cx.clock_of(hour, minute)
}

/// The unit of temperatures chosen.
///
/// # Errors
/// What the settings say.
pub fn temperature_unit(cx: &Cx<'_>) -> Result<Temperature, Error> {
    text_of(cx.host, "weather.temperature", Temperature::of)
}

/// The unit of wind speeds chosen.
///
/// # Errors
/// What the settings say.
pub fn wind_unit(cx: &Cx<'_>) -> Result<Wind, Error> {
    text_of(cx.host, "weather.wind", Wind::of)
}

/// How old the weather is, where it is old (`age` seconds): in hours, from
/// two days in days.
///
/// # Errors
/// `NoMemory`, or what the host says.
pub fn age_text(cx: &mut Cx<'_>, age: Option<i64>) -> Result<Option<String>, Error> {
    let Some(age) = age else {
        return Ok(None);
    };
    let hours = age / 3600;
    Ok(Some(if hours < 48 {
        cx.plural("weather.age.hours", hours, &[])?
    } else {
        cx.plural("weather.age.days", hours / 24, &[])?
    }))
}
