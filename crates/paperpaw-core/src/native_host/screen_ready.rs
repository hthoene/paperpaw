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
//! * **What counts as a draw:** for a native screen, its `draw` callback
//!   returned `OK` (the dispatcher marks it, the kernel reads and clears the
//!   mark around each screen's draw): a trap, a failure status, a frame the host
//!   cut short and a screen that was not called (not on top) do not count, and
//!   the next good draw does. Other screens count when drawn. A draw of only the
//!   rows an overlay leaves uncovered, or of one band of a frame, counts as the
//!   first page.
//! * **Early command:** a `waits_ready` plugin may send `command.screen_ready`
//!   before the first draw (it says when the screen is ready, not the host).
//!
//! Memory: nothing is kept but the two flags of the stack entry; the event is
//! held by the per-plugin edge store ([`core_events`](super::core_events)),
//! dropped and logged when that is full, never retried.
use crate::{Kernel, Shared, screens::ActiveScreen};

impl Shared {
    /// Draws `active` and notes whether the draw counts for 87: a native
    /// screen's callback returned `OK` (not a trap, a failure, nor a screen that
    /// was not called, below the top); any other screen when it was drawn.
    pub(crate) fn draw_counted(
        &self,
        active: &mut ActiveScreen,
        canvas: &mut paperpaw_graphics::Canvas<'_>,
        env: &paperpaw_sdk::Env<'_>,
    ) {
        if active.drawn || active.ready {
            // Counted already: nothing to note, no lookup.
            active.screen.draw(canvas, env);
            return;
        }
        let native = (self.registry.screens.get(&active.id)).is_some_and(|spec| spec.is_native());
        self.native_instances.take_drew();
        active.screen.draw(canvas, env);
        active.drawn |= !native || self.native_instances.take_drew();
    }
}

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
