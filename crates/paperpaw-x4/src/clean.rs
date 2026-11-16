//! Cleaning the panel to white while the next frame is drawn (#267).
//!
//! After grays (a book's cover page) a refresh has to drive every pixel: a
//! half refresh of 1.7 s on the X4. Home's first frame after Back draws for as
//! long before it is sent, one after the other. The firmware starts the half
//! refresh to white first, draws meanwhile into the kept bands, and shows the
//! frame with a fast refresh of 0.5 s. This is what it decides and how it
//! writes the white.

/// The bytes of the white band the firmware keeps (1 bit a pixel, 1 is white).
pub const WHITE_BYTES: usize = 4000;

/// Whether the frame about to be drawn is shown by a clean to white and a fast
/// refresh instead of a half refresh of itself: after grays only (else the half
/// refresh drives every pixel anyway for nothing it could overlap), for a frame
/// whose drawing takes as long as the clean (the home screen: the others draw
/// in well under half a second, and the extra fast refresh costs more than
/// they gain), on a panel that can (`panel`, the SSD1677 that keeps no frame).
#[must_use]
pub const fn wanted(panel: bool, after_gray: bool, half: bool, home: bool) -> bool {
    panel && after_gray && half && home
}

/// The bands of white that cover a screen `width` by `height`: each a first row
/// and a number of rows, in whole bytes of 8 rows, a band's bytes fitting
/// [`WHITE_BYTES`]. A screen of no width has none.
pub fn bands(width: u16, height: u16) -> impl Iterator<Item = (u16, u16)> {
    let stride = usize::from(width).div_ceil(8);
    let rows = if stride == 0 {
        0
    } else {
        u16::try_from(WHITE_BYTES / stride / 8 * 8)
            .unwrap_or(8)
            .max(8)
    };
    let mut top = 0u16;
    core::iter::from_fn(move || {
        if rows == 0 || top >= height {
            return None;
        }
        let band = rows.min(height - top);
        let from = top;
        top += band;
        Some((from, band))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bands_cover_the_screen_once_in_whole_bytes_that_fit() {
        for (width, height) in [(480, 800), (800, 480), (792, 528), (8, 8), (480, 5)] {
            let stride = usize::from(width).div_ceil(8);
            let mut next = 0;
            for (top, rows) in bands(width, height) {
                assert_eq!(top, next);
                assert_eq!(top % 8, 0);
                assert!(rows > 0 && usize::from(rows) * stride <= WHITE_BYTES);
                next = top + rows;
            }
            assert_eq!(next, height, "{width}x{height}");
        }
        assert_eq!(bands(0, 800).count(), 0);
    }

    #[test]
    fn only_the_home_screen_after_grays_on_a_panel_that_can_is_cleaned_while_drawn() {
        assert!(wanted(true, true, true, true));
        for (panel, after_gray, half, home) in [
            (false, true, true, true),
            (true, false, true, true),
            (true, true, false, true),
            (true, true, true, false),
        ] {
            assert!(!wanted(panel, after_gray, half, home));
        }
    }
}
