//! Kernel retries a borrowed UI store after the callback scope has ended.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::{
    HostResources,
    ui_service::UiServices,
    ui_store::{Limits, Stamp, UiStore},
};
use paperpaw_native::host_glue::ERR_BUSY;
use paperpaw_sdk::Id;

fn pool(store: &Shared<UiStore>) -> HostResources {
    let pool = HostResources::try_new().unwrap();
    pool.hosts()
        .wire_ui(UiServices::new(store.try_clone().unwrap(), false))
        .unwrap();
    pool
}

#[test]
fn busy_instance_cleanup_stays_queued_until_a_later_main_loop_pass() {
    let store = Shared::try_new(UiStore::new(Limits::ELSEWHERE)).unwrap();
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
    let store = Shared::try_new(UiStore::new(Limits::ELSEWHERE)).unwrap();
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

#[test]
fn fixed_stack_and_deferred_cleanup_have_a_bounded_resident_size() {
    let size = core::mem::size_of::<crate::native_host::stack_rows::StackRows>();
    assert!(size <= 512);
    std::println!("StackRows incl fixed deferred cleanup: {size} bytes");
}

#[test]
fn cleanup_sweep_recovers_more_than_sixteen_closures_and_preserves_live_instances() {
    use alloc::boxed::Box;
    use paperpaw_sdk::{
        Manifest, Params, Plugin, RegisterError, Registrar, Screen, ScreenSpec, Version,
    };
    struct Live;
    impl Screen for Live {
        fn on_action(&mut self, _: &Id, _: &mut paperpaw_sdk::Cx<'_, '_>) -> paperpaw_sdk::Handled {
            paperpaw_sdk::Handled::No
        }
        fn draw(&mut self, _: &mut paperpaw_graphics::Canvas<'_>, _: &paperpaw_sdk::Env<'_>) {}
    }
    impl Plugin for Live {
        fn manifest(&self) -> Manifest {
            Manifest::new(Id::new_static("live"), "Live", Version::new(1, 0, 0))
        }
        fn register(&self, registrar: &mut Registrar) -> Result<(), RegisterError> {
            registrar.add_screen(ScreenSpec::new(
                Id::new_static("live.screen"),
                "Live",
                Box::new(|_, _| Box::new(Live)),
            ))
        }
    }
    let store = Shared::try_new(UiStore::new(Limits::ELSEWHERE)).unwrap();
    let pool = pool(&store);
    let owner = pool.try_owner().unwrap();
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(Live)
        .build()
        .unwrap();
    kernel.open_screen(
        paperpaw_platform::Instant(0),
        Id::new_static("live.screen"),
        Params::new(),
    );
    let live = kernel.stack.last().unwrap().serial;
    kernel
        .shared
        .native_instances
        .adopt_pool(owner.try_view().unwrap())
        .unwrap();
    let who = Stamp::new(owner.owner_id(), 1).unwrap();
    let mut handovers = alloc::vec::Vec::new();
    for instance in core::iter::once(live).chain(100..120) {
        let id = store.handover_make(who, &[]).unwrap();
        let claims = store.claim(who, &[id]).unwrap();
        store.attach(Some(id), instance).unwrap();
        drop(claims);
        handovers.push((instance, id));
    }
    let borrowed = store.inner_busy().unwrap();
    for instance in 100..120 {
        assert_eq!(kernel.native_instance_closed(instance), Err(ERR_BUSY));
        kernel.shared.stack_rows.retry_cleanup(instance).unwrap();
    }
    kernel.drain_deferred_ui_closes();
    assert!(kernel.shared.stack_rows.cleanup_pending());
    assert!(kernel.native_unload_due().is_some());
    drop(borrowed);
    kernel.drain_deferred_ui_closes();
    assert!(!kernel.shared.stack_rows.cleanup_pending());
    let mut out = [0; 64];
    for (instance, id) in handovers {
        assert_eq!(
            store.params(instance, id, &mut out).is_ok(),
            instance == live
        );
    }
}
