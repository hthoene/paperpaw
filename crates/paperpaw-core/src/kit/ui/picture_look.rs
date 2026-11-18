//! How the host draws pictures (#82): contrast and gamma as one 256-entry
//! table applied to a picture's grays before they are dithered, and the
//! dither itself (Atkinson, Floyd–Steinberg, ordered). Whole numbers only
//! (chips without floating point). Built once when a setting changes; a
//! draw does one table lookup a pixel, only when the table is not the
//! identity.

use alloc::vec::Vec;

/// The contrast of pictures ([`CONTRAST_LEVELS`]).
pub const CONTRAST: &str = "core.picture-contrast";
/// The gamma of pictures ([`GAMMA_LEVELS`]).
pub const GAMMA: &str = "core.picture-gamma";
/// The dither of pictures ([`DITHER_LEVELS`]).
pub const DITHER: &str = "core.picture-dither";

/// Lower, normal (today's look), higher, highest.
pub const CONTRAST_LEVELS: &[&str] = &[
    "core.picture-contrast.lower",
    "core.picture-contrast.normal",
    "core.picture-contrast.higher",
    "core.picture-contrast.highest",
];
/// Lightest, lighter, normal (today's look), darker, darkest: how dark the
/// middle grays are.
pub const GAMMA_LEVELS: &[&str] = &[
    "core.picture-gamma.lightest",
    "core.picture-gamma.lighter",
    "core.picture-gamma.normal",
    "core.picture-gamma.darker",
    "core.picture-gamma.darkest",
];
/// Atkinson (today's look), Floyd–Steinberg, ordered.
pub const DITHER_LEVELS: &[&str] = &[
    "core.picture-dither.atkinson",
    "core.picture-dither.floyd-steinberg",
    "core.picture-dither.ordered",
];

/// The step of each setting that is today's look.
const NORMAL: Steps = Steps {
    contrast: 1,
    gamma: 2,
    dither: 0,
};

/// How a picture's grays are turned into the few of the frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dither {
    /// Error spread to six pixels, seven eighths of it (`paperpaw_graphics::diffuse`).
    #[default]
    Atkinson,
    /// Error spread to four pixels, all of it: smoother tones, more specks in
    /// white paper. Starts with no error at the first row of each band.
    FloydSteinberg,
    /// A 4 by 4 Bayer pattern: no error carried, the same in every band and
    /// part, a regular grid.
    Ordered,
}

/// The chosen step of each picture setting (an index into its levels).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Steps {
    contrast: u8,
    gamma: u8,
    dither: u8,
}

impl Steps {
    /// The steps the texts of the three settings name; today's look for
    /// any text that names none.
    #[must_use]
    pub fn of(contrast: &str, gamma: &str, dither: &str) -> Self {
        let at = |levels: &[&str], text: &str, normal: u8| {
            levels
                .iter()
                .position(|level| *level == text)
                .map_or(normal, |at| at as u8)
        };
        Self {
            contrast: at(CONTRAST_LEVELS, contrast, NORMAL.contrast),
            gamma: at(GAMMA_LEVELS, gamma, NORMAL.gamma),
            dither: at(DITHER_LEVELS, dither, NORMAL.dither),
        }
    }
}

/// The look pictures are drawn with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PictureLook {
    steps: Steps,
    /// Contrast then gamma as one table; none where both are normal.
    curve: Option<[u8; 256]>,
}

impl PictureLook {
    /// Today's look: no table, Atkinson.
    #[must_use]
    pub const fn standard() -> Self {
        Self {
            steps: NORMAL,
            curve: None,
        }
    }

    /// The look of `steps`; the table is made here, once.
    #[must_use]
    pub fn new(steps: Steps) -> Self {
        let curve = (steps.contrast != NORMAL.contrast || steps.gamma != NORMAL.gamma).then(|| {
            let mut curve = [0u8; 256];
            for (gray, out) in curve.iter_mut().enumerate() {
                let x = gray as i32;
                *out = gamma(steps.gamma, contrast(steps.contrast, x)).clamp(0, 255) as u8;
            }
            curve
        });
        Self { steps, curve }
    }

