//! Masks (glyphs, icons) blended a row at a time
//! ([`Canvas::draw_mask`](crate::Canvas::draw_mask)) where the frame is
//! black and white or in packed grays, as most panels are: every glyph
//! of a page comes here, pixel by pixel.
//!
//! What does not change from pixel to pixel is worked out once per mask:
//! the stored value of the colour, the threshold of black and white, a
//! level's step. The pixels come out exactly as
//! [`Framebuffer::blend_pixel`] blends each one.

use paperpaw_platform::{Color, PixelFormat, Rect};

use crate::framebuffer::Framebuffer;

/// The most pixels of a row asked for at once: a buffer of this many on the
/// stack, filled by the caller, so that the work that does not change from
/// pixel to pixel is done once for the run, not asked for again by each.
pub(crate) const CHUNK: usize = 64;

/// The eight bytes of `word` (the first in the lowest) that are at least
/// `least` (1 to 255), one bit each in one byte, the first in its highest
/// bit: eight pixels in a few operations, not eight tests. Each byte is
/// compared in its own place (its low seven bits with `least`'s, then its
/// high bit), so that nothing borrows from the next; the bits are gathered by
/// one multiplication whose partial products land on bits no other one does.
#[inline]
fn gather_at_least(word: u64, least: u8) -> u8 {
    const HIGH: u64 = 0x8080_8080_8080_8080;
    const ONES: u64 = 0x0101_0101_0101_0101;
    const GATHER: u64 = 1 | 1 << 9 | 1 << 18 | 1 << 27 | 1 << 36 | 1 << 45 | 1 << 54 | 1 << 63;
    // The bytes whose low seven bits are at least `least`'s low seven bits.
    let low = ((word & !HIGH) | HIGH).wrapping_sub(u64::from(least & 0x7f) * ONES);
    let at_least = match least {
        128 => word,
        0..=127 => low | word,
        _ => low & word,
    };
    (((at_least & HIGH) >> 7).wrapping_mul(GATHER) >> 56) as u8
}

impl Framebuffer {
    /// Whether [`Framebuffer::blend_mask_rows`] draws this frame (black and
    /// white or packed grays; a frame that is blind draws nothing and counts).
    pub(crate) fn has_row_masks(&self) -> bool {
        self.is_blind()
            || matches!(
                self.format(),
                PixelFormat::Mono1 | PixelFormat::Gray2 | PixelFormat::Gray4
            )
    }

    /// Blends `color` over `rect` (inside the band) as
    /// [`Framebuffer::blend_pixel`] blends each pixel, with the coverage
    /// `coverage(dx, dy)` (0 to 255), counted from `from` at `rect`'s
    /// corner. `false` where the format is drawn otherwise: nothing is
    /// drawn then. Does not mark anything dirty.
    pub(crate) fn blend_mask(
        &mut self,
        rect: Rect,
        from: (u32, u32),
        color: Color,
        coverage: impl Fn(u32, u32) -> u8,
    ) -> bool {
        self.blend_mask_rows(rect, from, color, |dy, dx, out| {
            for (n, a) in out.iter_mut().enumerate() {
                *a = coverage(dx + n as u32, dy);
            }
        })
    }

    /// [`Framebuffer::blend_mask`] with the coverage given a run of a row at
    /// a time: `rows(dy, dx, out)` fills `out` with the coverage of the
    /// pixels `dx`, `dx + 1`... of row `dy` (at most [`CHUNK`]), rows in
    /// order, a row left to right.
    pub(crate) fn blend_mask_rows(
        &mut self,
        rect: Rect,
        from: (u32, u32),
        color: Color,
        rows: impl FnMut(u32, u32, &mut [u8]),
    ) -> bool {
        if self.is_blind() {
            return true;
        }
        match self.format() {
            PixelFormat::Mono1 => self.mask_mono(rect, from, color, rows),
            PixelFormat::Gray2 | PixelFormat::Gray4 => self.mask_gray(rect, from, color, rows),
            _ => return false,
        }
        true
    }

