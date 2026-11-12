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
        // Memory pressure (an unpark found no memory): one idle plugin is
        // parked again first, so the start waiting may find room (D3).
        if self.shared.native_instances.take_pressure() {
            self.native_repark_idle(1);
        }
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
        if made {
            // What waited for it in the document loop (a pending open, a
            // cover or details step) goes on at the next pass, not after the
            // loop's back-off: it backed off only because the plugin was not
            // made yet (#248, a parked format).
            self.shared.native_instances.set_doc_retry(None);
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

impl crate::Kernel {
    /// Parks again up to `at_most` plugins that were registered parked and
    /// run idle now (#248, D3); how many. Only at a safe point outside every
    /// callback, and only on standby or under memory pressure (Decision 166):
    /// [`Kernel::suspend`](crate::Kernel::suspend) and
    /// `native_unpark_pending` after an unpark found no memory. A plugin is
    /// parked again only where all hold: its slot allows it (registered
    /// parked, running, no call holds it, no callback runs), none of its
    /// screens is on the stack, no action or event waits for it, no
    /// hand-over and no staged batch waits, and it holds nothing any host
    /// service would end with it. Its `destroy` runs, and it is made again at
    /// its next use.
    // Its own frame: off the command loop's route (#250).
    #[inline(never)]
    pub(crate) fn native_repark_idle(&mut self, at_most: usize) -> usize {
        let instances = &self.shared.native_instances;
        // Nothing staged may wait: a batch runs for the plugin that staged it.
        let staged = instances.outbox.full()
            || instances
                .screen_stage()
                .is_some_and(super::screen_stage::ScreenStage::waiting);
        if at_most == 0
            || staged
            || instances.depth() != 0
            || instances.pending_handover().is_some()
        {
            return 0;
        }
        let mut parked = 0;
        let mut index = 0;
        while parked < at_most {
            let Some(id) = instances.plugin_ids().nth(index) else {
                break;
            };
            index += 1;
            let Some(key) = instances.find(id.as_str()) else {
                continue;
            };
            let Some(lease) = instances.lease(key) else {
                continue;
            };
            let idle = lease.slot().may_repark()
                && !instances.plugin_has_screen(id.as_str())
                && !instances.waits_for(key)
                && lease
                    .peek()
                    .is_some_and(|instance| instance.holds_nothing());
            if idle && instances.parking_again(key, || lease.slot().repark()) {
                parked += 1;
            }
        }
        parked
    }
}

/// At most this many parked plugins are made in one pass.
const MAX_STARTS_A_PASS: usize = 8;

#[cfg(test)]
#[path = "parked_tests.rs"]
mod tests;
