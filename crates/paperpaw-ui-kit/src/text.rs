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
    let (ascent, descent, _) = crate::geometry::line_metrics(font, size)?;
    let baseline = i32::from(area.y)
        .saturating_add(((f32::from(area.height) + ascent - descent) / 2.0) as i32);
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
    let (top, bottom) = crate::geometry::cap_bounds(font, size)?;
    let height = i64::from(area.height) - (i64::from(bottom) - i64::from(top));
    let at = i64::from(area.y) + height.div_euclid(2) - i64::from(top);
    Ok(at.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
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

/// `text` wrapped and centred in `area`, in the small size and muted colour.
///
/// # Errors
/// [`Error::NoMemory`] or a refused font reply.
pub fn message(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    style: &crate::Style,
    area: Rect,
    text: &str,
) -> Result<(), Error> {
    let room = f32::from(area.width.saturating_sub(style.margin.saturating_mul(4))).max(1.0);
    let lines = wrap_with(|l| font.measure_text(style.small, l), text, room, 8)?;
    let line = crate::geometry::line_px(font, style.small)?;
    let total = u32::from(line) * lines.len() as u32;
    let mut y = u32::from(area.y) + u32::from(area.height).saturating_sub(total) / 2;
    for text in &lines {
        let wide = font.measure_text(style.small, text)?;
        let x = f32::from(area.x) + (f32::from(area.width) - wide).max(0.0) / 2.0;
        let row = Rect::new(
            area.x,
            crate::geometry::clamp_u16(i64::from(y)),
            area.width,
            line,
        );
        label(paint, font, text, style.small, x, row, style.ink, None)?;
        y = y.saturating_add(u32::from(line));
    }
    Ok(())
}

/// Compares two texts by their lower-case characters, without allocating.
#[must_use]
pub fn cmp_lowercase(a: &str, b: &str) -> core::cmp::Ordering {
    a.chars()
        .flat_map(char::to_lowercase)
        .cmp(b.chars().flat_map(char::to_lowercase))
}

/// The decimal digits of `value`, written into `digits`.
#[must_use]
pub fn decimal(value: usize, digits: &mut [u8; 20]) -> &str {
    decimal_u64(value as u64, digits)
}

/// The decimal digits of `value` (up to 20), written into `digits`.
#[must_use]
pub fn decimal_u64(mut value: u64, digits: &mut [u8; 20]) -> &str {
    let mut at = digits.len();
    loop {
        at -= 1;
        digits[at] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    // Every byte written above is an ASCII digit.
    core::str::from_utf8(&digits[at..]).unwrap_or("")
}

/// The parts joined into one string, made with fallible allocation.
///
/// # Errors
/// [`Error::NoMemory`] when the string cannot be allocated.
pub fn concat(parts: &[&str]) -> Result<String, Error> {
    let mut len = 0usize;
    for part in parts {
        len = len.checked_add(part.len()).ok_or(Error::NoMemory)?;
    }
    let mut result = String::new();
    result.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
    for part in parts {
        result.push_str(part);
    }
    Ok(result)
}

/// `text` and an ellipsis, made with fallible allocation.
///
/// # Errors
/// [`Error::NoMemory`] when the copy cannot be allocated.
pub fn with_ellipsis(text: &str) -> Result<String, Error> {
    let mut result = String::new();
    result
        .try_reserve_exact(text.len().checked_add(3).ok_or(Error::NoMemory)?)
        .map_err(|_| Error::NoMemory)?;
    result.push_str(text);
    result.push('…');
    Ok(result)
}

/// Breaks `text` into at most `max_lines` lines (none when it is 0) no wider than `width` as
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
    if max_lines == 0 {
        return Ok(lines);
    }
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
                    .try_reserve(rest.len().checked_add(1).ok_or(Error::NoMemory)?)
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
                piece
                    .try_reserve_exact(rest.len())
                    .map_err(|_| Error::NoMemory)?;
                for (at, ch) in rest.char_indices() {
                    piece.push(ch);
                    if measure(&piece)? > width && taken > 0 {
                        piece.pop();
                        break;
                    }
                    taken = at + ch.len_utf8();
                }
                line = piece;
                // `taken` ends a character of `rest` (`at + len_utf8`): a boundary.
                debug_assert!(rest.is_char_boundary(taken));
                rest = rest.get(taken..).unwrap_or_default();
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
            let with = with_ellipsis(last)?;
            if measure(&with)? <= width {
                *last = with;
                break;
            }
            last.pop();
        }
    }
    Ok(lines)
}

/// `notes` joined as a line of notes: "a · b · c".
///
/// # Errors
/// [`Error::NoMemory`].
pub fn join_notes(notes: &[String]) -> Result<String, Error> {
    let mut joined = String::new();
    let total = notes.iter().map(|note| note.len() + 4).sum::<usize>();
    joined.try_reserve(total).map_err(|_| Error::NoMemory)?;
    for (n, note) in notes.iter().enumerate() {
        if n > 0 {
            joined.push_str(" · ");
        }
        joined.push_str(note);
    }
    Ok(joined)
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

    #[test]
    fn wrapping_survives_every_refused_allocation() {
        for (text, width, lines) in [
            ("abcdefghijklmnopqrstuvwxyz", 40.0, 4),
            ("one two three four five six", 70.0, 2),
            ("a b c d e f g h", 30.0, 3),
            ("αβγδε ζηθικ λμνξο", 50.0, 2),
        ] {
            crate::fail_alloc::sweep(|| wrap_with(measure, text, width, lines).map(|l| l.len()));
        }
    }

    #[test]
    fn no_lines_means_no_text() {
        assert!(wrap_with(measure, "one two", 70.0, 0).unwrap().is_empty());
    }

    #[test]
    fn copies_and_joins_survive_every_refused_allocation() {
        crate::fail_alloc::sweep(|| copy("hello").map(|s| s.len()));
        crate::fail_alloc::sweep(|| with_ellipsis("hello").map(|s| s.len()));
        crate::fail_alloc::sweep(|| concat(&["a", "bc", "d"]).map(|s| s.len()));
    }

    #[test]
    fn decimal_writes_the_largest_numbers() {
        let mut digits = [0; 20];
        assert_eq!(decimal_u64(u64::MAX, &mut digits), "18446744073709551615");
        assert_eq!(decimal(0, &mut digits), "0");
    }
}
