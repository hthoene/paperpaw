//! LAB ONLY (#40, feature `turn-lab`, off by default and in no release): how a
//! page turn (`RefreshMode::Partial`) is shown, chosen by the file
//! `turn-lab.txt` on the card, one digit, read before each turn (none or
//! another: 0). The numbers of frames are guesses to be judged on a device:
//! the lab exists to find the fastest variant whose ghosting is acceptable.
//!
//! | digit | page turn |
//! |---|---|
//! | 0 | as the product: the panel's own fast waveform (about 505 ms) |
//! | 1 | the panel's own fast tables for a warm panel (90 °C, the shortest phases the vendor made), whatever the real temperature |
//! | 2 | direct drive, 6 frames |
//! | 3 | direct drive, 4 frames |
//! | 4 | A2-like: direct drive, 2 frames, every [`CLEAN_EVERY`]-th turn a half refresh (cleans all ghosts) |
//!
//! Every turn logs `turn-lab: variant N turn K [clean]: shown in X ms`.

use paperpaw_platform::RefreshMode;
use paperpaw_x4::display::Experiment as Ssd1677;
use paperpaw_x4::panel::Experiment;
use paperpaw_x4::waveform::Direct;

use super::Firmware;

/// Variant 4 cleans with a half refresh every this many turns.
pub(super) const CLEAN_EVERY: u32 = 5;
const MOST_VARIANT: u8 = 4;

/// Which variant runs and how many turns it has shown.
#[derive(Debug, Default)]
pub(super) struct Lab {
    variant: u8,
    turns: u32,
}

/// What the lab does with the turn being shown.
#[derive(Debug, Clone, Copy)]
pub(super) struct Turn {
    pub variant: u8,
    pub number: u32,
    /// Show it as a half refresh (cleans).
    pub clean: bool,
}

impl Turn {
    /// The waveform of a turn that is a fast refresh.
    pub(super) fn waveform(self) -> Option<Experiment> {
        let ssd1677 = match self.variant {
            1 => Ssd1677::Temperature(90),
            2 => Ssd1677::Direct(Direct::new(6, 0)),
            3 => Ssd1677::Direct(Direct::new(4, 0)),
            4 => Ssd1677::Direct(Direct::new(2, 0)),
            _ => return None,
        };
        Some(Experiment::Ssd1677(ssd1677))
    }
}

impl Firmware {
    /// The next page turn, if `mode` is one; a change of variant starts the
    /// count again.
    pub(super) fn turn_lab(&mut self, mode: RefreshMode) -> Option<Turn> {
        if mode != RefreshMode::Partial {
            return None;
        }
        let mut digit = *b"0";
        let _ = self.kernel.storage().read_at("turn-lab.txt", 0, &mut digit);
        let variant = digit[0].saturating_sub(b'0').min(MOST_VARIANT);
        let lab = &mut self.state.turn_lab;
        if variant != lab.variant {
            lab.variant = variant;
            lab.turns = 0;
        }
        lab.turns += 1;
        let number = lab.turns;
        Some(Turn {
            variant,
            number,
            clean: variant == MOST_VARIANT && number.is_multiple_of(CLEAN_EVERY),
        })
    }
}