    /// The pixels from the coverage set on take the colour, the bits of a
    /// byte gathered and written together.
    fn mask_mono(
        &mut self,
        rect: Rect,
        from: (u32, u32),
        color: Color,
        mut rows: impl FnMut(u32, u32, &mut [u8]),
    ) {
        let ink: u8 = if self.encode(color) & 1 != 0 { 0xFF } else { 0 };
        let least = self.mono_coverage().max(1);
        let (data, stride, top) = self.rows_mut();
        let (left, right) = (usize::from(rect.x), rect.right() as usize);
        let mut run = [0u8; CHUNK];
        // The pixels of `run` (at most 8, from `x`, inside one byte) that
        // take the colour, as the bits of that byte.
        let bits_of = |run: &[u8], x: usize| {
            // Eight pixels from the byte's first.
            if let (Ok(group), 0) = (<[u8; 8]>::try_from(run), x % 8) {
                return gather_at_least(u64::from_le_bytes(group), least);
            }
            run.iter().enumerate().fold(0u8, |bits, (i, &a)| {
                bits | (u8::from(a >= least) * (0x80 >> ((x + i) % 8)))
            })
        };
        for (dy, y) in (from.1..).zip(rect.y..rect.bottom() as u16) {
            let row = usize::from(y - top) * stride;
            let mut x = left;
            while x < right {
                // A run of the row at once (a glyph's row is one run), ending
                // on a byte of the row where it does not end the row.
                let n = (right - x).min(CHUNK - x % 8);
                rows(dy, from.0 + (x - left) as u32, &mut run[..n]);
                // The first pixels, to the next byte.
                let head = ((8 - x % 8) % 8).min(n);
                if head > 0 {
                    let bits = bits_of(&run[..head], x);
                    let at = row + x / 8;
                    data[at] = (data[at] & !bits) | (ink & bits);
                }
                // Whole bytes: eight pixels, one byte.
                let mut at = row + (x + head) / 8;
                let mut done = head;
                while n - done >= 8 {
                    let bits = bits_of(&run[done..done + 8], 0);
                    if bits != 0 {
                        data[at] = (data[at] & !bits) | (ink & bits);
                    }
                    at += 1;
                    done += 8;
                }
                // The last pixels, in part of a byte.
                if done < n {
                    let bits = bits_of(&run[done..n], x + done);
                    data[at] = (data[at] & !bits) | (ink & bits);
                }
                x += n;
            }
        }
    }

