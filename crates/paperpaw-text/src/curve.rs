//! How a glyph's coverage becomes ink: a table of 16 opacities, one for
//! each coverage level of a glyph bitmap. A contrast curve makes edges
//! darker or lighter, finer than widening by whole pixels; a sharp curve
//! draws each pixel black or white, without grays. A curve of one's own
//! is made of 16 values and read from and written as text (see
//! [`ToneCurve::parse_for`] for the grammar of a setting).

/// How the 16 coverage levels of a glyph are drawn: each as an opacity
/// from 0 to 255. Integers alone, so that chips without floating point
/// draw it at the cost of one lookup a pixel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToneCurve {
    alphas: [u8; 16],
}

/// Contrast curves, lightest first: opacity `255 * (level / 15) ^ e`
/// rounded, for `e` of 1.4, 1, 0.7 and 0.5. The second is coverage as it
/// is.
const CURVES: [[u8; 16]; 4] = [
    [
        0, 6, 15, 27, 40, 55, 71, 88, 106, 125, 145, 165, 187, 209, 232, 255,
    ],
    [
        0, 17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255,
    ],
    [
        0, 38, 62, 83, 101, 118, 134, 150, 164, 178, 192, 205, 218, 231, 243, 255,
    ],
    [
        0, 66, 93, 114, 132, 147, 161, 174, 186, 198, 208, 218, 228, 237, 246, 255,
    ],
];

impl ToneCurve {
    /// Coverage as it is: smooth edges, as dark as the font draws.
    pub const SMOOTH: ToneCurve = ToneCurve { alphas: CURVES[1] };

    /// The lightest step of [`ToneCurve::contrast`].
    pub const LIGHTEST: i8 = -1;
    /// The darkest step of [`ToneCurve::contrast`].
    pub const DARKEST: i8 = 2;

    /// Coverage on a contrast curve: `0` is [`ToneCurve::SMOOTH`], `1` darker
    /// and `2` darker still, `-1` lighter. Steps beyond those are the
    /// nearest. Empty stays empty and full stays full.
    pub fn contrast(step: i8) -> ToneCurve {
        let index = step.clamp(Self::LIGHTEST, Self::DARKEST) - Self::LIGHTEST;
        ToneCurve {
            alphas: CURVES[index as usize],
        }
    }

    /// This curve without grays: a level drawn at `threshold` opacity or
    /// more (1 to 255) is drawn full, a lighter one not at all. A
    /// threshold below half keeps thin strokes of small text. A darker
    /// curve below it draws fuller letters.
    pub fn sharp(self, threshold: u8) -> ToneCurve {
        let threshold = threshold.max(1);
        ToneCurve {
            alphas: self
                .alphas
                .map(|alpha| if alpha >= threshold { 255 } else { 0 }),
        }
    }

    /// Whether it draws without grays: every level empty or full.
    pub fn is_sharp(&self) -> bool {
        self.alphas.iter().all(|alpha| matches!(alpha, 0 | 255))
    }

    /// The opacity of coverage level `level` (0 to 15; higher ones read
    /// as their low four bits).
    #[inline]
    pub fn alpha(&self, level: u8) -> u8 {
        self.alphas[usize::from(level & 0x0f)]
    }

    /// A curve of its own: the opacity, 0 to 255, of coverage levels 0
    /// to 15. Level 0 is made 0 (no ink where nothing is, or a whole
    /// band would turn gray); the others need not rise, so a curve may
    /// thin or fatten edges as it likes.
    ///
    /// The 16 values are knots over coverage 0 to 255 (level `k` is
    /// coverage `17 * k`): a renderer with finer coverage interpolates
    /// linearly between them, so a curve made today holds for it.
    pub const fn from_alphas(alphas: [u8; 16]) -> ToneCurve {
        let mut alphas = alphas;
        alphas[0] = 0;
        ToneCurve { alphas }
    }

    /// The 16 opacities, for coverage levels 0 to 15.
    pub const fn alphas(&self) -> [u8; 16] {
        self.alphas
    }
}

