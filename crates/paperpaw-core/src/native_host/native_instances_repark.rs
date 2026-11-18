//! What parking a plugin again asks of the plugin table (#248, D3): whether
//! anything queued waits for a plugin, the memory-pressure mark, and (tests)
//! the handle count of the pool.

impl super::NativeInstances {
    /// Whether anything queued waits for `owner`: an action or an event
    /// (a plugin is not parked again then).
    pub(crate) fn waits_for(&self, owner: u64) -> bool {
        self.setting_events.waits_for(owner)
            || self
                .action_queue_made()
                .is_some_and(|queue| queue.waits_for(owner))
    }

    /// Whether an unpark found no memory since the last call.
    pub(crate) fn take_pressure(&self) -> bool {
        self.callbacks.take_pressure()
    }

    /// Runs `park` with `owner`'s library receipts kept through the
    /// retirement it causes (Senior, R1 on #1365).
    pub(crate) fn parking_again(&self, owner: u64, park: impl FnOnce() -> bool) -> bool {
        match self.pool.as_ref() {
            Some(pool) => pool.hosts().parking_again(owner, park),
            None => park(),
        }
    }

    /// Whether the parked plugin `key` gave up being made (see
    /// [`Slot::give_up`](super::super::lease_slot::Slot::give_up)), or was
    /// starved (no memory, and nothing could be parked again to make room:
    /// [`Slot::take_starved`](super::super::lease_slot::Slot::take_starved));
    /// taken once. For the document loop's pending opens (Senior, R2 on #1365;
    /// Manager's decision 2).
    pub(crate) fn format_gave_up(&self, key: u64) -> bool {
        self.lease(key).is_some_and(|lease| {
            lease.is_parked() && (lease.slot().take_starved() || lease.slot().give_up())
        })
    }

    /// Parking an idle plugin again freed nothing: every parked plugin whose
    /// last make found no memory is starved
    /// ([`Slot::starve`](super::super::lease_slot::Slot::starve)). Whether one
    /// was.
    pub(crate) fn starve_waiting(&self) -> bool {
        let mut any = false;
        for entry in self.entries.iter().filter(|entry| entry.enabled) {
            if let Some(lease) = entry.instance.as_ref() {
                any |= lease.slot().starve();
            }
        }
        any
    }

    /// How many handles the pool holds, all owners together; `None` without
    /// a pool or while its table is in use. For the standby-cycle tests: a
    /// plugin parked again and made again must not leave handles behind.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn handle_count(&self) -> Option<usize> {
        self.pool.as_ref()?.handle_count()
    }
}