    /// Each pixel mixed in gray with what lies under it, as
    /// `blend_pixel` mixes, without its division: a level's step divides
    /// 255 (85 for four grays, 17 for sixteen).
    fn mask_gray(
        &mut self,
        rect: Rect,
        from: (u32, u32),
        color: Color,
        mut rows: impl FnMut(u32, u32, &mut [u8]),
    ) {
        let bpp = usize::from(self.bpp());
        let max = (1u32 << bpp) - 1;
        let step = 255 / max;
        let full = self.encode(color) as u8;
        let over = u32::from(color.luma());
        let inverted = self.is_inverted();
        let (data, stride, top) = self.rows_mut();
        let (left, right) = (usize::from(rect.x), rect.right() as usize);
        let mut run = [0u8; CHUNK];
        for (dy, y) in (from.1..).zip(rect.y..rect.bottom() as u16) {
            let row = usize::from(y - top) * stride;
            let mut x = left;
            while x < right {
                let n = (right - x).min(CHUNK);
                rows(dy, from.0 + (x - left) as u32, &mut run[..n]);
                for &a in &run[..n] {
                    let a = u32::from(a);
                    let at_x = x;
                    x += 1;
                    if a == 0 {
                        continue;
                    }
                    let bit = at_x * bpp;
                    let (at, shift) = (row + bit / 8, 8 - bpp - bit % 8);
                    let value = if a == 255 {
                        full
                    } else {
                        // What shows there now and the mix, as the page means
                        // them (not inverted).
                        let level = u32::from(data[at] >> shift) & max;
                        let under = if inverted {
                            255 - level * step
                        } else {
                            level * step
                        };
                        let mixed = (over * a + under * (255 - a) + 127) / 255;
                        let shown = if inverted { 255 - mixed } else { mixed };
                        ((shown * max + 127) / 255) as u8
                    };
                    let mask = (max as u8) << shift;
                    data[at] = (data[at] & !mask) | ((value << shift) & mask);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use paperpaw_platform::Rect;

    use super::*;

    /// A random number from `state` (xorshift), for masks of every shape.
    fn next(state: &mut u32) -> u32 {
        *state ^= *state << 13;
        *state ^= *state >> 17;
        *state ^= *state << 5;
        *state
    }

    /// A glyph's coverage: empty and full pixels most of all, as in
    /// letters, and edges of any coverage.
    fn glyph(state: &mut u32, width: usize, height: usize) -> Vec<u8> {
        (0..width * height)
            .map(|_| match next(state) % 8 {
                0..=2 => 0,
                3..=4 => 255,
                _ => next(state) as u8,
            })
            .collect()
    }

    /// Masks drawn a row at a time come out byte for byte as drawn pixel
    /// by pixel: in black and white and grays, in colours light and dark,
    /// inverted or not, at every place in a byte, cut by a clip and moved
    /// by an offset, over what is drawn there.
    #[test]
    fn masks_are_drawn_as_each_pixel_is_blended() {
        let colors = [
            Color::BLACK,
            Color::WHITE,
            Color::gray(90),
            Color::gray(200),
            Color::rgb(200, 30, 90),
        ];
        let mut state = 0x2545_f491;
        for format in [PixelFormat::Mono1, PixelFormat::Gray2, PixelFormat::Gray4] {
            for inverted in [false, true] {
                for _ in 0..400 {
                    let mut fast = Framebuffer::banded(211, 40, 24, format).unwrap();
                    fast.set_top(next(&mut state) as u16 % 17);
                    fast.set_inverted(inverted);
                    fast.set_mono_coverage(next(&mut state) as u8);
                    // Something under the masks to blend with.
                    for _ in 0..4 {
                        let gray = Color::gray(next(&mut state) as u8);
                        let (x, y) = (next(&mut state) % 211, next(&mut state) % 40);
                        let area = Rect::new(x as u16, y as u16, 20, 9);
                        fast.canvas().fill(area, gray);
                    }
                    let mut each = Framebuffer::banded(211, 40, 24, format).unwrap();
                    each.set_top(fast.band().y);
                    each.set_inverted(inverted);
                    each.set_mono_coverage(fast.mono_coverage());
                    each.write_rows(fast.band().y, fast.view());
                    fast.take_dirty();

                    // Up to three runs of 64 across, and glyph-sized ones.
                    let wide = if next(&mut state).is_multiple_of(2) {
                        150
                    } else {
                        23
                    };
                    let (width, height) = (next(&mut state) % wide + 1, next(&mut state) % 19 + 1);
                    let mask = glyph(&mut state, width as usize, height as usize);
                    let level = |dx: u32, dy: u32| mask[(dy * width + dx) as usize];
                    let x = (next(&mut state) % 240) as i32 - 15;
                    let y = (next(&mut state) % 60) as i32 - 15;
                    let (dx, dy) = (
                        (next(&mut state) % 9) as i32 - 4,
                        (next(&mut state) % 9) as i32,
                    );
                    let clip = Rect::new(
                        (next(&mut state) % 100) as u16,
                        (next(&mut state) % 30) as u16,
                        (next(&mut state) % 211) as u16,
                        (next(&mut state) % 40) as u16,
                    );
                    let [r, g, b, pick] = next(&mut state).to_le_bytes();
                    let color = colors
                        .get(usize::from(pick % 8))
                        .copied()
                        .unwrap_or(Color::rgb(r, g, b));
                    let mut canvas = fast.canvas().offset(dx, dy);
                    canvas
                        .clipped(clip)
                        .draw_mask(x, y, width, height, color, level);
                    let mut canvas = each.canvas().offset(dx, dy);
                    canvas.clipped(clip).mask_wide(
                        x.into(),
                        y.into(),
                        width.into(),
                        height.into(),
                        color,
                        |dx, dy| level(dx as u32, dy as u32),
                    );
                    let case = (format, inverted, x, y, width, height, clip, color);
                    assert_eq!(fast.view().data, each.view().data, "{case:?}");
                    assert_eq!(fast.take_dirty(), each.take_dirty(), "{case:?}");
                }
            }
        }
    }

    #[test]
    fn eight_pixels_gather_into_one_byte_as_the_loop_sets_them() {
        let mut state = 99u32;
        for least in 1..=255u8 {
            for round in 0..200 {
                let group: [u8; 8] = core::array::from_fn(|i| match (round + i) % 4 {
                    0 => least,
                    1 => least.wrapping_sub(1),
                    _ => next(&mut state) as u8,
                });
                let want = group
                    .iter()
                    .enumerate()
                    .fold(0u8, |bits, (i, &a)| bits | u8::from(a >= least) << (7 - i));
                assert_eq!(gather_at_least(u64::from_le_bytes(group), least), want);
            }
        }
    }

    /// A mask is asked for a run of a row at a time (a glyph's row is one
    /// call), not a pixel at a time: on the X4 each pixel's call was most of
    /// what drawing a word cost (#393).
    #[test]
    fn a_mask_is_asked_for_once_a_row_up_to_a_run_of_sixty_four() {
        for format in [PixelFormat::Mono1, PixelFormat::Gray2, PixelFormat::Gray4] {
            for (left, width) in [(0u16, 20u16), (3, 20), (5, 61), (3, 64), (3, 150)] {
                let mut frame = Framebuffer::new(211, 12, format).unwrap();
                let rect = Rect::new(left, 1, width, 9);
                let (mut calls, mut pixels) = (0, 0);
                frame.blend_mask_rows(rect, (0, 0), Color::BLACK, |_, _, out| {
                    calls += 1;
                    pixels += out.len();
                    out.fill(255);
                });
                assert_eq!(pixels, usize::from(width) * 9, "{format:?} {left}+{width}");
                // At most a run of 64 from the start of its byte: 64 pixels of a
                // row not on a byte's start take two.
                let per_row =
                    usize::from(width).div_ceil(64) + usize::from(left % 8 != 0 && width > 56);
                assert!(
                    calls <= 9 * per_row,
                    "{format:?} {left}+{width}: {calls} calls"
                );
            }
        }
    }
}