impl Default for ToneCurve {
    fn default() -> Self {
        Self::SMOOTH
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_curve_of_its_own_keeps_level_0_empty() {
        let mut alphas = [200; 16];
        alphas[15] = 255;
        let tone = ToneCurve::from_alphas(alphas);
        assert_eq!(tone.alpha(0), 0);
        assert_eq!((tone.alpha(1), tone.alpha(15)), (200, 255));
        assert_eq!(
            ToneCurve::from_alphas(ToneCurve::SMOOTH.alphas()),
            ToneCurve::SMOOTH
        );
        // Curves need not rise.
        assert_eq!(
            ToneCurve::from_alphas([
                0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255
            ])
            .alpha(2),
            0
        );
    }

    #[test]
    fn smooth_is_coverage_as_it_is() {
        for level in 0..16u8 {
            assert_eq!(ToneCurve::SMOOTH.alpha(level), level * 17);
        }
        assert_eq!(ToneCurve::contrast(0), ToneCurve::SMOOTH);
        assert_eq!(ToneCurve::default(), ToneCurve::SMOOTH);
        assert!(!ToneCurve::SMOOTH.is_sharp());
    }

    #[test]
    fn curves_keep_their_ends_and_order() {
        for step in ToneCurve::LIGHTEST..=ToneCurve::DARKEST {
            let tone = ToneCurve::contrast(step);
            assert_eq!((tone.alpha(0), tone.alpha(15)), (0, 255), "{step}");
            // Each curve rises with coverage.
            assert!(tone.alphas.windows(2).all(|w| w[0] < w[1]), "{step}");
        }
        // Each step darker than the one before, at every level between.
        for step in ToneCurve::LIGHTEST..ToneCurve::DARKEST {
            let (lighter, darker) = (ToneCurve::contrast(step), ToneCurve::contrast(step + 1));
            assert!(
                (1..15).all(|l| lighter.alpha(l) < darker.alpha(l)),
                "{step}"
            );
        }
        // Steps beyond the ends are the ends.
        assert_eq!(
            ToneCurve::contrast(-9),
            ToneCurve::contrast(ToneCurve::LIGHTEST)
        );
        assert_eq!(
            ToneCurve::contrast(9),
            ToneCurve::contrast(ToneCurve::DARKEST)
        );
    }

    #[test]
    fn sharp_curves_are_black_or_white() {
        let sharp = ToneCurve::SMOOTH.sharp(96);
        assert!(sharp.is_sharp());
        // From three eighths on: level 6 (102) is drawn, level 5 (85) not.
        assert_eq!((sharp.alpha(5), sharp.alpha(6)), (0, 255));
        assert_eq!((sharp.alpha(0), sharp.alpha(15)), (0, 255));
        // At half: level 8 (136) on.
        let half = ToneCurve::SMOOTH.sharp(128);
        assert_eq!((half.alpha(7), half.alpha(8)), (0, 255));
        // A threshold of 0 still leaves empty pixels empty.
        assert_eq!(ToneCurve::SMOOTH.sharp(0).alpha(0), 0);
        assert_eq!(ToneCurve::SMOOTH.sharp(0).alpha(1), 255);
    }

    #[test]
    fn darker_curves_draw_sharp_letters_fuller() {
        // The first level drawn, lightest to darkest, at three eighths.
        let first = |step| {
            let sharp = ToneCurve::contrast(step).sharp(96);
            (0..16u8).find(|l| sharp.alpha(*l) == 255)
        };
        let firsts = [-1, 0, 1, 2].map(first);
        assert_eq!(firsts, [Some(8), Some(6), Some(4), Some(3)]);
    }

    #[test]
    fn levels_read_as_their_low_bits() {
        assert_eq!(ToneCurve::SMOOTH.alpha(0x1f), 255);
        assert_eq!(ToneCurve::SMOOTH.alpha(0x10), 0);
    }
}
