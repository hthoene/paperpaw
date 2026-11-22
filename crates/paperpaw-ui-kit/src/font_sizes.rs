//! The sizes of book text that look different.
//!
//! The size of book text (`core.font-size`) is in points; the reading font
//! draws it at `points × dpi / 72` pixels. A font pack holds a few pixel
//! sizes and draws every size at the nearest of them, so neighbouring sizes
//! can look the same (on the 217 dpi of the X4 Pro and the X4 Classic, 9 and
//! 10 points are both 28 px of their packs). Screens that change the size offer only the smallest of the sizes
//! that look alike, as Beta 3 did: a step goes on to the next size that looks
//! different, and a size kept that looks like a smaller one is shown as that
//! one. A scalable font draws every size apart, so every size is offered.
use paperpaw_sdk::native::{
    Error, Host,
    fonts::{Font, FontRole},
    snapshot,
};

/// The setting of the size of book text, in points.
pub const SETTING: &str = paperpaw_sdk::builtin::settings::FONT_SIZE.static_str();

/// What tells two sizes apart: the width of this text at them.
const SAMPLE: &str = "Hamburgefonstiv";

/// What a size of book text looks like; sizes drawn alike look alike.
pub trait Looks {
    /// The look of `points`.
    ///
    /// # Errors
    /// The host's refusal to measure.
    fn looks(&self, points: i64) -> Result<u32, Error>;
}

/// The reading font as this screen draws it.
#[derive(Debug)]
pub struct Reading<'h> {
    font: Font<'h>,
    dpi: f32,
}

impl<'h> Reading<'h> {
    /// The reading font and the screen's dots per inch.
    ///
    /// # Errors
    /// The host's refusal of the font or of the snapshot.
    pub fn acquire(host: &'h Host<'h>) -> Result<Self, Error> {
        let font = Font::acquire_role(host, FontRole::Reading)?;
        match snapshot::capture_snapshot(host) {
            Ok(display) => Ok(Self {
                font,
                dpi: f32::from(display.display_dpi().max(1)),
            }),
            Err(error) => {
                font.release()?;
                Err(error)
            }
        }
    }

    /// Releases the font.
    ///
    /// # Errors
    /// The host's refusal.
    pub fn release(self) -> Result<(), Error> {
        self.font.release()
    }
}

impl Looks for Reading<'_> {
    fn looks(&self, points: i64) -> Result<u32, Error> {
        let px = points.max(1) as f32 * self.dpi / 72.0;
        self.font.measure_text(px, SAMPLE).map(f32::to_bits)
    }
}

/// The smallest of the sizes from `min` that look as `points` does.
///
/// # Errors
/// The refusal of `looks`.
pub fn first_alike(looks: &impl Looks, points: i64, (min, max): (i64, i64)) -> Result<i64, Error> {
    let mut points = points.clamp(min, max.max(min));
    let look = looks.looks(points)?;
    while points > min && looks.looks(points - 1)? == look {
        points -= 1;
    }
    Ok(points)
}

/// The size a step from `from` to `to` comes to: the first size that way
/// from `to` on that looks different from `from`, as [`first_alike`] shows
/// it; `from` where none that way does.
///
/// # Errors
/// The refusal of `looks`.
pub fn stepped(
    looks: &impl Looks,
    from: i64,
    to: i64,
    (min, max): (i64, i64),
) -> Result<i64, Error> {
    let max = max.max(min);
    let direction = (to - from).signum();
    if direction == 0 {
        return Ok(from);
    }
    let before = looks.looks(from)?;
    let mut points = to.clamp(min, max);
    while looks.looks(points)? == before {
        let next = (points + direction).clamp(min, max);
        if next == points {
            return Ok(from);
        }
        points = next;
    }
    first_alike(looks, points, (min, max))
}

/// How setting `key` shows `value`: for the size of book text the smallest
/// size the reading font draws alike ([`first_alike`]); any other value as
/// it is, and so where the host has no reading font.
///
/// # Errors
/// The host's refusal to measure.
pub fn shown(host: &Host<'_>, key: &str, value: i64, range: (i64, i64)) -> Result<i64, Error> {
    with_reading(host, key, value, |reading| {
        first_alike(reading, value, range)
    })
}

