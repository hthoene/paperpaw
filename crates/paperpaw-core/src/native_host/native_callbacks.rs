//! Checked callback clocks, shared without retaining the instance registry.
use super::shared::Shared;
use core::cell::Cell;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_FULL, ERR_NO_MEMORY, ERR_STATE};

#[derive(Default)]
struct State {
    depth: Cell<u8>,
    epoch: Cell<u64>,
    /// What the `screen_opened` of the open now being applied says of its
    /// hand-over: set by the kernel just before `Screen::on_reopen`, taken by
    /// the native screen in it (no allocation: a cell, not a carried value).
    open_info: Cell<Option<super::ui_store::Info>>,
    /// A native screen's `draw` callback returned `OK` since the kernel last
    /// cleared this (the first draw of an instance raises `event.screen_ready`).
    drew: Cell<bool>,
    /// Event batches being delivered and registration transactions open: no
    /// parked plugin is made again while one is (#248, parked plugins M5).
    holds: Cell<u16>,
    /// The kernel's time (ms), for the wait before a parked plugin is tried
    /// again after it found no memory.
    now_ms: Cell<u64>,
}
impl State {
    fn enter(&self, fresh: bool) -> Result<u64, i32> {
        let depth = self.depth.get().checked_add(1).ok_or(ERR_BUSY)?;
        let epoch = if fresh || depth == 1 {
            self.epoch.get().checked_add(1).ok_or(ERR_FULL)?
        } else {
            self.epoch.get()
        };
        self.epoch.set(epoch);
        self.depth.set(depth);
        Ok(epoch)
    }
    fn leave(&self) {
        self.depth.set(self.depth.get() - 1);
    }
}

enum Source {
    Local(State),
    Shared(Shared<State>),
}
impl Default for Source {
    fn default() -> Self {
        Self::Local(State::default())
    }
}

#[derive(Default)]
pub(crate) struct CallbackClock(Source);
impl CallbackClock {
    fn state(&self) -> &State {
        match &self.0 {
            Source::Local(state) => state,
            Source::Shared(state) => state,
        }
    }
    pub(crate) fn depth(&self) -> u8 {
        self.state().depth.get()
    }
    pub(crate) fn enter_outer(&self) -> Result<u64, i32> {
        self.state().enter(false)
    }
    pub(crate) fn leave(&self) {
        self.state().leave();
    }
    /// The hand-over info the next `on_reopen` of a native screen reports.
    pub(crate) fn set_open_info(&self, info: Option<super::ui_store::Info>) {
        self.state().open_info.set(info);
    }
    /// Whether a native screen's draw returned `OK` since the last call;
    /// clears it.
    pub(crate) fn take_drew(&self) -> bool {
        self.state().drew.take()
    }
    /// Whether a parked plugin may be made again now: no callback running,
    /// no event batch being delivered, no registration transaction open.
    pub(crate) fn safe_point(&self) -> bool {
        let state = self.state();
        state.depth.get() == 0 && state.holds.get() == 0
    }
    /// The kernel's time now (set at each tick and call).
    pub(crate) fn set_now(&self, now_ms: u64) {
        self.state().now_ms.set(now_ms);
    }
    /// Holds unparking off until the returned guard is dropped (an event
    /// batch being delivered, a registration transaction). `None` where the
    /// clock is not shared yet (no native plugin): nothing is parked then.
    pub(crate) fn hold(&self) -> Option<Hold> {
        let Source::Shared(state) = &self.0 else {
            return None;
        };
        let state = state.try_clone()?;
        state.holds.set(state.holds.get().saturating_add(1));
        Some(Hold(state))
    }
    pub(crate) fn runtime(&mut self) -> Result<CallbackRuntime, i32> {
        if self.depth() != 0 {
            return Err(ERR_STATE);
        }
        if let Source::Local(state) = &self.0 {
            let value = State {
                depth: Cell::new(0),
                epoch: Cell::new(state.epoch.get()),
                open_info: Cell::new(state.open_info.get()),
                drew: Cell::new(state.drew.get()),
                holds: Cell::new(state.holds.get()),
                now_ms: Cell::new(state.now_ms.get()),
            };
            let shared = Shared::try_new(value).map_err(|_| ERR_NO_MEMORY)?;
            self.0 = Source::Shared(shared);
        }
        match &self.0 {
            Source::Shared(state) => state.try_clone().map(CallbackRuntime).ok_or(ERR_NO_MEMORY),
            Source::Local(_) => Err(ERR_STATE),
        }
    }
}

