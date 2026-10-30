//! Converting frames to the window's RGBA pixels, in the colours of the
//! settings (`Kernel::colors`), and what kind of screen the device has.

use core::ops::Range;

use paperpaw_platform::{ColorPanel, FrameView};

/// What a screen shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// Grays: monochrome e-ink.
    Gray,
    /// Colour, through a colour filter on colour e-ink.
    Color(Option<ColorPanel>),
}

/// The screen of a device, by its maker and model (`android.os.Build`):
/// gray on Onyx Boox models without colour; colour e-ink (Kaleido) on
/// those named "Color" or "C" and the Palma 2 Pro; phones and tablets
/// show colour as it is.
pub fn screen(manufacturer: &str, model: &str) -> Screen {
    if !manufacturer.eq_ignore_ascii_case("onyx") {
        return Screen::Color(None);
    }
    let model: String = model
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let colour = model.contains("COLOR")
        || model.ends_with('C')
        || model.ends_with("CPRO")
        || model.starts_with("PALMA2PRO");
    if !colour {
        return Screen::Gray;
    }
    Screen::Color(Some(ColorPanel::new(
        // Kaleido 3: colour at half the resolution, muted.
        2, 55,
    )))
}

/// What each gray of a frame is shown as: black as the text colour, white
/// as the paper colour, the grays between them in between.
pub fn tint(colors: [[u8; 3]; 2]) -> [[u8; 4]; 256] {
    let [ink, paper] = colors;
    core::array::from_fn(|gray| {
        let mix = |i: usize| {
            let (from, to) = (i32::from(ink[i]), i32::from(paper[i]));
            (from + (to - from) * gray as i32 / 255) as u8
        };
        [mix(0), mix(1), mix(2), 0xff]
    })
}

/// Converts the `rows` of `frame` to RGBA_8888 for a window of `stride`
/// pixels per row, each gray as `tint` shows it and each colour (of
/// pictures) as it is: `put(byte offset, rgba)` writes one pixel. Pixels
/// wider than the window and rows below the frame are skipped.
pub fn to_rgba(
    frame: &FrameView<'_>,
    rows: Range<usize>,
    stride: usize,
    tint: &[[u8; 4]; 256],
    mut put: impl FnMut(usize, [u8; 4]),
) {
    for y in rows.start..rows.end.min(usize::from(frame.height)) {
        let row = y * stride * 4;
        for x in 0..usize::from(frame.width).min(stride) {
            let color = frame.pixel(x as u16, y as u16);
            let rgba = if color.r == color.g && color.g == color.b {
                tint[usize::from(color.r)]
            } else {
                [color.r, color.g, color.b, 0xff]
            };
            put(row + x * 4, rgba);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_platform::PixelFormat;

    const BLACK_ON_WHITE: [[u8; 3]; 2] = [[0, 0, 0], [255, 255, 255]];

    #[test]
    fn gray_becomes_opaque_rgba() {
        // Three Gray8 rows of two pixels: white, white / black, white /
        // gray, black.
        let data = [255u8, 255, 0, 255, 128, 0];
        let frame = FrameView::new(2, 3, PixelFormat::Gray8, 2, &data);
        // A window three pixels wide, of which rows 1 and 2 changed (and
        // a row below the frame is asked for).
        let mut buffer = [7u8; 3 * 4 * 4];
        to_rgba(&frame, 1..4, 3, &tint(BLACK_ON_WHITE), |at, rgba| {
            buffer[at..at + 4].copy_from_slice(&rgba)
        });
        assert_eq!(&buffer[12..20], &[0, 0, 0, 255, 255, 255, 255, 255]);
        assert_eq!(&buffer[24..28], &[128, 128, 128, 255]);
        // Row 0 is untouched, as are the third column and the window's
        // row below the frame.
        assert_eq!(&buffer[0..4], &[7, 7, 7, 7]);
        assert_eq!(&buffer[20..24], &[7, 7, 7, 7]);
        assert!(buffer[36..].iter().all(|&b| b == 7));
    }

    #[test]
    fn colours_of_pictures_stay_as_they_are() {
        // One Rgb888 row: red, then a gray.
        let data = [255u8, 0, 0, 128, 128, 128];
        let frame = FrameView::new(2, 1, PixelFormat::Rgb888, 6, &data);
        let mut buffer = [0u8; 8];
        let sepia = tint([[91, 70, 54], [244, 236, 216]]);
        to_rgba(&frame, 0..1, 2, &sepia, |at, rgba| {
            buffer[at..at + 4].copy_from_slice(&rgba)
        });
        assert_eq!(&buffer[..4], &[255, 0, 0, 255]);
        assert_eq!(&buffer[4..], &sepia[128]);
    }

    #[test]
    fn screens_by_maker_and_model() {
        assert_eq!(screen("Sony", "XQ-DQ54"), Screen::Color(None));
        assert_eq!(screen("ONYX", "Go7"), Screen::Gray);
        assert_eq!(screen("ONYX", "Poke5"), Screen::Gray);
        assert_eq!(screen("ONYX", "NoteAir2P"), Screen::Gray);
        for model in [
            "Go Color 7",
            "NoteAir3C",
            "Tab Ultra C Pro",
            "Palma2 Pro C",
            "TabMiniC",
        ] {
            assert!(
                matches!(screen("ONYX", model), Screen::Color(Some(_))),
                "{model}"
            );
        }
    }

    #[test]
    fn grays_lie_between_the_colours() {
        let sepia = tint([[91, 70, 54], [244, 236, 216]]);
        assert_eq!(sepia[0], [91, 70, 54, 255]);
        assert_eq!(sepia[255], [244, 236, 216, 255]);
        let middle = sepia[128];
        assert!((91..244).contains(&middle[0]) && (54..216).contains(&middle[2]));
        // Black on white leaves the grays as they are.
        assert_eq!(tint(BLACK_ON_WHITE)[77], [77, 77, 77, 255]);
    }
}
