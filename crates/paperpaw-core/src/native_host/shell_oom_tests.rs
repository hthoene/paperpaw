//! Every actual Core host allocation can fail without a panic or a leak.
use super::{NORMAL, arm_failure, reset};
use crate::native_host::environment::EnvironmentScope;
use crate::native_host::shared;
use alloc::rc::Rc;
use core::cell::Cell;
use paperpaw_sdk::{Id, Manifest, Version};

#[test]
fn private_shared_refusal_drops_input_once_and_cloning_does_not_allocate() {
    struct Probe(Rc<Cell<u32>>);
    impl Drop for Probe {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    let destroyed = Rc::new(Cell::new(0));
    let input = Probe(Rc::clone(&destroyed));
    arm_failure(1);
    let result = shared::Shared::try_new(input);
    arm_failure(0);
    assert!(result.is_err());
    assert_eq!(destroyed.get(), 1);
    let value = shared::Shared::try_new(17).unwrap();
    arm_failure(1);
    let copy = value.try_clone();
    arm_failure(0);
    assert!(copy.is_some());
    assert_eq!(*copy.unwrap(), 17);
}

/// Where the heap refuses the room for a
/// contribution, `Staging` says so precisely instead of aborting, the plugin is
/// listed as failed and nothing of it is published (#244).
#[test]
fn a_refused_room_for_a_contribution_fails_the_plugin_without_abort_or_anything_published() {
    use crate::kit::action::ActionSpec;
    use paperpaw_sdk::{PluginState, RegisterError};
    reset(NORMAL);
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let refused = Cell::new(false);
    let manifest = Manifest::new(Id::new_static("tight"), "Tight", Version::new(1, 0, 0));
    kernel.register_items(manifest, &|items: &mut crate::Staging| {
        let go = ActionSpec::new(Id::new_static("tight.go"), "Go");
        // The first allocation now is the room for the first contribution.
        arm_failure(1);
        let result = items.add_action(go);
        arm_failure(0);
        refused.set(matches!(&result,
            Err(RegisterError::Invalid { message }) if message == "contribution allocation failed"));
        result
    });
    assert!(refused.get(), "the precise refusal, no abort");
    assert_eq!(
        kernel.plugins().get("tight").unwrap().state,
        PluginState::Failed
    );
    assert!(
        kernel.actions().get("tight.go").is_none(),
        "nothing published"
    );
}

#[test]
fn resource_slot_growth_oom_creates_no_handle_and_can_retry() {
    use crate::native_host::{
        HostResources,
        resources::{FontKey, FontRole},
    };
    use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_STATE};
    let resources = HostResources::try_new().unwrap();
    let owner = resources.try_owner().unwrap();
    arm_failure(1);
    let refused = owner.insert_font(FontKey::Role(FontRole::Ui));
    arm_failure(0);
    assert_eq!(refused, Err(ERR_NO_MEMORY));
    assert_eq!(owner.with_font(1 << 10, |_| Ok(())), Err(ERR_STATE));
    let handle = owner.insert_font(FontKey::Role(FontRole::Ui)).unwrap();
    assert_eq!(handle, 1 << 10);
    owner.release_all().unwrap();
    assert_eq!(owner.with_font(handle, |_| Ok(())), Err(ERR_STATE));
}

#[test]
fn font_removal_map_or_id_oom_invalidates_older_handles_without_panicking() {
    use crate::native_host::font_removals::FontRemovals;
    let provider = Id::parse("owned-pack").unwrap();
    for allocation in [1, 2] {
        let tracker = FontRemovals::new();
        tracker.removed(&Id::new_static("existing"), 1);
        let before = tracker.generation().unwrap();
        arm_failure(allocation);
        tracker.removed(&provider, 1);
        arm_failure(0);
        assert!(!tracker.valid("existing", before));
        assert!(!tracker.valid("owned-pack", before));
        let after = tracker.generation().unwrap();
        assert!(tracker.valid("owned-pack", after));
    }
}

