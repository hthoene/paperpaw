//! Where the parts of *Weather now* go, without a host: which lines there
//! are room for, where the symbol sits and how large the temperature is.
//! The interface-6 code is `now.rs`; what it decided is kept, only that a
//! refused allocation or a font refusal ends the step with an error.
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::{Rect, native::Error};
use paperpaw_ui_kit::text;

use crate::native_draw::{cap, digits_height, digits_size};

/// What the layout asks of the font.
pub trait Fonts {
    /// How wide `text` is at `size` pixels.
    ///
    /// # Errors
    /// What the font says.
    fn width(&self, size: f32, text: &str) -> Result<f32, Error>;
    /// The height of a line of text `size` pixels large.
    ///
    /// # Errors
    /// What the font says.
    fn line(&self, size: f32) -> Result<u16, Error>;
    /// `size` as the device's font can draw digits (smaller where its sizes end).
    ///
    /// # Errors
    /// What the font says.
    fn effective(&self, size: f32) -> Result<f32, Error>;
}

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
    /// The temperature's size as set; `None` for "as large as it fits".
    pub fixed: Option<f32>,
    /// The symbol's size setting (an index of `SYMBOLS`; 0 large, 1 small, 2 none).
    pub symbol: usize,
    /// Where the symbol sits (an index of `PLACINGS`; 0 auto, 1 beside, 2 above).
    pub placing: usize,
}

impl Setup {
    /// A size usable as a divisor and a font size.
    fn title(&self) -> f32 {
        crate::native_draw::sane(self.title)
    }
}

/// A line of text: what it says, how large, in which colour, and how soon it
/// is left out where there is no room (lower first).
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub struct Line {
    /// What it says.
    pub text: String,
    /// How large, in pixels.
    pub size: f32,
    /// Drawn in the faint colour.
    pub faint: bool,
    /// How important it is (the higher, the later it is left out).
    pub rank: u8,
}

impl Line {
    /// A line.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new(text: &str, size: f32, faint: bool, rank: u8) -> Result<Self, Error> {
        Ok(Self {
            text: text::copy(text)?,
            size,
            faint,
            rank,
        })
    }

    fn dup(&self) -> Result<Self, Error> {
        Self::new(&self.text, self.size, self.faint, self.rank)
    }
}

/// Where the symbol goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Placing {
    /// Beside the text.
    Beside,
    /// Above the text.
    Above,
    /// Small, before the temperature.
    Inline,
    /// No symbol.
    None,
}

/// What is drawn where: the symbol's square, the temperature's size, the
/// lines kept, and the text's place.
#[derive(Debug)]
#[non_exhaustive]
pub struct Plan {
    /// Where the symbol is.
    pub placing: Placing,
    /// The symbol's square.
    pub symbol: Rect,
    /// The temperature's size.
    pub size: f32,
    /// The first of `lines` is the caption above the temperature.
    pub caption: bool,
    /// The lines kept (the caption first, if any).
    pub lines: Vec<Line>,
    /// The text's place.
    pub text: Rect,
    /// The temperature fits the text's width.
    pub wide_enough: bool,
}

/// `value` times `n` over `d`, for `n` below `d`.
fn part(value: u16, n: u32, d: u32) -> u16 {
    u16::try_from(u32::from(value) * n / d).unwrap_or(u16::MAX)
}

fn put<T>(list: &mut Vec<T>, item: T) -> Result<(), Error> {
    list.try_reserve(1).map_err(|_| Error::NoMemory)?;
    list.push(item);
    Ok(())
}

/// The height of the text: the temperature and the lines.
///
/// # Errors
/// What the font says.
pub fn text_height(fonts: &dyn Fonts, size: f32, lines: &[Line]) -> Result<u16, Error> {
    let mut total = digits_height(size);
    for line in lines {
        total = total.saturating_add(fonts.line(line.size)?);
    }
    Ok(total)
}

