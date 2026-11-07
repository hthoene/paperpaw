//! Kernel retries a borrowed UI store after the callback scope has ended.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
use super::{
    HostResources,
    ui_guard::UiStackView,
    ui_service::{UiHost, UiServices},
    ui_store::{Limits, Stamp, UiStore},
};
use alloc::rc::Rc;
use paperpaw_native::host_glue::ERR_BUSY;
use paperpaw_sdk::Id;

struct Host;
impl UiHost for Host {
    fn callback_epoch(&self) -> u64 {
        1
    }
    fn calling_instance(&self, _: u64) -> Option<u32> {
        None
    }
    fn with_stack(&self, _: &mut dyn FnMut(&dyn UiStackView)) {}
    fn x4(&self) -> bool {
        false
    }
    fn close_dialog(&self, _: u32) {
        unreachable!()
    }
}
fn pool(store: &Rc<UiStore>) -> HostResources {
    let pool = HostResources::try_new().unwrap();
    pool.hosts()
        .wire_ui(UiServices {
            store: Rc::clone(store),
            host: Rc::new(Host),
        })
        .unwrap();
    pool
}

#[test]
fn busy_instance_cleanup_stays_queued_until_a_later_main_loop_pass() {
    let store = Rc::new(UiStore::new(Limits::ELSEWHERE));
    let pool = pool(&store);
    let owner = pool.try_owner().unwrap();
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    kernel
        .shared
        .native_instances
        .adopt_pool(owner.try_view().unwrap())
        .unwrap();
    let who = Stamp::new(owner.owner_id(), 1).unwrap();
    let id = store.handover_make(who, &[]).unwrap();
    let claims = store.claim(who, &[id]).unwrap();
    store.attach(Some(id), 8).unwrap();
    drop(claims);
    kernel.shared.stack_rows.retry_cleanup(8).unwrap();
    let borrowed = store.inner_busy().unwrap();
    assert_eq!(kernel.native_instance_closed(8), Err(ERR_BUSY));
    kernel.drain_deferred_ui_closes();
    assert!(kernel.shared.stack_rows.cleanup_pending());
    drop(borrowed);
    let mut out = [0; 64];
    assert!(store.params(8, id, &mut out).is_ok());
    kernel.drain_deferred_ui_closes();
    assert!(!kernel.shared.stack_rows.cleanup_pending());
    assert!(store.params(8, id, &mut out).is_err());
}

#[test]
fn busy_owner_cleanup_leaves_the_stopped_owner_for_ordered_unload_retry() {
    let store = Rc::new(UiStore::new(Limits::ELSEWHERE));
    let pool = pool(&store);
    let owner = pool.try_owner().unwrap();
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    kernel
        .shared
        .native_instances
        .adopt_pool(owner.try_view().unwrap())
        .unwrap();
    let plugin = Id::new_static("probe");
    let key = kernel
        .shared
        .native_instances
        .insert(
            owner.try_view().unwrap(),
            plugin.try_clone().unwrap(),
            None,
            true,
        )
        .unwrap();
    let borrowed = store.inner_busy().unwrap();
    kernel.begin_native_unload(key, &plugin);
    assert!(kernel.shared.native_instances.find("probe").is_some());
    drop(borrowed);
    kernel.begin_native_unload(key, &plugin);
    assert!(kernel.shared.native_instances.find("probe").is_none());
}

#[test]
fn nested_frame_directory_upgrades_have_independent_scoped_strong_counts() {
    use crate::abi7::calls::Instance;
    use crate::native_host::{
        DispatchDepth, native_declarations::NativeDeclarations, shared::Shared,
    };
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let directory = Shared::try_new(NativeDeclarations::default()).unwrap();
    let mut host = kernel.flat_host();
    host.directory = directory.downgrade();
    let depth = DispatchDepth::new();
    let first = Instance::new();
    let second = Instance::new();
    let _outer_call = first.enter(&depth.0).unwrap();
    let outer = host.upgrade_directory().unwrap().unwrap();
    assert_eq!(directory.strong_count(), 2);
    {
        let _inner_call = second.enter(&depth.0).unwrap();
        assert_eq!(depth.current(), 2);
        let inner = host.upgrade_directory().unwrap().unwrap();
        assert_eq!(directory.strong_count(), 3);
        drop(inner);
    }
    assert_eq!(directory.strong_count(), 2);
    drop(outer);
    assert_eq!(directory.strong_count(), 1);
    drop(directory);
    assert!(matches!(
        host.upgrade_directory(),
        Err(paperpaw_native::host_glue::ERR_STATE)
    ));
}
