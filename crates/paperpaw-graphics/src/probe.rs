//! A framebuffer that keeps no pixels but where ink lands
//! ([`Framebuffer::probe`]): what text drawn on it covers, measured by the
//! very code that draws it, so that a measurement can never differ from the
//! drawing (shaping, joining, marks and ligatures included).

use paperpaw_platform::PixelFormat;

use crate::canvas::Canvas;
use crate::framebuffer::Framebuffer;

/// From what coverage a pixel of a gray or colour frame counts as inked:
/// at least half covered.
const HALF: u8 = 128;

/// The pixels a probe saw inked: left and top inclusive, right and bottom
/// exclusive, in the coordinates the probe's canvas was drawn in; and from
/// what coverage a pixel counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Probe {
    ink: Option<[i64; 4]>,
    inked_from: u8,
}

/// Where the ink of what was drawn on a probe lies ([`Framebuffer::probed`]):
/// the pixels at or above its threshold, `left` and `top` inclusive, `right` and
/// `bottom` exclusive, in the coordinates it was drawn in (saturated to
/// `i32`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct InkBounds {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Framebuffer {
    /// A framebuffer that keeps no pixels and costs no memory: drawing on
    /// its canvas only records where masks (glyphs) put pixels covered at
    /// least `inked_from` (out of 255), unclipped, read back with
    /// [`Framebuffer::probed`]. Pass a frame's
    /// [`ink_threshold`](Framebuffer::ink_threshold) to see ink as that
    /// frame shows it. Anything else drawn on it is dropped, as on a
    /// [`blind`](Framebuffer::blind) one; unlike that one, its canvas is not
    /// blind, so text is drawn glyph by glyph.
    pub fn probe(inked_from: u8) -> Self {
        let mut probe = Self::blind(u16::MAX, u16::MAX);
        probe.set_probe(Some(Probe {
            ink: None,
            inked_from: inked_from.max(1),
        }));
        probe
    }

    /// From what coverage this frame shows a pixel of a mask inked: in black
    /// and white, its [`mono_coverage`](Framebuffer::mono_coverage) (the
    /// pixel turns black); in grays and colour, at least half covered.
    pub fn ink_threshold(&self) -> u8 {
        match self.format() {
            PixelFormat::Mono1 => self.mono_coverage().max(1),
            _ => HALF,
        }
    }

    /// Whether it is a [`probe`](Framebuffer::probe).
    pub fn is_probe(&self) -> bool {
        self.probe_state().is_some()
    }

    /// The ink a [`probe`](Framebuffer::probe) saw, `None` where nothing was
    /// inked or it is not a probe.
    pub fn probed(&self) -> Option<InkBounds> {
        let [left, top, right, bottom] = self.probe_state()?.ink?;
        let fit = |v: i64| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        Some(InkBounds {
            left: fit(left),
            top: fit(top),
            right: fit(right),
            bottom: fit(bottom),
        })
    }

    /// Adds the pixel at `(x, y)` (canvas coordinates) to the ink seen.
    fn probe_ink(&mut self, x: i64, y: i64) {
        let Some(probe) = self.probe_state_mut() else {
            return;
        };
        let pixel = [x, y, x + 1, y + 1];
        probe.ink = Some(probe.ink.map_or(pixel, |[l, t, r, b]| {
            [l.min(x), t.min(y), r.max(x + 1), b.max(y + 1)]
        }));
    }
}

impl Canvas<'_> {
    /// [`draw_mask`](Canvas::draw_mask) on a probe: records the pixels
    /// covered from its threshold on, unclipped, in this canvas's
    /// coordinates.
    pub(crate) fn probe_mask(
        &mut self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        coverage: impl Fn(u32, u32) -> u8,
    ) {
        let Some(inked_from) = self.framebuffer_mut().probe_state().map(|p| p.inked_from) else {
            return;
        };
        for py in 0..height {
            for px in 0..width {
                if coverage(px, py) >= inked_from {
                    let at = (i64::from(x) + i64::from(px), i64::from(y) + i64::from(py));
                    self.framebuffer_mut().probe_ink(at.0, at.1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use paperpaw_platform::Color;

    use super::*;

    #[test]
    fn a_probe_records_half_covered_pixels_unclipped() {
        let mut probe = Framebuffer::probe(HALF);
        assert!(probe.is_probe());
        {
            let mut canvas = probe.canvas();
            assert!(!canvas.is_blind());
            // A 3×2 mask far left of the screen: only coverage >= 128 counts.
            canvas.draw_mask(-40, -7, 3, 2, Color::BLACK, |x, y| match (x, y) {
                (0, 0) => 127,
                (1, 0) => 128,
                (2, 1) => 255,
                _ => 0,
            });
            // Other drawing is dropped.
            canvas.fill_rect(0, 0, 10, 10, Color::BLACK);
        }
        assert_eq!(
            probe.probed(),
            Some(InkBounds {
                left: -39,
                top: -7,
                right: -37,
                bottom: -5
            })
        );
    }

    #[test]
    fn nothing_inked_and_other_framebuffers_probe_nothing() {
        let mut probe = Framebuffer::probe(HALF);
        probe
            .canvas()
            .draw_mask(0, 0, 4, 4, Color::BLACK, |_, _| 127);
        assert_eq!(probe.probed(), None);
        let mut low = Framebuffer::probe(96);
        low.canvas().draw_mask(0, 0, 1, 1, Color::BLACK, |_, _| 96);
        assert!(low.probed().is_some(), "inked from its own threshold");
        let mut mono = Framebuffer::new(8, 8, paperpaw_platform::PixelFormat::Mono1).unwrap();
        mono.set_mono_coverage(96);
        assert_eq!(mono.ink_threshold(), 96);
        let gray = Framebuffer::new(8, 8, paperpaw_platform::PixelFormat::Gray8).unwrap();
        assert_eq!(gray.ink_threshold(), HALF);
        let blind = Framebuffer::blind(10, 10);
        assert!(!blind.is_probe());
        assert_eq!(blind.probed(), None);
    }
}
