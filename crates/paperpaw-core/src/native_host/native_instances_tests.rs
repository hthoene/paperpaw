#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;
use alloc::rc::Rc;
use paperpaw_native::host_glue::ERR_CANCELLED;
use paperpaw_sdk::Env;
struct Fake {
    active: Cell<bool>,
    drops: Rc<Cell<u32>>,
    retired: Rc<Cell<Option<u64>>>,
    retire_error: Rc<Cell<Option<i32>>>,
    retire_calls: Rc<Cell<u32>>,
}
impl Drop for Fake {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}
impl NativeInstance for Fake {
    fn running(&self) -> bool {
        self.active.get()
    }
    fn stop(&self) {
        self.active.set(false);
    }
    fn retire(&self, owner: u64) -> Result<(), i32> {
        self.stop();
        self.retired.set(Some(owner));
        self.retire_calls.set(self.retire_calls.get() + 1);
        self.retire_error.get().map_or(Ok(()), Err)
    }
}
impl Dispatch for Fake {
    fn event(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        panic!("not called after stop")
    }
    fn input(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        panic!("not called after stop")
    }
    fn draw(
        &self,
        _: u32,
        _: &super::super::registrations::DrawFrame<'_>,
        _: &Env<'_>,
        _: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<i32, i32> {
        panic!("not called after stop")
    }
    fn dropped(&self, _: u32, _: i32) {}
}

#[test]
fn frozen_lease_keeps_instance_alive_but_cannot_call_after_registry_stop() {
    let drops = Rc::new(Cell::new(0));
    let instance = NativeLease::try_new(Fake {
        active: Cell::new(true),
        drops: drops.clone(),
        retired: Rc::new(Cell::new(None)),
        retire_error: Default::default(),
        retire_calls: Default::default(),
    })
    .unwrap();
    let mut registry = NativeInstances::default();
    let key = registry
        .test_insert(Id::new_static("probe"), Some(instance), true)
        .unwrap();
    let InstanceState::Running(lease) = registry.state(key).unwrap() else {
        panic!("running");
    };
    registry.stop(key).unwrap();
    assert_eq!(lease.live().err(), Some(ERR_CANCELLED));
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_platform::Instant(0));
    assert_eq!(lease.event(1, &[], &env), Err(ERR_CANCELLED));
    assert_eq!(drops.get(), 0, "frozen lease retains the old owner");
    drop(lease);
    assert_eq!(drops.get(), 0);
    assert!(matches!(registry.state(key), Ok(InstanceState::Stopped)));
    assert!(registry.retire(key).unwrap().is_none());
    assert_eq!(drops.get(), 1);
}