/// `lines` without those ranked below `least`, the details beside each
/// other where they fit `width` (" · " between them).
///
/// # Errors
/// `NoMemory`, or what the font says.
pub fn packed(
    fonts: &dyn Fonts,
    lines: &[Line],
    least: u8,
    width: f32,
) -> Result<Vec<Line>, Error> {
    let small = lines.iter().map(|l| l.size).fold(f32::MAX, f32::min);
    let mut out: Vec<Line> = Vec::new();
    for line in lines.iter().filter(|l| l.rank >= least) {
        if let Some(last) = out.last_mut()
            && last.rank < 5
            && last.size == line.size
            && last.faint == line.faint
            && line.rank < 3
            && last.rank < 3
        {
            let joined = text::format(format_args!("{} · {}", last.text, line.text))?;
            if fonts.width(line.size, &joined)? <= width {
                last.text = joined;
                last.rank = last.rank.max(line.rank);
                continue;
            }
        }
        let mut line = line.dup()?;
        // The condition smaller rather than cut short.
        if line.rank == 5 && fonts.width(line.size, &line.text)? > width {
            line.size = small;
        }
        // A line that does not fit even so is left out whole.
        if fonts.width(line.size, &line.text)? <= width {
            put(&mut out, line)?;
        }
    }
    Ok(out)
}

/// The small symbol before a temperature `size` pixels large: its side and
/// the room after it (`gap` and some).
#[must_use]
pub fn inline_symbol(size: f32, gap: u16) -> (u16, u16) {
    let side = (cap(size) * 1.25) as u16;
    (side, gap / 2 + side / 8)
}

/// How wide the temperature is, with the small symbol before it if `inline`.
///
/// # Errors
/// What the font says.
pub fn temperature_width(
    fonts: &dyn Fonts,
    size: f32,
    text: &str,
    inline: bool,
    gap: u16,
) -> Result<f32, Error> {
    let (side, after) = if inline {
        inline_symbol(size, gap)
    } else {
        (0, 0)
    };
    Ok(fonts.width(size, text)? + f32::from(side) + f32::from(after))
}

/// The largest temperature that fits `width` and `height` with `lines`, as
/// set or as large as fits (at most `most`).
fn fitting(
    fonts: &dyn Fonts,
    set: &Setup,
    text: &str,
    (lines, width, height): (&[Line], f32, u16),
    (most, inline): (f32, bool),
) -> Result<f32, Error> {
    if let Some(size) = set.fixed {
        return Ok(size);
    }
    let mut rest = 0u16;
    for line in lines {
        rest = rest.saturating_add(fonts.line(line.size)?);
    }
    let by_height = digits_size(f32::from(height.saturating_sub(rest)));
    let probe = set.title();
    let room = width - if inline { f32::from(set.gap / 2) } else { 0.0 };
    let wide = temperature_width(fonts, probe, text, inline, 0)?.max(1.0) / probe;
    let by_width = room / wide;
    Ok(by_height
        .min(by_width)
        .min(most)
        .max(set.small.min(by_width)))
}

/// Where the symbol goes as set; where it does not fit, the places to try
/// after it.
fn placings(set: &Setup, area: Rect) -> &'static [Placing] {
    use Placing::{Above, Beside, Inline, None};
    match set.symbol {
        2 => &[None],
        1 => &[Inline, None],
        _ => match set.placing {
            1 => &[Beside, Above, Inline, None],
            2 => &[Above, Inline, None],
            _ if u32::from(area.width) * 10 >= u32::from(area.height) * 13 => {
                &[Beside, Above, Inline, None]
            }
            _ => &[Above, Beside, Inline, None],
        },
    }
}

/// The plan: the first place of the symbol where the temperature is not too
/// small, with as many lines as fit.
///
/// # Errors
/// `NoMemory`, or what the font says.
pub fn plan(
    fonts: &dyn Fonts,
    set: &Setup,
    area: Rect,
    temperature: &str,
    all: &[Line],
) -> Result<Plan, Error> {
    // The least a temperature should be before lines (or the large symbol)
    // are left out; less where the font has no larger sizes.
    let largest = fonts.effective(400.0)?;
    let least = (set.title * 1.3).min(largest) * 0.9;
    let mut last = None;
    for &placing in placings(set, area) {
        let plan = plan_for(fonts, set, (area, temperature, all), placing, least)?;
        // A small symbol before the temperature, even smaller, rather than none.
        let enough = if placing == Placing::Inline {
            least * 0.5
        } else {
            least * 0.95
        };
        // A large symbol at least a line of text high, else not so.
        let seen = match placing {
            Placing::Beside | Placing::Above => plan.symbol.width >= fonts.line(set.text)?,
            _ => true,
        };
        if seen && ((plan.size >= enough && plan.wide_enough) || set.fixed.is_some()) {
            return Ok(plan);
        }
        last = Some(plan);
    }
    match last {
        Some(plan) => Ok(plan),
        None => plan_for(fonts, set, (area, temperature, all), Placing::None, least),
    }
}

