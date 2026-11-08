//! Text laid out in a widget's place: sized to fit a width, placed in a row,
//! centred by its ink, broken between words, packed into lines, stacked.
//! The results are [`Paint`] operations; every allocation is fallible.
use super::{look::Align, text::largest_size};
use crate::{Paint, paint, text};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Rect,
    native::{
        Error,
        fonts::{Font, TextOrigin},
    },
};

/// The height of a line of text `size` pixels large, as a float.
///
/// # Errors
/// Font refusals.
pub fn line(font: &Font<'_>, size: f32) -> Result<f32, Error> {
    crate::geometry::line_height(font, size)
}

/// The height of digits and capitals at `size`, about: most fonts' digits are
/// about three quarters of their ascent.
///
/// # Errors
/// Font refusals.
pub fn cap_height(font: &Font<'_>, size: f32) -> Result<f32, Error> {
    Ok(crate::geometry::line_metrics(font, size)?.0 * 0.74)
}

/// The size at which `text` is `width` pixels wide at most, and `size` at
/// most, measured as it is drawn; where none does, the smallest tried.
///
/// # Errors
/// Font refusals.
pub fn fit_size(font: &Font<'_>, size: f32, text: &str, width: f32) -> Result<f32, Error> {
    let least = (size * 0.1).max(1.0);
    Ok(largest_size(size, least, |s| Ok(font.measure_text(s, text)? <= width))?.unwrap_or(least))
}

/// Draws `text` `size` pixels large in the row `row`, placed as `align` says,
/// shortened with an ellipsis where it is wider; vertically centred by its line.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn label(
    out: &mut Vec<Paint>,
    font: &Font<'_>,
    (size, align): (f32, Align),
    row: Rect,
    text: &str,
    color: Color,
) -> Result<(), Error> {
    let shown = text::fit(font, size, text, f32::from(row.width))?;
    let x = align.x(row, font.measure_text(size, &shown)?);
    let (ascent, descent, _) = crate::geometry::line_metrics(font, size)?;
    let baseline =
        i32::from(row.y).saturating_add(((f32::from(row.height) + ascent - descent) / 2.0) as i32);
    paint::push(
        out,
        Paint::Text {
            text: shown,
            size,
            origin: TextOrigin::new(x, baseline),
            color,
            clip: Some(super::text::ink_room(row)),
        },
    )
}

/// Draws `text` with the middle of its digits (or capitals) at `y` and its
/// middle at `x`: centred by its ink, not by its line.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn centred(
    out: &mut Vec<Paint>,
    font: &Font<'_>,
    size: f32,
    (x, y): (f32, f32),
    text: &str,
    color: Color,
) -> Result<(), Error> {
    let width = font.measure_text(size, text)?;
    let baseline = y + cap_height(font, size)? / 2.0;
    paint::push(
        out,
        Paint::Text {
            text: text::copy(text)?,
            size,
            origin: TextOrigin::new(x - width / 2.0, baseline as i32),
            color,
            clip: None,
        },
    )
}

/// The size of each of `n` parts side by side in `room` pixels with `gap`
/// between them: nothing where the gaps take all the room.
#[must_use]
pub fn share(room: u16, gap: u16, n: u16) -> u16 {
    room.saturating_sub(gap.saturating_mul(n.saturating_sub(1))) / n.max(1)
}

/// `text` broken between words into lines `width` pixels wide at `size`
/// (never inside a word), and whether a word is wider than a line alone.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn break_words(
    font: &Font<'_>,
    size: f32,
    text: &str,
    width: f32,
) -> Result<(Vec<String>, bool), Error> {
    let mut lines: Vec<String> = Vec::new();
    let mut too_wide = false;
    let mut current = String::new();
    for word in text.split_whitespace() {
        if font.measure_text(size, word)? > width {
            too_wide = true;
        }
        if current.is_empty() {
            push_str(&mut current, word)?;
            continue;
        }
        let longer = text::concat(&[&current, " ", word])?;
        if font.measure_text(size, &longer)? <= width {
            current = longer;
        } else {
            lines.try_reserve(1).map_err(|_| Error::NoMemory)?;
            lines.push(core::mem::replace(&mut current, text::copy(word)?));
        }
    }
    if !current.is_empty() {
        lines.try_reserve(1).map_err(|_| Error::NoMemory)?;
        lines.push(current);
    }
    Ok((lines, too_wide))
}