#[test]
fn service_budget_refusal_never_allocates_another_x4_page() {
    use crate::abi7::core_frame::CoreFrame;
    use crate::native_host::{HostResources, service_slots, shell::DeclaredServices};
    use core::pin::pin;
    use paperpaw_native::{
        host_glue::{ERR_BUSY, ERR_NO_MEMORY, ERR_NOT_FOUND, OK, PpBytes},
        metadata::Declared,
    };
    use paperpaw_platform::Instant;
    let pool = HostResources::try_new().unwrap();
    let owner = pool.try_owner().unwrap();
    let view = owner.try_view().unwrap();
    let scope = pin!(EnvironmentScope::new());
    // SAFETY: the view outlives the scope's callbacks.
    unsafe { scope.expire_view(&view) };
    let declared =
        DeclaredServices::try_from_iter(["core.plugin", "core.usage"].into_iter().map(|key| {
            Declared {
                key,
                major: 1,
                semantic_revision: 0,
            }
        }))
        .unwrap();
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.place(Instant(0));
    let frame = CoreFrame {
        callback: None,
        instance: None,
        env: &env,
        meter: &kernel.shared.meter,
        depth: None,
        directory: None,
        stack_generation: kernel.shared.stack_rows.generation(),
        stack_rows: &kernel.shared.stack_rows,
        font_removals: &kernel.shared.font_removals,
        registry_generation: &kernel.shared.registry_generation,
        usage: &kernel.shared.usage,
        plugins: &kernel.shared.plugins,
        work: &kernel.shared.work_states,
        log: Default::default(),
        ui: None,
    };
    let query = |text: &str, out: &mut u32| {
        // SAFETY: borrowed fixture inputs and scalar output are live and disjoint.
        unsafe {
            service_slots::query(
                &scope,
                &owner,
                &declared,
                PpBytes {
                    ptr: text.as_ptr(),
                    len: text.len(),
                },
                1,
                0,
                out,
            )
        }
    };
    scope
        .as_ref()
        .scope_core(&frame, || {
            let mut first = 0;
            assert_eq!(query("core.usage", &mut first), OK);
            for (key, expected) in [
                ("core.usage", ERR_BUSY),
                ("core.plugin/missing", ERR_NOT_FOUND),
            ] {
                let mut out = 0x9876;
                arm_failure(1);
                let result = query(key, &mut out);
                let no_attempt = super::FAIL_NTH.with(|n| n.get() == 1);
                arm_failure(0);
                assert_eq!(result, expected);
                assert_eq!(out, 0x9876);
                assert!(no_attempt, "query must refuse before allocating scratch");
            }
            assert_eq!(service_slots::release(&scope, &owner, first), OK);
            let mut out = 0x9876;
            arm_failure(1);
            let result = query("core.usage", &mut out);
            arm_failure(0);
            assert_eq!(result, ERR_NO_MEMORY);
            assert_eq!(out, 0x9876);
            assert_eq!(query("core.usage", &mut out), OK);
        })
        .unwrap();
}

#[test]
fn nested_plugin_query_shares_the_x4_blob_budget_until_outer_release() {
    use crate::abi7::core_frame::CoreFrame;
    use crate::native_host::{HostResources, service_slots, shell::DeclaredServices};
    use core::pin::pin;
    use paperpaw_native::{
        host_glue::{ERR_BUSY, OK, PpBytes},
        metadata::Declared,
    };
    use paperpaw_platform::Instant;
    let pool = HostResources::try_new().unwrap();
    let outer = pool.try_owner().unwrap();
    let inner = pool.try_owner().unwrap();
    let (outer_view, inner_view) = (outer.try_view().unwrap(), inner.try_view().unwrap());
    let outer_scope = pin!(EnvironmentScope::new());
    let inner_scope = pin!(EnvironmentScope::new());
    // SAFETY: the views outlive the scopes' callbacks.
    unsafe {
        outer_scope.expire_view(&outer_view);
        inner_scope.expire_view(&inner_view);
    }
    let declared = DeclaredServices::try_from_iter(core::iter::once(Declared {
        key: "core.usage",
        major: 1,
        semantic_revision: 0,
    }))
    .unwrap();
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.place(Instant(0));
    let frame = CoreFrame {
        callback: None,
        instance: None,
        env: &env,
        meter: &kernel.shared.meter,
        depth: None,
        directory: None,
        stack_generation: kernel.shared.stack_rows.generation(),
        stack_rows: &kernel.shared.stack_rows,
        font_removals: &kernel.shared.font_removals,
        registry_generation: &kernel.shared.registry_generation,
        usage: &kernel.shared.usage,
        plugins: &kernel.shared.plugins,
        work: &kernel.shared.work_states,
        log: Default::default(),
        ui: None,
    };
    let query = |scope: &EnvironmentScope,
                 owner: &crate::native_host::resources::OwnerResources,
                 out: &mut u32| {
        // SAFETY: static readable key and live, exclusive scalar output.
        unsafe {
            service_slots::query(
                scope,
                owner,
                &declared,
                PpBytes {
                    ptr: b"core.usage".as_ptr(),
                    len: 10,
                },
                1,
                0,
                out,
            )
        }
    };
    outer_scope
        .as_ref()
        .scope_core(&frame, || {
            let mut outer_blob = 0;
            assert_eq!(query(&outer_scope, &outer, &mut outer_blob), OK);
            inner_scope
                .as_ref()
                .scope_core(&frame, || {
                    let mut inner_blob = 0x9876;
                    arm_failure(1);
                    let result = query(&inner_scope, &inner, &mut inner_blob);
                    let no_attempt = super::FAIL_NTH.with(|n| n.get() == 1);
                    arm_failure(0);
                    assert_eq!(result, ERR_BUSY);
                    assert_eq!(inner_blob, 0x9876);
                    assert!(no_attempt);
                    assert_eq!(service_slots::release(&outer_scope, &outer, outer_blob), OK);
                    assert_eq!(query(&inner_scope, &inner, &mut inner_blob), OK);
                })
                .unwrap();
            assert!(
                outer
                    .with_blob(outer_blob, outer_scope.callback_id(), |_| Ok(()))
                    .is_err()
            );
        })
        .unwrap();
}
