//! What a shell asks of the kernel: open a slot, a book or a screen, run an
//! action.

use super::*;

impl Kernel {
    /// Opens the screen bound to `slot` with `params`, e.g. the reader with
    /// a book's path: for a shell asked to open a file (Android's "open
    /// with", a remote control).
    pub fn open_slot(&mut self, now: Instant, slot: Id, params: Params) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.shared.reader = true;
        self.shared.now = now;
        self.last_activity = now;
        self.output.commands.push(Ask::OpenSlot(slot, params));
        self.run_commands();
        self.finish()
    }

    /// Opens the book at `path` with the reading app chosen for it (see
    /// `paperpaw_sdk::readers`): for a shell asked to open a file.
    pub fn open_book(&mut self, now: Instant, path: &str) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.shared.reader = true;
        self.shared.now = now;
        self.last_activity = now;
        self.output.commands.push(crate::kit::readers::open(path));
        self.run_commands();
        self.finish()
    }

    /// Opens the screen `screen` with `params`, as a plugin would, e.g.
    /// asked by a shell's remote control or a test.
    pub fn open_screen(&mut self, now: Instant, screen: Id, params: Params) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.shared.reader = true;
        self.shared.now = now;
        self.last_activity = now;
        self.output.commands.push(Ask::OpenScreen(screen, params));
        self.run_commands();
        self.finish()
    }

    /// Runs `action` as a button bound to it would (the screen shown has it
    /// first), e.g. asked by a shell's remote control or a test.
    pub fn run_action(&mut self, now: Instant, action: Id) -> Effects {
        let _running = self.shared.meter.enter(meter::CORE);
        self.shared.reader = true;
        self.shared.now = now;
        self.last_activity = now;
        self.dispatch(action);
        self.run_commands(); // As a key press: a dialog's queued close first (#266).
        self.finish()
    }

    /// Draws the off screen (the slot [`slots::OFF`]) for a device the shell
    /// is about to switch off by itself: `reason` is
    /// [`paperpaw_sdk::builtin::params::OFF_TIMER`] for the time of *Switch off after sleeping*.
    /// The frame is refreshed fully; the shell shows it, then cuts the power.
    /// `None` where no screen is bound to the slot (nothing is drawn). The
    /// power menu draws its own (it asks the slot to open, then to switch
    /// off), and a battery too low to draw never asks.
    pub fn switch_off_screen(&mut self, now: Instant, reason: &str) -> Option<Effects> {
        if !self.shared.slots.contains(&slots::OFF) {
            return None;
        }
        let _running = self.shared.meter.enter(meter::CORE);
        self.shared.now = now;
        self.output.commands.push(Ask::OpenSlot(
            slots::OFF,
            Params::new().with(paperpaw_sdk::builtin::params::OFF_REASON, reason),
        ));
        self.output.commands.push(Ask::Redraw(RefreshMode::Full));
        self.run_commands();
        Some(self.finish())
    }
}