fn push_str(target: &mut String, text: &str) -> Result<(), Error> {
    target
        .try_reserve(text.len())
        .map_err(|_| Error::NoMemory)?;
    target.push_str(text);
    Ok(())
}

/// `lines[from..]` joined with spaces.
fn join_from(lines: &[String], from: usize) -> Result<String, Error> {
    let mut joined = String::new();
    for (n, part) in lines.iter().skip(from).enumerate() {
        if n > 0 {
            push_str(&mut joined, " ")?;
        }
        push_str(&mut joined, part)?;
    }
    Ok(joined)
}

/// `text` broken between words into lines `width` pixels wide at `size`,
/// `most` at most, the last one shortened where there is more; a word wider
/// than a line ends the text there, shortened.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn wrap(
    font: &Font<'_>,
    size: f32,
    text: &str,
    width: f32,
    most: usize,
) -> Result<Vec<String>, Error> {
    let (mut lines, _) = break_words(font, size, text, width)?;
    let most = most.max(1);
    let mut wide = None;
    for (n, held) in lines.iter().enumerate() {
        if font.measure_text(size, held)? > width {
            wide = Some(n);
            break;
        }
    }
    if let Some(wide) = wide {
        let rest = join_from(&lines, wide)?;
        lines.truncate(wide);
        lines.try_reserve(1).map_err(|_| Error::NoMemory)?;
        lines.push(rest);
    }
    if lines.len() > most {
        let rest = join_from(&lines, most - 1)?;
        lines.truncate(most - 1);
        lines.try_reserve(1).map_err(|_| Error::NoMemory)?;
        lines.push(rest);
    }
    if let Some(last) = lines.last_mut() {
        let shown = if font.measure_text(size, last.as_str())? > width {
            Some(text::fit(font, size, last.as_str(), width)?)
        } else {
            None
        };
        if let Some(shown) = shown {
            *last = shown;
        }
    }
    Ok(lines)
}

/// `text` whole in `most` lines at most, `width` pixels wide and `height`
/// pixels high, broken between words: at the largest size from `max` down to
/// `min` at which it fits so; `None` where it does not even at `min`.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn wrap_whole(
    font: &Font<'_>,
    (max, min): (f32, f32),
    text: &str,
    (width, height): (f32, f32),
    most: usize,
) -> Result<Option<(f32, Vec<String>)>, Error> {
    let Some(size) = largest_size(max, min, |size| {
        let (lines, too_wide) = break_words(font, size, text, width)?;
        Ok(!too_wide
            && lines.len() <= most.max(1)
            && line(font, size)? * lines.len() as f32 <= height)
    })?
    else {
        return Ok(None);
    };
    Ok(Some((size, break_words(font, size, text, width)?.0)))
}

/// `items` (e.g. "Day 287 of 365", "Week 42") in lines `width` pixels wide at
/// `size`, separated by " · " within a line, `most` lines at most: an item is
/// not broken where it can go whole to the next line, and one wider than a
/// line is broken between its words; what does not fit is shortened on the
/// last line.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn pack(
    font: &Font<'_>,
    size: f32,
    items: &[String],
    width: f32,
    most: usize,
) -> Result<Vec<String>, Error> {
    let most = most.max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut pieces: Vec<(String, bool)> = Vec::new();
    for item in items {
        if font.measure_text(size, item)? <= width {
            pieces.try_reserve(1).map_err(|_| Error::NoMemory)?;
            pieces.push((text::copy(item)?, true));
        } else {
            let (parts, _) = break_words(font, size, item, width)?;
            for (n, part) in parts.into_iter().enumerate() {
                pieces.try_reserve(1).map_err(|_| Error::NoMemory)?;
                pieces.push((part, n == 0));
            }
        }
    }
    let mut cut = false;
    for (piece, starts_item) in pieces {
        let n = lines.len();
        if n > 0 {
            let sep = if starts_item { " · " } else { " " };
            let joined = text::concat(&[lines.get(n - 1).map_or("", String::as_str), sep, &piece])?;
            if font.measure_text(size, &joined)? <= width {
                if let Some(last) = lines.get_mut(n - 1) {
                    *last = joined;
                }
                continue;
            }
            if n >= most {
                if let Some(last) = lines.get_mut(n - 1) {
                    *last = joined;
                }
                cut = true;
                break;
            }
        }
        lines.try_reserve(1).map_err(|_| Error::NoMemory)?;
        lines.push(piece);
    }
    for held in &mut lines {
        if cut || font.measure_text(size, held.as_str())? > width {
            let shown = text::fit(font, size, held.as_str(), width)?;
            *held = shown;
        }
    }
    Ok(lines)
}

