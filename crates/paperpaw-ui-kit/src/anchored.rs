//! A window next to a word (#378): where a lookup overlay goes when the reader
//! says where the word is (the hand-over param `anchor`, `x,y,width,height` in
//! the screen's pixels): below the word where there is room, else above it,
//! never over it. Without a usable anchor the window is the sheet at the bottom
//! of the screen it always was.

use alloc::vec::Vec;
use paperpaw_sdk::Rect;

use crate::{Paint, Style, paint, sheet};
use paperpaw_sdk::native::Error;

/// The sheet at the bottom takes this many twentieths of the screen's height
/// from the top: the window is 11 twentieths high where there is room for it.
const TOP_TWENTIETHS: u32 = 9;

/// Where a window is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    /// The window's box, as wide as the screen.
    pub region: Rect,
    /// It is next to a word, with edges above and below, not the sheet at the
    /// bottom of the screen.
    pub anchored: bool,
}

/// The box in the param `anchor`: four whole numbers separated by commas.
#[must_use]
pub fn parse(text: &str) -> Option<Rect> {
    let mut parts = text.split(',').map(|n| n.trim().parse::<u16>().ok());
    let (x, y, width, height) = (
        parts.next()??,
        parts.next()??,
        parts.next()??,
        parts.next()??,
    );
    if parts.next().is_some() {
        return None;
    }
    Some(Rect::new(x, y, width, height))
}

/// The most bytes of an `anchor` param: four u16 and three commas.
pub const TEXT_BYTES: usize = 23;

/// `area` as the param `anchor` says it, written into `out`.
#[must_use]
pub fn text(area: Rect, out: &mut [u8; TEXT_BYTES]) -> &str {
    let mut len = 0;
    for (n, value) in [area.x, area.y, area.width, area.height]
        .into_iter()
        .enumerate()
    {
        if n > 0 {
            out[len] = b',';
            len += 1;
        }
        let mut digits = [0u8; 5];
        let mut count = 0;
        let mut rest = value;
        loop {
            digits[count] = b'0' + (rest % 10) as u8;
            count += 1;
            rest /= 10;
            if rest == 0 {
                break;
            }
        }
        while count > 0 {
            count -= 1;
            out[len] = digits[count];
            len += 1;
        }
    }
    core::str::from_utf8(&out[..len]).unwrap_or("")
}

/// The window for a screen of `width` x `height`: next to `anchor` with a
/// `gap` between, or the bottom sheet where there is no `anchor`, it is not
/// on the screen, or neither side of it leaves a quarter of the screen.
#[must_use]
pub fn place(anchor: Option<Rect>, width: u16, height: u16, gap: u16) -> Window {
    let top = u32::from(height) * TOP_TWENTIETHS / 20;
    let sheet = Window {
        region: Rect::new(0, top as u16, width, height - top as u16),
        anchored: false,
    };
    let Some(anchor) = anchor.filter(|a| a.height > 0 && u32::from(a.y) < u32::from(height)) else {
        return sheet;
    };
    let want = u32::from(sheet.region.height);
    let gap = u32::from(gap);
    let below = u32::from(height).saturating_sub(anchor.bottom() + gap);
    let above = u32::from(anchor.y).saturating_sub(gap);
    // Below the word where the whole window fits, else above, else the side
    // with more room if that is a quarter of the screen.
    let (below_side, room) = if below >= want {
        (true, want)
    } else if above >= want {
        (false, want)
    } else if below >= above {
        (true, below)
    } else {
        (false, above)
    };
    if room < u32::from(height) / 4 {
        return sheet;
    }
    let y = if below_side {
        anchor.bottom() + gap
    } else {
        u32::from(anchor.y) - gap - room
    };
    Window {
        region: Rect::new(0, y as u16, width, room as u16),
        anchored: true,
    }
}

/// The edges of `window` in `ink`: along the top, and for a window next to a
/// word along the bottom too.
///
/// # Errors
/// [`Error::NoMemory`] when the paint list cannot grow.
pub fn edges(
    out: &mut Vec<Paint>,
    style: &Style,
    window: Window,
    ink: paperpaw_sdk::Color,
) -> Result<(), Error> {
    let top = sheet::edge(out, style, window.region, ink)?;
    if window.anchored {
        let bottom = Rect::new(
            window.region.x,
            window.region.y + window.region.height - top.height,
            window.region.width,
            top.height,
        );
        paint::push(out, Paint::Fill(bottom, ink))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GAP: u16 = 8;

    #[test]
    fn the_anchor_param_is_four_numbers() {
        assert_eq!(parse("50,100,70,20"), Some(Rect::new(50, 100, 70, 20)));
        assert_eq!(parse(" 1, 2, 3, 4 "), Some(Rect::new(1, 2, 3, 4)));
        for bad in [
            "",
            "1,2,3",
            "1,2,3,4,5",
            "a,2,3,4",
            "-1,2,3,4",
            "1,2,3,70000",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_anchor_param_is_written_as_it_is_read() {
        let mut out = [0; TEXT_BYTES];
        let area = Rect::new(50, 100, 70, 20);
        assert_eq!(text(area, &mut out), "50,100,70,20");
        let big = Rect::new(0, 65535, 65535, 7);
        assert_eq!(parse(text(big, &mut out)), Some(big));
    }

    #[test]
    fn without_an_anchor_or_off_the_screen_it_is_the_bottom_sheet() {
        let sheet = Window {
            region: Rect::new(0, 360, 480, 440),
            anchored: false,
        };
        assert_eq!(place(None, 480, 800, GAP), sheet);
        assert_eq!(place(Some(Rect::new(0, 900, 50, 20)), 480, 800, GAP), sheet);
        assert_eq!(place(Some(Rect::new(0, 100, 50, 0)), 480, 800, GAP), sheet);
    }

    #[test]
    fn a_word_in_the_upper_part_gets_the_window_below_it() {
        let word = Rect::new(50, 100, 70, 20);
        let window = place(Some(word), 480, 800, GAP);
        assert!(window.anchored);
        assert_eq!(window.region, Rect::new(0, 128, 480, 440));
    }

    #[test]
    fn a_word_in_the_lower_part_gets_the_window_above_it_flipped() {
        let word = Rect::new(50, 600, 70, 20);
        let window = place(Some(word), 480, 800, GAP);
        assert!(window.anchored);
        assert_eq!(window.region, Rect::new(0, 600 - 8 - 440, 480, 440));
        assert!(window.region.bottom() <= 600);
    }

    #[test]
    fn a_word_in_the_middle_gets_the_side_with_more_room_and_is_never_covered() {
        for y in (0..780).step_by(10) {
            let word = Rect::new(10, y, 40, 20);
            let window = place(Some(word), 480, 800, GAP).region;
            let apart = window.y as u32 >= word.bottom() || window.bottom() <= u32::from(word.y);
            assert!(apart, "word at {y} is covered by {window:?}");
            assert!(window.bottom() <= 800);
        }
        // Room below 389 (word ends at 411), above 400: above wins.
        let window = place(Some(Rect::new(10, 400, 40, 20)), 480, 800, GAP);
        assert_eq!(window.region.bottom(), 392);
    }
}
