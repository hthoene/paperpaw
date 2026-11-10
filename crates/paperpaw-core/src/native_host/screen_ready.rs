//! `event.screen_ready` (87) and `command.screen_ready` (1557) (`docs/abi/ui.md`
//! 1.1 bit 3 and 7): the edge that tells the subscribers (the help plugin)
//! that a screen instance has something to show, at most once an instance.
//!
//! * **`waits_ready` screen** (`screen.spec` flags bit 3): raised when its
//!   plugin sends `command.screen_ready` while the instance is that plugin's
//!   own top one (else `ERR_STATE`, at submit). A second command for the same
//!   instance is `OK` and raises nothing, as is one for a screen that does not
//!   wait. The host never raises 87 itself for such a screen.
//! * **Every other screen:** raised by the host after the instance's first
//!   draw; a later draw raises nothing.
//! * **Closed first:** an instance closed before it was ready or drawn is
//!   never told of; one opened again is a new instance (a new serial) and
//!   gets its own.
//! * **Order:** `event.screen_shown` (38) comes before 87 for an instance: it
//!   is sent when the screen is shown, the draw comes after.
//!
//! Known gap: the kernel cannot tell a native screen's failed or trapped draw
//! from a good one (the `Screen` trait returns nothing from `draw`), so it
//! counts a draw as done when `draw` returned. The plugin that failed is
//! reported by the native screen as always; a subscriber may be told of a
//! screen whose first page failed.
//!
//! Memory: nothing is kept but the two flags of the stack entry; the event is
//! held by the per-plugin edge store ([`core_events`](super::core_events)),
//! dropped and logged when that is full, never retried.
use crate::Kernel;

impl Kernel {
    /// After a draw pass: raises 87 for every instance that was drawn and
    /// not told of, unless it is a `waits_ready` screen (its plugin says).
    pub(crate) fn raise_screen_ready(&mut self) {
        for index in 0..self.stack.len() {
            let entry = &self.stack[index];
            if !entry.drawn || entry.ready {
                continue;
            }
            if self.screen_waits_ready(index) {
                // Its plugin says when; the draw does not count.
                self.stack[index].drawn = false;
                continue;
            }
            self.raise_ready_at(index);
        }
    }

    /// `command.screen_ready` for the instance at `index` (checked at submit
    /// to be its plugin's own top one): raises 87 when the screen waits and
    /// was not told of yet.
    pub(crate) fn native_command_ready(&mut self, index: usize) {
        if self.stack[index].ready || !self.screen_waits_ready(index) {
            return;
        }
        self.raise_ready_at(index);
    }

    fn screen_waits_ready(&self, index: usize) -> bool {
        self.shared
            .registry
            .native_declarations
            .as_ref()
            .is_some_and(|directory| directory.screen_waits_ready(self.stack[index].id.as_str()))
    }

    fn raise_ready_at(&mut self, index: usize) {
        self.stack[index].ready = true;
        let (id, serial) = (self.stack[index].id.clone(), self.stack[index].serial);
        self.native_screen_ready(&id, serial);
    }
}

#[cfg(test)]
#[path = "screen_ready_tests.rs"]
mod tests;