struct Subscribing(Cell<bool>);
impl NativeInstance for Subscribing {
    fn running(&self) -> bool {
        self.0.get()
    }
    fn stop(&self) {
        self.0.set(false);
    }
    fn subscribed(&self, tag: u32) -> bool {
        tag == 99
    }
}
impl Dispatch for Subscribing {
    fn event(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        Ok(0)
    }
    fn input(&self, _: u32, _: &[u8], _: &Env<'_>) -> Result<i32, i32> {
        Ok(0)
    }
    fn draw(
        &self,
        _: u32,
        _: &super::super::registrations::DrawFrame<'_>,
        _: &Env<'_>,
        _: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<i32, i32> {
        Ok(0)
    }
    fn dropped(&self, _: u32, _: i32) {}
}

#[test]
fn the_directory_follows_admission_install_and_stop() {
    let id_of = |dir: &OwnerDirectory, key: u64| {
        let mut found = None;
        dir.with_id(key, &mut |id| found = Some(alloc::string::String::from(id)))
            .ok()
            .and(found)
    };
    let mut registry = NativeInstances::default();
    let lease = NativeLease::try_new(Subscribing(Cell::new(true))).unwrap();
    let key = registry
        .test_insert(Id::new_static("net.probe"), Some(lease), true)
        .unwrap();
    let lazy = registry
        .test_insert(Id::new_static("net.lazy"), None, true)
        .unwrap();
    let off = registry
        .test_insert(Id::new_static("net.off"), None, false)
        .unwrap();
    let dir = registry.directory().unwrap().try_clone().unwrap();
    assert_eq!(id_of(&dir, key).as_deref(), Some("net.probe"));
    assert_eq!(id_of(&dir, lazy).as_deref(), Some("net.lazy"));
    assert_eq!(id_of(&dir, off), None, "a disabled owner has no row");
    assert_eq!(id_of(&dir, key + 1000), None, "unknown owner");
    assert!(dir.subscribed(key, 99));
    assert!(!dir.subscribed(key, 101), "not that event");
    assert!(!dir.subscribed(lazy, 99), "not started: no subscription");
    let started = NativeLease::try_new(Subscribing(Cell::new(true))).unwrap();
    registry.install(lazy, started).unwrap();
    assert!(dir.subscribed(lazy, 99), "the install records it");
    registry.stop(key).unwrap();
    assert!(!dir.subscribed(key, 99), "a stopped owner has none");
    assert_eq!(id_of(&dir, key), None, "nor an id");
    registry.retire(lazy).unwrap();
    assert_eq!(id_of(&dir, lazy), None, "a retired owner has no row");
}

#[test]
fn an_admission_the_directory_cannot_hold_is_refused_whole() {
    let mut registry = NativeInstances::default();
    let first = registry
        .test_insert(Id::new_static("net.one"), None, true)
        .unwrap();
    crate::native_host::net_service::fail_nth(1);
    let refused = registry.test_insert(Id::new_static("net.two"), None, true);
    crate::native_host::net_service::fail_nth(0);
    assert_eq!(refused, Err(ERR_NO_MEMORY));
    assert_eq!(registry.retained_rows().0, 1, "no registry row either");
    assert!(registry.find("net.two").is_none());
    let dir = registry.directory().unwrap().try_clone().unwrap();
    assert!(dir.with_id(first, &mut |_| {}).is_ok(), "the first stays");
    registry
        .test_insert(Id::new_static("net.two"), None, true)
        .unwrap();
}

#[test]
fn lazy_start_deduplicates_and_flushes_only_after_outermost_epoch() {
    let mut registry = NativeInstances::default();
    let key = registry
        .test_insert(Id::new_static("lazy"), None, true)
        .unwrap();
    let disabled = registry
        .test_insert(Id::new_static("disabled"), None, false)
        .unwrap();
    assert!(matches!(registry.state(key), Ok(InstanceState::Lazy)));
    let outer = registry.callback().unwrap();
    let epoch = outer.epoch();
    let inner = registry.callback().unwrap();
    assert_eq!(inner.epoch(), epoch);
    assert_eq!(registry.depth(), 2);
    registry.request_start(key).unwrap();
    registry.request_start(key).unwrap();
    registry.request_start(disabled).unwrap();
    assert_eq!(registry.take_start(), None);
    drop(inner);
    assert_eq!(registry.take_start(), None);
    drop(outer);
    assert_eq!(registry.take_start(), Some(key));
    assert_eq!(registry.take_start(), None);
    let next = registry.callback().unwrap();
    assert!(next.epoch() > epoch);
}
#[test]
fn abandoned_admission_retries_with_a_new_owner_key() {
    let mut registry = NativeInstances::default();
    let first = registry
        .test_insert(Id::new_static("retry"), None, true)
        .unwrap();
    let identity = registry.identity(first).unwrap();
    let epoch = identity.callback().unwrap().epoch();
    registry.abandon(first).unwrap();
    assert!(registry.id(first).is_none());
    assert_eq!(registry.find("retry"), None);
    let second = registry
        .test_insert(Id::new_static("retry"), None, true)
        .unwrap();
    assert!(second > first);
    let next = registry.identity(second).unwrap();
    assert_ne!(identity.owner(), next.owner());
    assert!(next.callback().unwrap().epoch() > epoch);
}
#[test]
fn reclaiming_middle_row_preserves_live_keys_and_pending_starts() {
    let mut registry = NativeInstances::default();
    let failed = registry
        .test_insert(Id::new_static("failed"), None, true)
        .unwrap();
    let live = registry
        .test_insert(Id::new_static("live"), None, true)
        .unwrap();
    registry.request_start(live).unwrap();
    registry.abandon(failed).unwrap();
    assert_eq!(registry.id(live).unwrap().as_str(), "live");
    assert_eq!(registry.find("live"), Some(live));
    assert_eq!(registry.take_start(), Some(live));
    assert_eq!(registry.identity(live).unwrap().owner(), live);
    assert!(matches!(registry.state(failed), Err(ERR_STATE)));
    assert_eq!(registry.retained_rows().0, 1);
}
#[test]
fn retirement_passes_original_key_to_cleanup_and_reclaims_the_row() {
    let mut registry = NativeInstances::default();
    let retired = Rc::new(Cell::new(None));
    let drops = Rc::new(Cell::new(0));
    let lease = NativeLease::try_new(Fake {
        active: Cell::new(true),
        drops: drops.clone(),
        retired: retired.clone(),
        retire_error: Default::default(),
        retire_calls: Default::default(),
    })
    .unwrap();
    let key = registry
        .test_insert(Id::new_static("probe"), Some(lease), true)
        .unwrap();
    assert!(registry.retire(key).unwrap().is_none());
    assert_eq!(retired.get(), Some(key));
    assert_eq!(drops.get(), 1);
    assert!(registry.id(key).is_none());
    assert_eq!(registry.retained_rows().0, 0);
    let replacement = registry
        .test_insert(Id::new_static("probe"), None, true)
        .unwrap();
    assert!(replacement > key);
}
#[test]
fn registry_uses_pool_owner_and_rejects_a_second_pool_even_after_retirement() {
    let mut registry = NativeInstances::default();
    let pool = super::super::HostResources::try_new().unwrap();
    let first = pool.try_owner().unwrap();
    let key = registry
        .insert(
            first.try_view().unwrap(),
            Id::new_static("first"),
            None,
            true,
        )
        .unwrap();
    assert_eq!(key, first.owner_id());
    assert_eq!(registry.identity(key).unwrap().owner(), first.owner_id());
    assert!(registry.retire(key).unwrap().is_none());
    let other_pool = super::super::HostResources::try_new().unwrap();
    let other = other_pool.try_owner().unwrap();
    assert_eq!(
        registry.insert(
            other.try_view().unwrap(),
            Id::new_static("other"),
            None,
            true
        ),
        Err(ERR_STATE)
    );
    let second = pool.try_owner().unwrap();
    let next = registry
        .insert(
            second.try_view().unwrap(),
            Id::new_static("second"),
            None,
            true,
        )
        .unwrap();
    assert!(next > key);
    assert_eq!(next, second.owner_id());
}
#[test]
fn non_busy_cleanup_failure_backs_off_warns_once_and_can_recover() {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    kernel.take_warnings();
    let error = Rc::new(Cell::new(Some(paperpaw_native::host_glue::ERR_IO)));
    let calls = Rc::new(Cell::new(0));
    let lease = NativeLease::try_new(Fake {
        active: Cell::new(true),
        drops: Default::default(),
        retired: Default::default(),
        retire_error: error.clone(),
        retire_calls: calls.clone(),
    })
    .unwrap();
    let owner = Id::new_static("probe");
    let key = kernel
        .shared
        .native_instances
        .test_insert(owner.clone(), Some(lease), true)
        .unwrap();
    kernel.unload_plugin(&owner);
    assert_eq!(calls.get(), 1);
    assert_eq!(kernel.take_warnings().len(), 1);
    let effects = kernel.tick(paperpaw_platform::Instant(50));
    assert_eq!(calls.get(), 1, "permanent errors do not retry every 50ms");
    assert_eq!(effects.wake_at(), Some(paperpaw_platform::Instant(5_000)));
    kernel.tick(paperpaw_platform::Instant(5_000));
    assert_eq!(calls.get(), 2);
    assert!(
        kernel.take_warnings().is_empty(),
        "the same failure is reported once"
    );
    assert_eq!(
        kernel.native_unload_due(),
        Some(paperpaw_platform::Instant(10_000))
    );
    error.set(None);
    kernel.tick(paperpaw_platform::Instant(10_000));
    assert_eq!(calls.get(), 3);
    assert!(kernel.shared.native_instances.id(key).is_none());
    assert!(kernel.native_unload_due().is_none());
}
#[test]
fn lost_orphan_retirement_reports_queue_full_once_and_backs_off() {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    kernel.take_warnings();
    let pool = super::super::HostResources::try_new().unwrap();
    let owner = pool.try_owner().unwrap();
    kernel
        .shared
        .native_instances
        .adopt_pool(owner.try_view().unwrap())
        .unwrap();
    // Break the admission invariant deliberately: the old fixed queue
    // could lose an owner. The driver must report even an empty queue.
    pool.hosts().owner_gone(123, false);
    pool.hosts().owner_gone(456, false);
    let effects = kernel.tick(paperpaw_platform::Instant(0));
    assert_eq!(effects.wake_at(), Some(paperpaw_platform::Instant(5_000)));
    let warnings = kernel.take_warnings();
    assert_eq!(
        warnings
            .iter()
            .filter(|warning| warning.message.contains("retirement queue full"))
            .count(),
        1
    );
    assert!(pool.hosts().retirements_pending());
    kernel.tick(paperpaw_platform::Instant(50));
    kernel.tick(paperpaw_platform::Instant(5_000));
    assert!(
        kernel.take_warnings().is_empty(),
        "the invariant failure is reported once"
    );
    assert_eq!(
        kernel.native_unload_due(),
        Some(paperpaw_platform::Instant(10_000))
    );
}
