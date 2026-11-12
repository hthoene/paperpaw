//! Parked native plugins between calls (#248, `docs/measurements/parked-plugins.md`):
//! a start asked where a parked plugin could not be made runs at the next
//! safe point, and a plugin whose batch was refused at an unpark is withdrawn
//! through the normal path.

impl crate::Kernel {
    /// Makes the parked plugins asked to start whose turn is now; withdraws
    /// those whose batch was refused. `true` where one was made (what waited
    /// for it is delivered on the next pass). At safe points only: the top of
    /// `run_commands` and the end of a service pump pass.
    // Its own frame: off the command loop's route (#250).
    #[inline(never)]
    pub(crate) fn native_unpark_pending(&mut self) -> bool {
        self.shared.native_instances.set_now(self.now.millis());
        let mut made = false;
        // Each start is taken once; a bound all the same.
        for _ in 0..MAX_STARTS_A_PASS {
            let Some((key, refused)) = self.shared.native_instances.take_parked_start() else {
                break;
            };
            let Some(id) = self
                .shared
                .native_instances
                .id(key)
                .and_then(paperpaw_sdk::Id::try_clone)
            else {
                continue;
            };
            if refused {
                self.warn_runtime(
                    "a built-in plugin registered differently when started again; it was stopped",
                );
                let _ = self.unload_plugin(&id);
                self.plugins_changed();
                continue;
            }
            if let Some(lease) = self.shared.native_instances.lease(key) {
                // A failure leaves it parked (no memory: its wait) or
                // refused (withdrawn on the next pass).
                made |= lease.live().is_ok();
            }
        }
        made
    }
}

impl crate::Kernel {
    /// Whether `owner` can be told something now: running, or parked and
    /// made now. A parked plugin that cannot be made now (inside a hold, or
    /// waiting after no memory) keeps what waits for it, and is asked to
    /// start at the next safe point.
    pub(crate) fn native_ready(&self, owner: u64) -> bool {
        match self.shared.native_instances.lease(owner) {
            Some(lease) if lease.is_parked() => lease.live().is_ok(),
            _ => true,
        }
    }

    /// Whether `owner` is still parked: what waits for it does not ask for
    /// another pass now (its start does, `native_unpark_pending`).
    pub(crate) fn native_parked(&self, owner: u64) -> bool {
        self.shared
            .native_instances
            .lease(owner)
            .is_some_and(|lease| lease.is_parked())
    }
}

/// At most this many parked plugins are made in one pass.
const MAX_STARTS_A_PASS: usize = 8;

#[cfg(test)]
#[path = "parked_tests.rs"]
mod tests;
