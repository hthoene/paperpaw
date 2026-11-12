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

    /// How many handles the pool holds, all owners together; `None` without
    /// a pool or while its table is in use. For the standby-cycle tests: a
    /// plugin parked again and made again must not leave handles behind.
    #[cfg(any(test, feature = "testing"))]
    pub(crate) fn handle_count(&self) -> Option<usize> {
        self.pool.as_ref()?.handle_count()
    }
}
