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
    push_run(paint, font, text, size, x, area, color, clip)
}

/// [`label`] in an interface that may run right to left: laid out as for
/// left to right in `outer` (`x`, `area` and `clip` as then), then, where
/// `rtl` says so, mirrored in `outer` ([`crate::Style::flip_x`]): the run
/// starts as far from the right edge as it would have from the left one.
///
/// # Errors
/// [`Error::NoMemory`] or a refused font reply.
#[allow(
    clippy::too_many_arguments,
    reason = "one measured text run and its target geometry"
)]
pub fn label_in(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    rtl: bool,
    outer: Rect,
    text: &str,
    size: f32,
    x: f32,
    area: Rect,
    color: Color,
    clip: Option<Rect>,
) -> Result<(), Error> {
    if !rtl {
        return label(paint, font, text, size, x, area, color, clip);
    }
    let room = f32::from(area.x) + f32::from(area.width) - x;
    let text = if room > 0.0 && font.measure_text(size, text)? > room + 1.0 {
        fit(font, size, text, room)?
    } else {
        crate::text::copy(text)?
    };
    let wide = font.measure_text(size, &text)?;
    push_run(
        paint,
        font,
        text,
        size,
        crate::style::flip_span(true, outer, x, wide),
        crate::style::flip_rect(true, outer, area),
        color,
        clip.map(|clip| crate::style::flip_rect(true, outer, clip)),
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "one measured text run and its target geometry"
)]
fn push_run(
    paint: &mut Vec<Paint>,
    font: &Font<'_>,
    text: String,
    size: f32,
    x: f32,
    area: Rect,
    color: Color,
    clip: Option<Rect>,
) -> Result<(), Error> {
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

/// `value` with its unit written out: a unit that holds `{count}` has the number put in,
/// any other follows the number. For a unit that is a catalog key see
/// [`crate::Messages::with_unit`].
///
/// # Errors
/// [`Error::NoMemory`] when the string cannot be allocated.
pub fn with_unit(value: i64, unit: &str) -> Result<String, Error> {
    let mut digits = [0; 20];
    let number = if value < 0 {
        let positive = decimal_u64(value.unsigned_abs(), &mut digits);
        concat(&["-", positive])?
    } else {
        copy(decimal_u64(value as u64, &mut digits))?
    };
    if unit.is_empty() {
        return Ok(number);
    }
    // A byte search and `get`, not `split_once("{count}")`: a `&str` pattern search links
    // the slice panic that prints a character (Debug and Unicode tables, 4 KB).
    const MARK: &[u8] = b"{count}";
    let at = unit
        .as_bytes()
        .windows(MARK.len())
        .position(|window| window == MARK);
    match at.and_then(|at| Some((unit.get(..at)?, unit.get(at + MARK.len()..)?))) {
        Some((before, after)) => concat(&[before, &number, after]),
        None => concat(&[&number, " ", unit]),
    }
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

/// A string that grows only where the allocator allows.
struct Fallible<'a> {
    text: &'a mut String,
    refused: bool,
}

impl core::fmt::Write for Fallible<'_> {
    fn write_str(&mut self, part: &str) -> core::fmt::Result {
        if self.text.try_reserve(part.len()).is_err() {
            self.refused = true;
            return Err(core::fmt::Error);
        }
        self.text.push_str(part);
        Ok(())
    }
}

/// Appends the formatted `args` to `target`, growing it with fallible allocation.
///
/// # Errors
/// [`Error::NoMemory`] when `target` cannot grow; [`Error::Invalid`] when a
/// formatted value's own `Display` fails.
pub fn write_to(target: &mut String, args: core::fmt::Arguments<'_>) -> Result<(), Error> {
    let mut out = Fallible {
        text: target,
        refused: false,
    };
    match core::fmt::Write::write_fmt(&mut out, args) {
        Ok(()) => Ok(()),
        Err(_) if out.refused => Err(Error::NoMemory),
        Err(_) => Err(Error::Invalid),
    }
}

/// The formatted `args` as a new string, with fallible allocation.
///
/// # Errors
/// As [`write_to`].
pub fn format(args: core::fmt::Arguments<'_>) -> Result<String, Error> {
    let mut text = String::new();
    write_to(&mut text, args)?;
    Ok(text)
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
