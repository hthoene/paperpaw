//! Where the parts of *Forecast* go, without a host: how many days, how
//! large the digits are, what is left out where there is no room. The
//! interface-6 code is `forecast.rs`; what it decided is kept, only that a
//! font refusal ends the step with an error.
use alloc::string::String;

use paperpaw_sdk::native::Error;

use crate::native_draw::{ceil, digits_height, digits_size};
use crate::native_now::layout::Fonts;

/// What the style and the settings say about the layout.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct Setup {
    /// The size of text.
    pub text: f32,
    /// The size of small text.
    pub small: f32,
    /// The size of a title.
    pub title: f32,
    /// The gap between parts.
    pub gap: u16,
    /// The width of a rule.
    pub rule: u16,
    /// Six millimetres in pixels.
    pub mm6: u16,
    /// The temperatures' size as set; `None` for "as large as it fits".
    pub fixed: Option<f32>,
}

fn sane(size: f32) -> f32 {
    crate::native_draw::sane(size)
}

/// The days that go in a row: as many as have room for their numbers.
///
/// # Errors
/// What the font says.
pub fn row_days(fonts: &dyn Fonts, set: &Setup, width: u16, len: usize) -> Result<usize, Error> {
    let least = fonts.width(sane(set.small), "-88°")? * 1.4;
    let fit = ((f32::from(width) / least.max(1.0)) as usize).max(1);
    Ok(len.min(fit))
}

/// The days that go in a list: as many as have rows a line of small text high.
///
/// # Errors
/// What the font says.
pub fn list_days(fonts: &dyn Fonts, set: &Setup, height: u16, len: usize) -> Result<usize, Error> {
    let least = fonts.line(set.small)?.max(1);
    Ok(len.min(usize::from(height / least).max(1)))
}

/// Whether the days go in a list: as set, or where the place is tall or too
/// narrow for the columns.
#[must_use]
pub fn as_list(layout: Option<usize>, width: u16, height: u16, count: usize) -> bool {
    match layout {
        Some(1) => false,
        Some(2) => true,
        _ => {
            u32::from(height) * 10 > u32::from(width) * 12
                || u32::from(width) < u32::try_from(count).unwrap_or(u32::MAX).saturating_mul(40)
        }
    }
}

/// How the days are laid out as columns.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Row {
    /// The width of a column.
    pub column: u16,
    /// The highest temperature's size.
    pub high: f32,
    /// The lowest temperature's size.
    pub low: f32,
    /// The weekday's size.
    pub name: f32,
    /// The lowest is below the highest.
    pub with_low: bool,
    /// The lowest is beside the highest (no room below).
    pub low_beside: bool,
    /// The heights of the name, the highest, the lowest and the chance of rain.
    pub name_h: u16,
    /// The height of the highest.
    pub high_h: u16,
    /// The height of the lowest (0 if none below).
    pub low_h: u16,
    /// The height of the chance of rain (0 if none).
    pub rain_h: u16,
    /// The symbol's side (0: none).
    pub side: u16,
    /// The height of the whole column.
    pub total: u16,
    /// Where it starts.
    pub top: u16,
    /// Half the gap.
    pub gap: u16,
}

/// What a column shows, as set and as the days have it.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct Parts {
    /// The symbol.
    pub symbol: bool,
    /// The lowest.
    pub low: bool,
    /// Some day has a chance of rain to show.
    pub rain: bool,
}