/// A part of a stack of text ([`stack`]): its lines, their size and colour.
#[derive(Debug)]
pub struct Part<'a> {
    pub lines: Vec<&'a str>,
    pub size: f32,
    pub color: Color,
}

/// How far the ink of `part` reaches above its first baseline and below its
/// last (its lines a line height apart), in pixels (above negative).
fn part_ink(font: &Font<'_>, part: &Part<'_>) -> Result<(f32, f32), Error> {
    let pitch = line(font, part.size)?;
    let ink = |text: &str| -> Result<(f32, f32), Error> {
        Ok(match font.ink(part.size, text)? {
            Some(bounds) => {
                let (top, bottom) = crate::geometry::check_ink(bounds.top, bounds.bottom)?;
                (top as f32, bottom as f32)
            }
            None => {
                let (ascent, descent, _) = crate::geometry::line_metrics(font, part.size)?;
                (-ascent, -descent)
            }
        })
    };
    let first = match part.lines.first() {
        Some(held) => ink(held)?,
        None => (0.0, 0.0),
    };
    let last = match part.lines.last() {
        Some(held) => ink(held)?,
        None => (0.0, 0.0),
    };
    let below = pitch * part.lines.len().saturating_sub(1) as f32 + last.1;
    Ok((first.0, below))
}

/// The height the ink of `parts` takes stacked with `gap` between them.
///
/// # Errors
/// Font refusals.
pub fn stack_height(font: &Font<'_>, parts: &[Part<'_>], gap: f32) -> Result<f32, Error> {
    let mut inked = 0.0;
    for part in parts {
        let (top, bottom) = part_ink(font, part)?;
        inked += bottom - top;
    }
    Ok(inked + gap * parts.len().saturating_sub(1) as f32)
}

/// Draws `parts` one below the other, `gap` pixels between the ink of one and
/// the next, the whole in the middle of `area` by its ink from top to bottom,
/// each line placed as `align` says; the last parts are left out where they
/// have no room after all.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn stack(
    out: &mut Vec<Paint>,
    font: &Font<'_>,
    area: Rect,
    align: Align,
    parts: &[Part<'_>],
    gap: f32,
) -> Result<(), Error> {
    let mut count = parts.len();
    while count > 0
        && stack_height(font, parts.get(..count).unwrap_or(&[]), gap)? > f32::from(area.height)
    {
        count -= 1;
    }
    let parts = parts.get(..count).unwrap_or(&[]);
    let total = stack_height(font, parts, gap)?;
    let mut top = f32::from(area.y) + (f32::from(area.height) - total).max(0.0) / 2.0;
    let width = f32::from(area.width);
    for part in parts {
        let (above, below) = part_ink(font, part)?;
        let pitch = line(font, part.size)?;
        let mut baseline = top - above;
        for held in &part.lines {
            let shown = if font.measure_text(part.size, held)? > width {
                text::fit(font, part.size, held, width)?
            } else {
                text::copy(held)?
            };
            let wide = font.measure_text(part.size, &shown)?;
            if wide <= width {
                paint::push(
                    out,
                    Paint::Text {
                        text: shown,
                        size: part.size,
                        origin: TextOrigin::new(align.x(area, wide), (baseline + 0.5) as i32),
                        color: part.color,
                        clip: None,
                    },
                )?;
            }
            baseline += pitch;
        }
        top += below - above + gap;
    }
    Ok(())
}
