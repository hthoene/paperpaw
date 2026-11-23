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
/// (never inside a word), each line of the text (`\n`) a line of its own, and
/// whether a word is wider than a line alone.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn break_words(
    font: &Font<'_>,
    size: f32,
    text: &str,
    width: f32,
) -> Result<(Vec<String>, bool), Error> {
    let broken = break_lines(
        &mut |part| font.measure_text(size, part),
        text,
        width,
        (usize::MAX, false),
    )?;
    Ok(broken.unwrap_or_default())
}

/// The one breaker of [`break_words`], [`words_in_lines`] and the sizes tried
/// by [`wrap_whole`] and [`largest`]: `None` as soon as there are more than
/// `most` lines or, with `whole`, a word is wider than a line (the rest is not
/// measured). A word is measured alone, as it must be to tell whether it is
/// too wide; its line is measured with it only where the sum of their widths
/// lands near the edge (as [`text::wrap_with`] does), not for every word.
///
/// # Errors
/// What `measure` says and [`Error::NoMemory`].
fn break_lines(
    measure: &mut dyn FnMut(&str) -> Result<f32, Error>,
    text: &str,
    width: f32,
    (most, whole): (usize, bool),
) -> Result<Option<(Vec<String>, bool)>, Error> {
    let mut lines: Vec<String> = Vec::new();
    let mut too_wide = false;
    let mut space: Option<f32> = None;
    let slack = (width * text::NEAR_EDGE).max(0.0);
    // A text of one line and no words has no lines; an empty line between
    // others is one.
    let paragraphs = text.contains('\n');
    for paragraph in text.split('\n') {
        let mut current = String::new();
        let mut current_width = 0.0_f32;
        for word in paragraph.split_whitespace() {
            let word_width = measure(word)?;
            if word_width > width {
                if whole {
                    return Ok(None);
                }
                too_wide = true;
            }
            if current.is_empty() {
                push_str(&mut current, word)?;
                current_width = word_width;
                continue;
            }
            let space = match space {
                Some(space) => space,
                None => *space.insert(measure(" ")?),
            };
            let estimate = current_width + space + word_width;
            let joined = if estimate <= width - slack {
                Some(estimate)
            } else if estimate > width + slack {
                None
            } else {
                let longer = text::concat(&[&current, " ", word])?;
                Some(measure(&longer)?).filter(|joined| *joined <= width)
            };
            if let Some(joined) = joined {
                push_str(&mut current, " ")?;
                push_str(&mut current, word)?;
                current_width = joined;
            } else {
                if !push_line(&mut lines, core::mem::take(&mut current), most)? {
                    return Ok(None);
                }
                push_str(&mut current, word)?;
                current_width = word_width;
            }
        }
        if (!current.is_empty() || paragraphs) && !push_line(&mut lines, current, most)? {
            return Ok(None);
        }
    }
    Ok(Some((lines, too_wide)))
}

/// Adds `line` to `lines`; `false` where that makes more than `most`.
fn push_line(lines: &mut Vec<String>, line: String, most: usize) -> Result<bool, Error> {
    if lines.len() >= most {
        return Ok(false);
    }
    lines.try_reserve(1).map_err(|_| Error::NoMemory)?;
    lines.push(line);
    Ok(true)
}

/// How many lines of `size` stand in `height` pixels, a line's height apart,
/// `most` at most: the most `n` whose `n` lines are no higher than `height`;
/// `None` where not even none are (a height below 0).
///
/// # Errors
/// Font refusals.
fn lines_in(font: &Font<'_>, size: f32, height: f32, most: usize) -> Result<Option<usize>, Error> {
    let pitch = line(font, size)?;
    let high = |n: usize| pitch * n as f32 <= height;
    if !high(0) {
        return Ok(None);
    }
    if pitch <= 0.0 {
        return Ok(Some(most));
    }
    // The quotient, put right by a step where rounding left it off the
    // product the lines are checked by.
    let mut n = ((height / pitch) as usize).min(most);
    for _ in 0..2 {
        if n > 0 && !high(n) {
            n -= 1;
        } else if n < most && high(n + 1) {
            n += 1;
        }
    }
    Ok(Some(n))
}

