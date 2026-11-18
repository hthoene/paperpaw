//! The lines of a page that are not letters: under, over and through a run
//! (`TextStyle` underline, overline and strike, drawn as the run's line
//! style says: one, two, dotted or wavy, as Beta 3's `ReadingFaces::decorate`)
//! and a table's rules (`Page::rules`).
use crate::native_draw::Ink;
use paperpaw_sdk::{document::LineStyle, layout::PageRule, native::Error};

/// The most pieces of a dotted or wavy line: a longer one is cut, as the
/// bound of one run's drawing (a line is at most a screen wide).
const MAX_PIECES: u32 = 4_096;

/// The lines a run of `style` has from `x`, `width` wide, on `baseline`, in a
/// face of `size` pixels.
///
/// # Errors
/// What `ink` refuses.
pub fn decorate(
    ink: &mut impl Ink,
    style: paperpaw_sdk::document::TextStyle,
    size: f32,
    (x, width): (f32, f32),
    baseline: i32,
) -> Result<(), Error> {
    let (under, over, through) = (style.underline(), style.overline(), style.strike());
    if !(under || over || through) || width <= 0.0 {
        return Ok(());
    }
    let thick = ((size / 16.0) as u32).max(1);
    let (left, right) = ((x + 0.5) as i32, (x + width + 0.5) as i32);
    let length = (right - left).max(0) as u32;
    let line = style.line_style();
    if under {
        line_at(
            ink,
            line,
            (left, length),
            baseline + ((size / 10.0) as i32).max(1),
            thick,
        )?;
    }
    if over {
        // Above the capitals, below the line's top.
        line_at(
            ink,
            line,
            (left, length),
            baseline - (size * 0.78) as i32,
            thick,
        )?;
    }
    if through {
        line_at(
            ink,
            line,
            (left, length),
            baseline - (size * 0.28) as i32,
            thick,
        )?;
    }
    Ok(())
}

/// One line `thick` pixels thick, its top at `y`, from `left`, `length` long:
/// a second line as far below as it is thick (double), dots as long as the
/// line is thick with gaps as wide (dotted), or going up and down by its
/// thickness every two pieces (wavy).
fn line_at(
    ink: &mut impl Ink,
    line: LineStyle,
    (left, length): (i32, u32),
    y: i32,
    thick: u32,
) -> Result<(), Error> {
    match line {
        LineStyle::Double => {
            ink.fill(left, y, length, thick)?;
            ink.fill(left, y + 2 * thick as i32, length, thick)
        }
        LineStyle::Dotted | LineStyle::Wavy => {
            let step = thick.max(1) * 2;
            let mut at = 0;
            let mut n = 0;
            while at < length && n < MAX_PIECES {
                let piece = if line == LineStyle::Dotted {
                    thick.min(length - at)
                } else {
                    step.min(length - at)
                };
                let up = if line == LineStyle::Wavy && n % 2 == 1 {
                    thick as i32
                } else {
                    0
                };
                ink.fill(left + at as i32, y - up, piece, thick)?;
                at += step;
                n += 1;
            }
            Ok(())
        }
        // Solid, and a style of a later build: one line.
        _ => ink.fill(left, y, length, thick),
    }
}

/// The page's rules (a table's borders) with the content box's top left at
/// `(left, top)`; a rule at least one pixel thick.
///
/// # Errors
/// What `ink` refuses.
pub fn draw_rules(
    ink: &mut impl Ink,
    rules: &[PageRule],
    (left, top): (f32, f32),
) -> Result<(), Error> {
    for rule in rules {
        let (x, y) = ((left + rule.x + 0.5) as i32, (top + rule.y + 0.5) as i32);
        let (right, bottom) = (
            (left + rule.x + rule.width + 0.5) as i32,
            (top + rule.y + rule.height + 0.5) as i32,
        );
        let width = (right - x).max(1) as u32;
        let height = (bottom - y).max(1) as u32;
        ink.fill(x, y, width, height)?;
    }
    Ok(())
}
