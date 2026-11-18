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
}