    /// The steps this look was made from.
    #[must_use]
    pub const fn steps(&self) -> Steps {
        self.steps
    }

    /// The table every gray goes through before it is dithered, if it is
    /// not the identity.
    #[must_use]
    pub const fn curve(&self) -> Option<&[u8; 256]> {
        self.curve.as_ref()
    }

    /// The dither.
    #[must_use]
    pub const fn dither(&self) -> Dither {
        match self.steps.dither {
            1 => Dither::FloydSteinberg,
            2 => Dither::Ordered,
            _ => Dither::Atkinson,
        }
    }
}

impl Default for PictureLook {
    fn default() -> Self {
        Self::standard()
    }
}

/// Contrast at `step` of a gray `x` (0 to 255): lower pulls the ends in, higher
/// and highest blend in an S curve (smoothstep).
fn contrast(step: u8, x: i32) -> i32 {
    let s = x * x * (765 - 2 * x) / 65_025;
    match step {
        0 => x * 3 / 4 + 32,
        2 => (x + s) / 2,
        3 => s,
        _ => x,
    }
}

/// Gamma at `step` of a gray `x`: the middle grays lighter (towards the
/// mirrored square) or darker (towards the square), the ends stay.
fn gamma(step: u8, x: i32) -> i32 {
    let dark = x * x / 255;
    let light = 255 - (255 - x) * (255 - x) / 255;
    match step {
        0 => light,
        1 => (x + light) / 2,
        3 => (x + dark) / 2,
        4 => dark,
        _ => x,
    }
}