/// Unparking held off while it lives ([`CallbackClock::hold`]).
#[must_use]
pub(crate) struct Hold(Shared<State>);
impl Drop for Hold {
    fn drop(&mut self) {
        self.0.holds.set(self.0.holds.get().saturating_sub(1));
    }
}

pub(crate) struct CallbackRuntime(Shared<State>);
impl CallbackRuntime {
    /// The same clock again (a shared count, no copy).
    pub(crate) fn try_clone(&self) -> Option<Self> {
        self.0.try_clone().map(Self)
    }
    /// As [`CallbackClock::safe_point`], read through a lease's clock.
    pub(crate) fn safe_point(&self) -> bool {
        self.0.depth.get() == 0 && self.0.holds.get() == 0
    }
    /// As [`CallbackClock::set_now`]'s time.
    pub(crate) fn now_ms(&self) -> u64 {
        self.0.now_ms.get()
    }
    /// As [`CallbackClock::hold`], taken by an unpark itself (its own
    /// registration transaction).
    pub(crate) fn hold(&self) -> Option<Hold> {
        let state = self.0.try_clone()?;
        state.holds.set(state.holds.get().saturating_add(1));
        Some(Hold(state))
    }
    pub(crate) fn enter(&self) -> Result<NativeCallbackEpoch, i32> {
        let state = self.0.try_clone().ok_or(ERR_NO_MEMORY)?;
        let epoch = state.enter(true)?;
        Ok(NativeCallbackEpoch {
            state,
            epoch,
            ending: None,
            failed: Cell::new(false),
        })
    }

    /// Notes that a native screen's `draw` returned `OK` (see `take_drew`).
    pub(crate) fn mark_drew(&self) {
        self.0.drew.set(true);
    }

    /// Takes the hand-over info the kernel set for this `on_reopen`.
    pub(crate) fn take_open_info(&self) -> Option<super::ui_store::Info> {
        self.0.open_info.take()
    }
}

/// Owns only a clock handle. No registry borrow crosses foreign code.
pub(crate) struct NativeCallbackEpoch {
    state: Shared<State>,
    epoch: u64,
    /// The pool and plugin whose status handles end with this callback.
    ending: Option<(super::resources::OwnerResourceView, u64)>,
    /// Whether the callback returned a failure (what it asked never starts).
    failed: Cell<bool>,
}
impl NativeCallbackEpoch {
    /// The callback returned a failure status.
    pub(crate) fn set_failed(&self) {
        self.failed.set(true);
    }

    pub(crate) fn epoch(&self) -> u64 {
        self.epoch
    }