fn push_str(target: &mut String, text: &str) -> Result<(), Error> {
    target
        .try_reserve(text.len())
        .map_err(|_| Error::NoMemory)?;
    target.push_str(text);
    Ok(())
}

/// `lines[from..]` joined with spaces (an empty line between lines of the
/// text adds none).
fn join_from(lines: &[String], from: usize) -> Result<String, Error> {
    let mut joined = String::new();
    for part in lines.iter().skip(from).filter(|part| !part.is_empty()) {
        if !joined.is_empty() {
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
    let (mut lines, too_wide) = break_words(font, size, text, width)?;
    let most = most.max(1);
    let mut wide = None;
    // Only a word alone is wider than its line: with none, no line is.
    for (n, held) in lines.iter().enumerate().filter(|_| too_wide) {
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
    // The lines of the size that fits are kept: the largest is the first.
    let mut fitting = None;
    let size = largest_size(max, min, |size| {
        let Some(most) = lines_in(font, size, height, most.max(1))? else {
            return Ok(false);
        };
        let measure = &mut |part: &str| font.measure_text(size, part);
        fitting = break_lines(measure, text, width, (most, true))?;
        Ok(fitting.is_some())
    })?;
    Ok(size.zip(fitting).map(|(size, (lines, _))| (size, lines)))
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
                let (top, bottom) = crate::geometry::estimated_ink(part.size, text)?;
                (top as f32, bottom as f32)
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

/// The lines `text` breaks into at `size` in `width` pixels, between words
/// only; `None` where a word alone is wider.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn words_in_lines(
    font: &Font<'_>,
    size: f32,
    text: &str,
    width: f32,
) -> Result<Option<Vec<String>>, Error> {
    let measure = &mut |part: &str| font.measure_text(size, part);
    let broken = break_lines(measure, text, width, (usize::MAX, true))?;
    Ok(broken.map(|(lines, _)| lines))
}

/// The largest size, from `least` to `most`, at which `text` fits a room
/// `width` by `height` broken into lines between words (no word wider than a
/// line), as it is drawn; `least` where none does.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn largest(
    font: &Font<'_>,
    text: &str,
    (width, height): (u16, u16),
    (least, most): (f32, f32),
) -> Result<f32, Error> {
    let fits = |size: f32| -> Result<bool, Error> {
        let Some(most) = lines_in(font, size, f32::from(height), usize::MAX)? else {
            return Ok(false);
        };
        let measure = &mut |part: &str| font.measure_text(size, part);
        Ok(break_lines(measure, text, f32::from(width), (most, true))?.is_some())
    };
    Ok(largest_size(most.max(least), least, fits)?.unwrap_or(least))
}

/// `text` broken into as many lines as `width` needs at `size`, between words
/// only, the lines as even as can be (not one long and one short). A word
/// wider than a line has a line to itself.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn balanced(font: &Font<'_>, size: f32, text: &str, width: f32) -> Result<Vec<String>, Error> {
    let Some(first) = words_in_lines(font, size, text, width)? else {
        let mut words: Vec<String> = Vec::new();
        for word in text.split_whitespace() {
            words.try_reserve(1).map_err(|_| Error::NoMemory)?;
            words.push(text::copy(word)?);
        }
        return Ok(words);
    };
    let count = first.len();
    if count <= 1 {
        return Ok(first);
    }
    // The narrowest width that needs no more lines.
    let (mut low, mut high) = (width / count as f32, width);
    let mut best = first;
    for _ in 0..8 {
        let mid = f32::midpoint(low, high);
        match words_in_lines(font, size, text, mid)? {
            Some(narrow) if narrow.len() <= count => {
                high = mid;
                best = narrow;
            }
            _ => low = mid,
        }
    }
    Ok(best)
}

/// The height a caption takes ([`caption`]), the gap below it included.
///
/// # Errors
/// Font refusals.
pub fn caption_height(font: &Font<'_>, small: f32, gap: u16) -> Result<u16, Error> {
    Ok(crate::geometry::line_px(font, small)?.saturating_add(gap / 2))
}

/// Draws `text` small at the top of `area`, placed by `align`, e.g. "Today"
/// over the numbers; returns the area left below it.
///
/// # Errors
/// Font refusals and [`Error::NoMemory`].
pub fn caption(
    out: &mut Vec<Paint>,
    font: &Font<'_>,
    (small, gap): (f32, u16),
    (area, align, color): (Rect, Align, Color),
    text: &str,
) -> Result<Rect, Error> {
    let height = caption_height(font, small, gap)?.min(area.height);
    let row = Rect::new(
        area.x,
        area.y,
        area.width,
        crate::geometry::line_px(font, small)?,
    );
    label(out, font, (small, align), row, text, color)?;
    Ok(Rect::new(
        area.x,
        area.y.saturating_add(height),
        area.width,
        area.height - height,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every letter ten wide, a space four; "AV" kerned a pixel closer, so a
    /// line is not quite the sum of its words.
    fn measure(text: &str) -> f32 {
        let letters: f32 = text
            .chars()
            .map(|c| if c == ' ' { 4.0 } else { 10.0 })
            .sum();
        letters - text.matches("AV").count() as f32
    }

    /// The breaker as it was: each word measured alone and with its line.
    fn measured_whole(text: &str, width: f32) -> (Vec<String>, bool) {
        let (mut lines, mut too_wide, mut current) = (Vec::new(), false, String::new());
        for word in text.split_whitespace() {
            too_wide |= measure(word) > width;
            if current.is_empty() {
                current = word.into();
                continue;
            }
            let longer = alloc::format!("{current} {word}");
            if measure(&longer) <= width {
                current = longer;
            } else {
                lines.push(core::mem::replace(&mut current, word.into()));
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
        (lines, too_wide)
    }

    fn broken(text: &str, width: f32, most: usize, whole: bool) -> Option<(Vec<String>, bool)> {
        break_lines(&mut |part| Ok(measure(part)), text, width, (most, whole)).unwrap()
    }

    #[test]
    fn words_break_as_measured_whole_with_fewer_and_shorter_measures() {
        let text = "AVA wavy AVAV lines of a quote that runs on and on, past the edge \
                    of the widget, with an unbreakablewordthatiswide in it";
        for width in [
            0.0, 30.0, 40.0, 44.0, 64.0, 100.0, 104.0, 150.0, 250.0, 1000.0,
        ] {
            assert_eq!(
                broken(text, width, usize::MAX, false),
                Some(measured_whole(text, width)),
                "{width}"
            );
        }
        let mut letters = 0;
        break_lines(
            &mut |part| {
                letters += part.chars().count();
                Ok(measure(part))
            },
            text,
            150.0,
            (usize::MAX, false),
        )
        .unwrap();
        // Each word alone and again with its growing line took 425.
        assert!(letters < 250, "{letters}");
    }

    #[test]
    fn a_line_of_the_text_starts_a_line_and_an_empty_one_is_kept() {
        assert_eq!(
            broken(
                "Milk\nEggs and bread\n\nCall Anna",
                100.0,
                usize::MAX,
                false
            ),
            Some((
                ["Milk", "Eggs and", "bread", "", "Call Anna"]
                    .map(String::from)
                    .to_vec(),
                false
            ))
        );
        // A text of one line and no words has no lines.
        assert_eq!(
            broken(" ", 100.0, usize::MAX, false),
            Some((Vec::new(), false))
        );
    }

    #[test]
    fn the_breaking_stops_at_a_word_too_wide_or_a_line_too_many() {
        let text = "one two three four";
        assert_eq!(broken(text, 100.0, 1, true), None);
        assert!(broken(text, 100.0, 2, true).is_some());
        assert_eq!(broken("one unbreakable", 80.0, usize::MAX, true), None);
        assert_eq!(
            broken("one unbreakable", 80.0, usize::MAX, false).map(|b| b.1),
            Some(true)
        );
        let mut measures = 0;
        break_lines(
            &mut |part| {
                measures += 1;
                Ok(measure(part))
            },
            "toolongforaline and many more words after it",
            80.0,
            (usize::MAX, true),
        )
        .unwrap();
        assert_eq!(measures, 1);
    }

    #[test]
    fn breaking_survives_every_refused_allocation() {
        crate::fail_alloc::sweep(|| {
            break_lines(
                &mut |part| Ok(measure(part)),
                "a b\n\nc d e",
                30.0,
                (usize::MAX, false),
            )
            .map(|b| b.map_or(0, |(lines, _)| lines.len()))
        });
    }
}
