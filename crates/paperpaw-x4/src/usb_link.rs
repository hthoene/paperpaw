//! Whether a computer is talking over the USB cable, from the number of the
//! USB frames the unit last received (#209). A computer sends a frame every
//! millisecond, so the number moves on by about as many as milliseconds pass;
//! nothing else does that. The firmware (`shells/xteink`, `usb_power.rs`)
//! reads the number; this is the judgement of the readings, on a
//! computer-tested machine.
//!
//! A reading that merely differs from the one before is not a bus: a unit
//! that reads noise, or a number that jumps, differs as well. Here the
//! number has to move on the way a bus moves it, in two steps one after the
//! other ([`advancing`]), and that has to be found at two looks a second
//! apart before it counts ([`Link`]); a still number ends it at once.

/// The frame number has eleven bits.
pub const FRAME_MASK: u16 = 0x07FF;
/// A step may be this many frames more than the milliseconds that passed
/// (the delay between two readings is not exact).
const EXTRA_FRAMES: u32 = 2;
/// How many milliseconds apart two looks are at least.
pub const LOOK_MS: u64 = 1000;

/// Whether the number moved on from `first` to `second` as a bus does in
/// `elapsed_ms` (at least one frame, at most as many as milliseconds passed,
/// with a little room), counting round past 2047 to 0.
pub fn advancing(first: u16, second: u16, elapsed_ms: u32) -> bool {
    let step = u32::from(second.wrapping_sub(first) & FRAME_MASK);
    (1..=elapsed_ms + EXTRA_FRAMES).contains(&step)
}

/// Whether a series of readings, one `elapsed_ms` after the other, moved on
/// as a bus does throughout. Fewer than two readings never do.
pub fn bus(readings: &[u16], elapsed_ms: u32) -> bool {
    readings.len() >= 2
        && readings
            .windows(2)
            .all(|pair| advancing(pair[0], pair[1], elapsed_ms))
}

/// What the firmware keeps of the looks.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    /// When the last look was (ms), if there was one.
    at: Option<u64>,
    /// The last look found a bus.
    moving: bool,
    /// The look before that did too.
    before: bool,
}

impl Link {
    pub const fn new() -> Self {
        Self {
            at: None,
            moving: false,
            before: false,
        }
    }

    /// Whether it is time to look again at `now_ms`.
    pub fn due(&self, now_ms: u64) -> bool {
        self.at
            .is_none_or(|at| now_ms.saturating_sub(at) >= LOOK_MS)
    }

    /// Takes a look at `now_ms` that found a bus (`moving`) or not.
    pub fn look(&mut self, now_ms: u64, moving: bool) {
        self.before = self.moving;
        self.moving = moving;
        self.at = Some(now_ms);
    }

    /// A computer is talking: the last two looks found a bus. One that finds
    /// none ends it at once.
    pub fn talking(&self) -> bool {
        self.moving && self.before
    }

    /// The last look found a bus (for showing it; not yet counted).
    pub fn moving(&self) -> bool {
        self.moving
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bus_moves_about_a_frame_a_millisecond() {
        assert!(advancing(100, 103, 3));
        assert!(advancing(100, 102, 3));
        assert!(advancing(100, 101, 3));
        assert!(advancing(100, 105, 3));
    }

    #[test]
    fn a_still_or_jumping_number_is_no_bus() {
        assert!(!advancing(100, 100, 3));
        assert!(!advancing(100, 90, 3));
        assert!(!advancing(100, 106, 3));
        assert!(!advancing(0, 1000, 3));
        assert!(!advancing(0x7FF, 0x7FF, 3));
    }

    #[test]
    fn the_number_counts_round_past_its_end() {
        assert!(advancing(0x7FE, 0x001, 3));
        assert!(advancing(0x7FF, 0x000, 1));
        assert!(!advancing(0x001, 0x7FE, 3));
    }

    #[test]
    fn upper_bits_of_the_register_are_ignored() {
        assert!(advancing(0x0800, 0x0803, 3));
    }

    #[test]
    fn a_series_must_move_on_throughout() {
        assert!(bus(&[10, 13, 16], 3));
        assert!(!bus(&[10, 13, 13], 3));
        assert!(!bus(&[10, 13, 90], 3));
        assert!(!bus(&[10], 3));
        assert!(!bus(&[], 3));
    }

    #[test]
    fn one_look_is_not_a_computer_two_are() {
        let mut link = Link::new();
        assert!(link.due(0));
        link.look(0, true);
        assert!(!link.talking());
        assert!(link.moving());
        assert!(!link.due(999));
        assert!(link.due(1000));
        link.look(1000, true);
        assert!(link.talking());
    }

    #[test]
    fn a_still_look_ends_it_at_once() {
        let mut link = Link::new();
        link.look(0, true);
        link.look(1000, true);
        assert!(link.talking());
        link.look(2000, false);
        assert!(!link.talking());
        assert!(!link.moving());
    }

    #[test]
    fn a_flicker_between_still_looks_never_counts() {
        let mut link = Link::new();
        for (i, moving) in [false, true, false, true, false].into_iter().enumerate() {
            link.look(i as u64 * 1000, moving);
            assert!(!link.talking(), "look {i}");
        }
    }

    #[test]
    fn nothing_before_a_look() {
        let link = Link::new();
        assert!(!link.talking());
        assert!(!link.moving());
        assert!(link.due(0));
    }
}
