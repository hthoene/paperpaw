//! Light or dark for the screens of the sleep, the switch-off, the start and
//! the restart (#420): each has its own setting of the sleep plugin
//! (`sleep.look`, `sleep.off-look`, `sleep.start-look`, `sleep.restart-look`),
//! `sleep.look.light`, `sleep.look.dark`, or anything else for as the device
//! is (night mode, [`core_settings::NIGHT_MODE`]).
//!
//! The frame of such a screen is drawn inverted or not as that says, whoever
//! draws the screen (the sleep plugin or another), the start picture drawn
//! ahead included. Nothing is passed to the panel for it but the frame itself.

use super::*;

/// The setting of the sleep screen (the slot [`slots::SLEEP`]).
const SLEEP_LOOK: &str = "sleep.look";
/// The setting of the off screen (the slot [`slots::OFF`]).
const OFF_LOOK: &str = "sleep.off-look";
/// The setting of the start screen (the slot [`slots::START`]).
const START_LOOK: &str = "sleep.start-look";
/// The setting of the restart screen (the slot [`slots::RESTART`]).
const RESTART_LOOK: &str = "sleep.restart-look";
/// The value that draws the screen light.
const LIGHT: &str = "sleep.look.light";
/// The value that draws the screen dark.
const DARK: &str = "sleep.look.dark";

/// The setting that says how a screen from `slot` looks, for the slots that
/// have one.
fn setting_of(slot: &Id) -> Option<&'static str> {
    if *slot == slots::SLEEP {
        Some(SLEEP_LOOK)
    } else if *slot == slots::OFF {
        Some(OFF_LOOK)
    } else if *slot == slots::START {
        Some(START_LOOK)
    } else if *slot == slots::RESTART {
        Some(RESTART_LOOK)
    } else {
        None
    }
}

impl Kernel {
    /// Whether the screen on top is drawn inverted: as its own setting says
    /// if it is one of the four, else night mode.
    pub(crate) fn inverted_now(&self) -> bool {
        let settings = &self.shared.settings;
        let own = self
            .stack
            .last()
            .and_then(|top| top.slot.as_ref())
            .and_then(setting_of)
            .map(|id| settings.text(id));
        match own {
            Some(LIGHT) => false,
            Some(DARK) => true,
            _ => settings.bool(core_settings::NIGHT_MODE.as_str()),
        }
    }

    /// Draws as [`Kernel::inverted_now`] says; true where that changed what
    /// the framebuffer does, so what is on it is drawn anew.
    pub(crate) fn sync_look(&mut self) -> bool {
        let inverted = self.inverted_now();
        let changed = inverted != self.framebuffer.is_inverted();
        self.framebuffer.set_inverted(inverted);
        changed
    }

    /// `region`, or the whole screen where the look changed (a screen with
    /// a look of its own came, or left).
    pub(crate) fn region_for_look(&mut self, region: Rect) -> Rect {
        if self.sync_look() {
            self.framebuffer.bounds()
        } else {
            region
        }
    }
}