/// The value setting `key` steps to from `from` towards `to`: for the size
/// of book text the next size that looks different ([`stepped`]); `to` for
/// any other setting, and where the host has no reading font.
///
/// # Errors
/// The host's refusal to measure.
pub fn step(
    host: &Host<'_>,
    key: &str,
    (from, to): (i64, i64),
    range: (i64, i64),
) -> Result<i64, Error> {
    with_reading(host, key, to, |reading| stepped(reading, from, to, range))
}

/// `f` with the reading font, for the size of book text; `plain` for any
/// other setting and where the host gives no reading font.
fn with_reading(
    host: &Host<'_>,
    key: &str,
    plain: i64,
    f: impl FnOnce(&Reading<'_>) -> Result<i64, Error>,
) -> Result<i64, Error> {
    if key != SETTING {
        return Ok(plain);
    }
    let Ok(reading) = Reading::acquire(host) else {
        return Ok(plain);
    };
    let result = f(&reading);
    let released = reading.release();
    let value = result?;
    released?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A font pack's sizes at a screen's dots per inch: drawn at the
    /// nearest pixel size (the smaller of two as near), as the kernel's
    /// packs are.
    struct Pack {
        dpi: f32,
        sizes: &'static [u16],
    }

    impl Looks for Pack {
        fn looks(&self, points: i64) -> Result<u32, Error> {
            let quarters = (points as f32 * self.dpi / 72.0 * 4.0) as i32;
            let nearest = self
                .sizes
                .iter()
                .map(|px| i32::from(*px) * 4)
                .min_by_key(|q| (q.abs_diff(quarters), *q))
                .ok_or(Error::NotFound)?;
            Ok(nearest as u32)
        }
    }

    /// The 217 dpi of the X4 Pro and the X4 Classic, with their packs.
    const X4_CLASSIC: Pack = Pack {
        dpi: 217.0,
        sizes: &[20, 24, 28, 32],
    };

    /// A scalable font: every size drawn apart.
    struct Outline;

    impl Looks for Outline {
        fn looks(&self, points: i64) -> Result<u32, Error> {
            Ok(points as u32)
        }
    }

    #[test]
    fn sizes_drawn_alike_are_stepped_over() {
        let range = (6, 11);
        // 9 and 10 pt are both 28 px: up from 9 is 11, down from 11 is 9.
        assert_eq!(stepped(&X4_CLASSIC, 9, 10, range).unwrap(), 11);
        assert_eq!(stepped(&X4_CLASSIC, 11, 10, range).unwrap(), 9);
        // 6 and 7 pt are both 20 px: down from 8 is 6.
        assert_eq!(stepped(&X4_CLASSIC, 8, 7, range).unwrap(), 6);
        // Nothing that looks different further on: it stays.
        assert_eq!(stepped(&X4_CLASSIC, 11, 12, range).unwrap(), 11);
        assert_eq!(stepped(&X4_CLASSIC, 7, 6, range).unwrap(), 7);
    }

    #[test]
    fn a_size_kept_shows_as_the_smallest_drawn_alike() {
        let range = (6, 11);
        assert_eq!(first_alike(&X4_CLASSIC, 10, range).unwrap(), 9);
        assert_eq!(first_alike(&X4_CLASSIC, 7, range).unwrap(), 6);
        assert_eq!(first_alike(&X4_CLASSIC, 9, range).unwrap(), 9);
        // Out of the range: its nearest end.
        assert_eq!(first_alike(&X4_CLASSIC, 30, range).unwrap(), 11);
        // From 10 (shown as 9) a step up comes to 11, a step down to 8.
        assert_eq!(stepped(&X4_CLASSIC, 10, 11, range).unwrap(), 11);
        assert_eq!(stepped(&X4_CLASSIC, 10, 9, range).unwrap(), 8);
    }

    #[test]
    fn a_scalable_font_offers_every_size() {
        let range = (6, 40);
        assert_eq!(stepped(&Outline, 9, 10, range).unwrap(), 10);
        assert_eq!(stepped(&Outline, 10, 9, range).unwrap(), 9);
        assert_eq!(first_alike(&Outline, 10, range).unwrap(), 10);
    }
}
