//! The progress line's bar: an outline across the strip, filled from the
//! left up to where the page is, a mark standing out above and below it
//! where each chapter begins; the page's label on the right. Whole numbers,
//! rectangles of one ink; the host-free geometry.
use alloc::vec::Vec;

use paperpaw_sdk::Rect;
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::mem;

/// Millionths in the whole book.
const WHOLE: u32 = 1_000_000;

/// The rectangles of the bar in `strip`, for text of `size` pixels and a label
/// `label` pixels wide (0: none); `ppm` is how far into the book the page is,
/// `marks` the chapters' starts in thousandths of the book (only those
/// strictly inside the book are drawn).
///
/// # Errors
/// [`Error::NoMemory`].
pub fn rects(
    strip: Rect,
    size: u16,
    label: u16,
    ppm: u32,
    marks: &[u16],
) -> Result<Vec<Rect>, Error> {
    let size = u32::from(size);
    let gap = if label == 0 { 0 } else { size };
    let width = u32::from(strip.width)
        .saturating_sub(u32::from(label) + gap)
        .max(1);
    let thickness = (size * 3 / 10).max(3);
    let line = (size / 12).max(1).min(thickness / 2);
    let mid = u32::from(strip.y) + u32::from(strip.height) / 2;
    let top = mid.saturating_sub(thickness / 2);
    let at = |x: u32, y: u32, w: u32, h: u32| {
        let narrow = |v: u32| u16::try_from(v).unwrap_or(u16::MAX);
        Rect::new(narrow(x), narrow(y), narrow(w), narrow(h))
    };
    let x0 = u32::from(strip.x);
    let mut found = Vec::new();
    let outline = [
        at(x0, top, width, line),
        at(x0, top + thickness - line, width, line),
        at(x0, top, line, thickness),
        at(
            x0 + width - line.min(width),
            top,
            line.min(width),
            thickness,
        ),
    ];
    for rect in outline {
        mem::push(&mut found, rect)?;
    }
    let filled = u32::try_from(u64::from(width) * u64::from(ppm.min(WHOLE)) / u64::from(WHOLE))
        .unwrap_or(width);
    if filled > 0 {
        mem::push(&mut found, at(x0, top, filled, thickness))?;
    }
    // The marks stand out by half the bar's thickness above and below.
    let reach = thickness / 2;
    for mark in marks.iter().filter(|m| **m > 0 && **m < 1000) {
        let x = x0 + width * u32::from(*mark) / 1000;
        mem::push(
            &mut found,
            at(x, top.saturating_sub(reach), line, thickness + 2 * reach),
        )?;
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STRIP: Rect = Rect::new(40, 741, 400, 44);

    #[test]
    fn the_bar_is_filled_up_to_the_progress() {
        // 6 pt at 254 dpi: 21 px, a bar 6 px thick centred in the strip; a label of
        // 60 px and a gap of one text size leave 319.
        let found = rects(STRIP, 21, 60, 500_000, &[]).unwrap();
        assert_eq!(found.len(), 5);
        let fill = found[4];
        assert_eq!((fill.x, fill.y, fill.width, fill.height), (40, 760, 159, 6));
        // The outline spans the width.
        assert_eq!((found[0].width, found[3].x), (319, 40 + 319 - 1));
        // Nothing read: no fill; all read: the whole bar.
        assert_eq!(rects(STRIP, 21, 60, 0, &[]).unwrap().len(), 4);
        assert_eq!(rects(STRIP, 21, 60, 2_000_000, &[]).unwrap()[4].width, 319);
    }

    #[test]
    fn a_chapter_starts_a_mark_that_stands_out() {
        let found = rects(STRIP, 21, 0, 0, &[0, 250, 1000, 1500, 999]).unwrap();
        // Four outline rectangles and the marks at 250 and 999 only.
        assert_eq!(found.len(), 6);
        let mark = found[4];
        assert_eq!((mark.x, mark.y, mark.width, mark.height), (140, 757, 1, 12));
        assert_eq!(found[5].x, 40 + 399);
    }

    #[test]
    fn a_strip_too_narrow_still_draws_a_bar() {
        let found = rects(Rect::new(0, 0, 10, 20), 21, 60, 500_000, &[500]).unwrap();
        assert!(found.iter().all(|r| r.width >= 1));
    }
}
