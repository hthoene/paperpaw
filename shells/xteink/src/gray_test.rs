//! Four grays on the panel, to judge them (`gray` over USB): bars of the
//! four, a gradient dithered to them, and a picture PaperPaw decoded (e.g.
//! a book's cover) below, drawn straight into the panel's memories and
//! shown with the gray waveform. The screen is drawn again after.

use alloc::vec;
use alloc::vec::Vec;

use paperpaw_platform::{FrameView, PixelFormat, Storage};
use paperpaw_sdk::image::GrayFile;
use paperpaw_x4::display::Error;

use crate::board::Panel;

/// Rows drawn at a time.
const BAND: u16 = 40;

/// Shows the test picture: `picture`, if given, is a gray picture
/// PaperPaw decoded (`.gray`).
pub fn show(panel: &mut Panel, storage: &dyn Storage, picture: Option<&str>) -> Result<(), Error> {
    let (width, height) = panel.size();
    let picture = picture.and_then(|path| GrayFile::open(storage, path));
    // The picture in the lower half, as large as fits, in the middle.
    let area = (height / 2, height - height / 2);
    let fit = picture.as_ref().map(|p| {
        let scale = (f32::from(width) / p.width as f32).min(f32::from(area.1) / p.height as f32);
        let (w, h) = (
            (p.width as f32 * scale) as u16,
            (p.height as f32 * scale) as u16,
        );
        (w, h, (width - w) / 2, area.0 + (area.1 - h) / 2)
    });
    let stride = usize::from(width).div_ceil(4);
    let mut band = vec![0u8; stride * usize::from(BAND)];
    let mut row = Vec::new();
    let mut top = 0;
    while top < height {
        let rows = BAND.min(height - top);
        band.fill(0);
        for dy in 0..rows {
            let y = top + dy;
            // The picture's row, for this one.
            let source = match (&picture, fit) {
                (Some(p), Some((_, h, _, py))) if y >= py && y < py + h => {
                    let sy = u32::from(y - py) * p.height / u32::from(h.max(1));
                    row.resize(p.width as usize, 255);
                    p.read_row(storage, sy, &mut row).then_some(())
                }
                _ => None,
            };
            for x in 0..width {
                let luma = if y < height / 4 {
                    // The four, left to right: black to white.
                    [0, 85, 170, 255][usize::from(x * 4 / width)]
                } else if y < height / 2 {
                    (u32::from(x) * 255 / u32::from(width - 1)) as u8
                } else {
                    match (source, fit, &picture) {
                        (Some(()), Some((w, _, px, _)), Some(p)) if x >= px && x < px + w => {
                            let sx = u32::from(x - px) * p.width / u32::from(w.max(1));
                            row.get(sx as usize).copied().unwrap_or(255)
                        }
                        _ => 255,
                    }
                };
                let level = dither(luma, x, y);
                band[usize::from(dy) * stride + usize::from(x / 4)] |= level << (6 - 2 * (x % 4));
            }
        }
        let view = FrameView::new(
            width,
            rows,
            PixelFormat::Gray2,
            stride,
            &band[..stride * usize::from(rows)],
        );
        panel.write_gray_rows(top, view)?;
        top += rows;
    }
    panel.refresh_gray()
}

/// `luma` as one of four levels (0 black, 3 white), ordered dithered.
fn dither(luma: u8, x: u16, y: u16) -> u8 {
    const BAYER: [[i32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    let threshold = BAYER[usize::from(y % 4)][usize::from(x % 4)];
    let offset = (2 * threshold - 15) * 255 / (32 * 3);
    let luma = (i32::from(luma) + offset).clamp(0, 255);
    ((luma * 3 + 127) / 255) as u8
}
