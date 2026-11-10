//! The library's move job in the kernel's main loop: one bounded step of
//! [`HostServices::pump_library_moves`] a pass of the service pump
//! (`lib_moves_service`).
//!
//! * Never inside a callback (the pass asks again): a step carries books,
//!   settings and data under the moved path, and no plugin code runs while it
//!   does.
//! * The step gets the kernel's `Env`, whose storage is the card the job was
//!   wired over (`wire_library_moves`), and what it sends (the library's
//!   book-moved events, setting changes) goes to the kernel's command queue,
//!   which runs right after the pump (`Kernel::finish`).
//! * `More`: another pass at once. `Wait` (the card or memory refused the
//!   step; the job is kept): the next try is [`WAIT_MS`] later, and the
//!   service pump's next wake-up says so (`service_due`). `Idle`: nothing is
//!   scheduled; each pass looks again, which costs one flag read, so a move
//!   that starts in a callback is carried by the next pass.
//! * Journals that could not be read were set aside by the job; the pump logs
//!   how many, once a pass at most.
use super::lib_moves_service::Pump;

/// How long a refused step waits before it is tried again.
pub(crate) const WAIT_MS: u64 = 2_000;

impl crate::Kernel {
    /// One step of the library's move job. `true` when another pass should
    /// follow at once.
    pub(crate) fn native_moves_pump(&mut self) -> bool {
        let instances = &self.shared.native_instances;
        let Some(hosts) = instances
            .hosts()
            .filter(|hosts| hosts.library_moves().is_some())
        else {
            return false;
        };
        // Never inside a callback: its step comes after it returns.
        if instances.depth() > 0 {
            return true;
        }
        let now = self.now.millis();
        if instances.moves_retry().is_some_and(|at| now < at) {
            return false;
        }
        instances.set_moves_retry(None);
        let env = self.shared.env(self.now);
        let out = &mut self.output.commands;
        let pump = hosts.pump_library_moves(&env, &mut |command| out.push(command));
        let set_aside = hosts
            .library_moves()
            .map_or(0, |moves| moves.take_set_aside());
        if set_aside > 0 {
            self.warn_runtime("a library move journal could not be read and was set aside");
        }
        match pump {
            Pump::More => true,
            Pump::Idle => false,
            Pump::Wait => {
                self.shared
                    .native_instances
                    .set_moves_retry(Some(now.saturating_add(WAIT_MS)));
                false
            }
        }
    }
}

#[cfg(test)]
#[path = "moves_pump_tests.rs"]
mod tests;
