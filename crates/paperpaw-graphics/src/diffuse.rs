//! Error diffusion after Atkinson's ([`Atkinson`]): how pictures become
//! the few grays of a panel, row by row from the top. Each pixel's error
//! is passed on, seven eighths of it: two eighths to the pixel after it,
//! one to the one after that, to each of the three below and to the one
//! below those. Line drawings stay clean (few specks in white paper, no
//! grid as an ordered dither leaves); photos and covers keep their tones,
//! the lightest and darkest a little lighter and darker (Atkinson's own,
//! six eighths, loses a cover's pale sky; all of it, as Floyd and
//! Steinberg's, speckles drawings). Integers only (chips without floating
//! point), two rows of error at a time.

use alloc::vec::Vec;

/// The error a picture's rows leave to the next two, `width` pixels each.
#[derive(Debug, Clone, Default)]
pub struct Atkinson {
    /// The error left to the row to come (first) and the one after.
    rows: [Vec<i16>; 2],
}

impl Atkinson {
    /// For rows `width` pixels wide, with no error yet; none where memory
    /// is short.
    pub fn new(width: usize) -> Option<Self> {
        let row = || {
            let mut row = Vec::new();
            row.try_reserve_exact(width).ok()?;
            row.resize(width, 0i16);
            Some(row)
        };
        Some(Self {
            rows: [row()?, row()?],
        })
    }

    /// The width of its rows.
    pub fn width(&self) -> usize {
        self.rows[0].len()
    }

    /// No error any more, as at a picture's first row.
    pub fn reset(&mut self) {
        for row in &mut self.rows {
            row.fill(0);
        }
    }

    /// The error of `other` (as wide) in place of its own.
    pub fn copy_from(&mut self, other: &Atkinson) {
        for (row, theirs) in self.rows.iter_mut().zip(&other.rows) {
            row.copy_from_slice(theirs);
        }
    }

    /// Row `grays` (as wide as its rows) as grays of `levels` levels (2
    /// to 16: black and white, four grays, sixteen), each passed to `put`
    /// with its column as the gray of its level (0 to 255); then ready
    /// for the next row.
    pub fn row(&mut self, grays: &[u8], levels: u8, mut put: impl FnMut(usize, u8)) {
        let top = i32::from(levels.clamp(2, 16)) - 1;
        // A level's step: whole for 2, 4 and 16 grays (255 = 3 × 5 × 17),
        // so no division for each pixel.
        let step = 255 / top;
        let [here, below] = &mut self.rows;
        // The cells stay far inside `i16` (a pixel's part is at most 255,
        // a cell holds four of them), so none needs to saturate.
        let n = grays.len().min(here.len());
        let (grays, here, below) = (&grays[..n], &mut here[..n], &mut below[..n]);
        // The error left to the two pixels after this one, and the parts
        // of the two pixels before it (the cell below the one before is
        // done once this one's part is known).
        let mut ahead = (0i32, 0i32);
        let mut before = (0i32, 0i32);
        for column in 0..n {
            let value = i32::from(grays[column]) + i32::from(here[column]) + ahead.0;
            let level = (value.clamp(0, 255) * top + 127) / 255;
            let shown = level * step;
            let part = (value - shown) / 8;
            ahead = (ahead.1 + 2 * part, part);
            if column > 0 {
                below[column - 1] += (before.0 + before.1 + part) as i16;
            }
            before = (before.1, part);
            // The row after the next, where this pixel's error is spent.
            here[column] = part as i16;
            put(column, shown as u8);
        }
        if let Some(last) = n.checked_sub(1) {
            below[last] += (before.0 + before.1) as i16;
        }
        self.rows.swap(0, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn black_and_white_stay_and_a_middle_gray_is_half_white() {
        let mut atkinson = Atkinson::new(64).unwrap();
        let mut white = 0;
        for _ in 0..64 {
            atkinson.row(&[128; 64], 2, |_, gray| white += u32::from(gray == 255));
        }
        let share = white * 100 / (64 * 64);
        assert!((40..=60).contains(&share), "{share} % white");
        let mut shown = Vec::new();
        atkinson.reset();
        atkinson.row(&[0, 255, 0, 255], 2, |_, gray| shown.push(gray));
        assert_eq!(&shown[..4], [0, 255, 0, 255]);
    }

    #[test]
    fn four_grays_are_their_levels() {
        let mut atkinson = Atkinson::new(4).unwrap();
        let mut shown = Vec::new();
        atkinson.row(&[0, 85, 170, 255], 4, |_, gray| shown.push(gray));
        assert_eq!(shown, [0, 85, 170, 255]);
    }

    /// The first, plainly written version (saturating cell by cell).
    fn reference(rows: &mut [Vec<i16>; 2], grays: &[u8], levels: u8, out: &mut Vec<u8>) {
        let top = i32::from(levels.clamp(2, 16)) - 1;
        let step = 255 / top;
        let [here, below] = rows;
        let last = grays.len().min(here.len()).saturating_sub(1);
        let mut ahead = (0i32, 0i32);
        let sat = |v: i32| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        let add = |cell: &mut i16, part: i32| *cell = sat(i32::from(*cell) + part);
        for (column, &gray) in grays.iter().enumerate().take(here.len()) {
            let value = i32::from(gray) + i32::from(here[column]) + ahead.0;
            let shown = ((value.clamp(0, 255) * top + 127) / 255) * step;
            let part = (value - shown) / 8;
            ahead = (ahead.1 + 2 * part, part);
            if column > 0 {
                add(&mut below[column - 1], part);
            }
            add(&mut below[column], part);
            if column < last {
                add(&mut below[column + 1], part);
            }
            here[column] = sat(part);
            out.push(shown as u8);
        }
        rows.swap(0, 1);
    }

    #[test]
    fn rows_are_those_of_the_plain_version() {
        let mut seed = 0x2545_f491u32;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        for (width, levels) in [(1, 2), (2, 4), (3, 16), (37, 2), (64, 4), (65, 16), (9, 7)] {
            let mut fast = Atkinson::new(width).unwrap();
            let mut rows = [vec![0i16; width], vec![0i16; width]];
            for line in 0..40 {
                let grays: Vec<u8> = (0..width)
                    .map(|_| match (line, next() % 5) {
                        (0..=4, _) => 0,
                        (5..=9, _) => 255,
                        (_, 0) => 0,
                        (_, 1) => 255,
                        (_, n) => next() as u8 ^ n as u8,
                    })
                    .collect();
                let (mut a, mut b) = (Vec::new(), Vec::new());
                fast.row(&grays, levels, |_, gray| a.push(gray));
                reference(&mut rows, &grays, levels, &mut b);
                assert_eq!(a, b, "width {width}, {levels} levels, row {line}");
            }
        }
    }
}
