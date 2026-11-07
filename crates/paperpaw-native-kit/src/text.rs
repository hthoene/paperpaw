use crate::Paint;
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Color, Rect,
    native::{
        Error,
        fonts::{Font, TextOrigin},
    },
};
pub fn fit(font: &Font<'_>, size: f32, text: &str, width: f32) -> Result<String, Error> {
    if font.measure_text(size, text)? <= width {
        return crate::text::copy(text);
    }
    let mut result = String::new();
    result
        .try_reserve_exact(text.len().checked_add(3).ok_or(Error::NoMemory)?)
        .map_err(|_| Error::NoMemory)?;
    for c in text.chars() {
        result.push(c);
        result.push('…');
        if font.measure_text(size, &result)? > width {
            result.pop();
            result.pop();
            break;
        }
        result.pop();
    }
    result.push('…');
    Ok(result)
}
#[allow(
    clippy::too_many_arguments,
    reason = "one measured text run and its target geometry"
)]
pub fn label(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    text: &str,
    size: f32,
    x: f32,
    area: Rect,
    color: Color,
    clip: Option<Rect>,
) -> Result<(), Error> {
    let room = f32::from(area.x) + f32::from(area.width) - x;
    let text = if room > 0.0 && font.measure_text(size, text)? > room + 1.0 {
        fit(font, size, text, room)?
    } else {
        crate::text::copy(text)?
    };
    let metrics = font.read_line_metrics(size)?;
    let baseline = i32::from(area.y)
        + ((f32::from(area.height) + metrics.ascent - metrics.descent) / 2.0) as i32;
    paint.try_reserve(1).map_err(|_| Error::NoMemory)?;
    paint.push(Paint::Text {
        text,
        size,
        origin: TextOrigin::new(x, baseline),
        color,
        clip,
    });
    Ok(())
}
pub fn cap_baseline(font: &Font<'_>, size: f32, area: Rect) -> Result<i32, Error> {
    let (top, bottom) = if let Some(ink) = font.ink(size, "H")? {
        (ink.top, ink.bottom)
    } else {
        let m = font.read_line_metrics(size)?;
        (-(m.ascent as i32), -(m.descent as i32))
    };
    Ok(i32::from(area.y) + (i32::from(area.height) - (bottom - top)).div_euclid(2) - top)
}

/// An owned copy of `text` made with fallible allocation.
///
/// # Errors
/// [`Error::NoMemory`] when the copy cannot be allocated.
pub fn copy(text: &str) -> Result<String, Error> {
    let mut result = String::new();
    result
        .try_reserve_exact(text.len())
        .map_err(|_| Error::NoMemory)?;
    result.push_str(text);
    Ok(result)
}

/// Breaks `text` into at most `max_lines` lines no wider than `width` as
/// `measure` says, at spaces (inside a word where one alone is too wide). Where
/// there is more text than the lines hold, the last ends in "…".
///
/// # Errors
/// Propagates errors of `measure` and allocation errors.
pub fn wrap_with(
    mut measure: impl FnMut(&str) -> Result<f32, Error>,
    text: &str,
    width: f32,
    max_lines: usize,
) -> Result<Vec<String>, Error> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut cut = false;
    let mut words = text.split_whitespace().peekable();
    'outer: for word in words.by_ref() {
        let mut rest = word;
        loop {
            let joined = if line.is_empty() {
                copy(rest)?
            } else {
                let mut joined = copy(&line)?;
                joined
                    .try_reserve(rest.len() + 1)
                    .map_err(|_| Error::NoMemory)?;
                joined.push(' ');
                joined.push_str(rest);
                joined
            };
            if measure(&joined)? <= width || (line.is_empty() && rest.chars().count() <= 1) {
                line = joined;
                break;
            }
            if line.is_empty() {
                // One word wider than a line: as many letters as fit.
                let mut taken = 0;
                let mut piece = String::new();
                for (at, ch) in rest.char_indices() {
                    piece.push(ch);
                    if measure(&piece)? > width && taken > 0 {
                        piece.pop();
                        break;
                    }
                    taken = at + ch.len_utf8();
                }
                line = piece;
                rest = &rest[taken..];
            }
            if lines.len() + 1 >= max_lines {
                cut = true;
                break 'outer;
            }
            lines.try_reserve(1).map_err(|_| Error::NoMemory)?;
            lines.push(core::mem::take(&mut line));
            if rest.is_empty() {
                break;
            }
        }
    }
    if !line.is_empty() {
        lines.try_reserve(1).map_err(|_| Error::NoMemory)?;
        lines.push(line);
    }
    if (cut || words.peek().is_some())
        && let Some(last) = lines.last_mut()
    {
        while !last.is_empty() {
            let mut with = copy(last)?;
            with.push('…');
            if measure(&with)? <= width {
                *last = with;
                break;
            }
            last.pop();
        }
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character is ten wide.
    fn measure(text: &str) -> Result<f32, Error> {
        Ok(text.chars().count() as f32 * 10.0)
    }

    #[test]
    fn words_wrap_at_spaces() {
        let lines = wrap_with(measure, "one two three four", 90.0, 4).unwrap();
        assert_eq!(lines, ["one two", "three", "four"]);
    }

    #[test]
    fn a_word_wider_than_a_line_is_broken_inside() {
        let lines = wrap_with(measure, "abcdefghij", 40.0, 4).unwrap();
        assert_eq!(lines, ["abcd", "efgh", "ij"]);
    }

    #[test]
    fn more_text_than_lines_ends_in_an_ellipsis_that_fits() {
        let lines = wrap_with(measure, "one two three four five six", 70.0, 2).unwrap();
        assert_eq!(lines.len(), 2);
        assert!(lines[1].ends_with('…'));
        assert!(lines.iter().all(|l| measure(l).unwrap() <= 70.0));
    }

    #[test]
    fn nothing_wraps_to_nothing() {
        assert!(wrap_with(measure, "  ", 70.0, 2).unwrap().is_empty());
    }
}