/// The 4 by 4 Bayer thresholds.
const BAYER: [[i32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// Dithers by Floyd–Steinberg or ordered, row by row from the row it was
/// made for; no state outlives the call that made it.
pub(super) struct Spread {
    dither: Dither,
    /// The error left to this row (first) and the next (Floyd–Steinberg).
    rows: [Vec<i16>; 2],
    /// The picture column of the first column and the picture row of the
    /// next row (the ordered pattern's phase).
    at: (usize, usize),
}

impl Spread {
    /// For rows `width` pixels wide whose first is at picture column `x`
    /// and whose next row is picture row `y`; none where memory is short.
    pub(super) fn new(dither: Dither, width: usize, (x, y): (usize, usize)) -> Option<Self> {
        let room = if dither == Dither::FloydSteinberg {
            width
        } else {
            0
        };
        let row = || {
            let mut row = Vec::new();
            row.try_reserve_exact(room).ok()?;
            row.resize(room, 0i16);
            Some(row)
        };
        Some(Self {
            dither,
            rows: [row()?, row()?],
            at: (x, y),
        })
    }

    /// Row `grays` as grays of `levels` levels (2 to 16), each passed to
    /// `put` with its column.
    pub(super) fn row(&mut self, grays: &[u8], levels: u8, mut put: impl FnMut(usize, u8)) {
        let top = i32::from(levels.clamp(2, 16)) - 1;
        let step = 255 / top;
        let quantise = |value: i32| (value.clamp(0, 255) * top + 127) / 255 * step;
        if self.dither == Dither::Ordered {
            let line = &BAYER[self.at.1 & 3];
            for (column, &gray) in grays.iter().enumerate() {
                let m = line[(self.at.0 + column) & 3];
                let value = i32::from(gray) + (2 * m - 15) * step / 32;
                put(column, quantise(value) as u8);
            }
        } else {
            let [here, below] = &mut self.rows;
            let last = grays.len().min(here.len()).saturating_sub(1);
            let mut right = 0i32;
            for (column, &gray) in grays.iter().enumerate().take(here.len()) {
                let value = i32::from(gray) + i32::from(here[column]) + right;
                let shown = quantise(value);
                let error = value - shown;
                let (to_right, down_left, down) = (error * 7 / 16, error * 3 / 16, error * 5 / 16);
                right = to_right;
                let add = |cell: &mut i16, part: i32| {
                    *cell = (i32::from(*cell) + part)
                        .clamp(i32::from(i16::MIN), i32::from(i16::MAX))
                        as i16;
                };
                if column > 0 {
                    add(&mut below[column - 1], down_left);
                }
                add(&mut below[column], down);
                if column < last {
                    add(&mut below[column + 1], error - to_right - down_left - down);
                }
                here[column] = 0;
                put(column, shown as u8);
            }
            self.rows.swap(0, 1);
        }
        self.at.1 += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(contrast: &str, gamma: &str, dither: &str) -> PictureLook {
        PictureLook::new(Steps::of(contrast, gamma, dither))
    }

    #[test]
    fn the_standard_look_is_today_s_and_has_no_table() {
        let normal = look(CONTRAST_LEVELS[1], GAMMA_LEVELS[2], DITHER_LEVELS[0]);
        assert_eq!(normal, PictureLook::standard());
        assert!(normal.curve().is_none());
        assert_eq!(normal.dither(), Dither::Atkinson);
        // A text that names no step is the normal one.
        assert_eq!(Steps::of("", "x", "y"), NORMAL);
    }

    #[test]
    fn contrast_and_gamma_move_the_middle_and_keep_order() {
        let darker = look(CONTRAST_LEVELS[1], GAMMA_LEVELS[4], DITHER_LEVELS[0]);
        let lighter = look(CONTRAST_LEVELS[1], GAMMA_LEVELS[0], DITHER_LEVELS[0]);
        let (dark, light) = (darker.curve().unwrap(), lighter.curve().unwrap());
        assert!(dark[128] < 128 && light[128] > 128);
        assert_eq!((dark[0], dark[255], light[0], light[255]), (0, 255, 0, 255));
        let highest = look(CONTRAST_LEVELS[3], GAMMA_LEVELS[2], DITHER_LEVELS[0]);
        let high = highest.curve().unwrap();
        assert!(high[64] < 64 && high[192] > 192);
        let lower = look(CONTRAST_LEVELS[0], GAMMA_LEVELS[2], DITHER_LEVELS[0]);
        let low = lower.curve().unwrap();
        assert!(low[0] > 0 && low[255] < 255);
        for curve in [dark, light, high, low] {
            assert!(curve.windows(2).all(|pair| pair[0] <= pair[1]));
        }
    }

    #[test]
    fn the_three_dithers_keep_the_tone_of_a_middle_gray() {
        for dither in [Dither::FloydSteinberg, Dither::Ordered] {
            let mut spread = Spread::new(dither, 64, (0, 0)).unwrap();
            let mut white = 0u32;
            for _ in 0..64 {
                spread.row(&[128; 64], 2, |_, gray| white += u32::from(gray == 255));
            }
            let share = white * 100 / (64 * 64);
            assert!((42..=58).contains(&share), "{dither:?}: {share} % white");
        }
    }

    #[test]
    fn black_and_white_stay_and_the_ordered_pattern_is_a_grid() {
        for dither in [Dither::FloydSteinberg, Dither::Ordered] {
            let mut spread = Spread::new(dither, 4, (0, 0)).unwrap();
            let mut shown = Vec::new();
            spread.row(&[0, 255, 0, 255], 2, |_, gray| shown.push(gray));
            assert_eq!(shown, [0, 255, 0, 255], "{dither:?}");
        }
        // The same phase in every band: a band begun at row 5 equals rows
        // 5 and on of a picture begun at row 0.
        let (mut whole, mut band) = (
            Spread::new(Dither::Ordered, 8, (0, 0)).unwrap(),
            Spread::new(Dither::Ordered, 8, (0, 5)).unwrap(),
        );
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for y in 0..7 {
            whole.row(&[100; 8], 2, |_, gray| {
                if y >= 5 {
                    a.push(gray);
                }
            });
        }
        for _ in 5..7 {
            band.row(&[100; 8], 2, |_, gray| b.push(gray));
        }
        assert_eq!(a, b);
    }
}