/// The plan with the symbol placed so, as many lines left out as needed for
/// a temperature of at least `least`.
fn plan_for(
    fonts: &dyn Fonts,
    set: &Setup,
    (area, temperature, all): (Rect, &str, &[Line]),
    placing: Placing,
    least: f32,
) -> Result<Plan, Error> {
    let gap = set.gap;
    // Whether lines are left to leave out from `rank` on.
    let more = |rank: u8| all.iter().any(|l| l.rank >= rank && l.rank > 0) && rank < 6;
    let mut rank = 0u8;
    loop {
        let (side, text_area) = match placing {
            Placing::Beside => {
                let wide = part(area.width, 2, 5);
                let side = match set.fixed {
                    None => area.height.min(wide),
                    Some(size) => area.height.min(wide).min(digits_height(size).saturating_mul(2)),
                };
                let shift = side.saturating_add(gap);
                let text = Rect::new(
                    area.x.saturating_add(shift),
                    area.y,
                    area.width.saturating_sub(shift),
                    area.height,
                );
                (side, text)
            }
            Placing::Above => {
                let lines = packed(fonts, all, rank, f32::from(area.width))?;
                let words = text_height(fonts, set.fixed.unwrap_or(least), &lines)?;
                // What the symbol may take; too little: fewer lines.
                let room = area.height.saturating_sub(words.saturating_add(gap));
                if room < area.height / 4 && more(rank) {
                    rank += 1;
                    continue;
                }
                let side = match set.fixed {
                    None => room.min(part(area.height, 11, 20)),
                    Some(size) => room.min(digits_height(size).saturating_mul(2)),
                }
                .min(part(area.width, 3, 4));
                let shift = side.saturating_add(gap);
                let text = Rect::new(
                    area.x,
                    area.y.saturating_add(shift),
                    area.width,
                    area.height.saturating_sub(shift),
                );
                (side, text)
            }
            Placing::Inline | Placing::None => (0, area),
        };
        let packed_lines = packed(fonts, all, rank, f32::from(text_area.width))?;
        // The caption (rank 3) first, then the rest.
        let mut kept: Vec<Line> = Vec::new();
        let mut caption = false;
        for line in packed_lines.iter().filter(|l| l.rank == 3).take(1) {
            put(&mut kept, line.dup()?)?;
            caption = true;
        }
        for line in packed_lines.iter().filter(|l| l.rank != 3) {
            put(&mut kept, line.dup()?)?;
        }
        // Not larger than the symbol beside it, to keep them in balance.
        let most = match placing {
            Placing::Beside => (f32::from(side) * 0.55).max(least),
            Placing::Above => (f32::from(side) * 0.6).max(least),
            // Large, not huge.
            _ => (set.title * 3.5).min(f32::from(text_area.height)),
        };
        let size = fitting(
            fonts,
            set,
            temperature,
            (&kept, f32::from(text_area.width), text_area.height),
            (most, placing == Placing::Inline),
        )?;
        let size = fonts.effective(size)?;
        let wide = temperature_width(fonts, size, temperature, placing == Placing::Inline, gap)?;
        let fits = text_height(fonts, size, &kept)? <= text_area.height
            && wide <= f32::from(text_area.width)
            && (set.fixed.is_some() || size >= least * 0.95);
        if fits || !more(rank) {
            let symbol = match placing {
                Placing::Beside => Rect::new(
                    area.x,
                    area.y
                        .saturating_add(area.height.saturating_sub(side) / 2),
                    side,
                    side,
                ),
                Placing::Above => Rect::new(area.x, area.y, side, side),
                _ => Rect::new(0, 0, 0, 0),
            };
            return Ok(Plan {
                placing,
                symbol,
                size,
                caption,
                lines: kept,
                text: text_area,
                wide_enough: wide <= f32::from(text_area.width),
            });
        }
        rank += 1;
    }
}