/// The columns for `count` days in a place `width` by `height`.
///
/// # Errors
/// What the font says.
pub fn row(
    fonts: &dyn Fonts,
    set: &Setup,
    (width, height): (u16, u16),
    count: usize,
    parts: Parts,
) -> Result<Row, Error> {
    let count = u16::try_from(count.max(1)).unwrap_or(u16::MAX);
    let column = width / count;
    let gap = set.gap / 2;
    let mut with_low = parts.low;
    let rain_h = if parts.rain { fonts.line(set.small)? } else { 0 };
    // The temperatures as large as the column allows, as set, and smaller
    // where the column is low; the lowest left out where even the smallest do
    // not fit.
    let probe = sane(set.title);
    let widest = (fonts.width(probe, "-88°")? / probe).max(0.1);
    let fit = (f32::from(column) * 0.85 / widest).min(set.title * 1.5);
    let mut high = set.fixed.unwrap_or(fit).min(fit).max(set.small);
    let name_of = |high: f32| {
        if high > set.title * 1.2 {
            set.text
        } else {
            set.small
        }
    };
    let low_of = |high: f32| (high * 0.8).max(set.small);
    let words_of = |high: f32, low: bool| -> Result<u16, Error> {
        let mut words = fonts
            .line(name_of(high))?
            .saturating_add(digits_height(fonts.effective(high)?))
            .saturating_add(rain_h);
        if low {
            words = words.saturating_add(digits_height(fonts.effective(low_of(high))?));
        }
        Ok(words)
    };
    while words_of(high, with_low)? > height && high > set.small * 0.8 {
        high -= 1.0;
    }
    // The symbols rather than the largest digits: smaller digits where that
    // leaves room for them.
    if parts.symbol {
        // As the symbol is placed below: at least a name's height.
        let room = |high: f32| -> Result<bool, Error> {
            let name = fonts.line(name_of(high))?;
            Ok(height.saturating_sub(words_of(high, with_low)?.saturating_add(gap * 2)) >= name)
        };
        let mut smaller = high;
        while !room(smaller)? && smaller > set.text {
            smaller -= 1.0;
        }
        if room(smaller)? {
            high = smaller;
        }
    }
    if with_low && words_of(high, true)? > height {
        with_low = false;
    }
    // Without room below, the lowest beside the highest.
    let low_beside = !with_low && parts.low;
    let high = fonts.effective(high)?;
    let low = fonts.effective(low_of(high))?;
    let name = name_of(high);
    let name_h = fonts.line(name)?;
    let high_h = digits_height(high);
    let low_h = if with_low { digits_height(low) } else { 0 };
    let words = name_h
        .saturating_add(high_h)
        .saturating_add(low_h)
        .saturating_add(rain_h);
    let side = if parts.symbol {
        let room = height.saturating_sub(words.saturating_add(gap * 2));
        let wide = u16::try_from(u32::from(column) * 3 / 4).unwrap_or(u16::MAX);
        room.min(wide)
    } else {
        0
    };
    let side = if side < name_h { 0 } else { side };
    let total = words
        .saturating_add(side)
        .saturating_add(if side > 0 { gap * 2 } else { 0 });
    Ok(Row {
        column,
        high,
        low,
        name,
        with_low,
        low_beside,
        name_h,
        high_h,
        low_h,
        rain_h,
        side,
        total,
        top: height.saturating_sub(total) / 2,
        gap,
    })
}

/// What the list shows of each day, as text.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct Texts<'a> {
    /// The highest of each day.
    pub highs: &'a [String],
    /// The lowest of each day.
    pub lows: &'a [String],
    /// The chance of rain of each day, if shown.
    pub rains: &'a [Option<String>],
    /// The names of the weekdays: as set, then short, then the initials.
    pub names: [&'a [String]; 3],
}

/// What the list shows, as set.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct Wants {
    /// The symbol.
    pub symbol: bool,
    /// The lowest.
    pub low: bool,
    /// The bars.
    pub bars: bool,
}

/// How the days are laid out as rows: x positions are from the area's left.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct List {
    /// The height of a row.
    pub height: u16,
    /// The symbol's side (0: none).
    pub side: u16,
    /// The width of the chances of rain (0: none).
    pub rain_w: u16,
    /// The temperatures' size.
    pub size: f32,
    /// The size of the chances of rain.
    pub small: f32,
    /// The widths of the name, the highest and the lowest (0: none).
    pub name_w: u16,
    /// The width of the highest.
    pub high_w: u16,
    /// The width of the lowest.
    pub low_w: u16,
    /// Which of the names (0 as set, 1 short, 2 initials).
    pub names: usize,
    /// Where the symbol is.
    pub symbol_x: u16,
    /// Where the chances of rain end, less their width.
    pub rain_x: u16,
    /// Where the lowest starts.
    pub low_x: u16,
    /// Where the highest starts.
    pub high_x: u16,
    /// Whether the bars show, and where they start and how wide they are.
    pub bars: Option<(u16, u16)>,
}

