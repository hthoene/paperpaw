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
//!   step; the job is kept): the next try is [`WAIT_MS`] later (twice as long
//!   after each refusal in a row, at most [`WAIT_MAX_MS`]; the first one is
//!   logged; a move begun meanwhile starts the back-off again), and the
//!   service pump's next wake-up says so (`service_due`). `Idle`: nothing is
//!   scheduled; each pass looks again, which costs one flag read, so a move
//!   that starts in a callback is carried by the next pass.
//! * What a step sends is bounded by the step: at most `STEP_BOOKS` books, one
//!   event each, and the settings that follow the folder.
//! * Journals that could not be read were set aside by the job; the pump logs
//!   how many, once a pass at most.
use super::lib_moves_service::Pump;

/// How long a refused step waits before it is tried again; each refusal in a
/// row doubles it, up to [`WAIT_MAX_MS`], so a card that stays broken does
/// not keep the device awake every two seconds.
pub(crate) const WAIT_MS: u64 = 2_000;

/// The longest wait between two tries.
pub(crate) const WAIT_MAX_MS: u64 = 64_000;

impl crate::Kernel {
    /// One step of the library's move job. `true` when another pass should
    /// follow at once.
    // Its own frame: the pump keeps the services' passes apart, so that no
    // frame adds their locals (the firmware's frame limit for host code).
    #[inline(never)]
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
        // A move the user just began is not held back by the wait of an
        // earlier step.
        if let Some(moves) = hosts.library_moves() {
            instances.moves_begun_seen(moves.begun());
        }
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
            Pump::More => {
                self.shared.native_instances.moves_ok();
                true
            }
            Pump::Idle => {
                self.shared.native_instances.moves_ok();
                false
            }
            Pump::Wait => {
                let failures = self.shared.native_instances.moves_failed();
                // 2 s doubled at most five times is exactly `WAIT_MAX_MS`; a
                // longer shift would overflow and wrap to a short wait.
                let wait = (WAIT_MS << failures.saturating_sub(1).min(5)).min(WAIT_MAX_MS);
                self.shared
                    .native_instances
                    .set_moves_retry(Some(now.saturating_add(wait)));
                if failures == 1 {
                    self.warn_runtime("a library move step was refused and will be retried");
                }
                false
            }
        }
    }
}

#[cfg(test)]
#[path = "moves_pump_tests.rs"]
mod tests;