    /// When this callback returns, `plugin`'s status handles of it end
    /// (`core.ui` ops 14 and 15, valid inside the callback only).
    pub(crate) fn ending_status_handles(
        mut self,
        pool: super::resources::OwnerResourceView,
        plugin: u64,
    ) -> Self {
        self.ending = Some((pool, plugin));
        self
    }
}
impl Drop for NativeCallbackEpoch {
    fn drop(&mut self) {
        self.state.leave();
        if let Some((pool, plugin)) = self.ending.take()
            && let Some(ui) = pool.ui()
        {
            // A refusal (the table busy) leaves the handles to the plugin's
            // next grant, which drops its earlier callbacks', and to its
            // stop; none outlives its callback in a lookup (epoch check).
            ui.callback_ended(plugin, self.epoch).ok();
            if self.failed.get() {
                ui.callback_failed(plugin, self.epoch).ok();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixture failures fail the test")]
    use super::*;
    #[test]
    fn nested_real_callbacks_have_distinct_epochs_and_restore_depth() {
        let mut clock = CallbackClock::default();
        let runtime = clock.runtime().unwrap();
        let outer = runtime.enter().unwrap();
        let first = outer.epoch();
        assert_ne!(first, 0);
        let inner = runtime.enter().unwrap();
        assert!(inner.epoch() > first);
        assert_eq!(outer.epoch(), first);
        assert_eq!(clock.depth(), 2);
        drop(inner);
        assert_eq!(clock.depth(), 1);
        drop(outer);
        assert_eq!(clock.depth(), 0);
        assert!(runtime.enter().unwrap().epoch() > first);
    }
    #[test]
    fn a_good_native_draw_is_seen_once_by_the_kernel() {
        let mut clock = CallbackClock::default();
        let runtime = clock.runtime().unwrap();
        assert!(!clock.take_drew());
        runtime.mark_drew();
        assert!(clock.take_drew());
        // Taken: the next draw has to say it again (a trap or a failure does not).
        assert!(!clock.take_drew());
    }
    #[test]
    fn exhaustion_never_wraps_and_does_not_enter_a_callback() {
        let mut clock = CallbackClock::default();
        clock.state().epoch.set(u64::MAX - 1);
        let runtime = clock.runtime().unwrap();
        let last = runtime.enter().unwrap();
        assert_eq!(last.epoch(), u64::MAX);
        drop(last);
        assert!(matches!(runtime.enter(), Err(ERR_FULL)));
        assert_eq!(clock.depth(), 0);
        assert_eq!(clock.state().epoch.get(), u64::MAX);
    }
    #[test]
    fn callback_guard_retains_clock_after_registry_and_runtime_are_dropped() {
        let mut clock = CallbackClock::default();
        let runtime = clock.runtime().unwrap();
        let guard = runtime.enter().unwrap();
        drop(clock);
        drop(runtime);
        assert_ne!(guard.epoch(), 0);
        drop(guard);
    }
    #[cfg(panic = "unwind")]
    #[test]
    fn unwind_releases_only_the_nested_callback_depth() {
        extern crate std;
        let mut clock = CallbackClock::default();
        let runtime = clock.runtime().unwrap();
        let outer = runtime.enter().unwrap();
        let first = outer.epoch();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _inner = runtime.enter().unwrap();
            panic!("fixture callback unwound");
        }));
        assert!(result.is_err());
        assert_eq!(clock.depth(), 1);
        assert_eq!(outer.epoch(), first);
        drop(outer);
        assert_eq!(clock.depth(), 0);
    }
    #[test]
    fn depth_exhaustion_leaves_epoch_and_depth_unchanged_and_recovers() {
        let mut clock = CallbackClock::default();
        let runtime = clock.runtime().unwrap();
        let mut guards = alloc::vec::Vec::with_capacity(usize::from(u8::MAX));
        for _ in 0..u8::MAX {
            guards.push(runtime.enter().unwrap());
        }
        let epoch = clock.state().epoch.get();
        assert!(matches!(runtime.enter(), Err(ERR_BUSY)));
        assert_eq!(clock.depth(), u8::MAX);
        assert_eq!(clock.state().epoch.get(), epoch);
        drop(guards);
        assert_eq!(clock.depth(), 0);
        assert_eq!(runtime.enter().unwrap().epoch(), epoch + 1);
    }

    #[test]
    fn failed_shared_clock_allocation_preserves_local_state_for_retry() {
        let mut clock = CallbackClock::default();
        assert_eq!(clock.enter_outer().unwrap(), 1);
        clock.leave();
        crate::native_host::shell::tests::arm_failure(1);
        let failed = clock.runtime();
        crate::native_host::shell::tests::arm_failure(0);
        assert!(matches!(failed, Err(ERR_NO_MEMORY)));
        assert!(matches!(clock.0, Source::Local(_)));
        assert_eq!(clock.depth(), 0);
        assert_eq!(clock.state().epoch.get(), 1);
        let runtime = clock.runtime().unwrap();
        assert_eq!(runtime.enter().unwrap().epoch(), 2);
        assert_eq!(clock.depth(), 0);
    }
}