fn widest<'a>(
    fonts: &dyn Fonts,
    size: f32,
    texts: impl Iterator<Item = &'a str>,
) -> Result<u16, Error> {
    let mut most = 0.0f32;
    for text in texts {
        most = most.max(fonts.width(sane(size), text)?);
    }
    Ok(ceil(most))
}

/// The rows for the days in a place `width` by `height`.
///
/// # Errors
/// What the font says.
pub fn list(
    fonts: &dyn Fonts,
    set: &Setup,
    (width, height): (u16, u16),
    texts: &Texts<'_>,
    wants: Wants,
) -> Result<List, Error> {
    let count = u16::try_from(texts.highs.len().max(1)).unwrap_or(u16::MAX);
    let height = height / count;
    let gap = set.gap;
    let mut side = if wants.symbol {
        ((f32::from(height) * 0.8) as u16).min(width / 6)
    } else {
        0
    };
    let mut rain_w = widest(
        fonts,
        set.small,
        texts.rains.iter().flatten().map(String::as_str),
    )?;
    // The text as large as the rows allow, as set, and as the width allows:
    // the names shorter where they would not fit.
    let by_height = digits_size(f32::from(height) * 0.55).min(set.title * 1.1);
    let wanted = set
        .fixed
        .unwrap_or(by_height)
        .min(by_height.max(set.small));
    let probe = sane(set.title);
    let fixed = f32::from(side) + f32::from(rain_w) + f32::from(gap) * 4.0;
    let mut size = set.small;
    let mut chosen = 2;
    for (variant, names) in texts.names.iter().enumerate() {
        let at_probe = f32::from(widest(fonts, probe, names.iter().map(String::as_str))?)
            + f32::from(widest(fonts, probe, texts.highs.iter().map(String::as_str))?)
            + if wants.low {
                f32::from(widest(fonts, probe, texts.lows.iter().map(String::as_str))?)
            } else {
                0.0
            };
        let by_width = (f32::from(width) - fixed) * probe / at_probe.max(1.0);
        size = wanted.min(by_width);
        chosen = variant;
        if size >= set.small {
            break;
        }
    }
    let size = fonts.effective(size.max(set.small * 0.8))?;
    let small = set.small.min(size);
    let names = texts.names.get(chosen).copied().unwrap_or(&[]);
    let name_w = widest(fonts, size, names.iter().map(String::as_str))?;
    let high_w = widest(fonts, size, texts.highs.iter().map(String::as_str))?;
    let mut low_w = if wants.low {
        widest(fonts, size, texts.lows.iter().map(String::as_str))?
    } else {
        0
    };
    // Where the parts do not fit across, the symbol goes, then the chance of
    // rain, then the lowest.
    let after = |w: u16| if w > 0 { w.saturating_add(gap) } else { 0 };
    let needs = |side: u16, rain_w: u16, low_w: u16| {
        name_w
            .saturating_add(gap)
            .saturating_add(after(side))
            .saturating_add(after(rain_w))
            .saturating_add(after(low_w))
            .saturating_add(high_w)
    };
    if needs(side, rain_w, low_w) > width {
        side = 0;
    }
    if needs(side, rain_w, low_w) > width {
        rain_w = 0;
    }
    if needs(side, rain_w, low_w) > width {
        low_w = 0;
    }
    // From the left: name, symbol, rain; from the right: highest, bar, lowest.
    let mut x = name_w.saturating_add(gap);
    let symbol_x = x;
    if side > 0 {
        x = x.saturating_add(side).saturating_add(gap);
    }
    let rain_x = x;
    if rain_w > 0 {
        x = x.saturating_add(rain_w).saturating_add(gap);
    }
    let high_x = width.saturating_sub(high_w);
    let room = high_x.saturating_sub(x.saturating_add(low_w).saturating_add(gap * 2));
    let bars = wants.bars && low_w > 0 && room >= (gap * 2).max(set.mm6);
    let low_x = if bars {
        x
    } else {
        high_x.saturating_sub(low_w.saturating_add(gap))
    };
    Ok(List {
        height,
        side,
        rain_w,
        size,
        small,
        name_w,
        high_w,
        low_w,
        names: chosen,
        symbol_x,
        rain_x,
        low_x,
        high_x,
        bars: bars.then(|| (x.saturating_add(low_w).saturating_add(gap), room)),
    })
}
